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
    use libp2p::Multiaddr;

    const VALID_PEER_ID: &str = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";

    #[test]
    fn craft_valid_multiaddr_accepts_udp_quic_v1_with_peer_id() {
        let input = format!("/ip4/127.0.0.1/udp/8080/quic-v1/p2p/{VALID_PEER_ID}");
        let addr: Multiaddr = input.parse().unwrap();

        let result = craft_valid_multiaddr(&addr).expect("should accept udp/quic-v1 with peer id");

        assert_eq!(result.to_string(), input);
    }

    #[test]
    fn craft_valid_multiaddr_accepts_udp_quic_v1_without_peer_id() {
        let input = "/ip4/127.0.0.1/udp/8080/quic-v1";
        let addr: Multiaddr = input.parse().unwrap();

        let result =
            craft_valid_multiaddr(&addr).expect("should accept udp/quic-v1 without peer id");

        assert_eq!(result.to_string(), input);
    }

    #[test]
    fn craft_valid_multiaddr_rejects_tcp_transport() {
        let input = format!("/ip4/127.0.0.1/tcp/8080/p2p/{VALID_PEER_ID}");
        let addr: Multiaddr = input.parse().unwrap();

        let result = craft_valid_multiaddr(&addr);

        assert!(result.is_none(), "should reject tcp transport");
    }

    #[test]
    fn craft_valid_multiaddr_rejects_tcp_with_websocket() {
        let input = format!("/ip4/127.0.0.1/tcp/8080/ws/p2p/{VALID_PEER_ID}");
        let addr: Multiaddr = input.parse().unwrap();

        let result = craft_valid_multiaddr(&addr);

        assert!(result.is_none(), "should reject tcp/ws transport");
    }

    #[test]
    fn craft_valid_multiaddr_rejects_udp_without_quic() {
        let input = "/ip4/127.0.0.1/udp/8080";
        let addr: Multiaddr = input.parse().unwrap();

        let result = craft_valid_multiaddr(&addr);

        assert!(result.is_none(), "should reject udp without quic-v1");
    }

    #[test]
    fn craft_valid_multiaddr_rejects_address_without_udp() {
        let input = format!("/ip4/127.0.0.1/p2p/{VALID_PEER_ID}");
        let addr: Multiaddr = input.parse().unwrap();

        let result = craft_valid_multiaddr(&addr);

        assert!(result.is_none(), "should reject address without udp");
    }

    #[test]
    fn craft_valid_multiaddr_rejects_address_without_ip() {
        let input = format!("/udp/8080/quic-v1/p2p/{VALID_PEER_ID}");
        let addr: Multiaddr = input.parse().unwrap();

        let result = craft_valid_multiaddr(&addr);

        assert!(result.is_none(), "should reject address without ip");
    }

    #[test]
    fn craft_valid_multiaddr_from_str_accepts_valid_address() {
        let input = format!("/ip4/127.0.0.1/udp/8080/quic-v1/p2p/{VALID_PEER_ID}");

        let result =
            craft_valid_multiaddr_from_str(&input).expect("should parse valid address string");

        assert_eq!(result.to_string(), input);
    }

    #[test]
    fn craft_valid_multiaddr_from_str_rejects_invalid_string() {
        let result = craft_valid_multiaddr_from_str("not a multiaddr");

        assert!(result.is_none(), "should reject invalid multiaddr string");
    }

    #[test]
    fn craft_valid_multiaddr_from_str_accepts_address_without_peer_id() {
        let input = "/ip4/127.0.0.1/udp/8080/quic-v1";

        let result = craft_valid_multiaddr_from_str(input)
            .expect("should accept address string without peer id");

        assert_eq!(result.to_string(), input);
    }

    #[test]
    fn craft_valid_multiaddr_from_str_rejects_tcp_address() {
        let input = format!("/ip4/127.0.0.1/tcp/8080/p2p/{VALID_PEER_ID}");

        let result = craft_valid_multiaddr_from_str(&input);

        assert!(result.is_none(), "should reject tcp address");
    }

    #[test]
    fn multiaddr_get_peer_id_extracts_peer_id_when_present() {
        let addr: Multiaddr = format!("/ip4/127.0.0.1/udp/8080/quic-v1/p2p/{VALID_PEER_ID}")
            .parse()
            .unwrap();

        let peer_id = multiaddr_get_peer_id(&addr).expect("should extract peer id");

        assert_eq!(peer_id.to_string(), VALID_PEER_ID);
    }

    #[test]
    fn multiaddr_get_peer_id_returns_none_when_absent() {
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();

        let result = multiaddr_get_peer_id(&addr);

        assert!(
            result.is_none(),
            "should return None when peer id is absent"
        );
    }
}
