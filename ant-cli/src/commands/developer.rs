// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Developer and analytics tools for network diagnostics.
//!
//! This module provides commands for debugging and analyzing the network from
//! the perspective of specific nodes. These commands require the `developer`
//! feature to be enabled on both the client (ant-cli) and the target node.

use crate::actions::{NetworkContext, connect_to_network};
use crate::utils::parse_network_address;
use ant_protocol::NetworkAddress;
use ant_protocol::storage::DataTypes;
use autonomi::Client;
use autonomi::networking::{Multiaddr, PaymentQuote, PeerId, PeerInfo};
use color_eyre::{Result, eyre::eyre};

/// Get the version of a node.
///
/// This command queries a specific node to retrieve its software version.
pub async fn node_version(node_addr: &str, network_context: NetworkContext) -> Result<()> {
    println!("Connecting to network...");
    let client = connect_to_network(network_context)
        .await
        .map_err(|(err, _exit_code)| err)?;

    // Resolve the node - either from multiaddr or by discovering PeerId
    let node_info = resolve_node(&client, node_addr).await?;
    let peer_id = node_info.peer_id;

    println!("Querying node {peer_id} for version...");
    println!();

    // Query the node for its version
    let version = client
        .get_node_version(node_info)
        .await
        .map_err(|e| eyre!("Failed to query node version: {e}"))?;

    println!("Node {peer_id}");
    println!("  Version: {version}");

    Ok(())
}

/// Query a specific node to get its network view of closest peers to a target address.
///
/// This command asks the specified node to perform an actual Kademlia network lookup
/// and returns the results from that node's network perspective.
pub async fn closest_peers(
    node_addr: &str,
    target: &str,
    num_peers: Option<usize>,
    compare: bool,
    network_context: NetworkContext,
) -> Result<()> {
    // Parse the target address (hex string)
    let target_addr = parse_network_address(target)?;

    println!("Connecting to network...");
    let client = connect_to_network(network_context)
        .await
        .map_err(|(err, _exit_code)| err)?;

    // Try to resolve the node - either from multiaddr or by discovering PeerId
    let node_info = resolve_node(&client, node_addr).await?;
    let peer_id = node_info.peer_id;

    println!("Querying node {peer_id} for closest peers to {target}...");

    // Perform the developer query
    let response = client
        .dev_get_closest_peers_from_node(node_info, target_addr.clone(), num_peers)
        .await
        .map_err(|e| eyre!("Failed to query node: {e}"))?;

    if compare {
        println!("Querying client's perspective...");
        println!();

        // Get the client's view of closest peers
        let client_peers = client
            .network()
            .get_closest_peers(target_addr.clone(), num_peers)
            .await
            .map_err(|e| eyre!("Failed to get client's closest peers: {e}"))?;

        display_comparison(&response, &client_peers, &target_addr, target, peer_id);
    } else {
        println!();
        display_node_results(&response, &target_addr, target);
    }

    Ok(())
}

/// Get a storage quote from a specific peer.
///
/// This command queries a specific node to get a quote for storing data.
pub async fn get_quote(
    peer_addr: &str,
    address: Option<&str>,
    data_type: u32,
    data_size: usize,
    network_context: NetworkContext,
) -> Result<()> {
    // Parse the target address to XorName, or generate a random one
    let xorname = if let Some(addr) = address {
        let target_addr = parse_network_address(addr)?;
        target_addr
            .xorname()
            .ok_or_else(|| eyre!("Could not extract XorName from address: {addr}"))?
    } else {
        xor_name::XorName::random(&mut rand::thread_rng())
    };

    // Convert data_type index to DataTypes enum
    let data_type_enum = DataTypes::from_index(data_type).ok_or_else(|| {
        eyre!(
            "Invalid data type index: {}. Valid values: 0 (Chunk), 1 (GraphEntry), 2 (Pointer), 3 (Scratchpad)",
            data_type
        )
    })?;

    println!("Connecting to network...");
    let client = connect_to_network(network_context)
        .await
        .map_err(|(err, _exit_code)| err)?;

    // Resolve the peer
    let peer_info = resolve_node(&client, peer_addr).await?;
    let peer_id = peer_info.peer_id;

    println!(
        "Requesting quote from peer {} for address {}...",
        peer_id,
        hex::encode(xorname)
    );
    println!("  Data type: {data_type_enum:?} (index {data_type})");
    println!("  Data size: {data_size} bytes");
    println!();

    // Get the quote
    match client
        .get_raw_quote_from_peer(xorname, peer_info, data_type_enum, data_size)
        .await
    {
        Ok(Some((quote_peer_id, addrs, quote))) => {
            display_quote(&quote_peer_id, &addrs.0, &quote);
        }
        Ok(None) => {
            println!("Record already exists at this address - no payment needed.");
        }
        Err(e) => {
            return Err(eyre!("Failed to get quote: {e}"));
        }
    }

    Ok(())
}

/// Display a quote's information.
fn display_quote(peer_id: &PeerId, addrs: &[Multiaddr], quote: &PaymentQuote) {
    use std::time::UNIX_EPOCH;

    println!("Quote received:");
    println!("{}", "=".repeat(60));
    println!();

    // Peer info
    println!("Peer Information:");
    println!("  PeerId:          {peer_id}");
    if !addrs.is_empty() {
        println!("  Addresses:");
        for addr in addrs {
            println!("                   {addr}");
        }
    }
    println!("  Rewards Address: {:?}", quote.rewards_address);
    println!();

    // Quote metadata
    println!("Quote Details:");
    println!("  Content:         {}", hex::encode(quote.content));
    if let Ok(duration) = quote.timestamp.duration_since(UNIX_EPOCH) {
        let secs: u64 = duration.as_secs();
        println!("  Timestamp:       {secs} (unix)");
    }
    println!();

    // Quoting metrics
    let metrics = &quote.quoting_metrics;
    println!("Quoting Metrics:");
    let dt_name = DataTypes::from_index(metrics.data_type)
        .map(|dt| format!("{dt:?}"))
        .unwrap_or_else(|| "Unknown".to_string());
    println!("  Data Type:             {} ({dt_name})", metrics.data_type);
    println!("  Data Size:             {} bytes", metrics.data_size);
    println!("  Close Records Stored:  {}", metrics.close_records_stored);
    println!("  Max Records:           {}", metrics.max_records);
    println!(
        "  Received Payment Count:{}",
        metrics.received_payment_count
    );
    println!("  Live Time:             {} seconds", metrics.live_time);

    // Records per type
    if !metrics.records_per_type.is_empty() {
        println!("  Records Per Type:");
        for (dtype, count) in &metrics.records_per_type {
            let type_name = DataTypes::from_index(*dtype)
                .map(|dt| format!("{dt:?}"))
                .unwrap_or_else(|| "Unknown".to_string());
            println!("    {type_name}: {count}");
        }
    }

    // Network metrics
    if let Some(size) = metrics.network_size {
        println!("  Network Size:          {size} nodes (estimated)");
    }
    if let Some(density) = &metrics.network_density {
        println!("  Network Density:       {}", hex::encode(density));
    }
}

/// Display results for the standard (non-comparison) mode.
fn display_node_results(
    response: &autonomi::networking::DevGetClosestPeersFromNetworkResponse,
    target_addr: &NetworkAddress,
    target: &str,
) {
    println!(
        "Closest peers to {} from node {}:",
        target, response.queried_node
    );
    println!();

    if response.peers.is_empty() {
        println!("  No peers found.");
    } else {
        println!(
            "  {:<4} {:<54} {:<15} Multiaddrs",
            "#", "PeerId", "Distance"
        );
        println!("  {}", "-".repeat(130));

        for (i, (peer_addr, multiaddrs)) in response.peers.iter().enumerate() {
            let distance = target_addr.distance(peer_addr);
            let distance_ilog2 = distance.ilog2().unwrap_or(0);

            let multiaddr_str = if multiaddrs.is_empty() {
                "N/A".to_string()
            } else {
                multiaddrs
                    .iter()
                    .map(|m: &Multiaddr| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            let peer_display = if let Some(peer_id) = peer_addr.as_peer_id() {
                peer_id.to_string()
            } else {
                peer_addr.to_string()
            };

            println!(
                "  {:<4} {:<54} {:<15} {}",
                i + 1,
                peer_display,
                distance_ilog2,
                multiaddr_str
            );
        }
    }

    println!();
    println!("Total: {} peers", response.peers.len());
}

/// Display comparison between node's view and client's view.
fn display_comparison(
    node_response: &autonomi::networking::DevGetClosestPeersFromNetworkResponse,
    client_peers: &[PeerInfo],
    target_addr: &NetworkAddress,
    target: &str,
    queried_peer_id: PeerId,
) {
    use std::collections::HashSet;

    // Build sets of peer IDs for comparison
    let node_peer_ids: HashSet<PeerId> = node_response
        .peers
        .iter()
        .filter_map(|(addr, _)| addr.as_peer_id())
        .collect();

    let client_peer_ids: HashSet<PeerId> = client_peers.iter().map(|p| p.peer_id).collect();

    let common: HashSet<PeerId> = node_peer_ids
        .intersection(&client_peer_ids)
        .copied()
        .collect();
    let node_only: HashSet<PeerId> = node_peer_ids
        .difference(&client_peer_ids)
        .copied()
        .collect();
    let client_only: HashSet<PeerId> = client_peer_ids
        .difference(&node_peer_ids)
        .copied()
        .collect();

    println!("Comparison of closest peers to {target}");
    println!("{}", "=".repeat(100));
    println!();

    // Summary
    println!("Summary:");
    println!("  Node's view:   {} peers", node_response.peers.len());
    println!("  Client's view: {} peers", client_peers.len());
    println!("  In common:     {} peers", common.len());
    println!("  Node only:     {} peers", node_only.len());
    println!("  Client only:   {} peers", client_only.len());
    println!();
    println!("  Note: Client's results include the queried node itself.");
    println!("        Nodes don't include themselves in their own results.");
    println!();

    // Node's perspective
    println!("NODE'S PERSPECTIVE (from {}):", node_response.queried_node);
    println!("  {:<4} {:<54} {:<10} Status", "#", "PeerId", "Distance");
    println!("  {}", "-".repeat(80));

    for (i, (peer_addr, _)) in node_response.peers.iter().enumerate() {
        let distance = target_addr.distance(peer_addr);
        let distance_ilog2 = distance.ilog2().unwrap_or(0);

        let peer_display = if let Some(peer_id) = peer_addr.as_peer_id() {
            peer_id.to_string()
        } else {
            peer_addr.to_string()
        };

        let status = if let Some(peer_id) = peer_addr.as_peer_id() {
            if common.contains(&peer_id) {
                "common"
            } else {
                "node-only"
            }
        } else {
            "unknown"
        };

        println!(
            "  {:<4} {:<54} {:<10} {}",
            i + 1,
            peer_display,
            distance_ilog2,
            status
        );
    }

    println!();

    // Client's perspective
    println!("CLIENT'S PERSPECTIVE:");
    println!("  {:<4} {:<54} {:<10} Status", "#", "PeerId", "Distance");
    println!("  {}", "-".repeat(85));

    // Sort client peers by distance
    let mut sorted_client_peers: Vec<_> = client_peers.iter().collect();
    sorted_client_peers.sort_by_key(|p| {
        let addr = NetworkAddress::from(p.peer_id);
        target_addr.distance(&addr)
    });

    for (i, peer_info) in sorted_client_peers.iter().enumerate() {
        let peer_addr = NetworkAddress::from(peer_info.peer_id);
        let distance = target_addr.distance(&peer_addr);
        let distance_ilog2 = distance.ilog2().unwrap_or(0);

        let status = if peer_info.peer_id == queried_peer_id {
            "queried-node*"
        } else if common.contains(&peer_info.peer_id) {
            "common"
        } else {
            "client-only"
        };

        println!(
            "  {:<4} {:<54} {:<10} {}",
            i + 1,
            peer_info.peer_id,
            distance_ilog2,
            status
        );
    }

    println!();

    // Peers only in node's view
    if !node_only.is_empty() {
        println!("PEERS ONLY IN NODE'S VIEW:");
        for peer_id in &node_only {
            let peer_addr = NetworkAddress::from(*peer_id);
            let distance = target_addr.distance(&peer_addr);
            let distance_ilog2 = distance.ilog2().unwrap_or(0);
            println!("  {peer_id} (distance: {distance_ilog2})");
        }
        println!();
    }

    // Peers only in client's view
    if !client_only.is_empty() {
        println!("PEERS ONLY IN CLIENT'S VIEW:");
        for peer_id in &client_only {
            let peer_addr = NetworkAddress::from(*peer_id);
            let distance = target_addr.distance(&peer_addr);
            let distance_ilog2 = distance.ilog2().unwrap_or(0);
            if *peer_id == queried_peer_id {
                println!(
                    "  {peer_id} (distance: {distance_ilog2}) <- queried node (nodes don't include themselves)"
                );
            } else {
                println!("  {peer_id} (distance: {distance_ilog2})");
            }
        }
    }
}

/// Resolve a node identifier to PeerInfo.
///
/// Accepts either:
/// - A full multiaddr (e.g., /ip4/127.0.0.1/udp/12000/quic-v1/p2p/12D3KooW...)
/// - Just a PeerId (e.g., 12D3KooW...)
///
/// When only a PeerId is provided, the network is queried to discover the peer's addresses.
async fn resolve_node(client: &Client, node_addr: &str) -> Result<PeerInfo> {
    // First, try to parse as a PeerId directly
    if let Ok(peer_id) = node_addr.parse::<PeerId>() {
        println!("Discovering addresses for peer {peer_id}...");

        // Query the network to find this peer's addresses
        let peer_network_addr = NetworkAddress::from(peer_id);
        let closest_peers = client
            .network()
            .get_closest_peers(peer_network_addr, Some(20))
            .await
            .map_err(|e| eyre!("Failed to discover peer addresses: {e}"))?;

        // Look for our target peer in the results
        for peer_info in closest_peers {
            if peer_info.peer_id == peer_id {
                if peer_info.addrs.is_empty() {
                    return Err(eyre!(
                        "Found peer {peer_id} but no addresses are known. Try using a full multiaddr."
                    ));
                }
                println!("Found peer at: {}", peer_info.addrs[0]);
                return Ok(peer_info);
            }
        }

        return Err(eyre!(
            "Could not find peer {peer_id} in the network. Make sure the node is online and try using a full multiaddr."
        ));
    }

    // Try to parse as a multiaddr
    let multiaddr: Multiaddr = node_addr
        .parse()
        .map_err(|e| eyre!("Invalid node address. Expected PeerId or multiaddr: {e}"))?;

    // Extract PeerId from multiaddr
    let peer_id = extract_peer_id(&multiaddr)
        .ok_or_else(|| eyre!("Multiaddr must contain a peer ID (p2p component)"))?;

    Ok(PeerInfo {
        peer_id,
        addrs: vec![multiaddr],
    })
}

/// Extract PeerId from a Multiaddr
fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    // The multiaddr should end with /p2p/<peer_id>
    // We'll extract it from the string representation
    let addr_str = addr.to_string();
    let p2p_idx = addr_str.find("/p2p/")?;
    let peer_id_str = &addr_str[p2p_idx + 5..];
    peer_id_str.parse().ok()
}
