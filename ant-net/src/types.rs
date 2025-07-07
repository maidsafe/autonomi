// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Core networking types used throughout ant-net.
//!
//! This module provides type-safe wrappers and re-exports of libp2p types,
//! allowing us to control the API surface and add extensions as needed.

use serde::{Deserialize, Serialize};
use std::fmt;

// Re-export core libp2p types that we wrap
pub use libp2p::{PeerId, Multiaddr};
pub use libp2p::swarm::ConnectionId;

// Re-export ant-protocol types
pub use ant_protocol::NetworkAddress;

/// A collection of multiaddresses for a peer.
/// 
/// This is used throughout the networking layer to represent
/// the various addresses a peer might be reachable at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Addresses(pub Vec<Multiaddr>);

impl Addresses {
    /// Create a new empty address collection.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Create an address collection from a single address.
    pub fn from_single(addr: Multiaddr) -> Self {
        Self(vec![addr])
    }

    /// Create an address collection from a vector of addresses.
    pub fn from_vec(addrs: Vec<Multiaddr>) -> Self {
        Self(addrs)
    }

    /// Check if the address collection is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get the number of addresses in the collection.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Add an address to the collection.
    pub fn push(&mut self, addr: Multiaddr) {
        self.0.push(addr);
    }

    /// Get an iterator over the addresses.
    pub fn iter(&self) -> std::slice::Iter<'_, Multiaddr> {
        self.0.iter()
    }

    /// Convert to a vector of addresses.
    pub fn into_vec(self) -> Vec<Multiaddr> {
        self.0
    }
}

impl From<Vec<Multiaddr>> for Addresses {
    fn from(addrs: Vec<Multiaddr>) -> Self {
        Self(addrs)
    }
}

impl From<Multiaddr> for Addresses {
    fn from(addr: Multiaddr) -> Self {
        Self::from_single(addr)
    }
}

impl IntoIterator for Addresses {
    type Item = Multiaddr;
    type IntoIter = std::vec::IntoIter<Multiaddr>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Addresses {
    type Item = &'a Multiaddr;
    type IntoIter = std::slice::Iter<'a, Multiaddr>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Peer information containing ID and addresses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
    /// The peer's ID.
    pub peer_id: PeerId,
    /// The peer's known addresses.
    pub addresses: Addresses,
}

impl PeerInfo {
    /// Create new peer information.
    pub fn new(peer_id: PeerId, addresses: Addresses) -> Self {
        Self { peer_id, addresses }
    }

    /// Create peer information with a single address.
    pub fn with_single_address(peer_id: PeerId, address: Multiaddr) -> Self {
        Self {
            peer_id,
            addresses: Addresses::from_single(address),
        }
    }
}

/// Connection state information.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Connection is being established.
    Connecting,
    /// Connection is established and ready for use.
    Connected,
    /// Connection is being closed.
    Closing,
    /// Connection is closed.
    Closed,
    /// Connection failed to establish.
    Failed(String),
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionState::Connecting => write!(f, "connecting"),
            ConnectionState::Connected => write!(f, "connected"),
            ConnectionState::Closing => write!(f, "closing"),
            ConnectionState::Closed => write!(f, "closed"),
            ConnectionState::Failed(reason) => write!(f, "failed: {}", reason),
        }
    }
}

/// Direction of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionDirection {
    /// We initiated the connection.
    Outbound,
    /// The peer initiated the connection.
    Inbound,
}

impl fmt::Display for ConnectionDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionDirection::Outbound => write!(f, "outbound"),
            ConnectionDirection::Inbound => write!(f, "inbound"),
        }
    }
}

/// Protocol identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtocolId(String);

impl ProtocolId {
    /// Create a new protocol ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the protocol ID as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProtocolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ProtocolId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for ProtocolId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Stream protocol identifier used in libp2p.
impl From<ProtocolId> for libp2p::StreamProtocol {
    fn from(id: ProtocolId) -> Self {
        libp2p::StreamProtocol::try_from_owned(id.0)
            .expect("Invalid protocol ID")
    }
}

impl From<libp2p::StreamProtocol> for ProtocolId {
    fn from(protocol: libp2p::StreamProtocol) -> Self {
        Self(protocol.to_string())
    }
}