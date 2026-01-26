// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
#![allow(clippy::mutable_key_type)]

use crate::networking::interface::NetworkEvent;
use ant_protocol::{
    NetworkAddress, PrettyPrintRecordKey,
    constants::CLOSE_GROUP_SIZE,
    storage::{DataTypes, ValidationType},
};
use libp2p::{
    PeerId,
    kad::{KBucketDistance as Distance, RecordKey},
};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque, hash_map::Entry};
use std::time::Instant;
use tokio::spawn;
use tokio::{sync::mpsc, time::Duration};

#[cfg(feature = "open-metrics")]
use crate::networking::log_markers::Marker;
#[cfg(feature = "open-metrics")]
use crate::networking::metrics::NetworkMetricsRecorder;

// Max parallel fetches that can be undertaken at the same time.
const MAX_PARALLEL_FETCH: usize = 5;

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

/// Manages replication fetching with trust-based and majority-based validation.
///
/// # Trust Scoring System
///
/// The ReplicationFetcher uses a dual-approach strategy for validating replication sources:
///
/// ## 1. Trust-Based Validation (via `peers_scores`)
///
/// Peers are evaluated based on their StorageChallenge response history:
/// - Each peer maintains a history of up to 2 recent challenge results (bool: healthy/unhealthy)
/// - A peer is considered "trustworthy" if they have 2+ scores and at least 2 are healthy
/// - A peer is considered "untrustworthy" if they have 2+ scores but fewer than 2 healthy
/// - Peers without sufficient history are "unknown"
///
/// Trust scoring is populated by the StorageChallenge mechanism in node.rs:
/// - Challenges are sent periodically to verify peers actually store claimed data
/// - Response time and proof correctness determine if a peer passes (healthy) or fails
/// - Results are fed to `add_peer_scores()` to update the trust history
///
/// ## 2. Majority-Based Validation (via `initial_replicates`)
///
/// Provides Byzantine fault tolerance for unknown peers:
/// - Accumulates which peers report each (address, validation_type) pair
/// - Once CLOSE_GROUP_SIZE/2 different peers report the same key, it's accepted
/// - Prevents a single malicious peer from injecting arbitrary data
///
/// ## Parallel Operation
///
/// Both approaches run simultaneously for defense-in-depth:
/// - Trustworthy peers: Immediate acceptance + contribute to majority
/// - Untrustworthy peers: Rejected entirely (don't contribute to majority)
/// - Unknown peers: Only accepted via majority consensus
///
/// This ensures fast replication from proven peers while maintaining security
/// against new/malicious peers through distributed consensus.
pub(crate) struct ReplicationFetcher {
    self_peer_id: PeerId,
    /// Pending entries that to be fetched from the target peer.
    to_be_fetched: HashMap<(RecordKey, ValidationType, PeerId), ReplicationTimeout>,
    /// Avoid fetching same chunk from different nodes AND carry out too many parallel tasks.
    on_going_fetches: HashMap<(RecordKey, ValidationType), (PeerId, ReplicationTimeout)>,
    event_sender: mpsc::Sender<NetworkEvent>,
    /// Distance range that the incoming key shall be fetched
    distance_range: Option<Distance>,
    /// Restrict fetch range to closer than this value
    /// used when the node is full, but we still have "close" data coming in
    /// that is _not_ closer than our farthest max record
    farthest_acceptable_distance: Option<Distance>,
    /// Trust scores for peers based on StorageChallenge results.
    ///
    /// Maps PeerId → (recent_scores, last_seen_timestamp)
    /// - `recent_scores`: VecDeque of up to 2 booleans (true = healthy, false = unhealthy)
    /// - `last_seen_timestamp`: Used for pruning old entries (keeps most recent 20 peers)
    ///
    /// A peer is trustworthy if they have 2+ scores with at least 2 being healthy.
    /// This requires consistent good behavior over multiple challenge rounds.
    peers_scores: HashMap<PeerId, (VecDeque<bool>, Instant)>,
    /// Accumulator for majority-based replication validation.
    ///
    /// Maps (NetworkAddress, ValidationType) → Set of PeerIds that reported this key.
    /// When the set size reaches CLOSE_GROUP_SIZE/2, the key is accepted for replication.
    ///
    /// This provides Byzantine fault tolerance: a single malicious peer cannot
    /// inject arbitrary data without collusion from multiple peers.
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
        #[cfg(feature = "open-metrics")] metrics_recorder: Option<&NetworkMetricsRecorder>,
    ) -> Vec<(PeerId, RecordKey)> {
        let candidates = if is_fresh_replicate {
            incoming_keys
                .into_iter()
                .map(|(addr, val_type)| (holder, addr, val_type))
                .collect()
        } else {
            self.valid_candidates(
                &holder,
                incoming_keys,
                locally_stored_keys,
                closest_k_peers,
                #[cfg(feature = "open-metrics")]
                metrics_recorder,
            )
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
        let self_addr = NetworkAddress::from(self.self_peer_id);

        let new_farthest_distance = if let Some(farthest_in) = farthest_in {
            let addr = NetworkAddress::from(&farthest_in);
            self_addr.distance(&addr)
        } else {
            return;
        };

        if let Some(old_farthest_distance) = self.farthest_acceptable_distance
            && new_farthest_distance >= old_farthest_distance
        {
            return;
        }

        // Remove any ongoing or pending fetches that is farther than the current farthest
        self.to_be_fetched.retain(|(key, _t, _), _| {
            let addr = NetworkAddress::from(key);
            self_addr.distance(&addr) <= new_farthest_distance
        });
        self.on_going_fetches.retain(|(key, _t), _| {
            let addr = NetworkAddress::from(key);
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
            warn!(
                "Replication Fetcher doesn't have free fetch capacity. Currently has {} entries in queue.",
                self.to_be_fetched.len()
            );
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

        let self_address = NetworkAddress::from(self.self_peer_id);

        to_be_fetched_sorted.sort_by(|((a, _, _), _), ((b, _, _), _)| {
            let a = NetworkAddress::from(a);
            let b = NetworkAddress::from(b);
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

    /// Records peer health status from StorageChallenge results.
    ///
    /// # Parameters
    /// - `scores`: Vector of (PeerId, is_healthy) tuples from the latest StorageChallenge round
    ///   - `is_healthy = true`: Peer passed the challenge (correct proofs, acceptable response time)
    ///   - `is_healthy = false`: Peer failed the challenge (false proof, timeout, or too slow)
    ///
    /// # Behavior
    ///
    /// For each peer in the scores:
    /// 1. Adds the new score to their history (maintains last 2 scores only)
    /// 2. Updates their last_seen timestamp
    ///
    /// This creates a sliding window of recent behavior that determines trustworthiness:
    /// - 2 healthy scores → trustworthy (immediate replication acceptance)
    /// - <2 healthy scores → untrustworthy (replication rejected)
    /// - <2 total scores → unknown (use majority consensus)
    ///
    /// # Memory Management
    ///
    /// - Each peer's history is capped at 2 entries (most recent behavior matters)
    /// - Total tracked peers is capped at 20 (oldest by last_seen is pruned)
    /// - This ensures bounded memory usage even with many peers
    pub(crate) fn add_peer_scores(&mut self, scores: Vec<(PeerId, bool)>) {
        for (peer_id, is_healthy) in scores {
            let (peer_scores, last_seen) = self
                .peers_scores
                .entry(peer_id)
                .or_insert((VecDeque::new(), Instant::now()));
            peer_scores.push_back(is_healthy);
            // Keep only the last 2 scores - recent behavior is most relevant
            if peer_scores.len() > 2 {
                let _ = peer_scores.pop_front();
            }
            *last_seen = Instant::now();
        }

        // Note: The `majority` approach now works in parallel with trust-based scoring.
        // We no longer clear initial_replicates when scoring knowledge is sufficient.
        // This provides defense-in-depth: both approaches validate replication sources.

        // Pruning to avoid infinite growing, only keep the recent 20 peers.
        // This bounds memory usage while keeping the most recently seen peers.
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
    //
    // This function uses a dual-approach strategy for replication validation:
    //   1. Trust-based approach: If we have enough scoring history for a peer, trust/reject based on scores
    //   2. Majority approach: Always accumulate replication requests and accept when majority reached
    //
    // Both approaches work in parallel:
    //   - Trustworthy peers (Some(true)): Keys accepted immediately AND added to majority accumulator
    //   - Untrustworthy peers (Some(false)): Keys rejected entirely (not added to majority either)
    //   - Unknown peers (None): Keys added to majority accumulator only (wait for majority consensus)
    fn valid_candidates(
        &mut self,
        holder: &PeerId,
        incoming_keys: Vec<(NetworkAddress, ValidationType)>,
        locally_stored_keys: &HashMap<RecordKey, (NetworkAddress, ValidationType, DataTypes)>,
        closest_peers: Vec<NetworkAddress>,
        #[cfg(feature = "open-metrics")] metrics_recorder: Option<&NetworkMetricsRecorder>,
    ) -> Vec<(PeerId, NetworkAddress, ValidationType)> {
        let new_incoming_keys = self.in_range_new_keys(
            holder,
            incoming_keys,
            locally_stored_keys,
            closest_peers,
            #[cfg(feature = "open-metrics")]
            metrics_recorder,
        );

        match self.is_peer_trustworthy(holder) {
            Some(true) => {
                debug!("Replication source {holder:?} is trustworthy based on scoring history.");
                // Trustworthy peer: accept keys immediately AND add to majority accumulator
                // This allows the majority approach to continue building consensus even for
                // trustworthy peers, providing redundancy in the validation system.
                let mut result: Vec<(PeerId, NetworkAddress, ValidationType)> = new_incoming_keys
                    .iter()
                    .map(|(addr, val_type)| (*holder, addr.clone(), val_type.clone()))
                    .collect();

                // Also accumulate for majority - this maintains parallel operation of both approaches
                let majority_results =
                    self.initial_majority_replicates(holder, new_incoming_keys);
                for item in majority_results {
                    if !result.contains(&item) {
                        result.push(item);
                    }
                }
                result
            }
            Some(false) => {
                warn!("Replication source {holder:?} is not trustworthy based on scoring history.");
                // Untrustworthy peer: reject entirely - don't add to majority either
                // This prevents malicious peers from poisoning the majority consensus
                vec![]
            }
            None => {
                debug!(
                    "No scoring history for {holder:?}, using majority scheme for validation."
                );
                // Unknown peer: use majority approach only
                // Keys will be accepted once enough different peers report the same keys
                self.initial_majority_replicates(holder, new_incoming_keys)
            }
        }
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
        mut closest_peers: Vec<NetworkAddress>,
        #[cfg(feature = "open-metrics")] metrics_recorder: Option<&NetworkMetricsRecorder>,
    ) -> Vec<(NetworkAddress, ValidationType)> {
        // Pre-calculate self_address since it's used multiple times
        let self_address = NetworkAddress::from(self.self_peer_id);
        closest_peers.push(self_address.clone());
        let total_incoming_keys = incoming_keys.len();

        // Avoid multiple allocations by using with_capacity
        let mut new_incoming_keys = Vec::with_capacity(incoming_keys.len());
        let mut out_of_range_keys_count = 0;
        let mut locally_present_count = 0;
        let mut fetch_in_progress_count = 0;
        // Single pass filtering instead of multiple retain() calls
        for (addr, record_type) in incoming_keys {
            let key = addr.to_record_key();

            // Skip if locally stored or already pending fetch
            if locally_stored_keys.contains_key(&key) {
                locally_present_count += 1;
                continue;
            } else if self
                .to_be_fetched
                .contains_key(&(key.clone(), record_type.clone(), *holder))
            {
                fetch_in_progress_count += 1;
                continue;
            }

            new_incoming_keys.push((addr, record_type));
        }

        // Sort closest_peers by distance to self
        closest_peers.sort_by_key(|peer| self_address.distance(peer));

        // Get the distance to the farthest peer in closest_peers
        let farthest_peer_distance = closest_peers.last().map(|peer| self_address.distance(peer));

        // Filter out those out_of_range ones among the incoming_keys.
        // A key is in range if it's closer to self than the farthest peer in closest_peers
        if let Some(max_distance) = farthest_peer_distance {
            new_incoming_keys.retain(|(addr, _record_type)| {
                let distance = self_address.distance(addr);
                debug!(
                    "Distance to target {addr:?} is {distance:?}, max peer distance: {max_distance:?}"
                );
                let is_in_range = distance <= max_distance;
                if !is_in_range {
                    out_of_range_keys_count += 1;
                    debug!(
                        "Record {addr:?} is out of range (distance: {distance:?} > {max_distance:?})"
                    );
                } else {
                    debug!(
                        "Record {addr:?} is in range (distance: {distance:?} <= {max_distance:?})"
                    );
                }
                is_in_range
            });
        }

        let marker = Marker::IncomingReplicationKeysStats {
            holder: *holder,
            total_keys: total_incoming_keys,
            locally_present_keys: locally_present_count,
            fetch_in_progress_keys: fetch_in_progress_count,
            new_keys: new_incoming_keys.len(),
            out_of_range_keys: out_of_range_keys_count,
        };
        marker.log();

        // Record metrics for incoming replication keys
        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = metrics_recorder {
            metrics_recorder.record_from_marker(marker);
        }

        new_incoming_keys
    }
    // fn in_range_new_keys(
    //     &mut self,
    //     holder: &PeerId,
    //     incoming_keys: Vec<(NetworkAddress, ValidationType)>,
    //     locally_stored_keys: &HashMap<RecordKey, (NetworkAddress, ValidationType, DataTypes)>,
    //     mut closest_peers: Vec<NetworkAddress>,
    // ) -> Vec<(NetworkAddress, ValidationType)> {
    //     // Pre-calculate self_address since it's used multiple times
    //     let self_address = NetworkAddress::from(self.self_peer_id);
    //     closest_peers.push(self_address.clone());
    //     let total_incoming_keys = incoming_keys.len();

    //     // Avoid multiple allocations by using with_capacity
    //     let mut new_incoming_keys = Vec::with_capacity(incoming_keys.len());
    //     let mut out_of_range_keys = Vec::new();

    //     // Single pass filtering instead of multiple retain() calls
    //     for (addr, record_type) in incoming_keys {
    //         let key = addr.to_record_key();

    //         // Skip if locally stored or already pending fetch
    //         if locally_stored_keys.contains_key(&key)
    //             || self
    //                 .to_be_fetched
    //                 .contains_key(&(key.clone(), record_type.clone(), *holder))
    //         {
    //             continue;
    //         }

    //         // Check distance constraints
    //         if let Some(farthest_distance) = self.farthest_acceptable_distance
    //             && self_address.distance(&addr) > farthest_distance
    //         {
    //             out_of_range_keys.push(addr);
    //             continue;
    //         }

    //         new_incoming_keys.push((addr, record_type));
    //     }

    //     // Filter out those out_of_range ones among the incoming_keys.
    //     if let Some(ref distance_range) = self.distance_range {
    //         new_incoming_keys.retain(|(addr, _record_type)| {
    //             let distance = &self_address.distance(addr);
    //             debug!(
    //                 "Distance to target {addr:?} is {distance:?}, against range {distance_range:?}"
    //             );
    //             let mut is_in_range = distance <= distance_range;
    //             // For middle-range records, they could be farther than distance_range,
    //             // but still supposed to be held by the closest group to us.
    //             if !is_in_range && distance.0 - distance_range.0 < distance_range.0 {
    //                 closest_peers.sort_by_key(|key| key.distance(addr));
    //                 let closest_group: HashSet<_> = closest_peers.iter().take(CLOSE_GROUP_SIZE + 2).collect();
    //                 if closest_group.contains(&self_address) {
    //                     info!("Record {addr:?} has a far distance but still among {} closest within {} neighbourd.",
    //                         CLOSE_GROUP_SIZE + 2, closest_peers.len());
    //                     is_in_range = true;
    //                 }
    //             }
    //             if !is_in_range {
    //                 out_of_range_keys.push(addr.clone());
    //             }
    //             is_in_range
    //         });
    //     }

    //     if !out_of_range_keys.is_empty() || !new_incoming_keys.is_empty() {
    //         info!(
    //             "Among {total_incoming_keys} incoming replications from {holder:?}, {} new records and {} out of range",
    //             new_incoming_keys.len(),
    //             out_of_range_keys.len()
    //         );
    //     }

    //     new_incoming_keys
    // }

    /// Determines if a peer is a trustworthy replication source based on scoring history.
    ///
    /// # Returns
    /// - `Some(true)`: Peer has 2+ challenge scores with at least 2 healthy results → trustworthy
    /// - `Some(false)`: Peer has 2+ challenge scores but fewer than 2 healthy → untrustworthy
    /// - `None`: Peer has insufficient scoring history (0-1 scores) → cannot determine
    ///
    /// # Trust Criteria
    ///
    /// A peer must demonstrate consistent healthy behavior over multiple StorageChallenge
    /// rounds to be considered trustworthy. The requirement of "at least 2 healthy scores
    /// out of 2" ensures that:
    /// - A single good response isn't enough (could be luck)
    /// - Any recent failure drops the peer from trusted status
    /// - New peers start as "unknown" until they prove themselves
    ///
    /// # Usage in Replication
    ///
    /// - Trustworthy peers: Their replication requests are accepted immediately
    /// - Untrustworthy peers: Their replication requests are rejected entirely
    /// - Unknown peers: Replication requests go through majority consensus instead
    fn is_peer_trustworthy(&self, holder: &PeerId) -> Option<bool> {
        if let Some((scores, _last_seen)) = self.peers_scores.get(holder) {
            if scores.len() > 1 {
                // Require at least 2 healthy scores to be considered trustworthy
                let healthy_count = scores.iter().filter(|is_healthy| **is_healthy).count();
                let is_trustworthy = healthy_count > 1;
                if !is_trustworthy {
                    info!(
                        "Peer {holder:?} is not trustworthy: {healthy_count} healthy scores out of {} (scores: {scores:?})",
                        scores.len()
                    );
                }
                Some(is_trustworthy)
            } else {
                // Only 0-1 scores - insufficient history to determine trustworthiness
                None
            }
        } else {
            // No scoring history for this peer
            None
        }
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
    use super::{FETCH_TIMEOUT, MAX_PARALLEL_FETCH, ReplicationFetcher};
    use ant_protocol::{NetworkAddress, storage::ValidationType};
    use eyre::Result;
    use libp2p::{PeerId, kad::RecordKey};
    use std::{collections::HashMap, time::Duration};
    use tokio::{sync::mpsc, time::sleep};

    #[tokio::test]
    async fn verify_max_parallel_fetches() -> Result<()> {
        //random peer_id
        let peer_id = PeerId::random();
        let self_addr = NetworkAddress::from(peer_id);
        let (event_sender, _event_receiver) = mpsc::channel(4);
        let mut replication_fetcher = ReplicationFetcher::new(peer_id, event_sender);
        let locally_stored_keys = HashMap::new();

        // using single random peer to simulate the closest group
        let farthest_peer = NetworkAddress::from(PeerId::random());
        let distance_range = self_addr.distance(&farthest_peer);

        // Helper to generate a key within range
        let generate_in_range_key = |self_addr: &NetworkAddress, distance_range: &_| loop {
            let random_data: Vec<u8> = (0..50).map(|_| rand::random::<u8>()).collect();
            let candidate_key = NetworkAddress::from(&RecordKey::from(random_data));
            let distance = self_addr.distance(&candidate_key);
            if distance < *distance_range {
                break candidate_key;
            }
        };

        let mut incoming_keys = Vec::new();
        (0..MAX_PARALLEL_FETCH * 2).for_each(|_| {
            let key = generate_in_range_key(&self_addr, &distance_range);
            incoming_keys.push((key, ValidationType::Chunk));
        });

        // Pre-generate fixed peers for the initial keys.
        // When majority is reached, entries are created for BOTH peers (A and B).
        // The 5 keys selected for fetching will randomly pick either A or B for each key
        // (based on HashMap iteration order).
        let initial_peer_a = PeerId::random();
        let initial_peer_b = PeerId::random();

        // First iteration with peer A - count becomes 1 for each key
        let keys_to_fetch = replication_fetcher.add_keys(
            initial_peer_a,
            incoming_keys.clone(),
            &locally_stored_keys,
            false,
            vec![farthest_peer.clone()],
            #[cfg(feature = "open-metrics")]
            None,
        );
        assert!(
            keys_to_fetch.is_empty(),
            "First iteration should not trigger fetch (majority not reached)"
        );

        // Second iteration with peer B - count becomes 2, majority reached.
        // The returned keys_to_fetch tells us which peers were selected for the 5 fetched keys.
        let initial_keys_to_fetch = replication_fetcher.add_keys(
            initial_peer_b,
            incoming_keys.clone(),
            &locally_stored_keys,
            false,
            vec![farthest_peer.clone()],
            #[cfg(feature = "open-metrics")]
            None,
        );
        assert_eq!(
            initial_keys_to_fetch.len(),
            MAX_PARALLEL_FETCH,
            "Should fetch MAX_PARALLEL_FETCH keys after majority reached"
        );

        // Analyze which peers were selected for the 5 initial keys.
        // This determines what we expect after timeout cleanup.
        let has_peer_a = initial_keys_to_fetch
            .iter()
            .any(|(peer, _)| *peer == initial_peer_a);
        let has_peer_b = initial_keys_to_fetch
            .iter()
            .any(|(peer, _)| *peer == initial_peer_b);
        let both_initial_peers_selected = has_peer_a && has_peer_b;

        // Generate two new keys for testing capacity blocking
        let key_1 = generate_in_range_key(&self_addr, &distance_range);
        let key_2 = generate_in_range_key(&self_addr, &distance_range);

        // Use fixed, distinct peers for key_1/key_2 that are different from initial peers.
        // This ensures their entries won't be removed when initial peers fail.
        let key12_peer_a = PeerId::random();
        let key12_peer_b = PeerId::random();

        // Add key_1 and key_2 with exactly 2 peers to reach majority once
        // (avoids creating extra entries from additional majority rounds)
        let keys_to_fetch = replication_fetcher.add_keys(
            key12_peer_a,
            vec![
                (key_1.clone(), ValidationType::Chunk),
                (key_2.clone(), ValidationType::Chunk),
            ],
            &locally_stored_keys,
            false,
            vec![farthest_peer.clone()],
            #[cfg(feature = "open-metrics")]
            None,
        );
        assert!(
            keys_to_fetch.is_empty(),
            "Should not fetch - majority not reached yet"
        );

        let keys_to_fetch = replication_fetcher.add_keys(
            key12_peer_b,
            vec![
                (key_1.clone(), ValidationType::Chunk),
                (key_2.clone(), ValidationType::Chunk),
            ],
            &locally_stored_keys,
            false,
            vec![farthest_peer.clone()],
            #[cfg(feature = "open-metrics")]
            None,
        );
        // Still empty because on_going_fetches is at capacity (MAX_PARALLEL_FETCH)
        assert!(
            keys_to_fetch.is_empty(),
            "Should not fetch - on_going_fetches at capacity"
        );

        // Fresh replication shall be fetched immediately (bypasses capacity check)
        let fresh_key = generate_in_range_key(&self_addr, &distance_range);
        let fresh_peer = PeerId::random();
        let keys_to_fetch = replication_fetcher.add_keys(
            fresh_peer,
            vec![(fresh_key, ValidationType::Chunk)],
            &locally_stored_keys,
            true,
            vec![],
            #[cfg(feature = "open-metrics")]
            None,
        );
        assert_eq!(
            keys_to_fetch.len(),
            1,
            "Fresh replicate should bypass capacity limit"
        );

        sleep(FETCH_TIMEOUT + Duration::from_secs(1)).await;

        // After timeout, all on_going_fetches have expired and their peers become failed_holders.
        // The cleanup removes ALL entries from failed_holders in to_be_fetched.
        //
        // Expected results based on which initial peers were selected:
        // - If BOTH initial_peer_a AND initial_peer_b were selected (both are now failed_holders):
        //   All initial key entries are removed, only key_1/key_2 entries remain → expect 2 keys
        // - If only ONE initial peer was selected (only one is failed_holder):
        //   The other peer's entries for all 10 initial keys remain, plus key_1/key_2 → expect 5 keys
        //   (5 remaining initial keys from the non-failed peer, sorted by distance, up to MAX_PARALLEL_FETCH)
        let keys_to_fetch = replication_fetcher.next_keys_to_fetch();

        if both_initial_peers_selected {
            // Both initial peers failed, only key_1/key_2 entries remain
            assert_eq!(
                keys_to_fetch.len(),
                2,
                "Expected 2 keys (key_1 and key_2) when both initial peers are in failed_holders"
            );
        } else {
            // Only one initial peer failed, the other peer's entries remain
            // next_keys_to_fetch returns up to MAX_PARALLEL_FETCH keys
            assert_eq!(
                keys_to_fetch.len(),
                MAX_PARALLEL_FETCH,
                "Expected MAX_PARALLEL_FETCH keys when only one initial peer is in failed_holders"
            );
        }

        // Verify no more keys to fetch after this batch
        // (the entries are now in on_going_fetches, so next call returns empty)
        let more_keys = replication_fetcher.next_keys_to_fetch();
        assert!(
            more_keys.is_empty(),
            "Should have no more keys to fetch (all moved to on_going_fetches)"
        );

        Ok(())
    }

    #[test]
    fn verify_in_range_check() {
        //random peer_id
        let peer_id = PeerId::random();
        let self_address = NetworkAddress::from(peer_id);
        let (event_sender, _event_receiver) = mpsc::channel(4);
        let mut replication_fetcher = ReplicationFetcher::new(peer_id, event_sender);

        let mut closest_k_peers = vec![];
        (0..19).for_each(|_| {
            closest_k_peers.push(NetworkAddress::from(PeerId::random()));
        });

        // Sort closest_peers by distance to self
        closest_k_peers.sort_by_key(|peer| self_address.distance(peer));

        // Get the distance to the farthest peer in closest_peers
        let distance_range = closest_k_peers
            .last()
            .map(|peer| self_address.distance(peer))
            .unwrap();

        let mut incoming_keys = Vec::new();
        let mut in_range_keys = 0;
        let mut closest_k_peers_include_self = closest_k_peers.clone();
        closest_k_peers_include_self.push(self_address.clone());
        (0..100).for_each(|_| {
            let random_data: Vec<u8> = (0..50).map(|_| rand::random::<u8>()).collect();
            let key = NetworkAddress::from(&RecordKey::from(random_data));

            let distance = key.distance(&self_address);
            if distance <= distance_range {
                in_range_keys += 1;
            }

            incoming_keys.push((key, ValidationType::Chunk));
        });

        // let replication_src = PeerId::random();
        // replication_fetcher.add_peer_scores(vec![(replication_src, true)]);
        // replication_fetcher.add_peer_scores(vec![(replication_src, true)]);

        // Adding multiple times to trigger the majority replicas
        let mut keys_to_fetch = Vec::new();
        for i in 0..5 {
            let replication_src = PeerId::random();
            keys_to_fetch = replication_fetcher.add_keys(
                replication_src,
                incoming_keys.clone(),
                &Default::default(),
                false,
                closest_k_peers.clone(),
                #[cfg(feature = "open-metrics")]
                None,
            );
            if !keys_to_fetch.is_empty() {
                break;
            }
            if i > 3 {
                panic!("Majority replicas shall not be higher than {i}");
            }
        }
        assert_eq!(
            keys_to_fetch.len(),
            replication_fetcher.on_going_fetches.len(),
            "keys to fetch and ongoing fetches should match"
        );

        // replication_fetcher holds entries of `(key, peer_id)`, hence need a multiples here
        assert_eq!(
            in_range_keys * 2,
            replication_fetcher.on_going_fetches.len() + replication_fetcher.to_be_fetched.len(),
            "all keys to fetch should be in range"
        );
    }
}
