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
    network_context: NetworkContext,
) -> Result<()> {
    // Parse the node multiaddr
    let multiaddr: Multiaddr = node_addr
        .parse()
        .map_err(|e| eyre!("Invalid node multiaddr: {e}"))?;

    // Extract PeerId from multiaddr
    let peer_id = extract_peer_id(&multiaddr)
        .ok_or_else(|| eyre!("Node multiaddr must contain a peer ID (p2p component)"))?;

    // Parse the target address (hex string)
    let target_addr = parse_target_address(target)?;

    println!("Connecting to network...");
    let client = connect_to_network(network_context)
        .await
        .map_err(|(err, _exit_code)| err)?;

    println!("Querying node {peer_id} for closest peers to {target}...");
    println!();

    // Create PeerInfo for the target node
    let node_info = PeerInfo {
        peer_id,
        addrs: vec![multiaddr],
    };

    // Perform the developer query
    let response = client
        .dev_get_closest_peers_from_node(node_info, target_addr.clone(), num_peers)
        .await
        .map_err(|e| eyre!("Failed to query node: {e}"))?;

    // Display results
    println!(
        "Closest peers to {} from node {}:",
        target, response.queried_node
    );
    println!();

    if response.peers.is_empty() {
        println!("  No peers found.");
    } else {
        println!(
            "  {:<4} {:<52} {:<20} Multiaddrs",
            "#", "PeerId", "Distance"
        );
        println!("  {}", "-".repeat(120));

        for (i, (peer_addr, multiaddrs)) in response.peers.iter().enumerate() {
            let distance = target_addr.distance(peer_addr);
            let distance_hex = format!("0x{:016x}...", distance.0.leading_zeros());

            let multiaddr_str = if multiaddrs.is_empty() {
                "N/A".to_string()
            } else {
                multiaddrs
                    .iter()
                    .map(|m: &Multiaddr| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            // Truncate peer address for display
            let peer_str = peer_addr.to_string();
            let peer_display = if peer_str.len() > 50 {
                format!("{}...", &peer_str[..47])
            } else {
                peer_str
            };

            println!(
                "  {:<4} {:<52} {:<20} {}",
                i + 1,
                peer_display,
                distance_hex,
                multiaddr_str
            );
        }
    }

    println!();
    println!("Total: {} peers", response.peers.len());

    Ok(())
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

/// Parse a target address from a hex string
fn parse_target_address(target: &str) -> Result<NetworkAddress> {
    // Remove 0x prefix if present
    let hex_str = target.strip_prefix("0x").unwrap_or(target);

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
        "Invalid target address. Expected 32-byte hex string or peer ID. Got: {target}"
    ))
}
