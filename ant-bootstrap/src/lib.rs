// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

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
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use thiserror::Error;

pub use cache_store::BootstrapCacheStore;
pub use config::BootstrapCacheConfig;
pub use contacts::ContactsFetcher;
pub use error::{Error, Result};
pub use initial_peers::{InitialPeersConfig, ANT_PEERS_ENV};

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
