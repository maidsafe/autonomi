// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! # Network Replication Simulation
//!
//! This simulation tests the data replication behavior of the Autonomi Network.
//! It models:
//! - Kademlia routing tables with XOR distance
//! - Client upload with quote selection
//! - Node storage decisions
//! - Periodic replication between nodes
//!
//! The goal is to verify that data is properly replicated to the closest nodes
//! and that the network maintains good coverage (chunks stored by their close group).

use ant_protocol::{
    CLOSE_GROUP_SIZE, NetworkAddress,
    storage::{DataTypes, RecordKind},
};
use autonomi::ChunkAddress;
use libp2p::{
    PeerId,
    kad::{KBucketDistance as Distance, U256},
};
use rand::seq::index::sample;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use xor_name::XorName;

// ============================================================================
// Global Counters
// ============================================================================

// Global counters for tracking replication candidate selection paths
static REPLICATION_PEERS_WITHIN_RESPONSIBLE_RANGE: AtomicUsize = AtomicUsize::new(0);
static REPLICATION_PEERS_FALLBACK_TO_CLOSEST_K: AtomicUsize = AtomicUsize::new(0);

// Global counters for tracking is_in_range decision paths during replication
// Simplified: production uses farthest peer distance comparison, no middle-range fallback
static REPLICATION_IN_RANGE: AtomicUsize = AtomicUsize::new(0);
static REPLICATION_OUT_OF_RANGE: AtomicUsize = AtomicUsize::new(0);

// Global counters for tracking payment validation paths during upload
static UPLOAD_PAYMENT_ALL_PAYEES_IN_K_CLOSEST: AtomicUsize = AtomicUsize::new(0);
static UPLOAD_PAYMENT_VALIDATED_VIA_RESPONSIBLE_RANGE: AtomicUsize = AtomicUsize::new(0);
static UPLOAD_PAYMENT_NO_RESPONSIBLE_RANGE_TRUSTED: AtomicUsize = AtomicUsize::new(0);

// Global counters for tracking replication message acceptance/rejection
static REPLICATION_MSG_ACCEPTED_FROM_K_CLOSEST: AtomicUsize = AtomicUsize::new(0);
static REPLICATION_MSG_REJECTED_NOT_IN_K_CLOSEST: AtomicUsize = AtomicUsize::new(0);

// Global counters for tracking majority accumulation (production requires 2+ peers reporting same record)
static REPLICATION_MAJORITY_REACHED: AtomicUsize = AtomicUsize::new(0);
static REPLICATION_PENDING_MAJORITY: AtomicUsize = AtomicUsize::new(0);

// ============================================================================
// Constants (from codebase)
// ============================================================================

const LIBP2P_K_VALUE: usize = 20; // Max peers per Kademlia bucket (from rust-libp2p)
const MAX_RECORDS_COUNT: usize = 16384;
const REPLICATION_MAJORITY_THRESHOLD: usize = CLOSE_GROUP_SIZE / 2; // 2 peers required (production uses 3+ but we use 2+ for simulation)

// ============================================================================
// Simulation Configuration
// ============================================================================

const SIMULATION_NUM_NODES: usize = 1000;
const SIMULATION_NUM_CHUNKS: usize = 1000;
const SIMULATION_REPLICATION_ROUNDS: usize = 10;
const SIMULATION_PAYMENT_MODE: PaymentMode = PaymentMode::Standard;

// ============================================================================
// Core Data Structures
// ============================================================================

/// A simulated node in the network
#[derive(Debug)]
struct SimulatedNode {
    peer_id: PeerId,
    address: NetworkAddress,

    // Local routing table: ilog2_distance -> list of peers
    routing_table: BTreeMap<u32, Vec<PeerId>>,

    // Stored records
    stored_records: HashMap<NetworkAddress, StoredRecord>,

    // Network state
    responsible_distance: Option<Distance>, // Distance range this node is responsible for
    network_size_estimate: usize,

    // Quote generation factors
    received_payment_count: u64,
    live_time: Duration,
}

#[derive(Debug, Clone)]
struct StoredRecord {
    address: NetworkAddress,
    record_kind: RecordKind,
    data_size: usize,
    payment: Option<SimulatedPayment>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SimulatedPayment {
    payees: Vec<PeerId>,
    data_type: DataTypes,
}

#[derive(Debug)]
#[allow(dead_code)]
enum PaymentMode {
    SingleNode, // Pay 1 node (index 2) with 3x amount
    Standard,   // Pay 3 nodes (indices 2, 3, 4) with quoted amounts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreResult {
    Stored,
    PaymentNotForUs,
    PayeesOutOfRange,
    DistanceTooFar,
}

// ============================================================================
// JSON Export Structures
// ============================================================================

#[derive(Serialize)]
struct SimulationExport {
    config: SimulationConfig,
    rounds: Vec<RoundData>,
    final_coverage: CoverageExport,
    chunk_timelines: Vec<ChunkTimeline>,
}

#[derive(Serialize)]
struct SimulationConfig {
    num_nodes: usize,
    num_chunks: usize,
    replication_rounds: usize,
    close_group_size: usize,
    k_value: usize,
    majority_threshold: usize,
}

#[derive(Serialize)]
struct RoundData {
    round_number: usize,
    total_records: usize,
    replications: usize,
    avg_records_per_node: f64,
}

#[derive(Serialize)]
struct CoverageExport {
    total_chunks: usize,
    distribution: [usize; 8], // 0/7 through 7/7
    average_percent: f64,
}

#[derive(Serialize)]
struct ChunkTimeline {
    address: String,
    holders_per_round: Vec<usize>,
    final_coverage: usize,
}

/// Accumulator for tracking replication sources - requires majority consensus
/// Production requires 3+ peers (CLOSE_GROUP_SIZE / 2) reporting same record before accepting
use std::collections::HashSet;

struct ReplicationAccumulator {
    pending: HashMap<NetworkAddress, HashSet<PeerId>>,
}

impl ReplicationAccumulator {
    fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Add a peer report for a record address and check if majority threshold is reached
    fn add_and_check_majority(&mut self, addr: &NetworkAddress, peer: PeerId) -> bool {
        let peers = self.pending.entry(addr.clone()).or_default();
        peers.insert(peer);
        peers.len() >= REPLICATION_MAJORITY_THRESHOLD
    }

    /// Take record if majority threshold was reached, removing it from pending
    #[allow(dead_code)]
    fn take_if_majority(&mut self, addr: &NetworkAddress) -> Option<HashSet<PeerId>> {
        if self
            .pending
            .get(addr)
            .map(|p| p.len())
            .unwrap_or(0)
            >= REPLICATION_MAJORITY_THRESHOLD
        {
            self.pending.remove(addr)
        } else {
            None
        }
    }

    /// Get count of pending records that haven't reached majority
    fn pending_count(&self) -> usize {
        self.pending
            .values()
            .filter(|peers| peers.len() < REPLICATION_MAJORITY_THRESHOLD)
            .count()
    }
}

// ============================================================================
// Node Implementation
// ============================================================================

impl SimulatedNode {
    fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            address: NetworkAddress::from(peer_id),
            routing_table: BTreeMap::new(),
            stored_records: HashMap::new(),
            responsible_distance: None,
            network_size_estimate: 0, // Will be calculated after routing table is built
            received_payment_count: 0,
            live_time: Duration::from_secs(0),
        }
    }

    /// Estimate network size based on routing table (from driver/event/mod.rs::estimate_network_size)
    fn estimate_network_size(&mut self) {
        let (peers_in_non_full_buckets, num_of_full_buckets) =
            self.routing_table
                .values()
                .fold((0, 0), |(peers, full_buckets), bucket| {
                    if bucket.len() >= LIBP2P_K_VALUE {
                        (peers, full_buckets + 1)
                    } else {
                        (peers + bucket.len(), full_buckets)
                    }
                });

        self.network_size_estimate =
            (peers_in_non_full_buckets + 1) * 2_usize.pow(num_of_full_buckets as u32);
    }

    /// Get all peers from routing table
    fn all_routing_table_peers(&self, include_self: bool) -> Vec<PeerId> {
        let iter = self
            .routing_table
            .values()
            .flat_map(|peers| peers.iter())
            .filter(|&&peer| peer != self.peer_id);

        if include_self {
            std::iter::once(self.peer_id).chain(iter.copied()).collect()
        } else {
            iter.copied().collect()
        }
    }

    /// Find closest peers from local routing table
    fn find_closest_local(
        &self,
        target: &NetworkAddress,
        k: usize,
        include_self: bool,
    ) -> Vec<PeerId> {
        let mut peers: Vec<_> = self
            .all_routing_table_peers(include_self)
            .into_iter()
            .map(|peer| {
                let addr = NetworkAddress::from(peer);
                (peer, target.distance(&addr))
            })
            .collect();

        peers.sort_by_key(|(_, dist)| *dist);
        peers.into_iter().take(k).map(|(peer, _)| peer).collect()
    }

    /// Calculate responsible distance range (from driver/mod.rs, within set_farthest_record_interval.tick() )
    /// Uses only peers from the local routing table (realistic simulation)
    fn calculate_responsible_distance(&mut self) {
        // Early exit if not enough peers
        if self.network_size_estimate <= CLOSE_GROUP_SIZE {
            return;
        }

        // The entire Distance space is U256
        // (U256::MAX is 115792089237316195423570985008687907853269984665640564039457584007913129639935)
        // The network density (average distance among nodes) can be estimated as:
        //     network_density = entire_U256_space / estimated_network_size
        let density = U256::MAX / U256::from(self.network_size_estimate);
        let density_distance = density * U256::from(CLOSE_GROUP_SIZE);

        // Get closest peers to self from routing table
        let closest_k_peers = self.find_closest_local(
            &self.address.clone(),
            LIBP2P_K_VALUE,
            true, // as get_closest_k_local_peers_to_self includes self
        );

        // Use distance to close peer to avoid the situation that
        // the estimated density_distance is too narrow.
        if closest_k_peers.len() <= CLOSE_GROUP_SIZE + 2 {
            return;
        }

        let self_addr = NetworkAddress::from(self.peer_id);
        let close_peers_distance =
            self_addr.distance(&NetworkAddress::from(closest_k_peers[CLOSE_GROUP_SIZE + 1]));

        // Take the maximum of both approaches
        let distance = std::cmp::max(Distance(density_distance), close_peers_distance);

        self.responsible_distance = Some(distance);
    }

    /// Generate a quote for storing data (from record_store.rs::quoting_metrics)
    fn generate_quote(&self, data_size: usize) -> u64 {
        let records_stored = self.stored_records.len();

        // Get close_records_stored based on responsible_distance
        let close_records_stored = if let Some(resp_dist) = self.responsible_distance {
            self.stored_records
                .keys()
                .filter(|addr| self.address.distance(addr) <= resp_dist)
                .count()
        } else {
            records_stored
        };

        // QuotingMetrics calculation (simplified version of actual pricing)
        // Base factors:
        // 1. Storage utilization (close_records_stored / max_records)
        // 2. Payment history
        // 3. Data size
        // 4. Live time

        let utilization_ratio = close_records_stored as f64 / MAX_RECORDS_COUNT as f64;
        let base_cost = 1000u64; // Base cost in wei or smallest unit

        // Exponential pricing based on storage utilization
        let utilization_multiplier = if utilization_ratio < 0.5 {
            1.0
        } else if utilization_ratio < 0.75 {
            2.0
        } else if utilization_ratio < 0.9 {
            5.0
        } else {
            10.0
        };

        // Payment history increases price
        let payment_multiplier = 1.0 + (self.received_payment_count as f64 * 0.1);

        // Data size factor
        let size_cost = (data_size as f64 / 1024.0 / 1024.0) * 100.0; // Per MB

        // Live time slightly decreases price (rewards long-running nodes)
        let live_time_discount = if self.live_time.as_secs() > 3600 {
            0.9 // 10% discount for nodes running > 1 hour
        } else {
            1.0
        };

        let final_cost = (base_cost as f64
            + size_cost * utilization_multiplier * payment_multiplier * live_time_discount)
            as u64;

        final_cost.max(base_cost)
    }

    /// Check if this node should store a record based on distance
    fn should_store(&self, record_addr: &NetworkAddress) -> bool {
        // If under capacity, accept records within responsible distance
        if self.stored_records.len() < MAX_RECORDS_COUNT {
            if let Some(resp_dist) = self.responsible_distance {
                return self.address.distance(record_addr) <= resp_dist;
            }
            return true;
        }

        // If at capacity, only accept if closer than farthest current record
        if let Some(farthest_dist) = self.get_farthest_record_distance() {
            return self.address.distance(record_addr) < farthest_dist;
        }

        false
    }

    /// Get distance to farthest stored record
    fn get_farthest_record_distance(&self) -> Option<Distance> {
        self.stored_records
            .keys()
            .map(|addr| self.address.distance(addr))
            .max()
    }

    /// Validate payment according to production logic (from put_validation.rs::payment_for_us_exists_and_is_still_valid)
    /// Verifies that payees are within acceptable range (K closest or within network_density)
    /// Returns Ok(()) if valid, Err(reason) if invalid
    fn validate_payment(
        &self,
        record_addr: &NetworkAddress,
        payment: &SimulatedPayment,
    ) -> Result<(), StoreResult> {
        // Check 1: Verify we (this node) are in the payee list
        if !payment.payees.contains(&self.peer_id) {
            return Err(StoreResult::PaymentNotForUs);
        }

        // Check 2: Get K_VALUE (20) closest peers from our routing table to the data address
        // Production uses get_closest_local_peers_to_target which excludes self
        let closest_k = self.find_closest_local(
            record_addr,
            LIBP2P_K_VALUE,
            false, // Production excludes self
        );

        // Check 3: Filter payees that are NOT in our K closest peers
        let mut out_of_k_payees: Vec<PeerId> = payment
            .payees
            .iter()
            .filter(|p| !closest_k.contains(p))
            .cloned()
            .collect();

        // Check 4: If all payees are in K closest, accept (fast path)
        if out_of_k_payees.is_empty() {
            UPLOAD_PAYMENT_ALL_PAYEES_IN_K_CLOSEST.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        // Check 5: Check if out-of-K payees are within network_density distance
        // (from put_validation.rs::payment_for_us_exists_and_is_still_valid, network density validation)
        if let Some(resp_dist) = self.responsible_distance {
            out_of_k_payees.retain(|peer_id| {
                let peer_addr = NetworkAddress::from(*peer_id);
                // Keep only those BEYOND network_density (these are out of range)
                record_addr.distance(&peer_addr) > resp_dist
            });

            // Check 6: If any payees are still out of range, reject
            if !out_of_k_payees.is_empty() {
                return Err(StoreResult::PayeesOutOfRange);
            }

            // Track: validated via responsible range check
            UPLOAD_PAYMENT_VALIDATED_VIA_RESPONSIBLE_RANGE.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        // If no network_density estimate, trust the payment
        // (from put_validation.rs::payment_for_us_exists_and_is_still_valid, fallback behavior)
        UPLOAD_PAYMENT_NO_RESPONSIBLE_RANGE_TRUSTED.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Store a record, returns detailed result about success or failure reason
    fn store_record(&mut self, record: StoredRecord) -> StoreResult {
        // First validate payment if present
        if let Some(ref payment) = record.payment {
            if let Err(reason) = self.validate_payment(&record.address, payment) {
                // Payment validation failed
                return reason;
            }
            // Payment is valid, track it
            self.received_payment_count += 1;
        }

        // Then check distance-based storage
        if self.should_store(&record.address) {
            self.stored_records.insert(record.address.clone(), record);
            return StoreResult::Stored;
        }
        StoreResult::DistanceTooFar
    }

    /// Check if record is in range for replication (from replication_fetcher.rs::in_range_new_keys)
    /// Production logic: Simple comparison against farthest peer distance - NO middle-range fallback
    fn is_in_range(&self, record_addr: &NetworkAddress, closest_peers: &[PeerId]) -> bool {
        let self_address = &self.address;

        // Build list with self included, sort by distance to self
        let mut peers_with_self: Vec<_> = closest_peers.to_vec();
        peers_with_self.push(self.peer_id);
        peers_with_self.sort_by_key(|peer| self_address.distance(&NetworkAddress::from(*peer)));

        // Get distance to farthest peer
        let farthest_distance = peers_with_self
            .last()
            .map(|peer| self_address.distance(&NetworkAddress::from(*peer)));

        // Accept if record within farthest peer distance
        if let Some(max_distance) = farthest_distance {
            let record_distance = self_address.distance(record_addr);
            let is_in_range = record_distance <= max_distance;

            if is_in_range {
                REPLICATION_IN_RANGE.fetch_add(1, Ordering::Relaxed);
            } else {
                REPLICATION_OUT_OF_RANGE.fetch_add(1, Ordering::Relaxed);
            }

            is_in_range
        } else {
            false
        }
    }

    /// Get replicate candidates with responsible_distance filtering (from cmd.rs::get_replicate_candidates)
    /// For periodic replication, targets CLOSE_GROUP_SIZE * 2 (10) closest peers
    /// For fresh replication, targets CLOSE_GROUP_SIZE (5) closest peers
    fn get_replicate_candidates(&self, target: &NetworkAddress, is_periodic: bool) -> Vec<PeerId> {
        let expected_candidates = if is_periodic {
            CLOSE_GROUP_SIZE * 2
        } else {
            CLOSE_GROUP_SIZE
        };

        // Get closest peers from routing table
        let closest_k_peers = self.find_closest_local(
            target,
            LIBP2P_K_VALUE,
            false, // does not include self
        );

        // Try to filter by responsible_distance range
        if let Some(responsible_range) = self.responsible_distance {
            let peers_in_range: Vec<_> = closest_k_peers
                .iter()
                .filter(|peer_id| {
                    let peer_addr = NetworkAddress::from(**peer_id);
                    target.distance(&peer_addr) <= responsible_range
                })
                .copied()
                .collect();

            if peers_in_range.len() >= expected_candidates {
                // Track that we used peers within responsible range
                REPLICATION_PEERS_WITHIN_RESPONSIBLE_RANGE.fetch_add(1, Ordering::Relaxed);
                return peers_in_range;
            }
        } else {
            tracing::error!("Node {} has no responsible distance set!", self.peer_id);
        }

        // Fall back to at least expected_candidates peers if range is too narrow
        // Track that we used the fallback path
        REPLICATION_PEERS_FALLBACK_TO_CLOSEST_K.fetch_add(1, Ordering::Relaxed);
        closest_k_peers
            .into_iter()
            .take(expected_candidates)
            .collect()
    }
}

// ============================================================================
// Main Simulation Test
// ============================================================================

#[test]
fn test_replication_simulation() {
    println!("\n=== Network Replication Simulation ===\n");

    // Configuration
    let num_nodes = SIMULATION_NUM_NODES;
    let num_chunks = SIMULATION_NUM_CHUNKS;
    let replication_rounds = SIMULATION_REPLICATION_ROUNDS;
    let payment_mode = SIMULATION_PAYMENT_MODE;

    // ========================================================================
    // Phase 1: Setup Network
    // ========================================================================
    println!("Phase 1: Setting up network with {num_nodes} nodes...");
    let phase1_start = std::time::Instant::now();

    // Create nodes in parallel
    let create_start = std::time::Instant::now();
    let mut nodes: HashMap<PeerId, SimulatedNode> = (0..num_nodes)
        .into_par_iter()
        .map(|_| {
            let peer_id = PeerId::random();
            let node = SimulatedNode::new(peer_id);
            (peer_id, node)
        })
        .collect();

    // Build routing tables in parallel - each node adds other nodes
    println!(
        "  ✓ Created {} nodes - {:.2}s",
        nodes.len(),
        create_start.elapsed().as_secs_f64()
    );

    println!("Building routing tables...");
    let routing_start = std::time::Instant::now();
    let all_peer_ids: Vec<_> = nodes.keys().copied().collect();

    nodes.par_iter_mut().for_each(|(_, node)| {
        // Pre-group peers by bucket distance to avoid redundant operations
        let mut bucket_candidates: BTreeMap<u32, Vec<PeerId>> = BTreeMap::new();

        for other_peer in &all_peer_ids {
            if *other_peer == node.peer_id {
                continue;
            }
            let other_addr = NetworkAddress::from(*other_peer);
            let bucket_index = node.address.distance(&other_addr).ilog2().unwrap_or(0);
            bucket_candidates
                .entry(bucket_index)
                .or_default()
                .push(*other_peer);
        }

        // Add up to LIBP2P_K_VALUE randomly selected peers from each bucket
        let mut rng = rand::thread_rng();
        for (bucket_index, peers) in bucket_candidates {
            let bucket = node.routing_table.entry(bucket_index).or_default();
            let k = LIBP2P_K_VALUE.min(peers.len());
            let random_indices = sample(&mut rng, peers.len(), k);
            for idx in random_indices {
                bucket.push(peers[idx]);
            }
        }
    });
    println!(
        "  ✓ Built routing tables (max {LIBP2P_K_VALUE} peers per bucket) - {:.2}s",
        routing_start.elapsed().as_secs_f64()
    );

    // Estimate network size based on routing tables
    println!("Estimating network size from routing tables...");
    let estimation_start = std::time::Instant::now();
    nodes.par_iter_mut().for_each(|(_, node)| {
        node.estimate_network_size();
    });

    // Calculate and print average network size estimate
    let total_estimate: usize = nodes.values().map(|n| n.network_size_estimate).sum();
    let avg_estimate = total_estimate as f64 / nodes.len() as f64;
    let min_estimate = nodes
        .values()
        .map(|n| n.network_size_estimate)
        .min()
        .unwrap_or(0);
    let max_estimate = nodes
        .values()
        .map(|n| n.network_size_estimate)
        .max()
        .unwrap_or(0);
    println!(
        "  ✓ Network size estimates: avg {avg_estimate:.0}, min {min_estimate}, max {max_estimate} (actual: {num_nodes}) - {:.2}s",
        estimation_start.elapsed().as_secs_f64()
    );

    // Calculate responsible distances in parallel
    println!("Calculating responsible distances...");
    let resp_dist_start = std::time::Instant::now();
    nodes.par_iter_mut().for_each(|(_, node)| {
        node.calculate_responsible_distance();
    });
    println!(
        "  ✓ Calculated responsible distances - {:.2}s",
        resp_dist_start.elapsed().as_secs_f64()
    );
    println!(
        "Phase 1 complete - {:.2}s\n",
        phase1_start.elapsed().as_secs_f64()
    );

    // ========================================================================
    // Phase 2: Client Upload
    // ========================================================================
    println!("Phase 2: Uploading {num_chunks} chunks with {payment_mode:?} payment mode...");
    let phase2_start = std::time::Instant::now();

    let chunk_addresses = Mutex::new(Vec::new());
    let upload_stats = Mutex::new(UploadStats::new());
    let nodes = Mutex::new(nodes);
    let uploaded_count = AtomicUsize::new(0);

    (0..num_chunks).into_par_iter().for_each(|_| {
        // Generate random chunk
        let chunk_addr = NetworkAddress::ChunkAddress(ChunkAddress::new(XorName::random(
            &mut rand::thread_rng(),
        )));
        let chunk_size = 1024 * 1024; // 1MB

        // Add to chunk_addresses (thread-safe)
        chunk_addresses.lock().unwrap().push(chunk_addr.clone());

        // Find 5 closest nodes globally (perfect client)
        let mut closest_5: Vec<_> = all_peer_ids
            .iter()
            .map(|peer| {
                let addr = NetworkAddress::from(*peer);
                (*peer, chunk_addr.distance(&addr))
            })
            .collect();
        closest_5.sort_by_key(|(_, dist)| *dist);
        let closest_5: Vec<_> = closest_5.into_iter().take(CLOSE_GROUP_SIZE).collect();

        // Get quotes from all 5 (needs read access to nodes)
        let mut quotes: Vec<_> = {
            let nodes_guard = nodes.lock().unwrap();
            closest_5
                .iter()
                .map(|(peer, _)| (*peer, nodes_guard[peer].generate_quote(chunk_size)))
                .collect()
        };
        quotes.sort_by_key(|(_, price)| *price);

        // Select payment targets based on mode
        let _paid_nodes: Vec<PeerId> = match payment_mode {
            PaymentMode::SingleNode => {
                // Pay node at index 2 (median) with 3x amount
                vec![quotes[2].0]
            }
            PaymentMode::Standard => {
                // Pay nodes at indices 2, 3, 4
                vec![quotes[2].0, quotes[3].0, quotes[4].0]
            }
        };

        // Create payment object that will be sent to all 5 nodes
        let payment = SimulatedPayment {
            payees: quotes.iter().map(|(peer, _)| *peer).collect(), // all nodes will get the payment, but the others will get 0 amount
            data_type: DataTypes::Chunk,
        };

        // Send to all 5 nodes (they decide whether to store)
        let mut stored_count = 0;
        let mut payment_not_for_us = 0;
        let mut payees_out_of_range = 0;
        let mut distance_too_far = 0;

        {
            let mut nodes_guard = nodes.lock().unwrap();
            for (peer_id, _) in &closest_5 {
                // Each node receives the payment with full payee list
                // They will validate if they're in the payee list and if payees are in range
                let record = StoredRecord {
                    address: chunk_addr.clone(),
                    record_kind: RecordKind::DataOnly(DataTypes::Chunk),
                    data_size: chunk_size,
                    payment: Some(payment.clone()),
                };

                match nodes_guard.get_mut(peer_id).unwrap().store_record(record) {
                    StoreResult::Stored => stored_count += 1,
                    StoreResult::PaymentNotForUs => payment_not_for_us += 1,
                    StoreResult::PayeesOutOfRange => payees_out_of_range += 1,
                    StoreResult::DistanceTooFar => distance_too_far += 1,
                }
            }
        }

        upload_stats.lock().unwrap().add_upload(
            stored_count,
            payment_not_for_us,
            payees_out_of_range,
            distance_too_far,
        );

        let count = uploaded_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_multiple_of(100) {
            println!("  Uploaded {count} chunks...");
        }
    });

    // Unwrap from Mutex
    let chunk_addresses = chunk_addresses.into_inner().unwrap();
    let upload_stats = upload_stats.into_inner().unwrap();
    let mut nodes = nodes.into_inner().unwrap();

    let total_records_after_upload: usize = nodes.values().map(|n| n.stored_records.len()).sum();
    let avg_records_after_upload = total_records_after_upload as f64 / nodes.len() as f64;
    let min_records = nodes
        .values()
        .map(|n| n.stored_records.len())
        .min()
        .unwrap_or(0);
    let max_records = nodes
        .values()
        .map(|n| n.stored_records.len())
        .max()
        .unwrap_or(0);

    println!("  ✓ Uploaded {num_chunks} chunks");
    println!(
        "Phase 2 complete - {:.2}s\n",
        phase2_start.elapsed().as_secs_f64()
    );

    // ========================================================================
    // Phase 2.5: Fresh Replication (immediate replication after upload)
    // ========================================================================
    // DISABLED: Client changed to upload to ALL payees, hence no longer need this.
    // May need again once client change back to upload to just one to save traffic.
    // This matches the actual node behavior where fresh replication is commented out.
    // See: ant-node/src/replication.rs (fresh replication logic) and ant-node/src/put_validation.rs

    // println!("Phase 2.5: Fresh replication to close group...");
    //
    // let mut fresh_replications = 0;
    //
    // // For each chunk, the nodes that stored it immediately replicate to CLOSE_GROUP_SIZE closest to the DATA
    // for chunk_addr in &chunk_addresses {
    //     // Find which nodes currently store this chunk
    //     let holders: Vec<PeerId> = nodes
    //         .iter()
    //         .filter(|(_, node)| node.stored_records.contains_key(chunk_addr))
    //         .map(|(peer_id, _)| *peer_id)
    //         .collect();
    //
    //     // Each holder replicates to CLOSE_GROUP_SIZE closest peers to the DATA (not to self)
    //     let mut replications_to_apply: Vec<(PeerId, PeerId, StoredRecord)> = Vec::new(); // (target, holder, record)
    //
    //     for holder_id in &holders {
    //         let holder_node = &nodes[holder_id];
    //
    //         // Get fresh replication targets: CLOSE_GROUP_SIZE closest to DATA
    //         let targets = holder_node.get_replicate_candidates(
    //             chunk_addr, false, // is_periodic = false (fresh replication)
    //         );
    //
    //         // Get the stored record to replicate
    //         if let Some(stored_record) = holder_node.stored_records.get(chunk_addr) {
    //             // Collect replications to each target
    //             for target_id in targets {
    //                 if target_id == *holder_id {
    //                     continue; // Don't replicate to self
    //                 }
    //
    //                 // Create replicated record (no payment for replication)
    //                 let replicated_record = StoredRecord {
    //                     address: chunk_addr.clone(),
    //                     record_kind: stored_record.record_kind,
    //                     data_size: stored_record.data_size,
    //                     payment: None,
    //                 };
    //
    //                 replications_to_apply.push((target_id, *holder_id, replicated_record));
    //             }
    //         }
    //     }
    //
    //     // Apply all replications
    //     for (target_id, _holder_id, replicated_record) in replications_to_apply {
    //         // Try to store on target node
    //         if nodes
    //             .get_mut(&target_id)
    //             .unwrap()
    //             .store_record(replicated_record)
    //         {
    //             fresh_replications += 1;
    //         }
    //     }
    // }
    //
    // let total_records_after_fresh: usize = nodes.values().map(|n| n.stored_records.len()).sum();
    // let avg_records_after_fresh = total_records_after_fresh as f64 / nodes.len() as f64;
    // let min_records_fresh = nodes
    //     .values()
    //     .map(|n| n.stored_records.len())
    //     .min()
    //     .unwrap_or(0);
    // let max_records_fresh = nodes
    //     .values()
    //     .map(|n| n.stored_records.len())
    //     .max()
    //     .unwrap_or(0);
    //
    // println!("  ✓ Fresh replications: {fresh_replications}");
    // println!(
    //     "  ✓ Records per node: {avg_records_after_fresh:.2} avg, {min_records_fresh} min, {max_records_fresh} max, {total_records_after_fresh} total\n"
    // );

    // ========================================================================
    // Phase 3: Periodic Replication
    // ========================================================================
    println!("Phase 3: Running {replication_rounds} replication rounds...");
    let phase3_start = std::time::Instant::now();

    // Track round data for JSON export
    let mut round_data_vec: Vec<RoundData> = Vec::new();

    for round in 1..=replication_rounds {
        println!("  Starting round {round}...");
        let round_start = std::time::Instant::now();
        let mut total_replications = 0;

        // Collect all replication messages in parallel
        let nodes_processed = AtomicUsize::new(0);
        let empty_nodes_skipped = AtomicUsize::new(0);

        let replication_messages: Vec<_> = nodes
            .par_iter()
            .flat_map(|(node_id, node)| {
                let processed = nodes_processed.fetch_add(1, Ordering::Relaxed) + 1;

                // Get all stored record keys (from try_interval_replication)
                let keys: Vec<_> = node
                    .stored_records
                    .iter()
                    .map(|(addr, record)| (addr.clone(), record.record_kind))
                    .collect();

                if keys.is_empty() {
                    empty_nodes_skipped.fetch_add(1, Ordering::Relaxed);
                    return vec![];
                }

                // Get periodic replication targets using proper filtering (from get_replicate_candidates)
                // Targets: CLOSE_GROUP_SIZE * 2 (10) closest to SELF, filtered by responsible_distance and recent replication
                let targets = node.get_replicate_candidates(
                    &node.address.clone(), // Target is self address for periodic replication
                    true,                  // is_periodic = true
                );

                // Print progress every 100 nodes
                if processed.is_multiple_of(100) {
                    let empty = empty_nodes_skipped.load(Ordering::Relaxed);
                    println!(
                        "    Collecting messages: {processed}/{} nodes processed ({empty} empty)...",
                        nodes.len()
                    );
                }

                // Return messages for this node
                targets
                    .into_iter()
                    .map(|target| (target, *node_id, keys.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();

        println!(
            "  Processing {} replication messages...",
            replication_messages.len()
        );

        // Process replication messages - Hybrid approach (parallel read, sequential write)
        let total_messages = replication_messages.len();
        let messages_processed = AtomicUsize::new(0);

        // Parallel phase 1: Analyze messages and determine what to store (read-only)
        let records_to_store: Vec<_> = replication_messages
            .par_iter()
            .filter_map(|(recipient, sender, keys)| {
                let processed = messages_processed.fetch_add(1, Ordering::Relaxed) + 1;

                // Print progress every 100 messages
                if processed.is_multiple_of(500) {
                    println!("    Analyzed {processed}/{total_messages} messages...");
                }

                // Collect records to fetch (read-only operations)
                let recipient_node = nodes.get(recipient)?;

                // Accept replication requests only from K_VALUE peers away (from add_keys_to_replication_fetcher)
                // Production uses get_closest_local_peers_to_target which excludes self
                let recipient_closest_k = recipient_node.find_closest_local(
                    &recipient_node.address,
                    LIBP2P_K_VALUE,
                    false, // Production excludes self - is_in_range adds self internally
                );
                if !recipient_closest_k.contains(sender) || sender == recipient {
                    // Reject if sender not in K closest OR sender is self
                    REPLICATION_MSG_REJECTED_NOT_IN_K_CLOSEST.fetch_add(1, Ordering::Relaxed);
                    return None;
                }

                // Track: accepted replication message from K closest peer
                REPLICATION_MSG_ACCEPTED_FROM_K_CLOSEST.fetch_add(1, Ordering::Relaxed);

                let mut records_to_fetch = Vec::new();

                // from in_range_new_keys
                for (addr, record_kind) in keys {
                    // Check if should fetch (in-range, not already stored)
                    if recipient_node.stored_records.contains_key(addr) {
                        continue;
                    }

                    if !recipient_node.is_in_range(addr, &recipient_closest_k) {
                        continue;
                    }

                    // Get the original record from sender
                    if let Some(sender_node) = nodes.get(sender)
                        && let Some(original_record) = sender_node.stored_records.get(addr)
                    {
                        records_to_fetch.push((
                            addr.clone(),
                            *record_kind,
                            original_record.data_size,
                        ));
                    }
                }

                if records_to_fetch.is_empty() {
                    None
                } else {
                    Some((*recipient, *sender, records_to_fetch))
                }
            })
            .collect();

        // Sequential phase 2: Apply stores with majority accumulation
        // Production requires multiple peers (CLOSE_GROUP_SIZE / 2) to report the same record
        // before accepting it for storage

        // Group records by recipient and accumulate peer reports
        let mut accumulators: HashMap<PeerId, ReplicationAccumulator> = HashMap::new();
        let mut record_metadata: HashMap<(PeerId, NetworkAddress), (RecordKind, usize)> =
            HashMap::new();

        for (recipient, sender, records) in &records_to_store {
            let accumulator = accumulators
                .entry(*recipient)
                .or_insert_with(ReplicationAccumulator::new);

            for (addr, record_kind, data_size) in records {
                accumulator.add_and_check_majority(addr, *sender);
                record_metadata
                    .entry((*recipient, addr.clone()))
                    .or_insert((*record_kind, *data_size));
            }
        }

        // Now store only records that reached majority threshold
        for (recipient, accumulator) in accumulators {
            let recipient_node = nodes.get_mut(&recipient).unwrap();

            // Get all addresses that reached majority
            let addresses_with_majority: Vec<_> = accumulator
                .pending
                .iter()
                .filter(|(_, peers)| peers.len() >= REPLICATION_MAJORITY_THRESHOLD)
                .map(|(addr, _)| addr.clone())
                .collect();

            for addr in addresses_with_majority {
                if let Some((record_kind, data_size)) =
                    record_metadata.get(&(recipient, addr.clone()))
                {
                    // Skip if already stored
                    if recipient_node.stored_records.contains_key(&addr) {
                        continue;
                    }

                    let record = StoredRecord {
                        address: addr.clone(),
                        record_kind: *record_kind,
                        data_size: *data_size,
                        payment: None, // Replicated records have no payment
                    };

                    if recipient_node.store_record(record) == StoreResult::Stored {
                        total_replications += 1;
                        REPLICATION_MAJORITY_REACHED.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }

            // Track pending records that didn't reach majority
            REPLICATION_PENDING_MAJORITY
                .fetch_add(accumulator.pending_count(), Ordering::Relaxed);
        }

        let total_records: usize = nodes.values().map(|n| n.stored_records.len()).sum();
        let avg_per_node = total_records as f64 / nodes.len() as f64;
        let min_per_node = nodes
            .values()
            .map(|n| n.stored_records.len())
            .min()
            .unwrap_or(0);
        let max_per_node = nodes
            .values()
            .map(|n| n.stored_records.len())
            .max()
            .unwrap_or(0);

        let round_duration = round_start.elapsed();
        println!(
            "  ✓ Round {round} completed: {total_replications} replications, {total_records} total records ({avg_per_node:.1} avg, {min_per_node} min, {max_per_node} max per node) - {:.2}s",
            round_duration.as_secs_f64()
        );

        // Track round data for JSON export
        round_data_vec.push(RoundData {
            round_number: round,
            total_records,
            replications: total_replications,
            avg_records_per_node: avg_per_node,
        });

        // Early termination if no new replications
        if total_replications == 0 && round > 1 {
            println!("  No new replications in round {round}, stopping early\n");
            break;
        }
    }

    println!(
        "Phase 3 complete - {:.2}s\n",
        phase3_start.elapsed().as_secs_f64()
    );

    // ========================================================================
    // Phase 4: Verification
    // ========================================================================
    println!("Phase 4: Verifying replication coverage...");
    let phase4_start = std::time::Instant::now();

    let coverage_stats = Mutex::new(CoverageStats::new());

    chunk_addresses.par_iter().for_each(|chunk_addr| {
        // Find true 7 closest nodes (pure computation)
        let mut closest_7: Vec<_> = all_peer_ids
            .iter()
            .map(|peer| {
                let addr = NetworkAddress::from(*peer);
                (*peer, chunk_addr.distance(&addr))
            })
            .collect();
        closest_7.sort_by_key(|(_, dist)| *dist);
        let expected_holders: Vec<_> = closest_7
            .into_iter()
            .take(CLOSE_GROUP_SIZE + 2)
            .map(|(peer, _)| peer)
            .collect();

        // Check how many actually store it (read-only)
        let stored_count = expected_holders
            .iter()
            .filter(|peer| nodes[peer].stored_records.contains_key(chunk_addr))
            .count();

        // Update stats (synchronized)
        coverage_stats.lock().unwrap().add_chunk(stored_count);
    });

    let coverage_stats = coverage_stats.into_inner().unwrap();
    println!(
        "Phase 4 complete - {:.2}s\n",
        phase4_start.elapsed().as_secs_f64()
    );

    // ========================================================================
    // Phase 5: Report
    // ========================================================================
    println!("=== Simulation Results ===\n");
    println!("Network Configuration:");
    println!("  - Nodes: {num_nodes}");
    println!("  - Chunks: {num_chunks}");
    println!("  - Payment Mode: {payment_mode:?}");
    println!("  - Replication Rounds: {replication_rounds}\n");

    // Upload statistics
    println!("Upload Phase Results:");
    println!(
        "  - Average initial storage: {:.2} nodes per chunk",
        upload_stats.average()
    );
    println!(
        "  - Initial records per node: {avg_records_after_upload:.2} avg, {min_records} min, {max_records} max, {total_records_after_upload} total"
    );

    // Payment validation statistics
    if upload_stats.payment_not_for_us > 0
        || upload_stats.payees_out_of_range > 0
        || upload_stats.distance_too_far > 0
    {
        println!("  - Payment validation rejections:");
        if upload_stats.payment_not_for_us > 0 {
            println!(
                "      • Not in payee list: {} rejections",
                upload_stats.payment_not_for_us
            );
        }
        if upload_stats.payees_out_of_range > 0 {
            println!(
                "      • Payees out of range: {} rejections",
                upload_stats.payees_out_of_range
            );
        }
        if upload_stats.distance_too_far > 0 {
            println!(
                "      • Distance too far: {} rejections",
                upload_stats.distance_too_far
            );
        }
    }
    println!();

    // Node statistics
    let final_total_records: usize = nodes.values().map(|n| n.stored_records.len()).sum();
    let final_avg_records = final_total_records as f64 / nodes.len() as f64;
    let final_min_records = nodes
        .values()
        .map(|n| n.stored_records.len())
        .min()
        .unwrap_or(0);
    let final_max_records = nodes
        .values()
        .map(|n| n.stored_records.len())
        .max()
        .unwrap_or(0);

    // Calculate standard deviation
    let variance: f64 = nodes
        .values()
        .map(|n| {
            let diff = n.stored_records.len() as f64 - final_avg_records;
            diff * diff
        })
        .sum::<f64>()
        / nodes.len() as f64;
    let std_dev = variance.sqrt();

    println!("Node Storage Distribution:");
    println!("  - Total records stored: {final_total_records}");
    println!("  - Average per node: {final_avg_records:.2}");
    println!("  - Min per node: {final_min_records}");
    println!("  - Max per node: {final_max_records}");
    println!("  - Std deviation: {std_dev:.2}");

    // Distribution histogram
    let nodes_at_capacity = nodes
        .values()
        .filter(|n| n.stored_records.len() >= MAX_RECORDS_COUNT)
        .count();
    let nodes_over_75 = nodes
        .values()
        .filter(|n| n.stored_records.len() >= MAX_RECORDS_COUNT * 3 / 4)
        .count();
    let nodes_over_50 = nodes
        .values()
        .filter(|n| n.stored_records.len() >= MAX_RECORDS_COUNT / 2)
        .count();
    let nodes_under_25 = nodes
        .values()
        .filter(|n| n.stored_records.len() < MAX_RECORDS_COUNT / 4)
        .count();

    println!("  - Nodes at capacity (>= {MAX_RECORDS_COUNT}): {nodes_at_capacity}");
    println!("  - Nodes > 75% full: {nodes_over_75}");
    println!("  - Nodes > 50% full: {nodes_over_50}");
    println!("  - Nodes < 25% full: {nodes_under_25}\n");

    println!("Coverage Analysis:");
    println!("  - Total chunks: {}", coverage_stats.total_chunks);
    println!(
        "  - 7/7 holders: {} ({:.1}%)",
        coverage_stats.holders_7,
        coverage_stats.percent(coverage_stats.holders_7)
    );
    println!(
        "  - 6/7 holders: {} ({:.1}%)",
        coverage_stats.holders_6,
        coverage_stats.percent(coverage_stats.holders_6)
    );
    println!(
        "  - 5/7 holders: {} ({:.1}%)",
        coverage_stats.holders_5,
        coverage_stats.percent(coverage_stats.holders_5)
    );
    println!(
        "  - 4/7 holders: {} ({:.1}%)",
        coverage_stats.holders_4,
        coverage_stats.percent(coverage_stats.holders_4)
    );
    println!(
        "  - 3/7 holders: {} ({:.1}%)",
        coverage_stats.holders_3,
        coverage_stats.percent(coverage_stats.holders_3)
    );
    println!(
        "  - 2/7 holders: {} ({:.1}%)",
        coverage_stats.holders_2,
        coverage_stats.percent(coverage_stats.holders_2)
    );
    println!(
        "  - 1/7 holders: {} ({:.1}%)",
        coverage_stats.holders_1,
        coverage_stats.percent(coverage_stats.holders_1)
    );
    println!(
        "  - 0/7 holders: {} ({:.1}%)",
        coverage_stats.holders_0,
        coverage_stats.percent(coverage_stats.holders_0)
    );
    println!(
        "  - Average coverage: {:.1}%\n",
        coverage_stats.average_percent()
    );

    // ASCII Visualization
    println!("=== ASCII Visualization ===\n");

    // Coverage bar chart
    println!("Overall Coverage: {} {:.1}%", coverage_stats.ascii_bar(20), coverage_stats.average_percent());

    // Coverage distribution histogram
    println!("\nCoverage Distribution:");
    let max_count = [
        coverage_stats.holders_0,
        coverage_stats.holders_1,
        coverage_stats.holders_2,
        coverage_stats.holders_3,
        coverage_stats.holders_4,
        coverage_stats.holders_5,
        coverage_stats.holders_6,
        coverage_stats.holders_7,
    ]
    .iter()
    .max()
    .copied()
    .unwrap_or(1);

    let bar_width = 30;
    for (i, count) in [
        coverage_stats.holders_0,
        coverage_stats.holders_1,
        coverage_stats.holders_2,
        coverage_stats.holders_3,
        coverage_stats.holders_4,
        coverage_stats.holders_5,
        coverage_stats.holders_6,
        coverage_stats.holders_7,
    ]
    .iter()
    .enumerate()
    {
        let bar_len = (*count as f64 / max_count as f64 * bar_width as f64).round() as usize;
        let bar = "█".repeat(bar_len);
        println!("  {}/7: {:>5} {}", i, count, bar);
    }

    // Storage distribution visualization
    println!("\nStorage Distribution (records per node):");
    let mut record_counts: Vec<usize> = nodes.values().map(|n| n.stored_records.len()).collect();
    record_counts.sort();

    let p0 = record_counts.first().copied().unwrap_or(0);
    let p25 = record_counts.get(record_counts.len() / 4).copied().unwrap_or(0);
    let p50 = record_counts.get(record_counts.len() / 2).copied().unwrap_or(0);
    let p75 = record_counts.get(record_counts.len() * 3 / 4).copied().unwrap_or(0);
    let p100 = record_counts.last().copied().unwrap_or(0);

    let storage_max = p100.max(1);
    let storage_bar = |val: usize| {
        let len = (val as f64 / storage_max as f64 * 20.0).round() as usize;
        "█".repeat(len)
    };

    println!("  max:    {:>5} {}", p100, storage_bar(p100));
    println!("  p75:    {:>5} {}", p75, storage_bar(p75));
    println!("  median: {:>5} {}", p50, storage_bar(p50));
    println!("  p25:    {:>5} {}", p25, storage_bar(p25));
    println!("  min:    {:>5} {}", p0, storage_bar(p0));
    println!();

    // Replication Candidate Selection statistics
    let within_range = REPLICATION_PEERS_WITHIN_RESPONSIBLE_RANGE.load(Ordering::Relaxed);
    let fallback = REPLICATION_PEERS_FALLBACK_TO_CLOSEST_K.load(Ordering::Relaxed);
    let total_replication_decisions = within_range + fallback;

    println!("Replication Candidate Selection:");
    if total_replication_decisions > 0 {
        let within_range_percent = 100.0 * within_range as f64 / total_replication_decisions as f64;
        let fallback_percent = 100.0 * fallback as f64 / total_replication_decisions as f64;

        println!("  - Peers within responsible range: {within_range} ({within_range_percent:.1}%)");
        println!("  - Peers fallback to closest K: {fallback} ({fallback_percent:.1}%)");
        println!("  - Total decisions: {total_replication_decisions}");
    } else {
        println!("  - No replication decisions recorded");
    }
    println!();

    // Replication In-Range Check statistics (simplified - matches production)
    let in_range = REPLICATION_IN_RANGE.load(Ordering::Relaxed);
    let out_of_range = REPLICATION_OUT_OF_RANGE.load(Ordering::Relaxed);
    let total_in_range_checks = in_range + out_of_range;

    println!("Replication In-Range Decision Paths:");
    if total_in_range_checks > 0 {
        let in_range_percent = 100.0 * in_range as f64 / total_in_range_checks as f64;
        let out_percent = 100.0 * out_of_range as f64 / total_in_range_checks as f64;

        println!("  - Within farthest peer distance (accepted): {in_range} ({in_range_percent:.1}%)");
        println!("  - Beyond farthest peer distance (rejected): {out_of_range} ({out_percent:.1}%)");
        println!("  - Total checks: {total_in_range_checks}");
    } else {
        println!("  - No in-range checks recorded");
    }
    println!();

    // Upload Payment Validation statistics
    let all_in_k = UPLOAD_PAYMENT_ALL_PAYEES_IN_K_CLOSEST.load(Ordering::Relaxed);
    let validated_via_range =
        UPLOAD_PAYMENT_VALIDATED_VIA_RESPONSIBLE_RANGE.load(Ordering::Relaxed);
    let trusted = UPLOAD_PAYMENT_NO_RESPONSIBLE_RANGE_TRUSTED.load(Ordering::Relaxed);
    let total_payment_validations = all_in_k + validated_via_range + trusted;

    println!("Upload Payment Validation Paths:");
    if total_payment_validations > 0 {
        let all_in_k_percent = 100.0 * all_in_k as f64 / total_payment_validations as f64;
        let validated_percent =
            100.0 * validated_via_range as f64 / total_payment_validations as f64;
        let trusted_percent = 100.0 * trusted as f64 / total_payment_validations as f64;

        println!("  - All payees in K closest (fast path): {all_in_k} ({all_in_k_percent:.1}%)");
        println!(
            "  - Validated via responsible range check: {validated_via_range} ({validated_percent:.1}%)"
        );
        println!("  - No responsible range, trusted: {trusted} ({trusted_percent:.1}%)");
        println!("  - Total validations: {total_payment_validations}");
    } else {
        println!("  - No payment validations recorded");
    }
    println!();

    // Replication Message Acceptance statistics
    let msg_accepted = REPLICATION_MSG_ACCEPTED_FROM_K_CLOSEST.load(Ordering::Relaxed);
    let msg_rejected = REPLICATION_MSG_REJECTED_NOT_IN_K_CLOSEST.load(Ordering::Relaxed);
    let total_replication_msgs = msg_accepted + msg_rejected;

    println!("Replication Message Acceptance:");
    if total_replication_msgs > 0 {
        let accepted_percent = 100.0 * msg_accepted as f64 / total_replication_msgs as f64;
        let rejected_percent = 100.0 * msg_rejected as f64 / total_replication_msgs as f64;

        println!("  - Accepted from K closest peers: {msg_accepted} ({accepted_percent:.1}%)");
        println!("  - Rejected (sender not in K closest): {msg_rejected} ({rejected_percent:.1}%)");
        println!("  - Total messages: {total_replication_msgs}");
    } else {
        println!("  - No replication messages recorded");
    }
    println!();

    // Majority Accumulation statistics (production requires 2+ peers reporting same record)
    let majority_reached = REPLICATION_MAJORITY_REACHED.load(Ordering::Relaxed);
    let pending_majority = REPLICATION_PENDING_MAJORITY.load(Ordering::Relaxed);
    let total_majority_checks = majority_reached + pending_majority;

    println!("Majority Accumulation (requires {REPLICATION_MAJORITY_THRESHOLD}+ peer reports):");
    if total_majority_checks > 0 {
        let majority_percent = 100.0 * majority_reached as f64 / total_majority_checks as f64;
        let pending_percent = 100.0 * pending_majority as f64 / total_majority_checks as f64;

        println!("  - Records reaching majority (stored): {majority_reached} ({majority_percent:.1}%)");
        println!(
            "  - Records pending majority (not stored): {pending_majority} ({pending_percent:.1}%)"
        );
        println!("  - Total candidate records: {total_majority_checks}");
    } else {
        println!("  - No majority accumulation recorded");
    }
    println!();

    // ========================================================================
    // Phase 6: JSON Export
    // ========================================================================
    println!("Phase 6: Exporting simulation results to JSON...");

    // Build chunk timelines (simplified - just final coverage)
    let chunk_timelines: Vec<ChunkTimeline> = chunk_addresses
        .iter()
        .take(100) // Limit to first 100 chunks to keep export manageable
        .map(|chunk_addr| {
            let holders_count = all_peer_ids
                .iter()
                .take(CLOSE_GROUP_SIZE + 2)
                .filter(|peer| nodes[peer].stored_records.contains_key(chunk_addr))
                .count();

            ChunkTimeline {
                address: format!("{:?}", chunk_addr),
                holders_per_round: vec![], // Would require tracking during simulation
                final_coverage: holders_count,
            }
        })
        .collect();

    let export = SimulationExport {
        config: SimulationConfig {
            num_nodes,
            num_chunks,
            replication_rounds,
            close_group_size: CLOSE_GROUP_SIZE,
            k_value: LIBP2P_K_VALUE,
            majority_threshold: REPLICATION_MAJORITY_THRESHOLD,
        },
        rounds: round_data_vec,
        final_coverage: coverage_stats.to_export(),
        chunk_timelines,
    };

    // Write to file
    let json_output = serde_json::to_string_pretty(&export).expect("Failed to serialize to JSON");
    let output_path = std::path::Path::new("simulation_results.json");
    std::fs::write(output_path, &json_output).expect("Failed to write JSON file");
    println!("  ✓ Exported results to {}", output_path.display());
}

// ============================================================================
// Statistics Helpers
// ============================================================================

struct UploadStats {
    total: usize,
    sum: usize,
    payment_not_for_us: usize,
    payees_out_of_range: usize,
    distance_too_far: usize,
}

impl UploadStats {
    fn new() -> Self {
        Self {
            total: 0,
            sum: 0,
            payment_not_for_us: 0,
            payees_out_of_range: 0,
            distance_too_far: 0,
        }
    }

    fn add_upload(
        &mut self,
        stored_count: usize,
        payment_not_for_us: usize,
        payees_out_of_range: usize,
        distance_too_far: usize,
    ) {
        self.total += 1;
        self.sum += stored_count;
        self.payment_not_for_us += payment_not_for_us;
        self.payees_out_of_range += payees_out_of_range;
        self.distance_too_far += distance_too_far;
    }

    fn average(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.sum as f64 / self.total as f64
        }
    }
}

struct CoverageStats {
    total_chunks: usize,
    holders_7: usize, // 7/7 (all holders)
    holders_6: usize, // 6/7
    holders_5: usize, // 5/7 (CLOSE_GROUP_SIZE)
    holders_4: usize, // 4/7
    holders_3: usize, // 3/7
    holders_2: usize, // 2/7
    holders_1: usize, // 1/7
    holders_0: usize, // 0/7 (lost)
}

impl CoverageStats {
    fn new() -> Self {
        Self {
            total_chunks: 0,
            holders_7: 0,
            holders_6: 0,
            holders_5: 0,
            holders_4: 0,
            holders_3: 0,
            holders_2: 0,
            holders_1: 0,
            holders_0: 0,
        }
    }

    fn add_chunk(&mut self, stored_count: usize) {
        self.total_chunks += 1;
        match stored_count {
            7 => self.holders_7 += 1,
            6 => self.holders_6 += 1,
            5 => self.holders_5 += 1,
            4 => self.holders_4 += 1,
            3 => self.holders_3 += 1,
            2 => self.holders_2 += 1,
            1 => self.holders_1 += 1,
            0 => self.holders_0 += 1,
            _ => {} // Should never happen
        }
    }

    fn percent(&self, count: usize) -> f64 {
        100.0 * count as f64 / self.total_chunks as f64
    }

    fn to_export(&self) -> CoverageExport {
        CoverageExport {
            total_chunks: self.total_chunks,
            distribution: [
                self.holders_0,
                self.holders_1,
                self.holders_2,
                self.holders_3,
                self.holders_4,
                self.holders_5,
                self.holders_6,
                self.holders_7,
            ],
            average_percent: self.average_percent(),
        }
    }

    /// Generate ASCII bar for coverage visualization
    fn ascii_bar(&self, width: usize) -> String {
        let pct = self.average_percent() / 100.0;
        let filled = (pct * width as f64).round() as usize;
        let empty = width.saturating_sub(filled);
        format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
    }

    fn average_percent(&self) -> f64 {
        let total_coverage = (self.holders_7 * 7
            + self.holders_6 * 6
            + self.holders_5 * 5
            + self.holders_4 * 4
            + self.holders_3 * 3
            + self.holders_2 * 2
            + self.holders_1) as f64;
        let max_coverage = (self.total_chunks * 7) as f64;
        100.0 * total_coverage / max_coverage
    }
}
