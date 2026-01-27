// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow expect usage and enum variant names (comes from thiserror derives)
#![allow(clippy::expect_used)]
#![allow(clippy::enum_variant_names)]

//! Bootstrap Cache for the Autonomous Network
//!
//! This crate provides a decentralized peer discovery and caching system for the Autonomi Network.
//! It implements a robust peer management system with the following features:
//!
//! - Decentralized Design: No dedicated bootstrap nodes required
//! - Cross-Platform Support: Works on Linux, macOS, and Windows
//! - Shared Cache: System-wide cache file accessible by both nodes and clients
//! - Concurrent Access: File locking for safe multi-process access
//! - Atomic Operations: Safe cache updates using atomic file operations
//! - Initial Peer Discovery: Fallback web endpoints for new/stale cache scenarios

#[macro_use]
extern crate tracing;

pub mod cache_store;
pub mod config;
pub mod contacts;
pub mod error;
mod initial_peers;

use ant_protocol::version::{get_network_id_str, get_truncate_version_str};
use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};

pub use cache_store::BootstrapCacheStore;
pub use config::BootstrapCacheConfig;
pub use contacts::ContactsFetcher;
pub use error::{Error, Result};
pub use initial_peers::{ANT_PEERS_ENV, InitialPeersConfig};

/// Craft a proper address to avoid any ill formed addresses
///
/// PeerId is optional, if not present, it will be ignored.
pub fn craft_valid_multiaddr(addr: &Multiaddr) -> Option<Multiaddr> {
    let peer_id = addr
        .iter()
        .find(|protocol| matches!(protocol, Protocol::P2p(_)));

    let mut output_address = Multiaddr::empty();

    let ip = addr
        .iter()
        .find(|protocol| matches!(protocol, Protocol::Ip4(_)))?;
    output_address.push(ip);

    let udp = addr
        .iter()
        .find(|protocol| matches!(protocol, Protocol::Udp(_)))?;

    output_address.push(udp);
    let quic = addr
        .iter()
        .find(|protocol| matches!(protocol, Protocol::QuicV1))?;
    output_address.push(quic);

    if let Some(peer_id) = peer_id {
        output_address.push(peer_id);
    }

    Some(output_address)
}

/// Craft a proper address to avoid any ill formed addresses
///
/// PeerId is optional, if not present, it will be ignored.
pub fn craft_valid_multiaddr_from_str(addr_str: &str) -> Option<Multiaddr> {
    let Ok(addr) = addr_str.parse::<Multiaddr>() else {
        warn!("Failed to parse multiaddr from str {addr_str}");
        return None;
    };
    craft_valid_multiaddr(&addr)
}

pub fn multiaddr_get_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    match addr.iter().find(|p| matches!(p, Protocol::P2p(_))) {
        Some(Protocol::P2p(id)) => Some(id),
        _ => None,
    }
}

pub fn get_network_version() -> String {
    format!("{}_{}", get_network_id_str(), get_truncate_version_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    // craft_valid_multiaddr tests (7 tests)
    #[test]
    fn test_craft_valid_multiaddr_with_peer_id() {
        // Valid QUIC address with peer ID should return Some
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse().unwrap();
        let result = craft_valid_multiaddr(&addr);
        assert!(
            result.is_some(),
            "Should return Some for valid QUIC address with peer ID"
        );

        // Verify the result contains expected protocols
        let crafted = result.unwrap();
        let crafted_str = crafted.to_string();
        assert!(crafted_str.contains("/ip4/127.0.0.1"), "Should contain IP");
        assert!(crafted_str.contains("/udp/8080"), "Should contain UDP port");
        assert!(
            crafted_str.contains("/quic-v1"),
            "Should contain QUIC protocol"
        );
        assert!(
            crafted_str.contains("/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"),
            "Should contain peer ID"
        );
    }

    #[test]
    fn test_craft_valid_multiaddr_without_peer_id() {
        // Valid QUIC address without peer ID should return Some
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let result = craft_valid_multiaddr(&addr);
        assert!(
            result.is_some(),
            "Should return Some for valid QUIC address without peer ID"
        );

        // Verify the result structure
        let crafted = result.unwrap();
        let crafted_str = crafted.to_string();
        assert!(crafted_str.contains("/ip4/127.0.0.1"), "Should contain IP");
        assert!(crafted_str.contains("/udp/8080"), "Should contain UDP port");
        assert!(
            crafted_str.contains("/quic-v1"),
            "Should contain QUIC protocol"
        );
        assert!(!crafted_str.contains("/p2p/"), "Should not contain peer ID");
    }

    #[test]
    fn test_craft_valid_multiaddr_missing_ipv4_returns_none() {
        // Address missing IP should return None
        let addr: Multiaddr = "/udp/8080/quic-v1".parse().unwrap();
        assert!(craft_valid_multiaddr(&addr).is_none());
    }

    #[test]
    fn test_craft_valid_multiaddr_missing_udp_returns_none() {
        // Address missing UDP should return None
        let addr: Multiaddr = "/ip4/127.0.0.1/quic-v1".parse().unwrap();
        assert!(craft_valid_multiaddr(&addr).is_none());
    }

    #[test]
    fn test_craft_valid_multiaddr_missing_quic_returns_none() {
        // Address missing QUIC should return None
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080".parse().unwrap();
        assert!(craft_valid_multiaddr(&addr).is_none());
    }

    #[test]
    fn test_craft_valid_multiaddr_tcp_only_returns_none() {
        // TCP-only address (no QUIC) should return None
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        assert!(craft_valid_multiaddr(&addr).is_none());
    }

    #[test]
    fn test_craft_valid_multiaddr_extracts_and_normalizes_protocols() {
        // Tests that craft_valid_multiaddr extracts ip4, udp, quic-v1, and p2p protocols
        // and outputs them in canonical order. Note: multiaddr parsing requires correct
        // protocol order, so we test extraction/normalization with a valid input.
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse().unwrap();
        let result = craft_valid_multiaddr(&addr);
        assert!(result.is_some(), "Should successfully craft multiaddr");

        // Verify the crafted address contains expected components in canonical order
        let crafted = result.unwrap();
        let crafted_str = crafted.to_string();
        assert!(
            crafted_str.contains("/ip4/"),
            "Crafted address should contain IP4"
        );
        assert!(
            crafted_str.contains("/udp/"),
            "Crafted address should contain UDP"
        );
        assert!(
            crafted_str.contains("/quic-v1"),
            "Crafted address should contain QUIC"
        );

        // Verify canonical order: ip4 -> udp -> quic-v1 -> p2p
        let ip4_pos = crafted_str.find("/ip4/").unwrap();
        let udp_pos = crafted_str.find("/udp/").unwrap();
        let quic_pos = crafted_str.find("/quic-v1").unwrap();
        let p2p_pos = crafted_str.find("/p2p/").unwrap();
        assert!(ip4_pos < udp_pos, "IP4 should come before UDP");
        assert!(udp_pos < quic_pos, "UDP should come before QUIC");
        assert!(quic_pos < p2p_pos, "QUIC should come before P2P");
    }

    // craft_valid_multiaddr_from_str tests (4 tests)
    #[test]
    fn test_craft_valid_multiaddr_from_str_valid() {
        let result = craft_valid_multiaddr_from_str(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
        );
        assert!(
            result.is_some(),
            "Should successfully parse valid multiaddr string"
        );

        // Verify the parsed result
        let crafted = result.unwrap();
        assert!(
            crafted.to_string().contains("/ip4/127.0.0.1"),
            "Should contain original IP"
        );
        assert!(
            crafted.to_string().contains("/quic-v1"),
            "Should contain QUIC protocol"
        );
    }

    #[test]
    fn test_craft_valid_multiaddr_from_str_invalid_format() {
        let result = craft_valid_multiaddr_from_str("not a valid multiaddr");
        assert!(result.is_none());
    }

    #[test]
    fn test_craft_valid_multiaddr_from_str_empty() {
        let result = craft_valid_multiaddr_from_str("");
        assert!(result.is_none());
    }

    #[test]
    fn test_craft_valid_multiaddr_from_str_whitespace() {
        let result = craft_valid_multiaddr_from_str("   ");
        assert!(result.is_none());
    }

    // multiaddr_get_peer_id tests (2 tests)
    #[test]
    fn test_multiaddr_get_peer_id_present() {
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse().unwrap();
        let result = multiaddr_get_peer_id(&addr);
        assert!(result.is_some());
        // Verify it's the correct peer ID
        let peer_id = result.unwrap();
        assert_eq!(
            peer_id.to_string(),
            "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
        );
    }

    #[test]
    fn test_multiaddr_get_peer_id_absent() {
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let result = multiaddr_get_peer_id(&addr);
        assert!(result.is_none());
    }
}
