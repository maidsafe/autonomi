// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::actions::NetworkContext;
use autonomi::PublicKey;
use autonomi::chunk::ChunkAddress;
use autonomi::client::analyze::Analysis;
use autonomi::graph::GraphEntryAddress;
use autonomi::networking::PeerId;
use autonomi::{
    Multiaddr, RewardsAddress, SecretKey, Wallet,
    client::analyze::AnalysisError,
    networking::{NetworkAddress, NetworkError},
};
use color_eyre::eyre::Result;
use comfy_table::{Cell, CellAlignment, Table};
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::str::FromStr;

/// Status of a closest peer's record for a given address
#[derive(Debug, Clone)]
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
        error: String,
    },
}

impl ClosestPeerStatus {
    /// Get the listen addresses of the peer
    pub fn listen_addrs(&self) -> &Vec<Multiaddr> {
        match self {
            ClosestPeerStatus::Holding { listen_addrs, .. } => listen_addrs,
            ClosestPeerStatus::NotHolding { listen_addrs, .. } => listen_addrs,
            ClosestPeerStatus::FailedQuery { listen_addrs, .. } => listen_addrs,
        }
    }

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

pub async fn analyze(
    addr: &str,
    closest_nodes: bool,
    holders: bool,
    recursive: bool,
    verbose: bool,
    network_context: NetworkContext,
) -> Result<()> {
    println_if!(verbose, "Analyzing address: {addr}");

    // then connect to network and check data
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let results = if recursive {
        println_if!(verbose, "Starting recursive analysis...");
        client.analyze_address_recursively(addr, verbose).await
    } else {
        let mut map = HashMap::new();
        let analysis = client.analyze_address(addr, verbose).await;
        map.insert(addr.to_string(), analysis);
        map
    };

    // Pre-compute closest nodes data
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
                let result = get_closest_nodes_status(&client, &addr, false).await;
                (addr, result)
            }
        });

        let closest_nodes_results: Vec<(String, Result<Vec<ClosestPeerStatus>>)> =
            stream::iter(query_tasks)
                .buffered(*autonomi::client::config::CHUNK_DOWNLOAD_BATCH_SIZE)
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
                let result = get_holders_status(&client, &addr, false).await;
                (addr, result)
            }
        });

        let holders_results: Vec<(String, Result<Vec<HolderStatus>>)> = stream::iter(query_tasks)
            .buffered(*autonomi::client::config::CHUNK_DOWNLOAD_BATCH_SIZE)
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

    // Print results
    if closest_nodes_data.is_some() || holders_data.is_some() {
        // Use unified summary for output
        print_summary(&results, closest_nodes_data, holders_data, verbose)?;
    }

    if results.len() == 1
        && let Some((_, analysis)) = results.iter().next()
    {
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

        // For single address with closest_nodes, show detailed view
        if closest_nodes {
            print_closest_nodes(&client, addr, verbose).await?;
        }
    } else {
        println!("No analysis results available.");
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

/// Get holders (along query path) status for an address
///
/// Returns a vector of HolderStatus for the given address
async fn get_holders_status(
    client: &autonomi::Client,
    addr: &str,
    verbose: bool,
) -> Result<Vec<HolderStatus>> {
    use autonomi::PublicKey;
    use autonomi::chunk::ChunkAddress;
    use autonomi::graph::GraphEntryAddress;
    use autonomi::networking::NetworkAddress;

    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }

    let hex_addr = addr.trim_start_matches("0x");

    println_if_verbose!("Querying holders of record at address...");

    // Try parsing as ChunkAddress (XorName) first
    let network_addr: NetworkAddress = if let Ok(chunk_addr) = ChunkAddress::from_hex(addr) {
        println_if_verbose!("Identified as ChunkAddress");
        chunk_addr.into()
    // Try parsing as PublicKey (could be GraphEntry, Pointer, or Scratchpad)
    } else if let Ok(public_key) = PublicKey::from_hex(hex_addr) {
        println_if_verbose!("Identified as PublicKey, using GraphEntryAddress");
        // Default to GraphEntryAddress for public keys
        let graph_entry_address = GraphEntryAddress::new(public_key);
        graph_entry_address.into()
    } else {
        return Err(color_eyre::eyre::eyre!(
            "Could not parse address. Expected a hex-encoded ChunkAddress or PublicKey"
        ));
    };

    let quorum = std::num::NonZeroUsize::new(20)
        .map(autonomi::networking::Quorum::N)
        .expect("20 is non-zero");

    let (record, holders) = match client
        .get_record_and_holders(network_addr.clone(), quorum)
        .await
    {
        Ok((record, holders)) => (record, holders),
        Err(NetworkError::GetRecordTimeout(holders)) => {
            println_if_verbose!("Request timed out, showing partial results");
            (None, holders)
        }
        Err(NetworkError::GetRecordQuorumFailed {
            got_holders,
            expected_holders,
            holders,
        }) => {
            println_if_verbose!(
                "Quorum not met (got {got_holders}/{expected_holders}), showing partial results"
            );
            (None, holders)
        }
        Err(e) => {
            return Err(color_eyre::eyre::eyre!("Failed to get record holders: {e}"));
        }
    };

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
        "Found {} holders for record at {addr}:",
        sorted_holders.len()
    );
    for (i, peer_id) in sorted_holders.iter().enumerate() {
        let peer_addr: NetworkAddress = (*peer_id).into();
        let distance = network_addr.distance(&peer_addr);

        println!(
            "{}. Peer ID: {peer_id} (distance: {distance:?}[{:?}])",
            i + 1,
            distance.ilog2().unwrap_or(0)
        );

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
    let hex_addr = addr.trim_start_matches("0x");

    println_if!(verbose, "Querying closest peers to address...");

    // Try parsing as ChunkAddress (XorName) first
    let target_addr = if let Ok(chunk_addr) = ChunkAddress::from_hex(addr) {
        println_if!(verbose, "Identified as ChunkAddress");
        NetworkAddress::from(chunk_addr)
    // Try parsing as PublicKey (could be GraphEntry, Pointer, or Scratchpad)
    } else if let Ok(public_key) = PublicKey::from_hex(hex_addr) {
        println_if!(verbose, "Identified as PublicKey, using GraphEntryAddress");
        // Default to GraphEntryAddress for public keys
        NetworkAddress::from(GraphEntryAddress::new(public_key))
    } else {
        return Err(color_eyre::eyre::eyre!(
            "Could not parse address. Expected a hex-encoded ChunkAddress or PublicKey"
        ));
    };

    // Get closest group to the target addr
    let peers = client
        .get_closest_to_address(target_addr.clone(), Some(20))
        .await
        .map_err(|e| color_eyre::eyre::eyre!("Failed to get closest peers: {e}"))?;

    println!("Found {} closest peers to: {}", peers.len(), addr);

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
                    error: e.to_string(),
                },
            }
        }
    });

    let closest_peers_statuses: Vec<ClosestPeerStatus> = stream::iter(query_tasks)
        .buffered(*autonomi::client::config::CHUNK_DOWNLOAD_BATCH_SIZE)
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
    kad_query_status: String, // "✓ Found" or "✗ Not found"
    closest_peers_count: Option<(usize, usize)>, // (holding, total) e.g. (15, 20)
    target_distance_stats: Option<TargetDistanceStats>, // Distance from target to peers
    peer_distance_stats: Option<PeerDistanceStats>, // Distance among peers
}

/// Format histogram as compact text showing only non-zero buckets
/// Example: "230-235: 2, 236-240: 6"
fn format_histogram_compact(histogram: &[(String, usize)]) -> String {
    let non_zero: Vec<String> = histogram
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(range, count)| format!("{range}: {count}"))
        .collect();

    if non_zero.is_empty() {
        "N/A".to_string()
    } else {
        non_zero.join(", ")
    }
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

async fn print_closest_nodes(client: &autonomi::Client, addr: &str, verbose: bool) -> Result<()> {
    let hex_addr = addr.trim_start_matches("0x");

    // Try parsing as ChunkAddress (XorName) first
    let target_addr = if let Ok(chunk_addr) = ChunkAddress::from_hex(addr) {
        NetworkAddress::from(chunk_addr)
    // Try parsing as PublicKey (could be GraphEntry, Pointer, or Scratchpad)
    } else if let Ok(public_key) = PublicKey::from_hex(hex_addr) {
        // Default to GraphEntryAddress for public keys
        NetworkAddress::from(GraphEntryAddress::new(public_key))
    } else {
        return Err(color_eyre::eyre::eyre!(
            "Could not parse address. Expected a hex-encoded ChunkAddress or PublicKey"
        ));
    };

    // Get the closest peer status
    let mut closest_peers_statuses = get_closest_nodes_status(client, addr, verbose).await?;

    // Sort peers by distance to target address
    closest_peers_statuses.sort_by_key(|status| {
        let peer_addr = NetworkAddress::from(*status.peer_id());
        target_addr.distance(&peer_addr)
    });

    // Print status for each peer
    for (i, status) in closest_peers_statuses.iter().enumerate() {
        let peer_id = status.peer_id();
        let peer_addr = NetworkAddress::from(*peer_id);
        let distance = target_addr.distance(&peer_addr);

        println!(
            "{}. Peer ID: {peer_id} (distance: {distance:?}[{:?}])",
            i + 1,
            distance.ilog2().unwrap_or(0)
        );

        // Print status from the map
        match status {
            ClosestPeerStatus::Holding { size, .. } => {
                println!("   Status: ✅ HOLDING record (size: {size} bytes)");
            }
            ClosestPeerStatus::NotHolding { .. } => {
                println!("   Status: ❌ NOT holding record");
            }
            ClosestPeerStatus::FailedQuery { error, .. } => {
                println!("   Status: ⚠️  Failed to query: {error}");
            }
        }

        if verbose {
            println!("   Addresses:");
            for addr in status.listen_addrs() {
                println!("     - {addr}");
            }
        }
        println!();
    }

    // Print 2-D distance matrix among sorted peers
    println!("\n{}", "=".repeat(80));
    println!("Distance Matrix Among Closest Peers:");
    println!("{}", "=".repeat(80));
    println!();

    // Print header row with peer indices
    print!("     ");
    for i in 0..closest_peers_statuses.len() {
        print!("Peer {:2} ", i + 1);
    }
    println!();
    print!("     ");
    for _ in 0..closest_peers_statuses.len() {
        print!("{} ", "-".repeat(7));
    }
    println!();

    // Print each row of the distance matrix
    for (i, status) in closest_peers_statuses.iter().enumerate() {
        print!("P{:2}  ", i + 1);
        let addr_i = NetworkAddress::from(*status.peer_id());

        for peer_j in closest_peers_statuses.iter() {
            let addr_j = NetworkAddress::from(*peer_j.peer_id());
            let distance = addr_i.distance(&addr_j);

            // Display distance with ilog2 for readability
            if addr_i == addr_j {
                print!("   -    ");
            } else {
                print!("{:?} ", distance.ilog2().unwrap_or(0));
            }
        }
        println!();
    }

    Ok(())
}

fn print_summary(
    results: &HashMap<String, Result<Analysis, AnalysisError>>,
    closest_nodes_data: Option<HashMap<String, Vec<ClosestPeerStatus>>>,
    holders_data: Option<HashMap<String, Vec<HolderStatus>>>,
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

        // Initialize with defaults
        let mut closest_peers_count = None;
        let mut target_distance_stats = None;
        let mut peer_distance_stats = None;

        // Set values if closest nodes data exists for this address
        if let Some(ref closest_nodes_map) = closest_nodes_data
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

        table_rows.push(AnalysisTableRow {
            address: address.clone(),
            type_name,
            kad_query_status: kad_status,
            closest_peers_count,
            target_distance_stats,
            peer_distance_stats,
        });
    }

    // Print the consolidated table
    let with_closest_nodes = closest_nodes_data.is_some();
    print_consolidated_table(&table_rows, with_closest_nodes);

    // Print verbose details if requested
    if verbose && with_closest_nodes {
        print_verbose_details(&table_rows, &closest_nodes_data)?;
    }

    if let Some(holders_data) = holders_data {
        print_holders(holders_data)
    }

    Ok(())
}

fn print_holders(holders_data: HashMap<String, Vec<HolderStatus>>) {
    use autonomi::networking::NetworkAddress;

    for (addr_str, holders) in holders_data {
        let (target_addr, size) = if let Some(first) = holders.first() {
            (first.target_address.clone(), first.size)
        } else {
            println!("No holders of target {addr_str}");
            continue;
        };

        // Sort holders by distance to target address
        let mut sorted_holders: Vec<PeerId> = holders.iter().map(|stat| stat.peer_id).collect();
        sorted_holders.sort_by_key(|peer_id| {
            let peer_addr: NetworkAddress = (*peer_id).into();
            target_addr.distance(&peer_addr)
        });

        println!(
            "Found {} holders for record with length of {size} at {addr_str}:",
            sorted_holders.len()
        );
        for (i, peer_id) in sorted_holders.iter().enumerate() {
            let peer_addr: NetworkAddress = (*peer_id).into();
            let distance = target_addr.distance(&peer_addr);

            println!(
                "{}. Peer ID: {peer_id} (distance: {distance:?}[{:?}])",
                i + 1,
                distance.ilog2().unwrap_or(0)
            );
        }
    }
}

/// Print the consolidated analysis table
fn print_consolidated_table(rows: &[AnalysisTableRow], with_closest_nodes: bool) {
    let mut table = Table::new();

    if with_closest_nodes {
        // Table with closest nodes information
        table.set_header(vec![
            Cell::new("Address").set_alignment(CellAlignment::Left),
            Cell::new("Type").set_alignment(CellAlignment::Left),
            Cell::new("Kad Query").set_alignment(CellAlignment::Left),
            Cell::new("Closest Peers").set_alignment(CellAlignment::Left),
            Cell::new("Target Distances (ilog2)").set_alignment(CellAlignment::Left),
            Cell::new("Target Distance Hist").set_alignment(CellAlignment::Left),
            Cell::new("Peer Distances (ilog2)").set_alignment(CellAlignment::Left),
            Cell::new("Peer Distance Hist").set_alignment(CellAlignment::Left),
        ]);

        for row in rows {
            let closest_display = row
                .closest_peers_count
                .map(|(h, t)| format!("{h}/{t}"))
                .unwrap_or_else(|| "N/A".to_string());

            let target_distance_display = row
                .target_distance_stats
                .as_ref()
                .map(|stats| format!("min={} avg={} max={}", stats.min, stats.avg, stats.max))
                .unwrap_or_else(|| "N/A".to_string());

            let target_histogram_display = row
                .target_distance_stats
                .as_ref()
                .map(|stats| format_histogram_compact(&stats.histogram))
                .unwrap_or_default();

            let peer_distance_display = row
                .peer_distance_stats
                .as_ref()
                .map(|stats| format!("min={} avg={} max={}", stats.min, stats.avg, stats.max))
                .unwrap_or_else(|| "N/A".to_string());

            let peer_histogram_display = row
                .peer_distance_stats
                .as_ref()
                .map(|stats| format_histogram_compact(&stats.histogram))
                .unwrap_or_default();

            table.add_row(vec![
                Cell::new(&row.address),
                Cell::new(&row.type_name),
                Cell::new(&row.kad_query_status),
                Cell::new(closest_display),
                Cell::new(target_distance_display),
                Cell::new(target_histogram_display),
                Cell::new(peer_distance_display),
                Cell::new(peer_histogram_display),
            ]);
        }
    } else {
        // Simple table without closest nodes information
        table.set_header(vec![
            Cell::new("Address").set_alignment(CellAlignment::Left),
            Cell::new("Type").set_alignment(CellAlignment::Left),
            Cell::new("Kad Query").set_alignment(CellAlignment::Left),
        ]);

        for row in rows {
            table.add_row(vec![
                Cell::new(&row.address),
                Cell::new(&row.type_name),
                Cell::new(&row.kad_query_status),
            ]);
        }
    }

    println!("\n{table}");
    println!("\nTotal: {} addresses", rows.len());
}

/// Print verbose details including histogram breakdown and peer IDs
fn print_verbose_details(
    rows: &[AnalysisTableRow],
    closest_nodes_data: &Option<HashMap<String, Vec<ClosestPeerStatus>>>,
) -> Result<()> {
    let rows_with_data: Vec<_> = rows
        .iter()
        .filter(|r| r.closest_peers_count.is_some())
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

        // Print type, kad query status, and closest peers count
        if let Some((holding, total)) = row.closest_peers_count {
            println!(
                "  Type: {} | Kad Query: {} | Closest: {}/{}",
                row.type_name, row.kad_query_status, holding, total
            );
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
                "  Peer Distances (ilog2): min={} avg={} max={}",
                stats.min, stats.avg, stats.max
            );

            // Print detailed histogram breakdown
            print!("  Peer Distance histogram: ");
            for (i, (range, count)) in stats.histogram.iter().enumerate() {
                if i > 0 {
                    print!("  ");
                }
                print!("[{range}]: {count}");
            }
            println!();
        }

        // Compute holding peer IDs
        if let Some(closest_nodes_map) = closest_nodes_data
            && let Some(peer_statuses) = closest_nodes_map.get(&row.address)
        {
            let holding_peer_ids: Vec<PeerId> = peer_statuses
                .iter()
                .filter_map(|s| match s {
                    ClosestPeerStatus::Holding { peer_id, .. } => Some(*peer_id),
                    _ => None,
                })
                .collect();

            if !holding_peer_ids.is_empty() {
                println!("  Holding peers ({}):", holding_peer_ids.len());
                for peer_id in &holding_peer_ids {
                    println!("    - {peer_id}");
                }
            }
        }

        println!();
    }

    Ok(())
}
