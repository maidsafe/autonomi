// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod error;
mod json;

pub use error::{AnalysisErrorDisplay, NetworkErrorDisplay};

use crate::actions::NetworkContext;
use crate::wallet::load_wallet;
use autonomi::PublicKey;
use autonomi::chunk::ChunkAddress;
use autonomi::client::analyze::{Analysis, AnalysisError};
use autonomi::client::config::CHUNK_DOWNLOAD_BATCH_SIZE;
use autonomi::client::payment::PaymentOption;
use autonomi::graph::GraphEntryAddress;
use autonomi::{
    Multiaddr, RewardsAddress, SecretKey, Wallet,
    networking::{NetworkAddress, NetworkError, PeerId, Record},
};
use color_eyre::eyre::Result;
use comfy_table::{Cell, CellAlignment, Table};
use futures::stream::{self, StreamExt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tracing::info;
use xor_name::XorName;

const KAD_HOLDERS_QUERY_RANGE: u32 = 20;

// Upload records in parallel with max concurrency
const MAX_PARALLEL_UPLOADS: usize = 20;

type PeerResults = Vec<(PeerId, Result<Vec<(NetworkAddress, Result<ant_protocol::messages::ChunkProof, ant_protocol::error::Error>)>, NetworkError>)>;

/// Status of a closest peer's record for a given address
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ClosestPeerStatus {
    /// Peer is holding the record with the given size in bytes
    Holding {
        peer_id: PeerId,
        target_address: NetworkAddress,
        listen_addrs: Vec<Multiaddr>,
        size: usize,
    },
    /// Peer is not holding the record
    NotHolding {
        peer_id: PeerId,
        target_address: NetworkAddress,
        listen_addrs: Vec<Multiaddr>,
    },
    /// Failed to query the peer
    FailedQuery {
        peer_id: PeerId,
        target_address: NetworkAddress,
        listen_addrs: Vec<Multiaddr>,
        error: NetworkErrorDisplay,
    },
}

/// Record that needs repair (less than 3 copies among closest 7)
#[derive(Debug, Clone)]
struct RecordToRepair {
    address: String,
    holders_count: usize,
    record_data: Record,
}

/// Result of a chunk re-upload operation
#[derive(Debug)]
struct ReuploadResult {
    address: String,
    holders_count: usize,
    result: Result<(autonomi::AttoTokens, autonomi::chunk::ChunkAddress), autonomi::client::PutError>,
}

/// Result of querying a single peer for a record
#[derive(Debug)]
struct PeerRecordResult {
    peer_id: PeerId,
    result: Result<Option<Record>, NetworkError>,
}

/// Result of a chunk health check from closest 7 nodes
#[derive(Debug)]
struct HealthCheckResult {
    address: String,
    network_address: NetworkAddress,
    peer_results: Vec<PeerRecordResult>,
}

impl ClosestPeerStatus {
    /// Get the peer ID
    pub fn peer_id(&self) -> &PeerId {
        match self {
            ClosestPeerStatus::Holding { peer_id, .. } => peer_id,
            ClosestPeerStatus::NotHolding { peer_id, .. } => peer_id,
            ClosestPeerStatus::FailedQuery { peer_id, .. } => peer_id,
        }
    }

    /// Get the target address
    pub fn target_address(&self) -> &NetworkAddress {
        match self {
            ClosestPeerStatus::Holding { target_address, .. } => target_address,
            ClosestPeerStatus::NotHolding { target_address, .. } => target_address,
            ClosestPeerStatus::FailedQuery { target_address, .. } => target_address,
        }
    }
}

/// Status of a holder for a given address
#[derive(Debug, Clone)]
pub struct HolderStatus {
    peer_id: PeerId,
    target_address: NetworkAddress,
    size: usize,
}

macro_rules! println_if {
    ($cond:expr, $($arg:tt)*) => {
        if $cond {
            println!($($arg)*);
        }
    };
}

// ============================================================================
// Helper Functions for Network Health Scanning
// ============================================================================

/// Parse a string address into a NetworkAddress
fn parse_network_address(addr: &str) -> Result<NetworkAddress> {
    let hex_addr = addr.trim_start_matches("0x");

    // Try parsing as ChunkAddress first
    if let Ok(chunk_addr) = ChunkAddress::from_hex(addr) {
        return Ok(NetworkAddress::from(chunk_addr));
    }

    // Try parsing as PublicKey (could be GraphEntry, Pointer, or Scratchpad)
    if let Ok(public_key) = PublicKey::from_hex(hex_addr) {
        return Ok(NetworkAddress::from(GraphEntryAddress::new(public_key)));
    }

    // Try parsing from NetworkAddress debug format:
    // NetworkAddress::RecordKey("e9d7b3208bcb7ef566102027ca9a7f3ced7c0f8abf87c9bb0ef9130b625572f2") - (...)
    if let Some(start) = addr.find('"')
        && let Some(end) = addr[start + 1..].find('"') {
        let hex_str = &addr[start + 1..start + 1 + end];
        
        // Try parsing the extracted hex string as ChunkAddress
        if let Ok(chunk_addr) = ChunkAddress::from_hex(hex_str) {
            return Ok(NetworkAddress::from(chunk_addr));
        }
    }

    Err(color_eyre::eyre::eyre!(
        "Could not parse address. Expected a hex-encoded ChunkAddress, PublicKey, or NetworkAddress debug format"
    ))
}

/// Perform health check for a single address by querying closest 7 peers
async fn perform_health_check_for_address(
    client: &autonomi::Client,
    net_addr: NetworkAddress,
) -> HealthCheckResult {
    let addr_str = format!("{net_addr:?}");
    
    // Get closest 7 nodes to this address
    let closest_peers = match client.get_closest_to_address(net_addr.clone(), None).await {
        Ok(peers) => peers,
        Err(_e) => {
            return HealthCheckResult {
                address: addr_str,
                network_address: net_addr,
                peer_results: vec![],
            };
        }
    };

    // Query each peer in parallel
    let peer_query_tasks = closest_peers.iter().map(|peer_info| {
        let client = client.clone();
        let net_addr = net_addr.clone();
        let peer_info = peer_info.clone();
        let peer_id = peer_info.peer_id;
        
        async move {
            let result = client.get_record_from_peer(net_addr, peer_info).await;
            PeerRecordResult {
                peer_id,
                result,
            }
        }
    });

    // Execute peer queries in parallel (up to 7 concurrent)
    let peer_results: Vec<PeerRecordResult> = stream::iter(peer_query_tasks)
        .buffer_unordered(7)
        .collect()
        .await;

    HealthCheckResult {
        address: addr_str,
        network_address: net_addr,
        peer_results,
    }
}

/// Execute health checks for multiple addresses in parallel
async fn execute_health_checks_in_parallel(
    client: &autonomi::Client,
    addresses: Vec<NetworkAddress>,
    max_concurrent: usize,
) -> Vec<HealthCheckResult> {
    let health_check_tasks: Vec<_> = addresses
        .into_iter()
        .map(|net_addr| {
            let client = client.clone();
            async move {
                perform_health_check_for_address(&client, net_addr).await
            }
        })
        .collect();

    stream::iter(health_check_tasks)
        .buffer_unordered(max_concurrent)
        .collect()
        .await
}

/// Process health check results and categorize chunks
/// Returns (holder_count, record_data)
fn process_health_check_result(
    health_result: &HealthCheckResult,
    verbose: bool,
) -> (usize, Option<Record>) {
    let mut holder_count = 0;
    let mut record_data: Option<Record> = None;

    for peer_result in &health_result.peer_results {
        match &peer_result.result {
            Ok(Some(record)) => {
                holder_count += 1;
                if record_data.is_none() {
                    record_data = Some(record.clone());
                }
                if verbose {
                    println!("    Peer {} holds the record", peer_result.peer_id);
                }
            }
            Ok(None) => {
                if verbose {
                    println!("    Peer {} does not hold the record", peer_result.peer_id);
                }
            }
            Err(e) => {
                if verbose {
                    println!("    Peer {} query failed: {e}", peer_result.peer_id);
                }
            }
        }
    }
    
    (holder_count, record_data)
}

/// Write repair results to CSV file
fn write_repair_results_to_csv(
    repair_file: &mut std::fs::File,
    results: &[ReuploadResult],
    batch_id: Option<usize>,
) -> Result<()> {
    use std::io::Write;
    
    for result in results {
        match &result.result {
            Ok((cost, _addr)) => {
                if let Some(batch) = batch_id {
                    writeln!(
                        repair_file,
                        "{batch},{},{},success,{cost},",
                        result.address,
                        result.holders_count,
                    )?;
                } else {
                    writeln!(
                        repair_file,
                        "{},{},success,{cost},",
                        result.address,
                        result.holders_count,
                    )?;
                }
            }
            Err(e) => {
                if let Some(batch) = batch_id {
                    writeln!(
                        repair_file,
                        "{batch},{},{},failed,0,\"{}\"",
                        result.address,
                        result.holders_count,
                        e.to_string().replace('"', "\"\"")
                    )?;
                } else {
                    writeln!(
                        repair_file,
                        "{},{},failed,0,\"{}\"",
                        result.address,
                        result.holders_count,
                        e.to_string().replace('"', "\"\"")
                    )?;
                }
            }
        }
    }
    repair_file.flush()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn analyze(
    addr: &str,
    closest_nodes: bool,
    holders: bool,
    nodes_health: bool,
    repair: bool,
    addr_range: Option<char>,
    recursive: bool,
    verbose: bool,
    network_context: NetworkContext,
    json_output_path: Option<PathBuf>,
) -> Result<()> {
    let json_output = json_output_path.is_some();
    println!("Analyzing address: {addr}");

    // then connect to network and check data
    let client =
        crate::actions::connect_to_network_with_config(network_context, Default::default())
            .await
            .map_err(|(err, _)| err)?;

    if nodes_health {
        return print_nodes_health(&client, addr, repair, addr_range, verbose).await;
    }

    if repair && !closest_nodes {
        println!("⚠️  Warning: --repair flag requires --closest-nodes to be set. Enabling --closest-nodes.");
    }

    let results = if recursive {
        println!("Starting recursive analysis...");
        client
            .analyze_address_recursively(addr, verbose && !json_output)
            .await
    } else {
        let mut map = HashMap::new();
        let analysis = client.analyze_address(addr, verbose && !json_output).await;
        map.insert(addr.to_string(), analysis);
        map
    };

    // Only being verbose when handling single address
    let single_addr_verbose = results.len() == 1;

    // Pre-compute closest nodes data if needed
    let closest_nodes_data = if closest_nodes {
        println!(
            "Querying closest peers for all {} addresses...",
            results.len()
        );

        let addresses: Vec<String> = results.keys().cloned().collect();
        let query_tasks = addresses.iter().map(|addr| {
            let client = client.clone();
            let addr = addr.clone();
            async move {
                let result = get_closest_nodes_status(&client, &addr, single_addr_verbose).await;
                (addr, result)
            }
        });

        let closest_nodes_results: Vec<(String, Result<Vec<ClosestPeerStatus>>)> =
            stream::iter(query_tasks)
                .buffered(*CHUNK_DOWNLOAD_BATCH_SIZE)
                .collect()
                .await;

        println!(
            "Completed querying closest peers for all {} addresses.",
            closest_nodes_results.len()
        );

        // Build map, only including successful results
        let map: HashMap<String, Vec<ClosestPeerStatus>> = closest_nodes_results
            .into_iter()
            .filter_map(|(addr, result)| result.ok().map(|statuses| (addr, statuses)))
            .collect();

        Some(map)
    } else {
        None
    };

    // Pre-compute holder data
    let holders_data = if holders {
        println!(
            "Querying kad::get_record holders for all {} addresses...",
            results.len()
        );
        let addresses: Vec<String> = results.keys().cloned().collect();
        let query_tasks = addresses.iter().map(|addr| {
            let client = client.clone();
            let addr = addr.clone();
            async move {
                let result = get_holders_status(&client, &addr, single_addr_verbose).await;
                (addr, result)
            }
        });

        let holders_results: Vec<(String, Result<Vec<HolderStatus>>)> = stream::iter(query_tasks)
            .buffered(*CHUNK_DOWNLOAD_BATCH_SIZE)
            .collect()
            .await;
        println!(
            "Completed querying kad::get_record holders for all {} addresses.",
            holders_results.len()
        );

        // Build map, only including successful results
        let map: HashMap<String, Vec<HolderStatus>> = holders_results
            .into_iter()
            .filter_map(|(addr, result)| result.ok().map(|statuses| (addr, statuses)))
            .collect();

        Some(map)
    } else {
        None
    };

    if closest_nodes_data.is_some() || holders_data.is_some() || recursive {
        print_summary(&results, &closest_nodes_data, &holders_data, verbose)?;
    } else if let Some((_, analysis)) = results.iter().next() {
        match analysis {
            Ok(analysis) => {
                println_if!(verbose, "Analysis successful");
                println!("{analysis}");
            }
            Err(AnalysisError::UnrecognizedInput) => {
                println!("🚨 Could not identify address type!");
                println_if!(
                    verbose,
                    "Provided string was not recognized as a data address, trying other types..."
                );
                try_other_types(addr, verbose);
            }
            Err(e) => {
                println!("Analysis inconclusive: {e}");
            }
        }
    } else {
        println!("No analysis results available.");
    }

    // Handle repair if requested
    if repair && closest_nodes && let Some(ref closest_data) = closest_nodes_data {
        handle_repair(&client, &results, closest_data, verbose).await?;
    }

    if let Some(json_path) = json_output_path {
        output_json(addr, &results, closest_nodes_data, holders_data, &json_path)?;
    }

    Ok(())
}

/// Output analysis results as JSON
fn output_json(
    provided_address: &str,
    results: &HashMap<String, Result<Analysis, AnalysisError>>,
    closest_nodes_data: Option<HashMap<String, Vec<ClosestPeerStatus>>>,
    holders_data: Option<HashMap<String, Vec<HolderStatus>>>,
    output_path: &Path,
) -> Result<()> {
    let mut json_output = json::JsonOutput::new(provided_address.to_string());

    for (address, analysis_result) in results {
        // Parse the address to get the NetworkAddress for distance calculations
        let target_addr = parse_network_address(address)?;

        // Get closest peers data for this address if available
        let closest_peers = closest_nodes_data
            .as_ref()
            .and_then(|map| map.get(address))
            .cloned();
        let holders = holders_data
            .as_ref()
            .and_then(|map| map.get(address))
            .cloned();

        let analyzed = json::AnalyzedAddress::new(
            address.clone(),
            analysis_result,
            closest_peers,
            holders,
            &target_addr,
        );

        json_output.add_address(analyzed);
    }

    // Output JSON to file (append-only with rotation if directory)
    let json_str = serde_json::to_string(&json_output)?;
    let mut writer = json::JsonWriter::new(output_path)?;
    writer.write_json(&json_str)?;

    // Also write the transformed JSON output in parallel
    writer.write_transformed_json(&json_output)?;

    println!("JSON output written to: {}", output_path.display());

    // Print transformed output location
    if output_path.is_dir() {
        println!(
            "Transformed JSON output written to: {}",
            output_path.join("transformedJson.json").display()
        );
    } else {
        let transformed_path = if let Some(parent) = output_path.parent() {
            let file_name = output_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("analyze");
            let ext = output_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("json");
            parent.join(format!("{file_name}Transformed.{ext}"))
        } else {
            let file_name = output_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("analyze");
            let ext = output_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("json");
            Path::new(&format!("{file_name}Transformed.{ext}")).to_path_buf()
        };
        println!(
            "Transformed JSON output written to: {}",
            transformed_path.display()
        );
    }

    Ok(())
}

fn try_other_types(addr: &str, verbose: bool) {
    // local reference to private data
    let try_private_address = crate::user_data::get_local_private_archive_access(addr).ok();
    if let Some(data_map) = try_private_address {
        println!(
            "✅ Identified input as a: Local Private Archive's DataMap local address (only works on your own machine)"
        );
        println_if!(
            verbose,
            "💡 This local address points to a DataMap which is stored on your local machine."
        );
        println_if!(
            verbose,
            "💡 Using this DataMap you can download your Private Archive from the Network."
        );
        println_if!(
            verbose,
            "💡 You can use the `file download` command to download the private data from the DataMap"
        );
        println!("DataMap in hex: {}", data_map.to_hex());
        return;
    }

    // cryptographic keys
    let hex_addr = addr.trim_start_matches("0x");
    let maybe_secret_key = SecretKey::from_hex(hex_addr).ok();
    let maybe_eth_sk = Wallet::new_from_private_key(Default::default(), hex_addr).ok();
    if maybe_secret_key.is_some() || maybe_eth_sk.is_some() {
        println!("🚨 Please keep your secret key safe! Don't use it as a data address!");
        println!("✅ Identified input as a: Secret Key");
        println_if!(
            verbose,
            "💡 A Secret Key is used to sign data or transactions on the Network."
        );
        return;
    }
    let maybe_eth_address = addr.parse::<RewardsAddress>().ok();
    if maybe_eth_address.is_some() {
        println!("✅ Identified input as an: Ethereum Address");
        println_if!(
            verbose,
            "💡 An Ethereum address is a cryptographic identifier for a blockchain account. It can be used to receive funds and rewards on the Network."
        );
        return;
    }

    // multiaddrs
    let maybe_multiaddr = Multiaddr::from_str(addr).ok();
    if maybe_multiaddr.is_some() {
        println!("✅ Identified input as a: Multiaddr");
        println_if!(
            verbose,
            "💡 A Mutliaddr is the url used to connect to a node on the Network."
        );
        return;
    }

    println!("⚠️ Unrecognized input");
}

/// Get record from network with best effort
async fn get_record_with_best_effort(
    client: &autonomi::Client,
    addr: NetworkAddress,
    verbose: bool,
) -> Result<(Option<Record>, Vec<PeerId>)> {
    println_if!(verbose, "Getting record with best effort at address {addr} ...");

    // `Quorum::N(20)` will be better than `Quorum::One` to drain the network to look for the record
    let quorum = std::num::NonZeroUsize::new(KAD_HOLDERS_QUERY_RANGE as usize)
        .map(autonomi::networking::Quorum::N)
        .expect("KAD_HOLDERS_QUERY_RANGE is non-zero");

    match client
        .get_record_and_holders(addr.clone(), quorum)
        .await
    {
        Ok((record, holders)) => Ok((record, holders)),
        Err(NetworkError::GetRecordTimeout(holders)) => {
            println_if!(verbose, "Request timed out, showing partial results");
            Ok((None, holders))
        }
        Err(NetworkError::GetRecordQuorumFailed {
            got_holders,
            expected_holders,
            holders,
        }) => {
            println_if!(
                verbose,
                "Quorum not met (got {got_holders}/{expected_holders}), showing partial results"
            );
            let record = holders.values().next().cloned();
            Ok((record, holders.keys().cloned().collect()))
        }
        Err(e) => {
            Err(color_eyre::eyre::eyre!("Failed to get record holders: {e}"))
        }
    }
}

/// Get holders (along query path) status for an address
///
/// Returns a vector of HolderStatus for the given address
async fn get_holders_status(
    client: &autonomi::Client,
    addr: &str,
    verbose: bool,
) -> Result<Vec<HolderStatus>> {
    let network_addr = parse_network_address(addr)?;
    let (record, holders) = get_record_with_best_effort(client, network_addr.clone(), verbose).await?;

    let mut holders_status = vec![];

    if record.is_none() && holders.is_empty() {
        println!("No record found at address: {addr}");
        return Ok(holders_status);
    }

    let size = if let Some(ref record) = record {
        record.value.len()
    } else {
        0
    };

    // Sort holders by distance to target address
    let mut sorted_holders = holders;
    sorted_holders.sort_by_key(|peer_id| {
        let peer_addr: NetworkAddress = (*peer_id).into();
        network_addr.distance(&peer_addr)
    });

    println!(
        "Found {} holders for record at {addr}",
        sorted_holders.len()
    );
    for peer_id in sorted_holders.iter() {
        holders_status.push(HolderStatus {
            peer_id: *peer_id,
            target_address: network_addr.clone(),
            size,
        });
    }

    Ok(holders_status)
}

/// Get closest peer status for an address
///
/// Returns a vector of ClosestPeerStatus for the given address
pub async fn get_closest_nodes_status(
    client: &autonomi::Client,
    addr: &str,
    verbose: bool,
) -> Result<Vec<ClosestPeerStatus>> {
    println_if!(verbose, "Querying closest peers to address...");

    let target_addr = parse_network_address(addr)?;

    // Get closest group to the target addr
    let peers = client
        .get_closest_to_address(target_addr.clone(), Some(20))
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to get closest peers: {e}"))?;

    println!("Found {} closest peers to: {addr}", peers.len());

    // Query all peers concurrently
    let query_tasks = peers.iter().map(|peer| {
        let client = client.clone();
        let target_addr = target_addr.clone();
        let peer = peer.clone();
        async move {
            match client
                .get_record_from_peer(target_addr.clone(), peer.clone())
                .await
            {
                Ok(Some(record)) => ClosestPeerStatus::Holding {
                    peer_id: peer.peer_id,
                    listen_addrs: peer.addrs.clone(),
                    target_address: target_addr.clone(),
                    size: record.value.len(),
                },
                Ok(None) => ClosestPeerStatus::NotHolding {
                    peer_id: peer.peer_id,
                    listen_addrs: peer.addrs.clone(),
                    target_address: target_addr.clone(),
                },
                Err(e) => ClosestPeerStatus::FailedQuery {
                    peer_id: peer.peer_id,
                    listen_addrs: peer.addrs.clone(),
                    target_address: target_addr.clone(),
                    error: NetworkErrorDisplay::from_network_error(&e),
                },
            }
        }
    });

    let closest_peers_statuses: Vec<ClosestPeerStatus> = stream::iter(query_tasks)
        .buffered(*CHUNK_DOWNLOAD_BATCH_SIZE)
        .collect()
        .await;

    Ok(closest_peers_statuses)
}

/// Statistics about distances from target address to peers
#[derive(Debug, Clone)]
pub struct TargetDistanceStats {
    pub min: u32,
    pub max: u32,
    pub avg: u32,
    pub histogram: Vec<(String, usize)>, // (range_label, count)
}

/// Statistics about distances among peers (peer-to-peer)
#[derive(Debug, Clone)]
pub struct PeerDistanceStats {
    pub min: u32,
    pub max: u32,
    pub avg: u32,
    pub histogram: Vec<(String, usize)>, // (range_label, count)
}

/// Unified row data for the consolidated analysis table
#[derive(Debug)]
struct AnalysisTableRow {
    address: String,
    type_name: String,
    kad_query_status: String,         // "✓ Found" or "✗ Not found"
    size_from_closest: Option<usize>, // Record size from closest peers query
    closest_peers_count: Option<(usize, usize)>, // (holding, total) e.g. (15, 20)
    target_distance_stats: Option<TargetDistanceStats>, // Distance from target to peers
    peer_distance_stats: Option<PeerDistanceStats>, // Distance among peers
    holders_count: Option<(usize, u32)>, // (found holders, KAD_HOLDERS_QUERY_RANGE)
    holders_distance_stats: Option<TargetDistanceStats>, // Distance from target to holder peers
}

/// Calculate distance statistics from target address to each peer
fn calculate_target_distance_stats(
    peer_statuses: &[ClosestPeerStatus],
    target_addr: &NetworkAddress,
) -> TargetDistanceStats {
    let distances: Vec<u32> = peer_statuses
        .iter()
        .map(|status| {
            let peer_addr = NetworkAddress::from(*status.peer_id());
            let distance = target_addr.distance(&peer_addr);
            distance.ilog2().unwrap_or(0)
        })
        .collect();

    let min = *distances.iter().min().unwrap_or(&0);
    let max = *distances.iter().max().unwrap_or(&0);
    let avg = if !distances.is_empty() {
        distances.iter().sum::<u32>() / distances.len() as u32
    } else {
        0
    };

    // Create histogram with 10 buckets, starting wide and getting narrower
    let buckets = vec![
        ("0-150", 0u32, 151u32),
        ("151-200", 151u32, 201u32),
        ("201-220", 201u32, 221u32),
        ("221-230", 221u32, 231u32),
        ("231-235", 231u32, 236u32),
        ("236-240", 236u32, 241u32),
        ("241-245", 241u32, 246u32),
        ("246-250", 246u32, 251u32),
        ("251-253", 251u32, 254u32),
        ("254-256", 254u32, 257u32),
    ];

    let histogram = buckets
        .into_iter()
        .map(|(label, start, end)| {
            let count = distances.iter().filter(|&&d| d >= start && d < end).count();
            (label.to_string(), count)
        })
        .collect();

    TargetDistanceStats {
        min,
        max,
        avg,
        histogram,
    }
}

/// Calculate distance statistics among peers (peer-to-peer distances)
fn calculate_peer_distance_stats(peer_statuses: &[ClosestPeerStatus]) -> PeerDistanceStats {
    let mut distances: Vec<u32> = Vec::new();

    // Calculate all pairwise distances
    for status_i in peer_statuses.iter() {
        let addr_i = NetworkAddress::from(*status_i.peer_id());

        for status_j in peer_statuses.iter() {
            let addr_j = NetworkAddress::from(*status_j.peer_id());

            // Skip self-distances (diagonal)
            if addr_i == addr_j {
                continue;
            }

            let distance = addr_i.distance(&addr_j);
            distances.push(distance.ilog2().unwrap_or(0));
        }
    }

    let min = *distances.iter().min().unwrap_or(&0);
    let max = *distances.iter().max().unwrap_or(&0);
    let avg = if !distances.is_empty() {
        distances.iter().sum::<u32>() / distances.len() as u32
    } else {
        0
    };

    // Create histogram with same 10 buckets as target distances
    let buckets = vec![
        ("0-150", 0u32, 151u32),
        ("151-200", 151u32, 201u32),
        ("201-220", 201u32, 221u32),
        ("221-230", 221u32, 231u32),
        ("231-235", 231u32, 236u32),
        ("236-240", 236u32, 241u32),
        ("241-245", 241u32, 246u32),
        ("246-250", 246u32, 251u32),
        ("251-253", 251u32, 254u32),
        ("254-256", 254u32, 257u32),
    ];

    let histogram = buckets
        .into_iter()
        .map(|(label, start, end)| {
            let count = distances.iter().filter(|&&d| d >= start && d < end).count();
            (label.to_string(), count)
        })
        .collect();

    PeerDistanceStats {
        min,
        max,
        avg,
        histogram,
    }
}

fn print_summary(
    results: &HashMap<String, Result<Analysis, AnalysisError>>,
    closest_nodes_data: &Option<HashMap<String, Vec<ClosestPeerStatus>>>,
    holders_data: &Option<HashMap<String, Vec<HolderStatus>>>,
    verbose: bool,
) -> Result<()> {
    // Build table rows
    let mut table_rows = Vec::new();

    for (address, analysis_result) in results {
        let (type_name, kad_status) = match analysis_result {
            Ok(analysis) => {
                let type_str = match analysis {
                    Analysis::Chunk(_) => "Chunk",
                    Analysis::GraphEntry(_) => "GraphEntry",
                    Analysis::Pointer(_) => "Pointer",
                    Analysis::Scratchpad(_) => "Scratchpad",
                    Analysis::Register { .. } => "Register",
                    Analysis::DataMap { .. } => "DataMap",
                    Analysis::RawDataMap { .. } => "RawDataMap",
                    Analysis::PublicArchive { .. } => "PublicArchive",
                    Analysis::PrivateArchive(_) => "PrivateArchive",
                };
                (type_str.to_string(), "✓ Found".to_string())
            }
            Err(_) => ("Unknown".to_string(), "✗ Not found".to_string()),
        };

        // Get size from closest nodes data if available
        let size = closest_nodes_data
            .as_ref()
            .and_then(|map| map.get(address))
            .and_then(|peers| {
                peers.iter().find_map(|p| match p {
                    ClosestPeerStatus::Holding { size, .. } => Some(*size),
                    _ => None,
                })
            });

        // Initialize with defaults
        let mut closest_peers_count = None;
        let mut target_distance_stats = None;
        let mut peer_distance_stats = None;
        let mut holders_count = None;
        let mut holders_distance_stats = None;

        // Set values if closest nodes data exists for this address
        if let Some(closest_nodes_map) = closest_nodes_data
            && let Some(peer_statuses) = closest_nodes_map.get(address)
        {
            let total = peer_statuses.len();
            let holding_count = peer_statuses
                .iter()
                .filter(|s| matches!(s, ClosestPeerStatus::Holding { .. }))
                .count();

            closest_peers_count = Some((holding_count, total));

            // Get target address from the first peer status
            if let Some(target_addr) = peer_statuses.first().map(|s| s.target_address()) {
                target_distance_stats =
                    Some(calculate_target_distance_stats(peer_statuses, target_addr));
            }

            // Calculate peer-to-peer distances
            peer_distance_stats = Some(calculate_peer_distance_stats(peer_statuses));
        }

        // Set values if holders data exists for this address
        if let Some(holders_map) = holders_data
            && let Some(holder_statuses) = holders_map.get(address)
        {
            let found_holders = holder_statuses.len();
            holders_count = Some((found_holders, KAD_HOLDERS_QUERY_RANGE));

            // Calculate holder distance stats
            if let Some(target_addr) = holder_statuses.first().map(|h| &h.target_address) {
                // Convert HolderStatus to ClosestPeerStatus for reusing distance calculation
                let holder_peer_statuses: Vec<ClosestPeerStatus> = holder_statuses
                    .iter()
                    .map(|h| ClosestPeerStatus::Holding {
                        peer_id: h.peer_id,
                        target_address: h.target_address.clone(),
                        listen_addrs: Vec::new(),
                        size: h.size,
                    })
                    .collect();

                holders_distance_stats = Some(calculate_target_distance_stats(
                    &holder_peer_statuses,
                    target_addr,
                ));
            }
        }

        table_rows.push(AnalysisTableRow {
            address: address.clone(),
            type_name,
            kad_query_status: kad_status,
            size_from_closest: size,
            closest_peers_count,
            target_distance_stats,
            peer_distance_stats,
            holders_count,
            holders_distance_stats,
        });
    }

    // Print the consolidated table
    let with_closest_nodes = closest_nodes_data.is_some();
    let with_holders = holders_data.is_some();
    print_consolidated_table(&table_rows, with_closest_nodes, with_holders);

    // Print verbose details if requested
    if verbose && (with_closest_nodes || with_holders) {
        print_verbose_details(&table_rows, closest_nodes_data, holders_data)?;
    }

    Ok(())
}

/// Print the consolidated analysis table
fn print_consolidated_table(
    rows: &[AnalysisTableRow],
    with_closest_nodes: bool,
    with_holders: bool,
) {
    let mut table = Table::new();

    // Build header based on available data
    let mut header = vec![
        Cell::new("Address").set_alignment(CellAlignment::Left),
        Cell::new("Type").set_alignment(CellAlignment::Left),
        Cell::new("Kad Query").set_alignment(CellAlignment::Left),
    ];

    if with_closest_nodes {
        header.extend(vec![
            Cell::new("Size (closest)").set_alignment(CellAlignment::Left),
            Cell::new("Closest Peers").set_alignment(CellAlignment::Left),
            Cell::new("Closest Distances (ilog2)").set_alignment(CellAlignment::Left),
        ]);
    }

    if with_holders {
        header.extend(vec![
            Cell::new("Holders").set_alignment(CellAlignment::Left),
            Cell::new("Holders Distances (ilog2)").set_alignment(CellAlignment::Left),
        ]);
    }

    table.set_header(header);

    // Add rows
    for row in rows {
        let mut cells = vec![
            Cell::new(&row.address),
            Cell::new(&row.type_name),
            Cell::new(&row.kad_query_status),
        ];

        if with_closest_nodes {
            let size_display = row
                .size_from_closest
                .map(|s| s.to_string())
                .unwrap_or_else(|| "N/A".to_string());

            let closest_display = row
                .closest_peers_count
                .map(|(h, t)| format!("{h}/{t}"))
                .unwrap_or_else(|| "N/A".to_string());

            let closest_distance_display = row
                .target_distance_stats
                .as_ref()
                .map(|stats| format!("min={} avg={} max={}", stats.min, stats.avg, stats.max))
                .unwrap_or_else(|| "N/A".to_string());

            cells.push(Cell::new(size_display));
            cells.push(Cell::new(closest_display));
            cells.push(Cell::new(closest_distance_display));
        }

        if with_holders {
            let holders_display = row
                .holders_count
                .map(|(h, max)| format!("{h}/{max}"))
                .unwrap_or_else(|| "N/A".to_string());

            let holders_distance_display = row
                .holders_distance_stats
                .as_ref()
                .map(|stats| format!("min={} avg={} max={}", stats.min, stats.avg, stats.max))
                .unwrap_or_else(|| "N/A".to_string());

            cells.push(Cell::new(holders_display));
            cells.push(Cell::new(holders_distance_display));
        }

        table.add_row(cells);
    }

    println!("\n{table}");
    println!("\nTotal: {} addresses", rows.len());
}

/// Print verbose details including histogram breakdown and peer IDs
fn print_verbose_details(
    rows: &[AnalysisTableRow],
    closest_nodes_data: &Option<HashMap<String, Vec<ClosestPeerStatus>>>,
    holders_data: &Option<HashMap<String, Vec<HolderStatus>>>,
) -> Result<()> {
    let rows_with_data: Vec<_> = rows
        .iter()
        .filter(|r| r.closest_peers_count.is_some() || r.holders_count.is_some())
        .collect();

    if rows_with_data.is_empty() {
        return Ok(());
    }

    println!("\n{}", "=".repeat(80));
    println!("Verbose Analysis Details");
    println!("{}", "=".repeat(80));
    println!();

    for row in rows_with_data {
        println!("Address: {}", row.address);
        println!(
            "  Type: {} | Kad Query: {}",
            row.type_name, row.kad_query_status
        );
        println!();

        // ========== CLOSEST PEERS ==========
        if row.closest_peers_count.is_some() {
            println!("  {}:", "━".repeat(40));
            println!("  CLOSEST PEERS");
            println!("  {}:", "━".repeat(40));

            if let Some((holding, total)) = row.closest_peers_count {
                println!("  Closest Peers Holding: {holding}/{total}");
            }

            // Print target distance stats if available
            if let Some(ref stats) = row.target_distance_stats {
                println!(
                    "  Target Distances (ilog2): min={} avg={} max={}",
                    stats.min, stats.avg, stats.max
                );

                // Print detailed histogram breakdown
                print!("  Target Distance histogram: ");
                for (i, (range, count)) in stats.histogram.iter().enumerate() {
                    if i > 0 {
                        print!("  ");
                    }
                    print!("[{range}]: {count}");
                }
                println!();
            }

            // Print peer distance stats if available
            if let Some(ref stats) = row.peer_distance_stats {
                println!(
                    "  Peer-to-Peer Distances (ilog2): min={} avg={} max={}",
                    stats.min, stats.avg, stats.max
                );

                // Print detailed histogram breakdown
                print!("  Peer-to-Peer Distance histogram: ");
                for (i, (range, count)) in stats.histogram.iter().enumerate() {
                    if i > 0 {
                        print!("  ");
                    }
                    print!("[{range}]: {count}");
                }
                println!();
            }

            // Compute holding peer IDs with distances
            if let Some(closest_nodes_map) = closest_nodes_data
                && let Some(peer_statuses) = closest_nodes_map.get(&row.address)
            {
                let holding_peers: Vec<(PeerId, u32)> = peer_statuses
                    .iter()
                    .filter_map(|s| match s {
                        ClosestPeerStatus::Holding {
                            peer_id,
                            target_address,
                            ..
                        } => {
                            let peer_addr = NetworkAddress::from(*peer_id);
                            let distance = target_address.distance(&peer_addr);
                            Some((*peer_id, distance.ilog2().unwrap_or(0)))
                        }
                        _ => None,
                    })
                    .collect();

                if !holding_peers.is_empty() {
                    println!("  Closest Peers Holding ({}):", holding_peers.len());
                    for (peer_id, distance) in &holding_peers {
                        println!("    - {peer_id} (distance: {distance})");
                    }
                }

                // Count not holding peers
                let not_holding_count = peer_statuses
                    .iter()
                    .filter(|s| matches!(s, ClosestPeerStatus::NotHolding { .. }))
                    .count();

                if not_holding_count > 0 {
                    println!("  Closest Peers Not Holding: {not_holding_count}");
                }

                // Collect failed query peers with errors
                let failed_peers: Vec<(PeerId, u32, &NetworkErrorDisplay)> = peer_statuses
                    .iter()
                    .filter_map(|s| match s {
                        ClosestPeerStatus::FailedQuery {
                            peer_id,
                            target_address,
                            error,
                            ..
                        } => {
                            let peer_addr = NetworkAddress::from(*peer_id);
                            let distance = target_address.distance(&peer_addr);
                            Some((*peer_id, distance.ilog2().unwrap_or(0), error))
                        }
                        _ => None,
                    })
                    .collect();

                if !failed_peers.is_empty() {
                    println!("  Failed Queries ({}):", failed_peers.len());
                    for (peer_id, distance, error) in &failed_peers {
                        println!("    - {peer_id} (distance: {distance}) - Error: {error:?}");
                    }
                }
            }
            println!();
        }

        // ========== HOLDERS ==========
        if row.holders_count.is_some() {
            println!("  {}:", "━".repeat(40));
            println!("  HOLDERS");
            println!("  {}:", "━".repeat(40));

            if let Some(size) = row.size_from_closest {
                println!("  Record Size (from closest): {size} bytes");
            }

            if let Some((found, limit)) = row.holders_count {
                println!("  Holders Found: {found}/{limit}");
            }

            // Print holders target distance stats if available
            if let Some(ref stats) = row.holders_distance_stats {
                println!(
                    "  Holder Target Distances (ilog2): min={} avg={} max={}",
                    stats.min, stats.avg, stats.max
                );

                // Print detailed histogram breakdown
                print!("  Holder Target Distance histogram: ");
                for (i, (range, count)) in stats.histogram.iter().enumerate() {
                    if i > 0 {
                        print!("  ");
                    }
                    print!("[{range}]: {count}");
                }
                println!();
            }

            // List all holder peer IDs with distances
            if let Some(holders_map) = holders_data
                && let Some(holder_statuses) = holders_map.get(&row.address)
                && !holder_statuses.is_empty()
            {
                // Sort by distance
                let mut holders_with_distance: Vec<(PeerId, u32)> = holder_statuses
                    .iter()
                    .map(|h| {
                        let peer_addr = NetworkAddress::from(h.peer_id);
                        let distance = h.target_address.distance(&peer_addr);
                        (h.peer_id, distance.ilog2().unwrap_or(0))
                    })
                    .collect();

                holders_with_distance.sort_by_key(|(_, dist)| *dist);

                println!("  Holder Peer IDs ({}):", holders_with_distance.len());
                for (peer_id, distance) in &holders_with_distance {
                    println!("    - {peer_id} (distance: {distance})");
                }
            }
            println!();
        }

        println!("{}", "-".repeat(80));
        println!();
    }

    Ok(())
}

/// Thread-safe storage for chunk health tracking
#[derive(Debug, Clone)]
struct ChunkHealthLists {
    white_list: Arc<Mutex<HashSet<String>>>,
    bad_list: Arc<Mutex<HashSet<String>>>,
}

impl ChunkHealthLists {
    fn new() -> Self {
        Self {
            white_list: Arc::new(Mutex::new(HashSet::new())),
            bad_list: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn add_to_white_list(&self, addr: String) {
        let mut white = self.white_list.lock().expect("Failed to access global white_list");
        let mut bad = self.bad_list.lock().expect("Failed to access global bad_list");
        bad.remove(&addr);
        white.insert(addr);
    }

    fn add_to_bad_list(&self, addr: String) {
        let white = self.white_list.lock().expect("Failed to access global white_list");
        let mut bad = self.bad_list.lock().expect("Failed to access global bad_list");
        if !white.contains(&addr) {
            bad.insert(addr);
        }
    }

    fn is_in_any_list(&self, addr: &str) -> bool {
        let white = self.white_list.lock().expect("Failed to access global white_list");
        let bad = self.bad_list.lock().expect("Failed to access global bad_list");
        white.contains(addr) || bad.contains(addr)
    }

    fn write_to_csv(&self, white_path: &str, bad_path: &str) -> Result<()> {
        use std::io::Write;

        let white = self.white_list.lock().expect("Failed to access global white_list");
        let bad = self.bad_list.lock().expect("Failed to access global bad_list");

        let mut white_file = std::fs::File::create(white_path)?;
        writeln!(white_file, "chunk_address")?;
        for addr in white.iter() {
            writeln!(white_file, "{addr}")?;
        }

        let mut bad_file = std::fs::File::create(bad_path)?;
        writeln!(bad_file, "chunk_address")?;
        for addr in bad.iter() {
            writeln!(bad_file, "{addr}")?;
        }

        Ok(())
    }

    fn read_from_csv(&mut self, white_path: &str, bad_path: &str) -> Result<()> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        if let Ok(file) = File::open(white_path) {
            let reader = BufReader::new(file);
            let mut white = self.white_list.lock().expect("Failed to access global white_list");
            for addr in reader.lines().skip(1).flatten() {
                // Skip header
                let addr = addr.trim();
                if !addr.is_empty() {
                    white.insert(addr.to_string());
                }
            }
        }

        if let Ok(file) = File::open(bad_path) {
            let reader = BufReader::new(file);
            let mut bad = self.bad_list.lock().expect("Failed to access global bad_list");
            for addr in reader.lines().skip(1).flatten() {
                // Skip header
                let addr = addr.trim();
                if !addr.is_empty() {
                    bad.insert(addr.to_string());
                }
            }
        }

        Ok(())
    }
}

/// Result of a single network scan round
#[derive(Debug)]
struct NetworkScanResult {
    round_id: usize,
    new_white: Vec<String>,
    new_bad: HashSet<NetworkAddress>,
}

/// Perform a single network scan round
async fn perform_network_scan_round(
    client: &autonomi::Client,
    round_id: usize,
    health_lists: ChunkHealthLists,
    addr_range: Option<char>,
    verbose: bool,
) -> Result<NetworkScanResult> {
    // Generate a random target address, optionally filtering by first hex character
    let mut rng = rand::thread_rng();
    let target_addr = loop {
        let addr = NetworkAddress::from(XorName::random(&mut rng));
        
        // If no filter is specified, accept any address
        if addr_range.is_none() {
            break addr;
        }
        
        // Extract the first hex character from the address string
        // Format like: NetworkAddress::ChunkAddress(d381843d...) - (9d8bbfc9...)
        // We need to extract the hex string after the first '('
        let addr_str = format!("{addr:?}");
        let first_hex_char = addr_str
            .split('(')
            .nth(1) // Get the part after the first '('
            .and_then(|hex_part| {
                hex_part
                    .chars()
                    .find(|c| c.is_ascii_hexdigit())
                    .map(|c| c.to_ascii_lowercase())
            });
        
        // Check if it matches the filter
        if let (Some(filter_char), Some(addr_first_char)) = (addr_range, first_hex_char)
            && filter_char.to_ascii_lowercase() == addr_first_char {
                break addr;
        }
    };

    if verbose {
        println!("Round {round_id}: Target address: {target_addr:?}");
    }

    // Get closest 7 nodes to the target
    let peers = client
        .get_closest_to_address(target_addr.clone(), None)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to get closest peers: {e}"))?;

    if verbose {
        println!("Round {round_id}: Found {} closest peers", peers.len());
    }

    // Collect storage proofs from each peer in parallel
    const MAX_PARALLEL_PEER_QUERIES: usize = 7; // Query up to 7 peers concurrently
    
    let tasks = peers.iter().map(|peer| {
        let client = client.clone();
        let peer_clone = peer.clone();
        let peer_id = peer.peer_id;
        let peer_target_addr = NetworkAddress::from(peer_id);
        let nonce: u64 = rand::random();
        let difficulty = 20;
        
        async move {
            let result = client
                .get_storage_proofs_from_peer(peer_target_addr, peer_clone, nonce, difficulty)
                .await;
            (peer_id, result)
        }
    });
    
    let peer_results: PeerResults = 
        stream::iter(tasks)
            .buffer_unordered(MAX_PARALLEL_PEER_QUERIES)
            .collect()
            .await;
    
    // Process results and populate chunk_counts
    let mut chunk_counts: HashMap<NetworkAddress, usize> = HashMap::new();
    
    for (peer_id, result) in peer_results {
        match result {
            Ok(storage_proofs) => {
                if verbose {
                    println!(
                        "Round {round_id}: Peer {peer_id} returned {} proofs",
                        storage_proofs.len()
                    );
                }

                for (addr, proof_result) in &storage_proofs {
                    if proof_result.is_ok() {
                        let addr_str = format!("{addr:?}").to_string();
                        // Skip if already in either list
                        if !health_lists.is_in_any_list(&addr_str) {
                            *chunk_counts.entry(addr.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
            Err(e) => {
                if verbose {
                    println!("Round {round_id}: Failed to query peer {peer_id}: {e}");
                }
            }
        }
    }

    // Classify chunks based on occurrence count
    let mut new_white = Vec::new();
    let mut new_bad = HashSet::new();

    for (addr, count) in chunk_counts {
        let addr_str = format!("{addr:?}").to_string();
        if count >= 3 {
            health_lists.add_to_white_list(addr_str.clone());
            new_white.push(addr_str);
        } else {
            health_lists.add_to_bad_list(addr_str);
            let _ = new_bad.insert(addr);
        }
    }

    Ok(NetworkScanResult {
        round_id,
        new_white,
        new_bad,
    })
}

/// Perform network health scanning with multiple rounds
async fn perform_network_health_scan(
    client: &autonomi::Client,
    num_rounds: u32,
    repair: bool,
    addr_range: Option<char>,
    verbose: bool,
) -> Result<()> {
    println!("\n{}", "=".repeat(80));
    println!("NETWORK HEALTH SCAN: {num_rounds} rounds");
    if let Some(filter_char) = addr_range {
        // Validate the filter character
        if !filter_char.is_ascii_hexdigit() {
            return Err(color_eyre::eyre::eyre!(
                "Invalid addr_range '{}': must be a hex character (0-9, a-f)", 
                filter_char
            ));
        }
        println!("Address range filter: only targeting addresses starting with '{}'", 
            filter_char.to_ascii_lowercase());
    }
    println!("{}", "=".repeat(80));

    let mut health_lists = ChunkHealthLists::new();

    // Try to load existing lists from disk
    let white_csv = "chunk_whitelist.csv";
    let bad_csv = "chunk_badlist.csv";

    let initial_bad_list_check = if std::path::Path::new(white_csv).exists() || std::path::Path::new(bad_csv).exists() {
        println!("Loading existing health lists from disk...");
        health_lists.read_from_csv(white_csv, bad_csv)?;
        let white_count = health_lists.white_list.lock().expect("Failed to access global white_list").len();
        let bad_count = health_lists.bad_list.lock().expect("Failed to access global bad_list").len();
        println!("Loaded {white_count} white-listed and {bad_count} bad-listed chunks");
        
        // Get snapshot of bad_list for initial health check
        if bad_count > 0 {
            let bad = health_lists.bad_list.lock().expect("Failed to access global bad_list");
            Some(bad.iter().cloned().collect::<Vec<String>>())
        } else {
            None
        }
    } else {
        None
    };

    // Perform initial health check on existing bad_list entries
    if let Some(bad_entries) = initial_bad_list_check {
        println!("\n{}", "=".repeat(80));
        println!("INITIAL CHECK: Verifying {} existing bad-listed chunks", bad_entries.len());
        println!("{}", "=".repeat(80));

        // Convert string addresses to NetworkAddress using helper
        let bad_addresses: Vec<NetworkAddress> = bad_entries
            .iter()
            .filter_map(|addr_str| match parse_network_address(addr_str) {
                Ok(addr) => Some(addr),
                Err(e) => {
                    println!("When parsing NetworkAddress from {addr_str}, failed with error {e:?}");
                    None
                }
            })
            .collect();

        if !bad_addresses.is_empty() {
            println!("\nChecking {} bad-listed chunks in batches...", bad_addresses.len());

            // Process in batches to avoid memory spikes with large address lists
            const BATCH_SIZE: usize = 60;
            const MAX_PARALLEL_INITIAL_CHECKS: usize = 20;
            
            let mut total_moved_to_white = 0;
            let mut total_still_bad = 0;
            let mut all_reupload_results: Vec<ReuploadResult> = Vec::new();
            
            // Prepare wallet and payment option once if repair is enabled
            let payment_option = if repair {
                let wallet = load_wallet(client.evm_network())?;
                Some(PaymentOption::from(&wallet))
            } else {
                None
            };

            for (batch_idx, batch) in bad_addresses.chunks(BATCH_SIZE).enumerate() {
                let batch_num = batch_idx + 1;
                let total_batches = (bad_addresses.len() + BATCH_SIZE - 1) / BATCH_SIZE;
                
                println!("\n{}", "=".repeat(80));
                println!("Processing batch {}/{} ({} addresses)...", batch_num, total_batches, batch.len());
                println!("{}", "=".repeat(80));

                // Execute health checks for this batch
                let health_check_results = execute_health_checks_in_parallel(
                    client,
                    batch.to_vec(),
                    MAX_PARALLEL_INITIAL_CHECKS,
                ).await;

                // Process results for this batch
                let mut chunks_to_repair: Vec<RecordToRepair> = Vec::new();
                let mut batch_moved_to_white = 0;
                let mut batch_still_bad = 0;

                for health_result in health_check_results {
                    // Use helper to process health check result
                    let (holder_count, record_data) = process_health_check_result(&health_result, verbose);

                    // Evaluate health
                    if holder_count >= 3 {
                        // Good health, move to white list
                        health_lists.add_to_white_list(health_result.address.clone());
                        batch_moved_to_white += 1;
                        println!("  ✅ {} - Healthy ({holder_count}/{} holders), moved to white list", 
                            health_result.address, health_result.peer_results.len());
                    } else if let Some(record) = record_data {
                        // Unhealthy, but we have data
                        batch_still_bad += 1;
                        if repair {
                            println!("  ⚠️  {} - Unhealthy ({holder_count}/{} holders), queuing for repair", 
                                health_result.address, health_result.peer_results.len());
                            chunks_to_repair.push(RecordToRepair {
                                address: health_result.address,
                                holders_count: holder_count,
                                record_data: record,
                            });
                        } else {
                            println!("  ⚠️  {} - Unhealthy ({holder_count}/{} holders), repair not enabled", 
                                health_result.address, health_result.peer_results.len());
                        }
                    } else {
                        // Try kad query as fallback
                        batch_still_bad += 1;
                        if repair {
                            match get_record_with_best_effort(client, health_result.network_address, verbose).await {
                                Ok((Some(record), _holders)) => {
                                    if verbose {
                                        println!("   ✅ Retrieved record {} via kad query", health_result.address);
                                    }
                                    chunks_to_repair.push(RecordToRepair {
                                        address: health_result.address.clone(),
                                        holders_count: 0,
                                        record_data: record,
                                    });
                                }
                                Ok((None, _holders)) => {
                                    println!("  ❌ {} - No record data found", health_result.address);
                                }
                                Err(e) => {
                                    println!("  ❌ {} - Error: {e}", health_result.address);
                                }
                            }
                        } else {
                            println!("  ❌ {} - No record data found from any of {} peers", 
                                health_result.address, health_result.peer_results.len());
                        }
                    }
                }

                total_moved_to_white += batch_moved_to_white;
                total_still_bad += batch_still_bad;

                println!("\nBatch {batch_num} complete: {batch_moved_to_white} moved to white list, {batch_still_bad} still unhealthy");

                // Repair unhealthy chunks in this batch immediately
                if repair && !chunks_to_repair.is_empty() {
                    println!("\nRepairing {} chunks from batch {batch_num}...", chunks_to_repair.len());

                    const MAX_PARALLEL_UPLOADS: usize = 10;
                    let reupload_results = reupload_chunks_in_parallel(
                        client,
                        chunks_to_repair,
                        payment_option.clone().unwrap(),
                        MAX_PARALLEL_UPLOADS,
                    ).await;

                    // Print results for this batch
                    for result in &reupload_results {
                        match &result.result {
                            Ok((cost, _addr)) => println!("  ✅ {} (cost: {cost})", result.address),
                            Err(e) => println!("  ❌ {} - Failed: {e}", result.address),
                        }
                    }

                    // Accumulate results for final report
                    all_reupload_results.extend(reupload_results);
                }

                // Write updated lists to disk after each batch
                if verbose {
                    println!("Updating health lists on disk after batch {batch_num}...");
                }
                health_lists.write_to_csv(white_csv, bad_csv)?;
            }

            println!("\n{}", "=".repeat(80));
            println!("INITIAL CHECK COMPLETE");
            println!("{}", "=".repeat(80));
            println!("Total: {total_moved_to_white} moved to white list, {total_still_bad} still unhealthy");

            // Write consolidated repair report if repairs were performed
            if repair && !all_reupload_results.is_empty() {
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let repair_csv = format!("initial_repair_{timestamp}.csv");
                let mut repair_file = std::fs::File::create(&repair_csv)?;
                use std::io::Write;
                writeln!(repair_file, "address,original_holders_count,upload_status,cost_paid,error")?;
                write_repair_results_to_csv(&mut repair_file, &all_reupload_results, None)?;
                println!("\nInitial repair report saved to: {repair_csv}");
            }
        }
    }

    // Process scans in batches
    let batch_size = *CHUNK_DOWNLOAD_BATCH_SIZE as u32;
    let total_batches = num_rounds.div_ceil(batch_size);
    println!("\nStarting {num_rounds} scan rounds in {total_batches} batches (max {batch_size} concurrent per batch)...");

    let mut total_new_white = 0;
    let mut total_new_bad = 0;
    let mut total_repaired = 0;
    
    // Process wallet once if repair is enabled
    let payment_option = if repair {
        let wallet = load_wallet(client.evm_network())?;
        Some(PaymentOption::from(&wallet))
    } else {
        None
    };

    // Open repair report file if repair is enabled
    let mut repair_file = if repair {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let repair_csv = format!("network_scan_repair_{timestamp}.csv");
        let mut file = std::fs::File::create(&repair_csv)?;
        use std::io::Write;
        writeln!(file, "batch,address,original_holders_count,upload_status,cost_paid,error")?;
        println!("Repair report will be saved to: {repair_csv}");
        Some((file, repair_csv))
    } else {
        None
    };

    for batch_num in 0..total_batches {
        let batch_start = batch_num * (*CHUNK_DOWNLOAD_BATCH_SIZE  as u32);
        let batch_end = ((batch_num + 1) * (*CHUNK_DOWNLOAD_BATCH_SIZE  as u32)).min(num_rounds);

        println!("\n{}", "=".repeat(80));
        println!("BATCH {}/{total_batches}: Processing rounds {batch_start} to {}", 
            batch_num + 1, batch_end - 1);
        println!("{}", "=".repeat(80));

        // Create tasks for this batch
        let tasks: Vec<_> = (batch_start..batch_end)
            .map(|round_id| {
                let client = client.clone();
                let lists = health_lists.clone();
                async move {
                    perform_network_scan_round(&client, round_id as usize, lists, addr_range, verbose).await
                }
            })
            .collect();

        // Execute scans in parallel for this batch
        let scan_results: Vec<Result<NetworkScanResult>> = stream::iter(tasks)
            .buffer_unordered(*CHUNK_DOWNLOAD_BATCH_SIZE)
            .collect()
            .await;

        // Collect bad chunks from this batch
        let mut batch_bad_chunks: HashSet<NetworkAddress> = HashSet::new();
        let mut batch_new_white = 0;
        let mut batch_new_bad = 0;

        for result in scan_results {
            match result {
                Ok(scan_result) => {
                    batch_new_white += scan_result.new_white.len();
                    batch_new_bad += scan_result.new_bad.len();
                    batch_bad_chunks.extend(scan_result.new_bad.clone());
                    
                    if verbose {
                        println!(
                            "  Round {} completed: {} new white, {} new bad",
                            scan_result.round_id,
                            scan_result.new_white.len(),
                            scan_result.new_bad.len()
                        );
                    }
                }
                Err(e) => {
                    println!("  Scan round failed: {e}");
                }
            }
        }

        total_new_white += batch_new_white;
        total_new_bad += batch_new_bad;

        println!("\nBatch {} summary: {batch_new_white} new white-listed, {batch_new_bad} new bad-listed chunks", 
            batch_num + 1);

        // Check and repair bad chunks from this batch
        if !batch_bad_chunks.is_empty() {
            println!("\nChecking {} bad-listed chunks from batch {} in parallel...", batch_bad_chunks.len(), batch_num + 1);
            
            // Execute health checks in parallel using helper
            let batch_bad_vec: Vec<NetworkAddress> = batch_bad_chunks.into_iter().collect();
            let health_check_results = execute_health_checks_in_parallel(
                client,
                batch_bad_vec,
                *CHUNK_DOWNLOAD_BATCH_SIZE,
            ).await;

            // Process health check results
            let mut chunks_to_repair: Vec<RecordToRepair> = Vec::new();

            println!("\nProcessing {} health check results...", health_check_results.len());

            for health_result in health_check_results {
                // Use helper to process health check result
                let (holder_count, record_data) = process_health_check_result(&health_result, verbose);

                // Evaluate health based on holder count
                if holder_count >= 3 {
                    // Good health, move to white list
                    health_lists.add_to_white_list(health_result.address.clone());
                    if verbose {
                        println!("  ✅ {} - Healthy ({holder_count}/{} holders), moved to white list", 
                            health_result.address, health_result.peer_results.len());
                    } else {
                        println!("  ✅ {} - Healthy ({holder_count}/{} holders)", 
                            health_result.address, health_result.peer_results.len());
                    }
                } else if let Some(record) = record_data {
                    // Unhealthy, but we have record
                    if repair {
                        println!("  ⚠️  {} - Unhealthy ({holder_count}/{} holders), queuing for repair", 
                            health_result.address, health_result.peer_results.len());

                        chunks_to_repair.push(RecordToRepair {
                            address: health_result.address,
                            holders_count: holder_count,
                            record_data: record,
                        });
                    } else {
                        println!("  ⚠️  {} - Unhealthy ({holder_count}/{} holders), repair not enabled", 
                            health_result.address, health_result.peer_results.len());
                    }
                } else {
                    // Try to get the record data via kad query
                    match get_record_with_best_effort(client, health_result.network_address, verbose).await {
                        Ok((Some(record), _holders)) => {
                            if verbose {
                                println!("   ✅ Retrieved record {} with {} bytes via kad query", health_result.address, record.value.len());
                            }
                            chunks_to_repair.push(RecordToRepair {
                                address: health_result.address.clone(),
                                // Holders among closest_7
                                holders_count: 0,
                                record_data: record,
                            });
                        }
                        Ok((None, _holders)) => {
                            println!("   ❌ Failed to retrieve record of {} via kad query", health_result.address);
                        }
                        Err(e) => {
                            println!("   ❌ Error retrieving record {} via kad query {e}", health_result.address);
                        }
                    }


                    // No record data found
                    println!("  ❌ {} - No record data found from any of {} peers", 
                        health_result.address, health_result.peer_results.len());
                }
            }

            // Re-upload chunks if repair is enabled
            if repair && !chunks_to_repair.is_empty()
                && let Some(payment_option) = payment_option.clone() {
                println!("\nBatch {}: Repairing {} chunks in parallel...", batch_num + 1, chunks_to_repair.len());

                let reupload_results = reupload_chunks_in_parallel(
                    client,
                    chunks_to_repair,
                    payment_option,
                    MAX_PARALLEL_UPLOADS,
                ).await;

                // Write results to repair report
                if let Some((ref mut file, _)) = repair_file {
                    for result in &reupload_results {
                        match &result.result {
                            Ok((cost, _addr)) => println!("  ✅ {} (cost: {cost})", result.address),
                            Err(e) => println!("  ❌ {} - Failed: {e}", result.address),
                        }
                    }
                    write_repair_results_to_csv(file, &reupload_results, Some(batch_num as usize + 1))?;
                }

                total_repaired += reupload_results.len();
            }
        }

        // Flush lists to disk after each batch
        println!("\nFlushing health lists to disk...");
        health_lists.write_to_csv(white_csv, bad_csv)?;
        
        let current_white = health_lists.white_list.lock().expect("Failed to access global white_list").len();
        let current_bad = health_lists.bad_list.lock().expect("Failed to access global bad_list").len();
        println!("Current status: {current_white} white-listed, {current_bad} bad-listed chunks");
    }

    // Close repair file if opened
    if let Some((file, path)) = repair_file {
        drop(file);
        println!("\nRepair report saved to: {path}");
    }

    println!("\n{}", "=".repeat(80));
    println!("ALL BATCHES COMPLETE");
    println!("Total: {total_new_white} new white-listed, {total_new_bad} new bad-listed, {total_repaired} repaired");
    println!("{}", "=".repeat(80));

    let final_white_count = health_lists.white_list.lock().expect("Failed to access global white_list").len();
    let final_bad_count = health_lists.bad_list.lock().expect("Failed to access global bad_list").len();
    println!("\n{}", "=".repeat(80));
    println!("Final status: {final_white_count} white-listed, {final_bad_count} bad-listed chunks");
    println!("{}", "=".repeat(80));

    Ok(())
}

async fn print_nodes_health(
    client: &autonomi::Client, 
    addr: &str, 
    repair: bool, 
    addr_range: Option<char>,
    verbose: bool
) -> Result<()> {
    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }

    let hex_addr = addr.trim_start_matches("0x");

    // Check if the input is a u32 for network scanning
    if let Ok(num_rounds) = hex_addr.parse::<u32>() {
        return perform_network_health_scan(client, num_rounds, repair, addr_range, verbose).await;
    }

    println_if_verbose!("Querying closest peers to address {addr:?}...");

    // Only accept ChunkAddress for single node health check
    let target_addr = if let Ok(chunk_addr) = ChunkAddress::from_hex(hex_addr) {
        println_if_verbose!("Identified as ChunkAddress");
        NetworkAddress::from(chunk_addr)
    } else {
        return Err(color_eyre::eyre::eyre!(
            "nodes-health requires either a number (for network scan rounds) or a hex-encoded ChunkAddress"
        ));
    };

    // Get closest group to the target addr
    let peers = client
        .get_closest_to_address(target_addr.clone(), None)
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to get closest peers: {e}"))?;

    // Sort peers by distance to target address
    let mut sorted_peers = peers;
    sorted_peers.sort_by_key(|peer| {
        let peer_addr = NetworkAddress::from(peer.peer_id);
        target_addr.distance(&peer_addr)
    });

    println!("Found {} closest peers to {addr}:", sorted_peers.len());
    println!();

    // Generate a random nonce for this health check
    let nonce: u64 = rand::random();
    let difficulty = 5;

    println!("Requesting storage proofs with nonce: {nonce:?}, difficulty: {difficulty}");
    println!();

    // Check storage proofs for each peer
    for (i, peer) in sorted_peers.iter().enumerate() {
        let peer_addr = NetworkAddress::from(peer.peer_id);
        let distance = target_addr.distance(&peer_addr);

        println!("{}. Peer ID: {} (distance: {distance:?})", i + 1, peer.peer_id);

        // Query the peer directly for storage proofs
        match client
            .get_storage_proofs_from_peer(target_addr.clone(), peer.clone(), nonce, difficulty)
            .await
        {
            Ok(storage_proofs) => {
                if storage_proofs.is_empty() {
                    println!("   Status: ⚠️  No storage proofs received");
                } else {
                    println!("   Status: ✅ Received {} storage proofs", storage_proofs.len());
                    if verbose {
                        println!("   Storage Proofs:");
                        for (addr, proof_result) in &storage_proofs {
                            match proof_result {
                                Ok(proof) => {
                                    println!("     - {addr}: {proof:?}");
                                }
                                Err(e) => {
                                    println!("     - {addr}: Error: {e}");
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                println!("   Status: ❌ Failed to query: {e}");
            }
        }

        if verbose {
            println!("   Addresses:");
            for addr in &peer.addrs {
                println!("     - {addr}");
            }
        }
        println!();
    }

    Ok(())
}

/// Re-upload chunks in parallel with max concurrency
async fn reupload_chunks_in_parallel(
    client: &autonomi::Client,
    records_to_repair: Vec<RecordToRepair>,
    payment_option: PaymentOption,
    batch_size: usize,
) -> Vec<ReuploadResult> {
    let total = records_to_repair.len();
    info!("Re-uploading {total} chunks with max concurrency of {batch_size}");
    
    let tasks = records_to_repair.into_iter().map(|record| {
        let client = client.clone();
        let payment_opt = payment_option.clone();
        let address = record.address.clone();
        let holders_count = record.holders_count;
        
        async move {
            let chunk: autonomi::Chunk = if let Ok(chunk) = ant_protocol::storage::try_deserialize_record(&record.record_data) {
                chunk
            } else {
                let err_msg = format!("Record {address:?} is not a Chunk").to_string();
                println!("  ❌  {err_msg}");
                return ReuploadResult {
                    address,
                    holders_count,
                    result: Err(autonomi::client::PutError::Serialization(err_msg)),
                };
            };
            let result = client.chunk_put(&chunk, payment_opt).await;
            
            ReuploadResult {
                address,
                holders_count,
                result,
            }
        }
    });
    
    stream::iter(tasks)
        .buffer_unordered(batch_size)
        .collect()
        .await
}

/// Handle repair of records with insufficient copies
async fn handle_repair(
    client: &autonomi::Client,
    _results: &HashMap<String, Result<Analysis, AnalysisError>>,
    closest_data: &HashMap<String, Vec<ClosestPeerStatus>>,
    verbose: bool,
) -> Result<()> {
    use std::io::Write;
    
    println!("\n{}", "=".repeat(80));
    println!("REPAIR MODE: Checking for records with insufficient copies...");
    println!("{}", "=".repeat(80));
    
    let mut records_to_repair: Vec<RecordToRepair> = Vec::new();
    
    // Identify records that need repair
    for (addr_str, statuses) in closest_data {
        let holders_count = statuses
            .iter()
            .filter(|s| matches!(s, ClosestPeerStatus::Holding { .. }))
            .count();
        
        if holders_count < 3 {
            if verbose {
                println!("⚠️  Address {addr_str} has only {holders_count} holder(s) in closest 7");
            }

            // Get target_address from the first entry (all variants have this field)
            if let Some(first_status) = statuses.first() {
                let target_address = first_status.target_address();
                // Try to get the record data via kad query
                match get_record_with_best_effort(client, target_address.clone(), verbose).await {
                    Ok((Some(record), _holders)) => {
                        if verbose {
                            println!("   ✅ Retrieved record {addr_str:?} with {} bytes", record.value.len());
                        }
                        records_to_repair.push(RecordToRepair {
                            address: addr_str.clone(),
                            holders_count,
                            record_data: record.clone(),
                        });
                    }
                    Ok((None, _holders)) => {
                        println!("   ❌ Failed to retrieve record of {addr_str:?}");
                    }
                    Err(e) => {
                        println!("   ❌ Error retrieving record {addr_str:?} {e}");
                    }
                }
            } else {
                println!("   ⚠️  No status entries found for {addr_str}, cannot repair");
            }
        }
    }
    
    println!("\nFound {} record(s) needing repair", records_to_repair.len());
    
    if records_to_repair.is_empty() {
        println!("✅ All records have sufficient copies!");
        return Ok(());
    }
    
    // Create CSV report file
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let csv_path = format!("repair_report_{timestamp}.csv");
    let mut csv_file = std::fs::File::create(&csv_path)?;
    writeln!(csv_file, "address,original_holders_count,upload_status,cost_paid,error")?;
    
    let wallet = load_wallet(client.evm_network())?;
    let payment_option = PaymentOption::from(&wallet);
    
    println!("\nUploading {} record(s) for repair in parallel...", records_to_repair.len());
    
    let reupload_results = reupload_chunks_in_parallel(
        client,
        records_to_repair,
        payment_option,
        MAX_PARALLEL_UPLOADS,
    ).await;
    
    // Write results to CSV
    for result in &reupload_results {
        match &result.result {
            Ok((cost, _addr)) => println!("  ✅ {} (cost: {cost})", result.address),
            Err(e) => println!("  ❌ {} - Failed: {e}", result.address),
        }
    }
    write_repair_results_to_csv(&mut csv_file, &reupload_results, None)?;
    
    println!("\n{}", "=".repeat(80));
    println!("Repair complete! Report saved to: {csv_path}");
    println!("{}", "=".repeat(80));
    
    Ok(())
}
