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
//! - Node failure injection and fault tolerance analysis
//!
//! The goal is to verify that data is properly replicated to the closest nodes
//! and that the network maintains good coverage (chunks stored by their close group).

pub mod config;
pub mod counters;
pub mod export;
pub mod node;
pub mod stats;
pub mod types;

use ant_protocol::{CLOSE_GROUP_SIZE, NetworkAddress, storage::{DataTypes, RecordKind}};
use autonomi::ChunkAddress;
use libp2p::PeerId;
use rand::seq::index::sample;
use rand::seq::SliceRandom;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use xor_name::XorName;

use config::*;
use counters::*;
use export::*;
use stats::*;
use types::*;

// ============================================================================
// Main Simulation Test
// ============================================================================

#[test]
fn test_replication_simulation() {
    println!("\n=== Network Replication Simulation ===\n");

    let num_nodes = SIMULATION_NUM_NODES;
    let num_chunks = SIMULATION_NUM_CHUNKS;
    let replication_rounds = SIMULATION_REPLICATION_ROUNDS;
    let payment_mode = &SIMULATION_PAYMENT_MODE;

    // ========================================================================
    // Phase 1: Setup Network
    // ========================================================================
    println!("Phase 1: Setting up network with {num_nodes} nodes...");
    let phase1_start = std::time::Instant::now();

    let create_start = std::time::Instant::now();
    let mut nodes: HashMap<PeerId, SimulatedNode> = (0..num_nodes)
        .into_par_iter()
        .map(|_| {
            let peer_id = PeerId::random();
            let node = SimulatedNode::new(peer_id);
            (peer_id, node)
        })
        .collect();

    println!(
        "  ✓ Created {} nodes - {:.2}s",
        nodes.len(),
        create_start.elapsed().as_secs_f64()
    );

    println!("Building routing tables...");
    let routing_start = std::time::Instant::now();
    let all_peer_ids: Vec<_> = nodes.keys().copied().collect();

    nodes.par_iter_mut().for_each(|(_, node)| {
        use std::collections::BTreeMap;
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

    println!("Estimating network size from routing tables...");
    let estimation_start = std::time::Instant::now();
    nodes.par_iter_mut().for_each(|(_, node)| {
        node.estimate_network_size();
    });

    let total_estimate: usize = nodes.values().map(|n| n.network_size_estimate).sum();
    let avg_estimate = total_estimate as f64 / nodes.len() as f64;
    let min_estimate = nodes.values().map(|n| n.network_size_estimate).min().unwrap_or(0);
    let max_estimate = nodes.values().map(|n| n.network_size_estimate).max().unwrap_or(0);
    println!(
        "  ✓ Network size estimates: avg {avg_estimate:.0}, min {min_estimate}, max {max_estimate} (actual: {num_nodes}) - {:.2}s",
        estimation_start.elapsed().as_secs_f64()
    );

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
    let uploaded_count = std::sync::atomic::AtomicUsize::new(0);

    (0..num_chunks).into_par_iter().for_each(|_| {
        let chunk_addr = NetworkAddress::ChunkAddress(ChunkAddress::new(XorName::random(
            &mut rand::thread_rng(),
        )));
        let chunk_size = 1024 * 1024;

        chunk_addresses.lock().unwrap().push(chunk_addr.clone());

        let mut closest_5: Vec<_> = all_peer_ids
            .iter()
            .map(|peer| {
                let addr = NetworkAddress::from(*peer);
                (*peer, chunk_addr.distance(&addr))
            })
            .collect();
        closest_5.sort_by_key(|(_, dist)| *dist);
        let closest_5: Vec<_> = closest_5.into_iter().take(CLOSE_GROUP_SIZE).collect();

        let mut quotes: Vec<_> = {
            let nodes_guard = nodes.lock().unwrap();
            closest_5
                .iter()
                .map(|(peer, _)| (*peer, nodes_guard[peer].generate_quote(chunk_size)))
                .collect()
        };
        quotes.sort_by_key(|(_, price)| *price);

        let payment = SimulatedPayment {
            payees: quotes.iter().map(|(peer, _)| *peer).collect(),
            data_type: DataTypes::Chunk,
        };

        let mut stored_count = 0;
        let mut payment_not_for_us = 0;
        let mut payees_out_of_range = 0;
        let mut distance_too_far = 0;

        {
            let mut nodes_guard = nodes.lock().unwrap();
            for (peer_id, _) in &closest_5 {
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

    let chunk_addresses = chunk_addresses.into_inner().unwrap();
    let upload_stats = upload_stats.into_inner().unwrap();
    let mut nodes = nodes.into_inner().unwrap();

    let total_records_after_upload: usize = nodes.values().map(|n| n.stored_records.len()).sum();
    let avg_records_after_upload = total_records_after_upload as f64 / nodes.len() as f64;
    let min_records = nodes.values().map(|n| n.stored_records.len()).min().unwrap_or(0);
    let max_records = nodes.values().map(|n| n.stored_records.len()).max().unwrap_or(0);

    println!("  ✓ Uploaded {num_chunks} chunks");
    println!(
        "Phase 2 complete - {:.2}s\n",
        phase2_start.elapsed().as_secs_f64()
    );

    // ========================================================================
    // Phase 3: Periodic Replication
    // ========================================================================
    println!("Phase 3: Running {replication_rounds} replication rounds...");
    let phase3_start = std::time::Instant::now();

    let mut round_data_vec: Vec<RoundData> = Vec::new();

    for round in 1..=replication_rounds {
        println!("  Starting round {round}...");
        let round_start = std::time::Instant::now();
        let mut total_replications = 0;

        let nodes_processed = std::sync::atomic::AtomicUsize::new(0);
        let empty_nodes_skipped = std::sync::atomic::AtomicUsize::new(0);

        let replication_messages: Vec<_> = nodes
            .par_iter()
            .flat_map(|(node_id, node)| {
                let processed = nodes_processed.fetch_add(1, Ordering::Relaxed) + 1;

                let keys: Vec<_> = node
                    .stored_records
                    .iter()
                    .map(|(addr, record)| (addr.clone(), record.record_kind))
                    .collect();

                if keys.is_empty() {
                    empty_nodes_skipped.fetch_add(1, Ordering::Relaxed);
                    return vec![];
                }

                let targets = node.get_replicate_candidates(
                    &node.address.clone(),
                    true,
                );

                if processed.is_multiple_of(100) {
                    let empty = empty_nodes_skipped.load(Ordering::Relaxed);
                    println!(
                        "    Collecting messages: {processed}/{} nodes processed ({empty} empty)...",
                        nodes.len()
                    );
                }

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

        let total_messages = replication_messages.len();
        let messages_processed = std::sync::atomic::AtomicUsize::new(0);

        let records_to_store: Vec<_> = replication_messages
            .par_iter()
            .filter_map(|(recipient, sender, keys)| {
                let processed = messages_processed.fetch_add(1, Ordering::Relaxed) + 1;

                if processed.is_multiple_of(500) {
                    println!("    Analyzed {processed}/{total_messages} messages...");
                }

                let recipient_node = nodes.get(recipient)?;

                let recipient_closest_k = recipient_node.find_closest_local(
                    &recipient_node.address,
                    LIBP2P_K_VALUE,
                    false,
                );
                if !recipient_closest_k.contains(sender) || sender == recipient {
                    REPLICATION_MSG_REJECTED_NOT_IN_K_CLOSEST.fetch_add(1, Ordering::Relaxed);
                    return None;
                }

                REPLICATION_MSG_ACCEPTED_FROM_K_CLOSEST.fetch_add(1, Ordering::Relaxed);

                let mut records_to_fetch = Vec::new();

                for (addr, record_kind) in keys {
                    if recipient_node.stored_records.contains_key(addr) {
                        continue;
                    }

                    if !recipient_node.is_in_range(addr, &recipient_closest_k) {
                        continue;
                    }

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

        // Apply stores with majority accumulation
        let mut accumulators: HashMap<PeerId, ReplicationAccumulator> = HashMap::new();
        let mut record_metadata: HashMap<(PeerId, NetworkAddress), (RecordKind, usize)> = HashMap::new();

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

        for (recipient, accumulator) in accumulators {
            let recipient_node = nodes.get_mut(&recipient).unwrap();

            let addresses_with_majority: Vec<_> = accumulator
                .pending
                .iter()
                .filter(|(_, peers)| peers.len() >= REPLICATION_MAJORITY_THRESHOLD)
                .map(|(addr, _)| addr.clone())
                .collect();

            for addr in addresses_with_majority {
                if let Some((record_kind, data_size)) = record_metadata.get(&(recipient, addr.clone())) {
                    if recipient_node.stored_records.contains_key(&addr) {
                        continue;
                    }

                    let record = StoredRecord {
                        address: addr.clone(),
                        record_kind: *record_kind,
                        data_size: *data_size,
                        payment: None,
                    };

                    if recipient_node.store_record(record) == StoreResult::Stored {
                        total_replications += 1;
                        REPLICATION_MAJORITY_REACHED.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }

            REPLICATION_PENDING_MAJORITY.fetch_add(accumulator.pending_count(), Ordering::Relaxed);
        }

        let total_records: usize = nodes.values().map(|n| n.stored_records.len()).sum();
        let avg_per_node = total_records as f64 / nodes.len() as f64;
        let min_per_node = nodes.values().map(|n| n.stored_records.len()).min().unwrap_or(0);
        let max_per_node = nodes.values().map(|n| n.stored_records.len()).max().unwrap_or(0);

        let round_duration = round_start.elapsed();
        println!(
            "  ✓ Round {round} completed: {total_replications} replications, {total_records} total records ({avg_per_node:.1} avg, {min_per_node} min, {max_per_node} max per node) - {:.2}s",
            round_duration.as_secs_f64()
        );

        round_data_vec.push(RoundData {
            round_number: round,
            total_records,
            replications: total_replications,
            avg_records_per_node: avg_per_node,
        });

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
    // Phase 3.5: Node Failure Injection
    // ========================================================================
    println!("Phase 3.5: Simulating node failures...");
    let phase35_start = std::time::Instant::now();

    let num_failures = (nodes.len() as f64 * SIMULATION_FAILURE_RATE) as usize;
    let mut rng = rand::thread_rng();

    let node_ids: Vec<PeerId> = nodes.keys().copied().collect();
    let failed_nodes: Vec<PeerId> = node_ids
        .choose_multiple(&mut rng, num_failures)
        .copied()
        .collect();

    for peer_id in &failed_nodes {
        nodes.remove(peer_id);
    }

    let all_peer_ids: Vec<PeerId> = nodes.keys().copied().collect();

    println!(
        "  ✓ Removed {} nodes ({:.1}% failure rate), {} nodes remaining",
        failed_nodes.len(),
        SIMULATION_FAILURE_RATE * 100.0,
        nodes.len()
    );
    println!(
        "Phase 3.5 complete - {:.2}s\n",
        phase35_start.elapsed().as_secs_f64()
    );

    // ========================================================================
    // Phase 4: Verification (Post-Failure)
    // ========================================================================
    println!("Phase 4: Verifying replication coverage after node failures...");
    let phase4_start = std::time::Instant::now();

    let coverage_stats = Mutex::new(CoverageStats::new());
    let per_chunk_holders = Mutex::new(Vec::new());

    chunk_addresses.par_iter().for_each(|chunk_addr| {
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

        let stored_count = expected_holders
            .iter()
            .filter(|peer| nodes[peer].stored_records.contains_key(chunk_addr))
            .count();

        coverage_stats.lock().unwrap().add_chunk(stored_count);
        per_chunk_holders.lock().unwrap().push(stored_count);
    });

    let coverage_stats = coverage_stats.into_inner().unwrap();
    let mut per_chunk_holders = per_chunk_holders.into_inner().unwrap();
    per_chunk_holders.sort();

    // Calculate fault tolerance metrics
    let total_chunks = per_chunk_holders.len();
    let data_loss_count = per_chunk_holders.iter().filter(|&&c| c == 0).count();
    let critical_risk_count = per_chunk_holders.iter().filter(|&&c| c < 3).count();
    let sum_holders: usize = per_chunk_holders.iter().sum();
    let mean_coverage = sum_holders as f64 / total_chunks as f64;

    let variance: f64 = per_chunk_holders
        .iter()
        .map(|&c| {
            let diff = c as f64 - mean_coverage;
            diff * diff
        })
        .sum::<f64>()
        / total_chunks as f64;
    let coverage_std_dev = variance.sqrt();

    let p99_idx = (total_chunks as f64 * 0.01).floor() as usize;
    let p99_coverage = per_chunk_holders.get(p99_idx).copied().unwrap_or(0);

    let avg_replication = mean_coverage.round() as i32;
    let survival_probability = 1.0 - SIMULATION_FAILURE_RATE.powi(avg_replication.max(1));

    let fault_metrics = FaultToleranceMetrics {
        data_loss_rate: data_loss_count as f64 / total_chunks as f64 * 100.0,
        critical_risk_rate: critical_risk_count as f64 / total_chunks as f64 * 100.0,
        mean_coverage,
        coverage_std_dev,
        p99_coverage,
        survival_probability: survival_probability * 100.0,
    };

    println!(
        "Phase 4 complete - {:.2}s\n",
        phase4_start.elapsed().as_secs_f64()
    );

    // ========================================================================
    // Phase 5: Report
    // ========================================================================
    print_report(
        num_nodes,
        num_chunks,
        payment_mode,
        replication_rounds,
        &nodes,
        &upload_stats,
        avg_records_after_upload,
        min_records,
        max_records,
        total_records_after_upload,
        &coverage_stats,
        &fault_metrics,
        data_loss_count,
        critical_risk_count,
    );

    // ========================================================================
    // Phase 6: JSON Export
    // ========================================================================
    println!("Phase 6: Exporting simulation results to JSON...");

    let chunk_timelines: Vec<ChunkTimeline> = chunk_addresses
        .iter()
        .take(100)
        .map(|chunk_addr| {
            let holders_count = all_peer_ids
                .iter()
                .take(CLOSE_GROUP_SIZE + 2)
                .filter(|peer| nodes[peer].stored_records.contains_key(chunk_addr))
                .count();

            ChunkTimeline {
                address: format!("{:?}", chunk_addr),
                holders_per_round: vec![],
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
        fault_tolerance: fault_metrics,
        chunk_timelines,
    };

    let json_output = serde_json::to_string_pretty(&export).expect("Failed to serialize to JSON");
    let output_path = std::path::Path::new("simulation_results.json");
    std::fs::write(output_path, &json_output).expect("Failed to write JSON file");
    println!("  ✓ Exported results to {}", output_path.display());
}

// ============================================================================
// Report Generation
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn print_report(
    num_nodes: usize,
    num_chunks: usize,
    payment_mode: &PaymentMode,
    replication_rounds: usize,
    nodes: &HashMap<PeerId, SimulatedNode>,
    upload_stats: &UploadStats,
    avg_records_after_upload: f64,
    min_records: usize,
    max_records: usize,
    total_records_after_upload: usize,
    coverage_stats: &CoverageStats,
    fault_metrics: &FaultToleranceMetrics,
    data_loss_count: usize,
    critical_risk_count: usize,
) {
    println!("=== Simulation Results ===\n");
    println!("Network Configuration:");
    println!("  - Nodes: {num_nodes} (initial), {} (after failures)", nodes.len());
    println!("  - Chunks: {num_chunks}");
    println!("  - Payment Mode: {payment_mode:?}");
    println!("  - Replication Rounds: {replication_rounds}");
    println!("  - Failure Rate: {:.1}%\n", SIMULATION_FAILURE_RATE * 100.0);

    println!("Fault Tolerance Analysis (post-failure):");
    println!("  - Data loss rate: {:.4}% ({} chunks with 0 holders)",
        fault_metrics.data_loss_rate, data_loss_count);
    println!("  - Critical risk rate: {:.4}% ({} chunks with <3 holders)",
        fault_metrics.critical_risk_rate, critical_risk_count);
    println!("  - Mean coverage: {:.2} holders/chunk", fault_metrics.mean_coverage);
    println!("  - Coverage std dev: {:.2}", fault_metrics.coverage_std_dev);
    println!("  - P1 coverage (worst 1%): {} holders", fault_metrics.p99_coverage);
    println!("  - Theoretical survival: {:.4}% (with {:.0} avg replication)\n",
        fault_metrics.survival_probability, fault_metrics.mean_coverage);

    println!("Upload Phase Results:");
    println!("  - Average initial storage: {:.2} nodes per chunk", upload_stats.average());
    println!(
        "  - Initial records per node: {avg_records_after_upload:.2} avg, {min_records} min, {max_records} max, {total_records_after_upload} total"
    );

    if upload_stats.payment_not_for_us > 0
        || upload_stats.payees_out_of_range > 0
        || upload_stats.distance_too_far > 0
    {
        println!("  - Payment validation rejections:");
        if upload_stats.payment_not_for_us > 0 {
            println!("      • Not in payee list: {} rejections", upload_stats.payment_not_for_us);
        }
        if upload_stats.payees_out_of_range > 0 {
            println!("      • Payees out of range: {} rejections", upload_stats.payees_out_of_range);
        }
        if upload_stats.distance_too_far > 0 {
            println!("      • Distance too far: {} rejections", upload_stats.distance_too_far);
        }
    }
    println!();

    // Node statistics
    let final_total_records: usize = nodes.values().map(|n| n.stored_records.len()).sum();
    let final_avg_records = final_total_records as f64 / nodes.len() as f64;
    let final_min_records = nodes.values().map(|n| n.stored_records.len()).min().unwrap_or(0);
    let final_max_records = nodes.values().map(|n| n.stored_records.len()).max().unwrap_or(0);

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

    let nodes_at_capacity = nodes.values().filter(|n| n.stored_records.len() >= MAX_RECORDS_COUNT).count();
    let nodes_over_75 = nodes.values().filter(|n| n.stored_records.len() >= MAX_RECORDS_COUNT * 3 / 4).count();
    let nodes_over_50 = nodes.values().filter(|n| n.stored_records.len() >= MAX_RECORDS_COUNT / 2).count();
    let nodes_under_25 = nodes.values().filter(|n| n.stored_records.len() < MAX_RECORDS_COUNT / 4).count();

    println!("  - Nodes at capacity (>= {MAX_RECORDS_COUNT}): {nodes_at_capacity}");
    println!("  - Nodes > 75% full: {nodes_over_75}");
    println!("  - Nodes > 50% full: {nodes_over_50}");
    println!("  - Nodes < 25% full: {nodes_under_25}\n");

    println!("Coverage Analysis:");
    println!("  - Total chunks: {}", coverage_stats.total_chunks);
    println!("  - 7/7 holders: {} ({:.1}%)", coverage_stats.holders_7, coverage_stats.percent(coverage_stats.holders_7));
    println!("  - 6/7 holders: {} ({:.1}%)", coverage_stats.holders_6, coverage_stats.percent(coverage_stats.holders_6));
    println!("  - 5/7 holders: {} ({:.1}%)", coverage_stats.holders_5, coverage_stats.percent(coverage_stats.holders_5));
    println!("  - 4/7 holders: {} ({:.1}%)", coverage_stats.holders_4, coverage_stats.percent(coverage_stats.holders_4));
    println!("  - 3/7 holders: {} ({:.1}%)", coverage_stats.holders_3, coverage_stats.percent(coverage_stats.holders_3));
    println!("  - 2/7 holders: {} ({:.1}%)", coverage_stats.holders_2, coverage_stats.percent(coverage_stats.holders_2));
    println!("  - 1/7 holders: {} ({:.1}%)", coverage_stats.holders_1, coverage_stats.percent(coverage_stats.holders_1));
    println!("  - 0/7 holders: {} ({:.1}%)", coverage_stats.holders_0, coverage_stats.percent(coverage_stats.holders_0));
    println!("  - Average coverage: {:.1}%\n", coverage_stats.average_percent());

    // ASCII Visualization
    println!("=== ASCII Visualization ===\n");
    println!("Overall Coverage: {} {:.1}%", coverage_stats.ascii_bar(20), coverage_stats.average_percent());

    println!("\nCoverage Distribution:");
    let max_count = [
        coverage_stats.holders_0, coverage_stats.holders_1, coverage_stats.holders_2,
        coverage_stats.holders_3, coverage_stats.holders_4, coverage_stats.holders_5,
        coverage_stats.holders_6, coverage_stats.holders_7,
    ].iter().max().copied().unwrap_or(1);

    let bar_width = 30;
    for (i, count) in [
        coverage_stats.holders_0, coverage_stats.holders_1, coverage_stats.holders_2,
        coverage_stats.holders_3, coverage_stats.holders_4, coverage_stats.holders_5,
        coverage_stats.holders_6, coverage_stats.holders_7,
    ].iter().enumerate() {
        let bar_len = (*count as f64 / max_count as f64 * bar_width as f64).round() as usize;
        let bar = "█".repeat(bar_len);
        println!("  {}/7: {:>5} {}", i, count, bar);
    }

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

    // Counter statistics
    print_counter_statistics();
}

fn print_counter_statistics() {
    let within_range = load(&REPLICATION_PEERS_WITHIN_RESPONSIBLE_RANGE);
    let fallback = load(&REPLICATION_PEERS_FALLBACK_TO_CLOSEST_K);
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

    let in_range = load(&REPLICATION_IN_RANGE);
    let out_of_range = load(&REPLICATION_OUT_OF_RANGE);
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

    let all_in_k = load(&UPLOAD_PAYMENT_ALL_PAYEES_IN_K_CLOSEST);
    let validated_via_range = load(&UPLOAD_PAYMENT_VALIDATED_VIA_RESPONSIBLE_RANGE);
    let trusted = load(&UPLOAD_PAYMENT_NO_RESPONSIBLE_RANGE_TRUSTED);
    let total_payment_validations = all_in_k + validated_via_range + trusted;

    println!("Upload Payment Validation Paths:");
    if total_payment_validations > 0 {
        let all_in_k_percent = 100.0 * all_in_k as f64 / total_payment_validations as f64;
        let validated_percent = 100.0 * validated_via_range as f64 / total_payment_validations as f64;
        let trusted_percent = 100.0 * trusted as f64 / total_payment_validations as f64;
        println!("  - All payees in K closest (fast path): {all_in_k} ({all_in_k_percent:.1}%)");
        println!("  - Validated via responsible range check: {validated_via_range} ({validated_percent:.1}%)");
        println!("  - No responsible range, trusted: {trusted} ({trusted_percent:.1}%)");
        println!("  - Total validations: {total_payment_validations}");
    } else {
        println!("  - No payment validations recorded");
    }
    println!();

    let msg_accepted = load(&REPLICATION_MSG_ACCEPTED_FROM_K_CLOSEST);
    let msg_rejected = load(&REPLICATION_MSG_REJECTED_NOT_IN_K_CLOSEST);
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

    let majority_reached = load(&REPLICATION_MAJORITY_REACHED);
    let pending_majority = load(&REPLICATION_PENDING_MAJORITY);
    let total_majority_checks = majority_reached + pending_majority;

    println!("Majority Accumulation (requires {REPLICATION_MAJORITY_THRESHOLD}+ peer reports):");
    if total_majority_checks > 0 {
        let majority_percent = 100.0 * majority_reached as f64 / total_majority_checks as f64;
        let pending_percent = 100.0 * pending_majority as f64 / total_majority_checks as f64;
        println!("  - Records reaching majority (stored): {majority_reached} ({majority_percent:.1}%)");
        println!("  - Records pending majority (not stored): {pending_majority} ({pending_percent:.1}%)");
        println!("  - Total candidate records: {total_majority_checks}");
    } else {
        println!("  - No majority accumulation recorded");
    }
    println!();
}
