// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
#![allow(clippy::mutable_key_type)]

use crate::time::spawn;
use crate::{event::NetworkEvent, time::Instant, CLOSE_GROUP_SIZE};
use ant_protocol::{
    storage::{DataTypes, ValidationType},
    NetworkAddress, PrettyPrintRecordKey,
};
use libp2p::{
    kad::{KBucketDistance as Distance, RecordKey, K_VALUE},
    PeerId,
};
use std::collections::{hash_map::Entry, BTreeMap, HashMap, HashSet, VecDeque};
use tokio::{sync::mpsc, time::Duration};

// Max parallel fetches that can be undertaken at the same time.
const MAX_PARALLEL_FETCH: usize = K_VALUE.get() / 2;

// The duration after which a peer will be considered failed to fetch data from,
// if no response got from that peer.
// Note this will also cover the period that node self write the fetched copy to disk.
// Hence shall give a longer time as allowance.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

// The duration after which a pending entry shall be cleared from the `to_be_fetch` list.
// This is to avoid holding too many outdated entries when the fetching speed is slow.
const PENDING_TIMEOUT: Duration = Duration::from_secs(900);

// The time the entry will be considered as `time out` and to be cleared.
type ReplicationTimeout = Instant;

#[derive(Debug)]
pub(crate) struct ReplicationFetcher {
    self_peer_id: PeerId,
    // Pending entries that to be fetched from the target peer.
    to_be_fetched: HashMap<(RecordKey, ValidationType, PeerId), ReplicationTimeout>,
    // Avoid fetching same chunk from different nodes AND carry out too many parallel tasks.
    on_going_fetches: HashMap<(RecordKey, ValidationType), (PeerId, ReplicationTimeout)>,
    event_sender: mpsc::Sender<NetworkEvent>,
    /// Distance range that the incoming key shall be fetched
    distance_range: Option<Distance>,
    /// Restrict fetch range to closer than this value
    /// used when the node is full, but we still have "close" data coming in
    /// that is _not_ closer than our farthest max record
    farthest_acceptable_distance: Option<Distance>,
    /// Scoring of peers collected from storage_challenge.
    /// To be a trustworthy replication source, the peer must has two latest scoring both healthy.
    peers_scores: HashMap<PeerId, (VecDeque<bool>, Instant)>,
    /// During startup, when the knowledge of peers scoring hasn't been built up,
    /// only records got `majority` of replicated in copies shall be trusted.
    /// This is the temp container to accumulate those intitial replicated in records.
    initial_replicates: HashMap<(NetworkAddress, ValidationType), HashSet<PeerId>>,
}

impl ReplicationFetcher {
    /// Instantiate a new replication fetcher with passed PeerId.
    pub(crate) fn new(self_peer_id: PeerId, event_sender: mpsc::Sender<NetworkEvent>) -> Self {
        Self {
            self_peer_id,
            to_be_fetched: HashMap::new(),
            on_going_fetches: HashMap::new(),
            event_sender,
            distance_range: None,
            farthest_acceptable_distance: None,
            peers_scores: HashMap::new(),
            initial_replicates: HashMap::new(),
        }
    }

    /// Set the distance range.
    pub(crate) fn set_replication_distance_range(&mut self, distance_range: Distance) {
        self.distance_range = Some(distance_range);
    }

    // Adds the non existing incoming keys from the peer to the fetcher.
    // Returns the next set of keys that has to be fetched from the peer/network.
    //
    // Note: for `fresh_replicate`, the verification is on payment and got undertaken by the caller
    //       Hence here it shall always be considered as valid to fetch.
    pub(crate) fn add_keys(
        &mut self,
        holder: PeerId,
        incoming_keys: Vec<(NetworkAddress, ValidationType)>,
        locally_stored_keys: &HashMap<RecordKey, (NetworkAddress, ValidationType, DataTypes)>,
        is_fresh_replicate: bool,
        closest_k_peers: Vec<NetworkAddress>,
    ) -> Vec<(PeerId, RecordKey)> {
        let candidates = if is_fresh_replicate {
            incoming_keys
                .into_iter()
                .map(|(addr, val_type)| (holder, addr, val_type))
                .collect()
        } else {
            self.valid_candidates(&holder, incoming_keys, locally_stored_keys, closest_k_peers)
        };

        // Remove any outdated entries in `to_be_fetched`
        self.remove_stored_keys(locally_stored_keys);
        self.to_be_fetched
            .retain(|_, time_out| *time_out > Instant::now());

        let mut keys_to_fetch = vec![];
        // add valid, in-range AND non existing keys to the fetcher
        candidates
            .into_iter()
            .for_each(|(peer_id, addr, record_type)| {
                if is_fresh_replicate {
                    // Fresh replicate shall always got prioritized.
                    let new_data_key = (addr.to_record_key(), record_type);
                    if let Entry::Vacant(entry) = self.on_going_fetches.entry(new_data_key) {
                        keys_to_fetch.push((holder, addr.to_record_key()));
                        let _ = entry.insert((holder, Instant::now() + FETCH_TIMEOUT));
                    }
                } else {
                    let _ = self
                        .to_be_fetched
                        .entry((addr.to_record_key(), record_type, peer_id))
                        .or_insert(Instant::now() + PENDING_TIMEOUT);
                }
            });

        keys_to_fetch.extend(self.next_keys_to_fetch());
        keys_to_fetch
    }

    // Node is full, any fetch (ongoing or new) shall no farther than the current farthest.
    pub(crate) fn set_farthest_on_full(&mut self, farthest_in: Option<RecordKey>) {
        let self_addr = NetworkAddress::from_peer(self.self_peer_id);

        let new_farthest_distance = if let Some(farthest_in) = farthest_in {
            let addr = NetworkAddress::from_record_key(&farthest_in);
            self_addr.distance(&addr)
        } else {
            return;
        };

        if let Some(old_farthest_distance) = self.farthest_acceptable_distance {
            if new_farthest_distance >= old_farthest_distance {
                return;
            }
        }

        // Remove any ongoing or pending fetches that is farther than the current farthest
        self.to_be_fetched.retain(|(key, _t, _), _| {
            let addr = NetworkAddress::from_record_key(key);
            self_addr.distance(&addr) <= new_farthest_distance
        });
        self.on_going_fetches.retain(|(key, _t), _| {
            let addr = NetworkAddress::from_record_key(key);
            self_addr.distance(&addr) <= new_farthest_distance
        });

        self.farthest_acceptable_distance = Some(new_farthest_distance);
    }

    // Notify the replication fetcher about a newly added Record to the node.
    // The corresponding key can now be removed from the replication fetcher.
    // Also returns the next set of keys that has to be fetched from the peer/network.
    pub(crate) fn notify_about_new_put(
        &mut self,
        new_put: RecordKey,
        record_type: ValidationType,
    ) -> Vec<(PeerId, RecordKey)> {
        self.to_be_fetched
            .retain(|(key, t, _), _| key != &new_put || t != &record_type);

        // if we're actively fetching for the key, reduce the on_going_fetches
        self.on_going_fetches.retain(|(key, _t), _| key != &new_put);

        self.next_keys_to_fetch()
    }

    // An early completion of a fetch means the target is an old version record
    pub(crate) fn notify_fetch_early_completed(
        &mut self,
        key_in: RecordKey,
        record_type: ValidationType,
    ) -> Vec<(PeerId, RecordKey)> {
        self.to_be_fetched.retain(|(key, current_type, _), _| {
            if current_type == &record_type {
                key != &key_in
            } else {
                true
            }
        });

        self.on_going_fetches.retain(|(key, current_type), _| {
            if current_type == &record_type {
                key != &key_in
            } else {
                true
            }
        });

        self.next_keys_to_fetch()
    }

    // Returns the set of keys that has to be fetched from the peer/network.
    // Target must not be under-fetching
    // and no more than MAX_PARALLEL_FETCH fetches to be undertaken at the same time.
    pub(crate) fn next_keys_to_fetch(&mut self) -> Vec<(PeerId, RecordKey)> {
        self.prune_expired_keys_and_slow_nodes();

        debug!("Next to fetch....");

        if self.on_going_fetches.len() >= MAX_PARALLEL_FETCH {
            warn!("Replication Fetcher doesn't have free fetch capacity. Currently has {} entries in queue.",
                self.to_be_fetched.len());
            return vec![];
        }

        // early return if nothing there
        if self.to_be_fetched.is_empty() {
            return vec![];
        }

        debug!(
            "Number of records still to be retrieved: {:?}",
            self.to_be_fetched.len()
        );

        // Pre-allocate vectors with known capacity
        let remaining_capacity = MAX_PARALLEL_FETCH - self.on_going_fetches.len();
        let mut data_to_fetch = Vec::with_capacity(remaining_capacity);

        // Sort to_be_fetched by key closeness to our PeerId
        let mut to_be_fetched_sorted: Vec<_> = self.to_be_fetched.iter_mut().collect();

        let self_address = NetworkAddress::from_peer(self.self_peer_id);

        to_be_fetched_sorted.sort_by(|((a, _, _), _), ((b, _, _), _)| {
            let a = NetworkAddress::from_record_key(a);
            let b = NetworkAddress::from_record_key(b);
            self_address.distance(&a).cmp(&self_address.distance(&b))
        });

        for ((key, t, holder), _) in to_be_fetched_sorted {
            // Already carried out expiration pruning above.
            // Hence here only need to check whether is ongoing fetching.
            // Also avoid fetching same record from different nodes.
            if self.on_going_fetches.len() < MAX_PARALLEL_FETCH
                && !self
                    .on_going_fetches
                    .contains_key(&(key.clone(), t.clone()))
            {
                data_to_fetch.push((*holder, key.clone(), t.clone()));
                let _ = self.on_going_fetches.insert(
                    (key.clone(), t.clone()),
                    (*holder, Instant::now() + FETCH_TIMEOUT),
                );
            }

            // break out the loop early if we can do no more now
            if self.on_going_fetches.len() >= MAX_PARALLEL_FETCH {
                break;
            }
        }

        let pretty_keys: Vec<_> = data_to_fetch
            .iter()
            .map(|(holder, key, t)| (*holder, PrettyPrintRecordKey::from(key), t.clone()))
            .collect();

        if !data_to_fetch.is_empty() {
            debug!(
                "Sending out replication request. Fetching {} keys {:?}",
                data_to_fetch.len(),
                pretty_keys
            );
        }

        data_to_fetch
            .iter()
            .map(|(holder, key, t)| {
                let entry_key = (key.clone(), t.clone(), *holder);
                let _ = self.to_be_fetched.remove(&entry_key);
                (*holder, key.clone())
            })
            .collect()
    }

    // Record peers' healthy status after the storage chanllenge.
    pub(crate) fn add_peer_scores(&mut self, scores: Vec<(PeerId, bool)>) {
        for (peer_id, is_healthy) in scores {
            let (peer_scores, last_seen) = self
                .peers_scores
                .entry(peer_id)
                .or_insert((VecDeque::new(), Instant::now()));
            peer_scores.push_back(is_healthy);
            if peer_scores.len() > 2 {
                let _ = peer_scores.pop_front();
            }
            *last_seen = Instant::now();
        }

        // Once got enough scoring knowledge, the `majority` approach shall no longer be used.
        if self.had_enough_scoring_knowledge() {
            self.initial_replicates.clear();
        }

        // Pruning to avoid infinite growing, only keep the recent 20.
        if self.peers_scores.len() > 20 {
            let mut oldest_peer = PeerId::random();
            let mut oldest_timestamp = Instant::now();
            for (peer_id, (_peer_scores, last_seen)) in self.peers_scores.iter() {
                if *last_seen < oldest_timestamp {
                    oldest_timestamp = *last_seen;
                    oldest_peer = *peer_id;
                }
            }
            let _ = self.peers_scores.remove(&oldest_peer);
        }
    }

    // Among the incoming keys, figure out those:
    //   * not already stored
    //   * not on pending
    //   * within the range
    //   * from valid source peer
    fn valid_candidates(
        &mut self,
        holder: &PeerId,
        incoming_keys: Vec<(NetworkAddress, ValidationType)>,
        locally_stored_keys: &HashMap<RecordKey, (NetworkAddress, ValidationType, DataTypes)>,
        closest_k_peers: Vec<NetworkAddress>,
    ) -> Vec<(PeerId, NetworkAddress, ValidationType)> {
        match self.is_peer_trustworthy(holder) {
            Some(true) => {
                debug!("Replication source {holder:?} is trustworthy.");
                let new_incoming_keys = self.in_range_new_keys(
                    holder,
                    incoming_keys,
                    locally_stored_keys,
                    closest_k_peers,
                );
                new_incoming_keys
                    .into_iter()
                    .map(|(addr, val_type)| (*holder, addr, val_type))
                    .collect()
            }
            Some(false) => {
                debug!("Replication source {holder:?} is not trustworthy.");
                vec![]
            }
            None => {
                debug!("Not having enough network knowledge, using majority scheme instead.");
                // Whenever we had enough scoring knowledge of peers,
                // we shall no longer use the `majority copies` approach.
                // This can prevent malicious neighbouring farming targeting existing nodes.
                if self.had_enough_scoring_knowledge() {
                    // The replication source is probably a `new peer`.
                    // Just wait for the scoring knowledge to be built up.
                    return vec![];
                }
                let new_incoming_keys = self.in_range_new_keys(
                    holder,
                    incoming_keys,
                    locally_stored_keys,
                    closest_k_peers,
                );
                self.initial_majority_replicates(holder, new_incoming_keys)
            }
        }
    }

    fn had_enough_scoring_knowledge(&self) -> bool {
        self.peers_scores
            .values()
            .filter(|(scores, _last_seen)| scores.len() > 1)
            .count()
            >= CLOSE_GROUP_SIZE
    }

    // Accumulates initial replicates when doesn't have enough knowledge of peers scores.
    // Returns with entries that reached majority copies.
    fn initial_majority_replicates(
        &mut self,
        holder: &PeerId,
        incoming_keys: Vec<(NetworkAddress, ValidationType)>,
    ) -> Vec<(PeerId, NetworkAddress, ValidationType)> {
        let mut majorities = vec![];
        for addr_val_type in incoming_keys {
            debug!(
                "adding record {:?} from holder {holder:?} into initial accumulator",
                addr_val_type.0
            );
            let peers = self
                .initial_replicates
                .entry(addr_val_type.clone())
                .or_default();
            let _ = peers.insert(*holder);
            if peers.len() >= CLOSE_GROUP_SIZE / 2 {
                majorities.push(addr_val_type);
            }
        }

        let mut result = vec![];
        for addr_val_type in majorities {
            debug!("Accumulated majorities: {:?}", addr_val_type.0);
            if let Some(peers) = self.initial_replicates.remove(&addr_val_type) {
                for peer in peers {
                    result.push((peer, addr_val_type.0.clone(), addr_val_type.1.clone()));
                }
            }
        }

        result
    }

    // Among the incoming keys, figure out those:
    //   * not already stored
    //   * not on pending
    //   * within the range
    fn in_range_new_keys(
        &mut self,
        holder: &PeerId,
        incoming_keys: Vec<(NetworkAddress, ValidationType)>,
        locally_stored_keys: &HashMap<RecordKey, (NetworkAddress, ValidationType, DataTypes)>,
        mut closest_k_peers: Vec<NetworkAddress>,
    ) -> Vec<(NetworkAddress, ValidationType)> {
        // Pre-calculate self_address since it's used multiple times
        let self_address = NetworkAddress::from_peer(self.self_peer_id);
        closest_k_peers.push(self_address.clone());
        let total_incoming_keys = incoming_keys.len();

        // Avoid multiple allocations by using with_capacity
        let mut new_incoming_keys = Vec::with_capacity(incoming_keys.len());
        let mut out_of_range_keys = Vec::new();

        // Single pass filtering instead of multiple retain() calls
        for (addr, record_type) in incoming_keys {
            let key = addr.to_record_key();

            // Skip if locally stored or already pending fetch
            if locally_stored_keys.contains_key(&key)
                || self
                    .to_be_fetched
                    .contains_key(&(key.clone(), record_type.clone(), *holder))
            {
                continue;
            }

            // Check distance constraints
            if let Some(farthest_distance) = self.farthest_acceptable_distance {
                if self_address.distance(&addr) > farthest_distance {
                    out_of_range_keys.push(addr);
                    continue;
                }
            }

            new_incoming_keys.push((addr, record_type));
        }

        // Filter out those out_of_range ones among the incoming_keys.
        if let Some(ref distance_range) = self.distance_range {
            new_incoming_keys.retain(|(addr, _record_type)| {
                let distance = &self_address.distance(addr);
                debug!(
                    "Distance to target {addr:?} is {distance:?}, against range {distance_range:?}"
                );
                let mut is_in_range = distance <= distance_range;
                // For middle-range records, they could be farther than distance_range,
                // but still supposed to be held by the closest group to us.
                if !is_in_range && distance.0 - distance_range.0 < distance_range.0 {
                    closest_k_peers.sort_by_key(|key| key.distance(addr));
                    let closest_group: HashSet<_> = closest_k_peers.iter().take(CLOSE_GROUP_SIZE).collect();
                    if closest_group.contains(&self_address) {
                        debug!("Record {addr:?} has a far distance but still among {CLOSE_GROUP_SIZE} closest within {} neighbourd.", closest_k_peers.len());
                        is_in_range = true;
                    }
                }
                if !is_in_range {
                    out_of_range_keys.push(addr.clone());
                }
                is_in_range
            });
        }

        if !out_of_range_keys.is_empty() && !new_incoming_keys.is_empty() {
            info!("Among {total_incoming_keys} incoming replications from {holder:?}, {} new records and {} out of range",
                new_incoming_keys.len(), out_of_range_keys.len());
        }

        new_incoming_keys
    }

    // Check whether the peer is a trustworthy replication source.
    //   * Some(true)  : peer is trustworthy
    //   * Some(false) : peer is not trustworthy
    //   * None        : not having enough know to tell
    fn is_peer_trustworthy(&self, _holder: &PeerId) -> Option<bool> {
        // TODO: re-enable once confirmed with upscaled test
        Some(true)
        // if let Some((scores, _last_seen)) = self.peers_scores.get(holder) {
        //     if scores.len() > 1 {
        //         let is_healthy = scores.iter().filter(|is_health| **is_health).count() >= 1;
        //         if !is_healthy {
        //             info!("Peer {holder:?} is not a trustworthy replication source, as bearing scores of {scores:?}");
        //         }
        //         Some(is_healthy)
        //     } else {
        //         None
        //     }
        // } else {
        //     None
        // }
    }

    // Just remove outdated entries in `on_going_fetch`, indicates a failure to fetch from network.
    // The node then considered to be in trouble and:
    //   1, the pending_entries from that node shall be removed from `to_be_fetched` list.
    //   2, firing event up to notify bad_nodes, hence trigger them to be removed from RT.
    fn prune_expired_keys_and_slow_nodes(&mut self) {
        let mut failed_fetches = vec![];

        self.on_going_fetches
            .retain(|(record_key, _), (peer_id, time_out)| {
                if *time_out < Instant::now() {
                    failed_fetches.push((record_key.clone(), *peer_id));
                    false
                } else {
                    true
                }
            });

        let mut failed_holders = BTreeMap::new();

        for (record_key, peer_id) in failed_fetches {
            debug!(
                "Replication_fetcher has outdated fetch of {:?} from {peer_id:?}",
                PrettyPrintRecordKey::from(&record_key)
            );
            let _ = failed_holders.insert(peer_id, record_key);
        }

        // now to clear any failed nodes from our lists.
        self.to_be_fetched
            .retain(|(_, _, holder), _| !failed_holders.contains_key(holder));

        // Such failed_hodlers (if any) shall be reported back and be excluded from RT.
        if !failed_holders.is_empty() {
            self.send_event(NetworkEvent::FailedToFetchHolders(failed_holders));
        }
    }

    /// Remove keys that we hold already and no longer need to be replicated.
    /// This checks the hash on GraphEntry to ensure we pull in divergent GraphEntry.
    fn remove_stored_keys(
        &mut self,
        existing_keys: &HashMap<RecordKey, (NetworkAddress, ValidationType, DataTypes)>,
    ) {
        self.to_be_fetched.retain(|(key, t, _), _| {
            if let Some((_addr, record_type, _data_type)) = existing_keys.get(key) {
                // check the address only against similar record types
                t != record_type
            } else {
                true
            }
        });
        self.on_going_fetches.retain(|(key, t), _| {
            if let Some((_addr, record_type, _data_type)) = existing_keys.get(key) {
                // check the address only against similar record types
                t != record_type
            } else {
                true
            }
        });
    }

    /// Sends an event after pushing it off thread so as to be non-blocking
    /// this is a wrapper around the `mpsc::Sender::send` call
    fn send_event(&self, event: NetworkEvent) {
        let event_sender = self.event_sender.clone();
        let capacity = event_sender.capacity();

        // push the event off thread so as to be non-blocking
        let _handle = spawn(async move {
            if capacity == 0 {
                warn!(
                    "NetworkEvent channel is full. Await capacity to send: {:?}",
                    event
                );
            }
            if let Err(error) = event_sender.send(event).await {
                error!("ReplicationFetcher failed to send event: {}", error);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{ReplicationFetcher, FETCH_TIMEOUT, MAX_PARALLEL_FETCH};
    use crate::CLOSE_GROUP_SIZE;
    use ant_protocol::{storage::ValidationType, NetworkAddress};
    use eyre::Result;
    use libp2p::{kad::RecordKey, PeerId};
    use std::{
        collections::{HashMap, HashSet},
        time::Duration,
    };
    use tokio::{sync::mpsc, time::sleep};

    #[tokio::test]
    async fn verify_max_parallel_fetches() -> Result<()> {
        //random peer_id
        let peer_id = PeerId::random();
        let (event_sender, _event_receiver) = mpsc::channel(4);
        let mut replication_fetcher = ReplicationFetcher::new(peer_id, event_sender);
        let locally_stored_keys = HashMap::new();

        let mut incoming_keys = Vec::new();
        (0..MAX_PARALLEL_FETCH * 2).for_each(|_| {
            let random_data: Vec<u8> = (0..50).map(|_| rand::random::<u8>()).collect();
            let key = NetworkAddress::from_record_key(&RecordKey::from(random_data));
            incoming_keys.push((key, ValidationType::Chunk));
        });

        let replication_src = PeerId::random();
        replication_fetcher.add_peer_scores(vec![(replication_src, true)]);
        replication_fetcher.add_peer_scores(vec![(replication_src, true)]);

        let keys_to_fetch = replication_fetcher.add_keys(
            replication_src,
            incoming_keys,
            &locally_stored_keys,
            false,
            vec![],
        );
        assert_eq!(keys_to_fetch.len(), MAX_PARALLEL_FETCH);

        let replication_src_1 = PeerId::random();
        replication_fetcher.add_peer_scores(vec![(replication_src_1, true)]);
        replication_fetcher.add_peer_scores(vec![(replication_src_1, true)]);
        // we should not fetch anymore keys
        let random_data: Vec<u8> = (0..50).map(|_| rand::random::<u8>()).collect();
        let key_1 = NetworkAddress::from_record_key(&RecordKey::from(random_data));
        let random_data: Vec<u8> = (0..50).map(|_| rand::random::<u8>()).collect();
        let key_2 = NetworkAddress::from_record_key(&RecordKey::from(random_data));
        let keys_to_fetch = replication_fetcher.add_keys(
            replication_src_1,
            vec![
                (key_1, ValidationType::Chunk),
                (key_2, ValidationType::Chunk),
            ],
            &locally_stored_keys,
            false,
            vec![],
        );
        assert!(keys_to_fetch.is_empty());

        // Fresh replication shall be fetched immediately
        let random_data: Vec<u8> = (0..50).map(|_| rand::random::<u8>()).collect();
        let key = NetworkAddress::from_record_key(&RecordKey::from(random_data));
        let keys_to_fetch = replication_fetcher.add_keys(
            replication_src,
            vec![(key, ValidationType::Chunk)],
            &locally_stored_keys,
            true,
            vec![],
        );
        assert_eq!(keys_to_fetch.len(), 1);

        sleep(FETCH_TIMEOUT + Duration::from_secs(1)).await;

        // all the previous fetches should have failed and fetching next batch...
        let keys_to_fetch = replication_fetcher.next_keys_to_fetch();
        // but as we've marked the previous fetches as failed, that node should be entirely removed from the list
        // leaving us with just _one_ peer left (but with two entries)
        assert_eq!(keys_to_fetch.len(), 2);
        let keys_to_fetch = replication_fetcher.next_keys_to_fetch();
        assert!(keys_to_fetch.is_empty());

        Ok(())
    }

    #[test]
    fn verify_in_range_check() {
        //random peer_id
        let peer_id = PeerId::random();
        let self_address = NetworkAddress::from_peer(peer_id);
        let (event_sender, _event_receiver) = mpsc::channel(4);
        let mut replication_fetcher = ReplicationFetcher::new(peer_id, event_sender);

        // Set distance range
        let distance_target = NetworkAddress::from_peer(PeerId::random());
        let distance_range = self_address.distance(&distance_target);
        replication_fetcher.set_replication_distance_range(distance_range);

        let mut closest_k_peers = vec![];
        (0..19).for_each(|_| {
            closest_k_peers.push(NetworkAddress::from_peer(PeerId::random()));
        });

        let mut incoming_keys = Vec::new();
        let mut in_range_keys = 0;
        let mut closest_k_peers_include_self = closest_k_peers.clone();
        closest_k_peers_include_self.push(self_address.clone());
        (0..100).for_each(|_| {
            let random_data: Vec<u8> = (0..50).map(|_| rand::random::<u8>()).collect();
            let key = NetworkAddress::from_record_key(&RecordKey::from(random_data));

            let distance = key.distance(&self_address);
            if distance <= distance_range {
                in_range_keys += 1;
            } else if distance.0 - distance_range.0 < distance_range.0 {
                closest_k_peers_include_self.sort_by_key(|addr| key.distance(addr));
                let closest_group: HashSet<_> = closest_k_peers_include_self
                    .iter()
                    .take(CLOSE_GROUP_SIZE)
                    .collect();
                if closest_group.contains(&self_address) {
                    in_range_keys += 1;
                }
            }

            incoming_keys.push((key, ValidationType::Chunk));
        });

        let replication_src = PeerId::random();
        replication_fetcher.add_peer_scores(vec![(replication_src, true)]);
        replication_fetcher.add_peer_scores(vec![(replication_src, true)]);

        let keys_to_fetch = replication_fetcher.add_keys(
            replication_src,
            incoming_keys,
            &Default::default(),
            false,
            closest_k_peers,
        );
        assert_eq!(
            keys_to_fetch.len(),
            replication_fetcher.on_going_fetches.len(),
            "keys to fetch and ongoing fetches should match"
        );
        assert_eq!(
            in_range_keys,
            keys_to_fetch.len() + replication_fetcher.to_be_fetched.len(),
            "all keys should be in range and in the fetcher"
        );
    }
}
