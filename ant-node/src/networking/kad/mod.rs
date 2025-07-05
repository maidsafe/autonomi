// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Transport-agnostic Kademlia DHT implementation.
//! 
//! This module provides a Kademlia DHT implementation that can work with different
//! underlying transport layers (libp2p, iroh, etc.) through the transport abstraction.

#![allow(dead_code, unused_imports)]

pub mod transport;
pub mod behaviour;
pub mod kbucket;
pub mod query;
pub mod record_store;
pub mod protocol;
#[cfg(feature = "libp2p-compat")]
pub mod libp2p_compat;

#[cfg(test)]
mod tests;

// Re-export key types for convenience
// Most kad types are only used by dual-stack and iroh features
#[cfg(any(feature = "dual-stack", feature = "iroh-transport"))]
pub use transport::{
    KadPeerId, KadAddress, QueryId, RecordKey, Record, PeerInfo,
    ConnectionStatus, KadMessage, KadResponse, KadError,
    KademliaTransport, QueryResult,
};

#[cfg(any(feature = "dual-stack", feature = "iroh-transport"))]
pub use behaviour::Kademlia;
#[cfg(feature = "libp2p-compat")]
pub use libp2p_compat::LibP2pTransport;

/// Maximum number of K-buckets (for 256-bit key space)
#[cfg(any(feature = "dual-stack", feature = "iroh-transport"))]
pub const MAX_BUCKETS: usize = 256;

