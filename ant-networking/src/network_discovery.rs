// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::target_arch::Instant;
use ant_protocol::NetworkAddress;
use libp2p::{kad::KBucketKey, PeerId};
use rand::{thread_rng, Rng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::{btree_map::Entry, BTreeMap};

// The number of PeerId to generate when starting an instance of NetworkDiscovery
const INITIAL_GENERATION_ATTEMPTS: usize = 10_000;
// The number of PeerId to generate during each invocation to refresh our candidates
const GENERATION_ATTEMPTS: usize = 1_000;
// The max number of PeerId to keep per bucket
const MAX_PEERS_PER_BUCKET: usize = 5;

/// Keep track of NetworkAddresses belonging to every bucket (if we can generate them with reasonable effort)
/// which we can then query using Kad::GetClosestPeers to effectively fill our RT.
#[derive(Debug, Clone)]
pub(crate) struct NetworkDiscovery {
    self_key: KBucketKey<PeerId>,
    candidates: BTreeMap<u32, Vec<NetworkAddress>>,
}

impl NetworkDiscovery {
    /// Create a new instance of NetworkDiscovery and tries to populate each bucket with random peers.
    pub(crate) fn new(self_peer_id: &PeerId) -> Self {
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
    pub(crate) fn handle_get_closest_query(&mut self, closest_peers: Vec<PeerId>) {
        let now = Instant::now();

        let candidates_map: BTreeMap<u32, Vec<NetworkAddress>> = closest_peers
            .into_iter()
            .filter_map(|peer| {
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
    pub(crate) fn candidates(&mut self, farthest_bucket: Option<u32>) -> Vec<&NetworkAddress> {
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
