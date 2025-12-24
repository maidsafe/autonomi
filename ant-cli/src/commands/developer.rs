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
use ant_protocol::NetworkAddress;
use autonomi::Client;
use autonomi::client::data_types::chunk::ChunkAddress;
use autonomi::client::data_types::graph::GraphEntryAddress;
use autonomi::PublicKey;
use autonomi::networking::{Multiaddr, PeerId, PeerInfo};
use color_eyre::{Result, eyre::eyre};

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
    let target_addr = parse_target_address(target)?;

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

    let common: HashSet<PeerId> = node_peer_ids.intersection(&client_peer_ids).copied().collect();
    let node_only: HashSet<PeerId> = node_peer_ids.difference(&client_peer_ids).copied().collect();
    let client_only: HashSet<PeerId> = client_peer_ids.difference(&node_peer_ids).copied().collect();

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
    println!(
        "NODE'S PERSPECTIVE (from {}):",
        node_response.queried_node
    );
    println!(
        "  {:<4} {:<54} {:<10} Status",
        "#", "PeerId", "Distance"
    );
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
    println!(
        "  {:<4} {:<54} {:<10} Status",
        "#", "PeerId", "Distance"
    );
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

/// Parse a target address from various formats.
///
/// Accepts:
/// - ChunkAddress (hex)
/// - PublicKey (hex) - for GraphEntry, Pointer, or Scratchpad addresses
/// - Raw 32-byte hex (XorName)
/// - PeerId
/// - NetworkAddress debug format (e.g., `NetworkAddress::RecordKey("...")`)
fn parse_target_address(target: &str) -> Result<NetworkAddress> {
    let hex_str = target.strip_prefix("0x").unwrap_or(target);

    // Try parsing as ChunkAddress first
    if let Ok(chunk_addr) = ChunkAddress::from_hex(target) {
        return Ok(NetworkAddress::from(chunk_addr));
    }

    // Try parsing as PublicKey (could be GraphEntry, Pointer, or Scratchpad)
    if let Ok(public_key) = PublicKey::from_hex(hex_str) {
        return Ok(NetworkAddress::from(GraphEntryAddress::new(public_key)));
    }

    // Try parsing from NetworkAddress debug format:
    // NetworkAddress::RecordKey("e9d7b3208bcb7ef566102027ca9a7f3ced7c0f8abf87c9bb0ef9130b625572f2") - (...)
    if let Some(start) = target.find('"')
        && let Some(end) = target[start + 1..].find('"')
    {
        let extracted_hex = &target[start + 1..start + 1 + end];
        if let Ok(chunk_addr) = ChunkAddress::from_hex(extracted_hex) {
            return Ok(NetworkAddress::from(chunk_addr));
        }
    }

    // Try to parse as raw hex bytes (xor_name)
    if let Ok(bytes) = hex::decode(hex_str)
        && bytes.len() == 32
    {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        return Ok(NetworkAddress::from(xor_name::XorName(arr)));
    }

    // Try to parse as a PeerId
    if let Ok(peer_id) = target.parse::<PeerId>() {
        return Ok(NetworkAddress::from(peer_id));
    }

    Err(eyre!(
        "Invalid target address. Expected ChunkAddress, PublicKey, 32-byte hex, PeerId, or NetworkAddress debug format. Got: {target}"
    ))
}
