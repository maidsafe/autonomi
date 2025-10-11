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

pub mod bootstrap;
pub mod cache_store;
pub mod config;
pub mod contacts_fetcher;
pub mod error;

use ant_protocol::version::{get_network_id_str, get_truncate_version_str};
use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};
use thiserror::Error;

pub use bootstrap::Bootstrap;
pub use cache_store::BootstrapCacheStore;
pub use config::BootstrapConfig;
pub use config::InitialPeersConfig;
pub use contacts_fetcher::ContactsFetcher;
pub use error::{Error, Result};

/// The name of the environment variable that can be used to pass peers to the node.
pub const ANT_PEERS_ENV: &str = "ANT_PEERS";

/// Craft a proper address to avoid any ill formed addresses
///
/// ignore_peer_id is only used for nat-detection contact list
pub fn craft_valid_multiaddr(addr: &Multiaddr, ignore_peer_id: bool) -> Option<Multiaddr> {
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
        .find(|protocol| matches!(protocol, Protocol::Udp(_)));
    let tcp = addr
        .iter()
        .find(|protocol| matches!(protocol, Protocol::Tcp(_)));

    // UDP or TCP
    if let Some(udp) = udp {
        output_address.push(udp);
        if let Some(quic) = addr
            .iter()
            .find(|protocol| matches!(protocol, Protocol::QuicV1))
        {
            output_address.push(quic);
        }
    } else if let Some(tcp) = tcp {
        output_address.push(tcp);

        if let Some(ws) = addr
            .iter()
            .find(|protocol| matches!(protocol, Protocol::Ws(_)))
        {
            output_address.push(ws);
        }
    } else {
        return None;
    }

    if let Some(peer_id) = peer_id {
        output_address.push(peer_id);
    } else if !ignore_peer_id {
        return None;
    }

    Some(output_address)
}

/// ignore_peer_id is only used for nat-detection contact list
pub fn craft_valid_multiaddr_from_str(addr_str: &str, ignore_peer_id: bool) -> Option<Multiaddr> {
    let Ok(addr) = addr_str.parse::<Multiaddr>() else {
        warn!("Failed to parse multiaddr from str {addr_str}");
        return None;
    };
    craft_valid_multiaddr(&addr, ignore_peer_id)
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
    use libp2p::Multiaddr;

    #[test]
    fn test_transport_protocol_variants() {
        let variants = [
            (
                "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
                true,
            ),
            (
                "/ip4/127.0.0.1/tcp/8080/ws/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
                true,
            ),
            (
                "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
                true,
            ),
            ("/ip4/127.0.0.1/tcp/8080", false),
            (
                "/ip4/127.0.0.1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
                false,
            ),
            (
                "/ip4/127.0.0.1/wss/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
                false,
            ),
        ];

        for (addr, should_be_valid) in variants {
            let parsed: Multiaddr = addr.parse().unwrap();
            let result = craft_valid_multiaddr(&parsed, false);

            if should_be_valid {
                let crafted = result.unwrap_or_else(|| panic!("Expected valid multiaddr: {addr}"));
                assert_eq!(crafted.to_string(), parsed.to_string());
            } else {
                assert!(result.is_none(), "Expected invalid multiaddr: {addr}");
            }
        }
    }

    #[test]
    fn test_craft_valid_multiaddr_from_str() {
        let valid = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
        assert!(craft_valid_multiaddr_from_str(valid, false).is_some());

        let invalid = "not a multiaddr";
        assert!(craft_valid_multiaddr_from_str(invalid, false).is_none());

        let missing_peer = "/ip4/127.0.0.1/tcp/8080";
        assert!(craft_valid_multiaddr_from_str(missing_peer, false).is_none());
        assert!(craft_valid_multiaddr_from_str(missing_peer, true).is_some());
    }

    #[test]
    fn test_craft_valid_multiaddr_ignore_peer_id() {
        let addr_without_peer: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        assert!(craft_valid_multiaddr(&addr_without_peer, false).is_none());
        assert!(craft_valid_multiaddr(&addr_without_peer, true).is_some());
    }

    #[test]
    fn test_multiaddr_get_peer_id() {
        let addr_with_peer: Multiaddr =
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                .parse()
                .unwrap();
        assert!(multiaddr_get_peer_id(&addr_with_peer).is_some());

        let addr_without_peer: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        assert!(multiaddr_get_peer_id(&addr_without_peer).is_none());
    }
}
