// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::time::{interval, Instant, Interval};
use crate::Addresses;
use crate::{driver::PendingGetClosestType, SwarmDriver};
use ant_protocol::NetworkAddress;
// use libp2p::kad::K_VALUE;
use libp2p::{kad::KBucketKey, PeerId};
use rand::rngs::OsRng;
use rand::{thread_rng, Rng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::{btree_map::Entry, BTreeMap};
use tokio::time::Duration;

// The number of PeerId to generate when starting an instance of NetworkDiscoveryCandidate.
const INITIAL_GENERATION_ATTEMPTS: usize = 10_000;
// The number of PeerId to generate during each invocation to refresh the candidate list.
const GENERATION_ATTEMPTS: usize = 1_000;
// The max number of PeerId to keep per bucket.
const MAX_PEERS_PER_BUCKET: usize = 5;

/// The default interval at which NetworkDiscovery is triggered.
/// The interval is increased as more peers are added to the routing table.
pub(crate) const NETWORK_DISCOVER_INTERVAL: Duration = Duration::from_secs(10);

/// Slow down the process if the previously added peer has been before LAST_PEER_ADDED_TIME_LIMIT.
/// This is to make sure we don't flood the network with `FindNode` msgs.
const LAST_PEER_ADDED_TIME_LIMIT: Duration = Duration::from_secs(180);

/// The network discovery interval to use if we haven't added any new peers in a while.
const NO_PEER_ADDED_SLOWDOWN_INTERVAL_MAX_S: u64 = 600;

impl SwarmDriver {
    /// This functions triggers network discovery based on when the last peer was added to the RT
    /// and the number of peers in RT. The function also returns a new interval that is proportional
    /// to the number of peers in RT, so more peers in RT, the longer the interval.
    pub(crate) async fn run_network_discover_continuously(
        &mut self,
        current_interval: Duration,
    ) -> Option<Interval> {
        let (should_discover, new_interval) = self
            .network_discovery
            .should_we_discover(self.peers_in_rt as u32, current_interval)
            .await;
        if should_discover {
            self.trigger_network_discovery();
        }
        new_interval
    }

    pub(crate) fn trigger_network_discovery(&mut self) {
        let now = Instant::now();

        // Find the farthest bucket that is not full.
        // This is used to skip refreshing the RT of farthest full buckets.
        // let farthest_unfilled_bucket = Some(256);
        let kbuckets: Vec<_> = self.swarm.behaviour_mut().kademlia.kbuckets().collect();
        // // Iterate from 255, 254 and so on by calling `rev()` to tackle the `hole` situation.
        // for kbucket in kbuckets.iter().rev() {
        //     if kbucket.num_entries() < K_VALUE.get() {
        //         let Some(ilog2) = kbucket.range().0.ilog2() else {
        //             continue;
        //         };
        //         farthest_unfilled_bucket = Some(ilog2);
        //         break;
        //     }
        // }

        // let addrs = self
        //     .network_discovery
        //     .candidates
        //     .get_candidates(farthest_unfilled_bucket);
        let num_of_candidates = std::cmp::max(10, kbuckets.len());
        let addrs: Vec<NetworkAddress> = (0..num_of_candidates)
            .map(|_| NetworkAddress::from_peer(PeerId::random()))
            .collect();
        info!("Triggering network discovery with {num_of_candidates} candidates.");
        // Fetches the candidates and also generates new candidates
        for addr in addrs {
            // The query_id is tracked here. This is to update the candidate list of network_discovery with the newly
            // found closest peers. It may fill up the candidate list of closer buckets which are harder to generate.
            let query_id = self
                .swarm
                .behaviour_mut()
                .kademlia
                .get_closest_peers(addr.as_bytes());
            let _ = self.pending_get_closest_peers.insert(
                query_id,
                (PendingGetClosestType::NetworkDiscovery, Default::default()),
            );
        }

        self.network_discovery.initiated();
        debug!("Trigger network discovery took {:?}", now.elapsed());
    }
}

/// The process for discovering new peers in the network.
/// This is done by generating random NetworkAddresses that are closest to our key and querying the network for the
/// closest peers to those Addresses.
///
/// The process slows down based on the number of peers in the routing table and the time since the last peer was added.
pub(crate) struct NetworkDiscovery {
    initial_bootstrap_done: bool,
    last_peer_added_instant: Instant,
    last_network_discover_triggered: Option<Instant>,
    candidates: NetworkDiscoveryCandidates,
}

impl NetworkDiscovery {
    pub(crate) fn new(self_peer_id: &PeerId) -> Self {
        Self {
            initial_bootstrap_done: false,
            last_peer_added_instant: Instant::now(),
            last_network_discover_triggered: None,
            candidates: NetworkDiscoveryCandidates::new(self_peer_id),
        }
    }

    /// The Kademlia Bootstrap request has been sent successfully.
    pub(crate) fn initiated(&mut self) {
        self.last_network_discover_triggered = Some(Instant::now());
    }

    /// Notify about a newly added peer to the RT. This will help with slowing down the process.
    /// Returns `true` if we have to perform the initial bootstrapping.
    pub(crate) fn notify_new_peer(&mut self) -> bool {
        self.last_peer_added_instant = Instant::now();
        // true to kick off the initial bootstrapping.
        // `run_network_discover_continuously` might kick of so soon that we might
        // not have a single peer in the RT and we'd not perform any network discovery for a while.
        if !self.initial_bootstrap_done {
            self.initial_bootstrap_done = true;
            true
        } else {
            false
        }
    }

    /// Returns `true` if we should carry out the Kademlia Bootstrap process immediately.
    /// Also optionally returns the new interval for network discovery.
    pub(crate) async fn should_we_discover(
        &self,
        peers_in_rt: u32,
        current_interval: Duration,
    ) -> (bool, Option<Interval>) {
        let should_network_discover = peers_in_rt >= 1;

        // if it has been a while (LAST_PEER_ADDED_TIME_LIMIT) since we have added a new peer,
        // slowdown the network discovery process.
        // Don't slow down if we haven't even added one peer to our RT.
        if self.last_peer_added_instant.elapsed() > LAST_PEER_ADDED_TIME_LIMIT && peers_in_rt != 0 {
            // To avoid a heart beat like cpu usage due to the 1K candidates generation,
            // randomize the interval within certain range
            let no_peer_added_slowdown_interval: u64 = OsRng.gen_range(
                NO_PEER_ADDED_SLOWDOWN_INTERVAL_MAX_S / 2..NO_PEER_ADDED_SLOWDOWN_INTERVAL_MAX_S,
            );
            let no_peer_added_slowdown_interval_duration =
                Duration::from_secs(no_peer_added_slowdown_interval);
            info!(
                    "It has been {LAST_PEER_ADDED_TIME_LIMIT:?} since we last added a peer to RT. Slowing down the continuous network discovery process. Old interval: {current_interval:?}, New interval: {no_peer_added_slowdown_interval_duration:?}"
                );

            let mut new_interval = interval(no_peer_added_slowdown_interval_duration);
            new_interval.tick().await;

            return (should_network_discover, Some(new_interval));
        }

        let duration_based_on_peers = Self::scaled_duration(peers_in_rt);
        let new_interval = if duration_based_on_peers > current_interval {
            info!("More peers have been added to our RT!. Slowing down the continuous network discovery process. Old interval: {current_interval:?}, New interval: {duration_based_on_peers:?}");

            let mut interval = interval(duration_based_on_peers);
            interval.tick().await;

            Some(interval)
        } else {
            None
        };

        (should_network_discover, new_interval)
    }

    pub(crate) fn handle_get_closest_query(&mut self, closest_peers: Vec<(PeerId, Addresses)>) {
        self.candidates.handle_get_closest_query(closest_peers);
    }

    /// Returns an exponentially increasing interval based on the number of peers in the routing table.
    /// Formula: y=30 * 1.00673^x
    /// Caps out at 600s for 400+ peers
    fn scaled_duration(peers_in_rt: u32) -> Duration {
        if peers_in_rt >= 450 {
            return Duration::from_secs(600);
        }
        let base: f64 = 1.00673;

        Duration::from_secs_f64(30.0 * base.powi(peers_in_rt as i32))
    }
}

/// Keep track of NetworkAddresses belonging to every bucket (if we can generate them with reasonable effort)
/// which we can then query using Kad::GetClosestPeers to effectively fill our RT.
#[derive(Debug, Clone)]
struct NetworkDiscoveryCandidates {
    self_key: KBucketKey<PeerId>,
    candidates: BTreeMap<u32, Vec<NetworkAddress>>,
}

impl NetworkDiscoveryCandidates {
    /// Create a new instance of NetworkDiscoveryCandidates and tries to populate each bucket with random peers.
    fn new(self_peer_id: &PeerId) -> Self {
        let start = Instant::now();
        let self_key = KBucketKey::from(*self_peer_id);
        let candidates = Self::generate_candidates(&self_key, INITIAL_GENERATION_ATTEMPTS);

        info!(
            "Time to generate NetworkDiscoveryCandidates: {:?}",
            start.elapsed()
        );
        let buckets_covered = candidates
            .iter()
            .map(|(ilog2, candidates)| (*ilog2, candidates.len()))
            .collect::<Vec<_>>();
        info!("The generated network discovery candidates currently cover these ilog2 buckets: {buckets_covered:?}");

        Self {
            self_key,
            candidates,
        }
    }

    /// The result from the kad::GetClosestPeers are again used to update our kbucket.
    fn handle_get_closest_query(&mut self, closest_peers: Vec<(PeerId, Addresses)>) {
        let now = Instant::now();

        let candidates_map: BTreeMap<u32, Vec<NetworkAddress>> = closest_peers
            .into_iter()
            .filter_map(|(peer, _)| {
                let peer = NetworkAddress::from_peer(peer);
                let peer_key = peer.as_kbucket_key();
                peer_key
                    .distance(&self.self_key)
                    .ilog2()
                    .map(|ilog2| (ilog2, peer))
            })
            // To collect the NetworkAddresses into a vector.
            .fold(BTreeMap::new(), |mut acc, (ilog2, peer)| {
                acc.entry(ilog2).or_default().push(peer);
                acc
            });

        for (ilog2, candidates) in candidates_map {
            self.insert_candidates(ilog2, candidates);
        }

        trace!(
            "It took {:?} to NetworkDiscovery::handle get closest query",
            now.elapsed()
        );
    }

    /// Returns one random candidate per bucket. Also tries to refresh the candidate list.
    /// Set the farthest_bucket to get candidates that are closer than or equal to the farthest_bucket.
    #[allow(dead_code)]
    fn get_candidates(&mut self, farthest_bucket: Option<u32>) -> Vec<&NetworkAddress> {
        self.try_refresh_candidates();

        let mut rng = thread_rng();
        let mut op = Vec::with_capacity(self.candidates.len());

        let candidates = self.candidates.iter().filter_map(|(ilog2, candidates)| {
            if let Some(farthest_bucket) = farthest_bucket {
                if *ilog2 > farthest_bucket {
                    debug!(
                        "Skipping candidates for ilog2: {ilog2} as it is greater than farthest_bucket: {farthest_bucket}"
                    );
                    return None;
                }
            }
            // get a random index each time
            let random_index = rng.gen::<usize>() % candidates.len();
            candidates.get(random_index)
        });
        op.extend(candidates);
        op
    }

    /// Tries to refresh our current candidate list. We replace the old ones with new if we find any.
    #[allow(dead_code)]
    fn try_refresh_candidates(&mut self) {
        let candidates_vec = Self::generate_candidates(&self.self_key, GENERATION_ATTEMPTS);
        for (ilog2, candidates) in candidates_vec {
            self.insert_candidates(ilog2, candidates);
        }
    }

    // Insert the new candidates and remove the old ones to maintain MAX_PEERS_PER_BUCKET.
    fn insert_candidates(&mut self, ilog2: u32, new_candidates: Vec<NetworkAddress>) {
        match self.candidates.entry(ilog2) {
            Entry::Occupied(mut entry) => {
                let existing_candidates = entry.get_mut();
                // insert only newly seen new_candidates
                let new_candidates: Vec<_> = new_candidates
                    .into_iter()
                    .filter(|candidate| !existing_candidates.contains(candidate))
                    .collect();
                existing_candidates.extend(new_candidates);
                // Keep only the last MAX_PEERS_PER_BUCKET elements i.e., the newest ones
                let excess = existing_candidates
                    .len()
                    .saturating_sub(MAX_PEERS_PER_BUCKET);
                if excess > 0 {
                    existing_candidates.drain(..excess);
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(new_candidates);
            }
        }
    }

    /// Uses rayon to parallelize the generation
    fn generate_candidates(
        self_key: &KBucketKey<PeerId>,
        num_to_generate: usize,
    ) -> BTreeMap<u32, Vec<NetworkAddress>> {
        (0..num_to_generate)
            .into_par_iter()
            .filter_map(|_| {
                let candidate = NetworkAddress::from_peer(PeerId::random());
                let candidate_key = candidate.as_kbucket_key();
                let ilog2 = candidate_key.distance(&self_key).ilog2()?;
                Some((ilog2, candidate))
            })
            // Since it is parallel iterator, the fold fn batches the items and will produce multiple outputs. So we
            // should use reduce fn to combine multiple outputs.
            .fold(
                BTreeMap::new,
                |mut acc: BTreeMap<u32, Vec<NetworkAddress>>, (ilog2, candidate)| {
                    acc.entry(ilog2).or_default().push(candidate);
                    acc
                },
            )
            .reduce(
                BTreeMap::new,
                |mut acc: BTreeMap<u32, Vec<NetworkAddress>>, map| {
                    for (ilog2, candidates) in map {
                        let entry = acc.entry(ilog2).or_default();
                        for candidate in candidates {
                            if entry.len() < MAX_PEERS_PER_BUCKET {
                                entry.push(candidate);
                            } else {
                                break;
                            }
                        }
                    }
                    acc
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaled_interval() {
        let test_cases = vec![
            (0, 30.0),
            (50, 40.0),
            (100, 60.0),
            (150, 80.0),
            (200, 115.0),
            (220, 130.0),
            (250, 160.0),
            (300, 220.0),
            (350, 313.0),
            (400, 430.0),
            (425, 520.0),
            (449, 600.0),
            (1000, 600.0),
        ];

        for (peers, expected_secs) in test_cases {
            let interval = NetworkDiscovery::scaled_duration(peers);
            let actual_secs = interval.as_secs_f64();

            let tolerance = 0.15 * expected_secs; // 5% tolerance

            assert!(
                (actual_secs - expected_secs).abs() < tolerance,
                "For {peers} peers, expected duration {expected_secs:.2}s but got {actual_secs:.2}s",
            );

            println!("Peers: {peers}, Expected: {expected_secs:.2}s, Actual: {actual_secs:.2}s",);
        }
    }
}
