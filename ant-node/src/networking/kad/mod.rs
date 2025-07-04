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

pub mod transport;
pub mod behaviour;
pub mod kbucket;
pub mod query;
pub mod record_store;
pub mod protocol;
pub mod libp2p_compat;

#[cfg(test)]
mod tests;

// Re-export key types for convenience
pub use transport::{
    KadPeerId, KadDistance, KadAddress, QueryId, RecordKey, Record, PeerInfo,
    ConnectionStatus, KadMessage, KadResponse, KadEvent, KadError, KadConfig,
    KadStats, KademliaTransport, KadEventHandler, QueryResult, RoutingAction,
};

pub use behaviour::Kademlia;
pub use kbucket::{KBucket, KBucketEntry, KBucketKey};
pub use query::{QueryPool, Query, QueryState};
pub use record_store::{RecordStore, MemoryRecordStore};
pub use protocol::KadProtocol;
pub use libp2p_compat::LibP2pTransport;

/// Version of our Kademlia implementation
pub const VERSION: &str = "1.0.0";

/// Maximum number of K-buckets (for 256-bit key space)
pub const MAX_BUCKETS: usize = 256;

/// Default replication factor (k value)
pub const DEFAULT_K_VALUE: usize = 20;

/// Default timeout for queries
pub const DEFAULT_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Stream protocol identifier for Kademlia
pub const KAD_STREAM_PROTOCOL_ID: &str = "/autonomi/kad/1.0.0";

