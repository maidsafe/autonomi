// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Transport abstraction layer for Kademlia DHT operations.
//! 
//! This module provides transport-agnostic interfaces that allow Kademlia logic
//! to work with different underlying networking implementations (libp2p, iroh, etc.).

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Transport-agnostic peer identifier.
/// 
/// This abstracts away the differences between libp2p::PeerId, iroh::NodeId, etc.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct KadPeerId {
    pub bytes: Vec<u8>,
}

impl KadPeerId {
    /// Create a new KadPeerId from raw bytes
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Get the raw bytes of this peer ID
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Calculate XOR distance to another peer ID
    pub fn distance(&self, other: &KadPeerId) -> KadDistance {
        let mut distance = [0u8; 32];
        let max_len = std::cmp::min(self.bytes.len(), other.bytes.len()).min(32);
        
        for i in 0..max_len {
            distance[i] = self.bytes[i] ^ other.bytes[i];
        }
        
        KadDistance { bytes: distance }
    }
}

impl Display for KadPeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(&self.bytes[..8.min(self.bytes.len())]))
    }
}

/// Kademlia distance between two peer IDs
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct KadDistance {
    pub bytes: [u8; 32],
}

impl KadDistance {
    /// Get the number of leading zero bits (common prefix length)
    pub fn leading_zeros(&self) -> u32 {
        let mut zeros = 0;
        for byte in &self.bytes {
            if *byte == 0 {
                zeros += 8;
            } else {
                zeros += byte.leading_zeros();
                break;
            }
        }
        zeros
    }
}

/// Transport-agnostic network address.
/// 
/// Abstracts away Multiaddr, SocketAddr, etc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KadAddress {
    pub protocol: String,
    pub address: String,
}

impl KadAddress {
    pub fn new(protocol: String, address: String) -> Self {
        Self { protocol, address }
    }
}

impl Display for KadAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.protocol, self.address)
    }
}

/// Unique identifier for Kademlia queries
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryId(pub u64);

impl QueryId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for QueryId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for QueryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Query({})", self.0)
    }
}

/// Record key for DHT storage operations
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordKey {
    pub key: Vec<u8>,
}

impl RecordKey {
    pub fn new(key: Vec<u8>) -> Self {
        Self { key }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self { key: bytes.to_vec() }
    }

    /// Convert to KadPeerId for key-based operations
    pub fn to_kad_peer_id(&self) -> KadPeerId {
        // Use hash of key as peer ID for key-based routing
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        self.key.hash(&mut hasher);
        let hash = hasher.finish();
        
        KadPeerId::new(hash.to_be_bytes().to_vec())
    }
}

/// DHT record value
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub key: RecordKey,
    pub value: Vec<u8>,
    pub publisher: Option<KadPeerId>,
    pub expires: Option<Instant>,
}

impl Record {
    pub fn new(key: RecordKey, value: Vec<u8>) -> Self {
        Self {
            key,
            value,
            publisher: None,
            expires: None,
        }
    }

    pub fn with_publisher(mut self, publisher: KadPeerId) -> Self {
        self.publisher = Some(publisher);
        self
    }

    pub fn with_expiration(mut self, expires: Instant) -> Self {
        self.expires = Some(expires);
        self
    }
}

/// Information about a peer in the network
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
    pub peer_id: KadPeerId,
    pub addresses: Vec<KadAddress>,
    pub connection_status: ConnectionStatus,
    pub last_seen: Option<Instant>,
}

impl PeerInfo {
    pub fn new(peer_id: KadPeerId, addresses: Vec<KadAddress>) -> Self {
        Self {
            peer_id,
            addresses,
            connection_status: ConnectionStatus::Unknown,
            last_seen: None,
        }
    }
}

/// Connection status to a peer
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Unknown,
}

/// Kademlia-specific protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KadMessage {
    /// Find the closest peers to a given key
    FindNode {
        target: KadPeerId,
        requester: KadPeerId,
    },
    /// Find the value for a given key
    FindValue {
        key: RecordKey,
        requester: KadPeerId,
    },
    /// Store a value at the given key
    PutValue {
        record: Record,
        requester: KadPeerId,
    },
    /// Add a provider for a given key
    AddProvider {
        key: RecordKey,
        provider: KadPeerId,
        provider_addresses: Vec<KadAddress>,
        requester: KadPeerId,
    },
    /// Find providers for a given key
    GetProviders {
        key: RecordKey,
        requester: KadPeerId,
    },
    /// Ping a peer (keep-alive)
    Ping {
        requester: KadPeerId,
    },
}

/// Responses to Kademlia protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KadResponse {
    /// Response to FindNode with closest known peers
    Nodes {
        closer_peers: Vec<PeerInfo>,
        requester: KadPeerId,
    },
    /// Response to FindValue with either the value or closer peers
    Value {
        record: Option<Record>,
        closer_peers: Vec<PeerInfo>,
        requester: KadPeerId,
    },
    /// Response to GetProviders
    Providers {
        key: RecordKey,
        providers: Vec<PeerInfo>,
        closer_peers: Vec<PeerInfo>,
        requester: KadPeerId,
    },
    /// Simple acknowledgment
    Ack {
        requester: KadPeerId,
    },
    /// Error response
    Error {
        error: String,
        requester: KadPeerId,
    },
}

/// Events generated by the Kademlia layer
#[derive(Debug, Clone)]
pub enum KadEvent {
    /// A query has completed successfully
    QueryCompleted {
        query_id: QueryId,
        result: QueryResult,
        duration: Duration,
    },
    /// A query has failed
    QueryFailed {
        query_id: QueryId,
        error: KadError,
        duration: Duration,
    },
    /// Routing table was updated (peer added/removed)
    RoutingUpdated {
        peer: KadPeerId,
        action: RoutingAction,
        bucket: u32,
    },
    /// Received an inbound request from a peer
    InboundRequest {
        peer: KadPeerId,
        message: KadMessage,
    },
    /// Successfully sent outbound message
    OutboundMessageSent {
        peer: KadPeerId,
        message: KadMessage,
    },
    /// Failed to send outbound message
    OutboundMessageFailed {
        peer: KadPeerId,
        message: KadMessage,
        error: KadError,
    },
    /// Peer connection status changed
    PeerConnectionChanged {
        peer: KadPeerId,
        status: ConnectionStatus,
    },
}

/// Actions performed on the routing table
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingAction {
    Added,
    Updated,
    Removed,
}

/// Results from Kademlia queries
#[derive(Debug, Clone)]
pub enum QueryResult {
    /// GetClosestPeers query result
    GetClosestPeers {
        target: KadPeerId,
        peers: Vec<PeerInfo>,
    },
    /// GetRecord query result
    GetRecord {
        key: RecordKey,
        record: Option<Record>,
        closest_peers: Vec<PeerInfo>,
    },
    /// PutRecord query result
    PutRecord {
        key: RecordKey,
        success: bool,
        replicas: u32,
    },
    /// GetProviders query result
    GetProviders {
        key: RecordKey,
        providers: Vec<PeerInfo>,
        closest_peers: Vec<PeerInfo>,
    },
    /// Bootstrap query result
    Bootstrap {
        peers_contacted: u32,
        buckets_refreshed: u32,
    },
}

/// Kademlia-specific error types
#[derive(Error, Debug, Clone)]
pub enum KadError {
    #[error("Transport error: {0}")]
    Transport(String),
    
    #[error("Query timeout after {duration:?}")]
    Timeout { duration: Duration },
    
    #[error("No peers available for query")]
    NoPeers,
    
    #[error("Record not found: {key}")]
    RecordNotFound { key: String },
    
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),
    
    #[error("Peer unreachable: {peer}")]
    PeerUnreachable { peer: String },
    
    #[error("Query failed: {reason}")]
    QueryFailed { reason: String },
    
    #[error("Storage error: {0}")]
    Storage(String),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
}

/// Configuration for Kademlia behavior
#[derive(Debug, Clone)]
pub struct KadConfig {
    /// Number of peers to return for get_closest_peers queries
    pub replication_factor: usize,
    /// Timeout for individual queries
    pub query_timeout: Duration,
    /// Timeout for individual requests
    pub request_timeout: Duration,
    /// Maximum number of concurrent queries
    pub max_concurrent_queries: usize,
    /// Bootstrap interval for periodic routing table refresh
    pub bootstrap_interval: Option<Duration>,
    /// K-bucket size (typically 20)
    pub k_value: usize,
    /// Maximum packet size for DHT messages
    pub max_packet_size: usize,
    /// Whether to perform periodic routing table refresh
    pub periodic_bootstrap: bool,
    /// Alpha parameter for parallel queries
    pub alpha: usize,
}

impl Default for KadConfig {
    fn default() -> Self {
        Self {
            replication_factor: 20,
            query_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5),
            max_concurrent_queries: 16,
            bootstrap_interval: Some(Duration::from_secs(300)), // 5 minutes
            k_value: 20,
            max_packet_size: 65536, // 64KB
            periodic_bootstrap: true,
            alpha: 3,
        }
    }
}

/// Abstract transport interface for Kademlia operations.
/// 
/// This trait allows Kademlia logic to work with different underlying transports
/// such as libp2p, iroh, or custom implementations.
#[async_trait]
pub trait KademliaTransport: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get the local peer ID
    fn local_peer_id(&self) -> KadPeerId;

    /// Get the current listening addresses
    fn listen_addresses(&self) -> Vec<KadAddress>;

    /// Send a Kademlia message to a specific peer and wait for response
    async fn send_request(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, Self::Error>;

    /// Send a Kademlia message without expecting a response
    async fn send_message(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<(), Self::Error>;

    /// Check if we have an active connection to a peer
    async fn is_connected(&self, peer: &KadPeerId) -> bool;

    /// Attempt to dial/connect to a peer
    async fn dial_peer(
        &self, 
        peer: &KadPeerId, 
        addresses: &[KadAddress]
    ) -> Result<(), Self::Error>;

    /// Add known addresses for a peer
    async fn add_peer_addresses(
        &self,
        peer: &KadPeerId,
        addresses: Vec<KadAddress>,
    ) -> Result<(), Self::Error>;

    /// Remove a peer from known addresses
    async fn remove_peer(&self, peer: &KadPeerId) -> Result<(), Self::Error>;

    /// Get connection info for a peer
    async fn peer_info(&self, peer: &KadPeerId) -> Option<PeerInfo>;

    /// Get all connected peers
    async fn connected_peers(&self) -> Vec<KadPeerId>;

    /// Start listening for incoming connections
    async fn start_listening(&mut self) -> Result<(), Self::Error>;

    /// Stop the transport and clean up resources
    async fn shutdown(&mut self) -> Result<(), Self::Error>;
}

/// Abstract event handler for receiving Kademlia events from the transport
#[async_trait]
pub trait KadEventHandler: Send + Sync + 'static {
    /// Handle a Kademlia event
    async fn handle_event(&mut self, event: KadEvent);
    
    /// Handle an inbound message that requires a response
    async fn handle_inbound_request(
        &mut self,
        peer: KadPeerId,
        message: KadMessage,
    ) -> KadResponse;
}

/// Statistics and metrics for Kademlia operations
#[derive(Debug, Clone, Default)]
pub struct KadStats {
    /// Total queries initiated
    pub queries_initiated: u64,
    /// Total queries completed successfully
    pub queries_completed: u64,
    /// Total queries that failed
    pub queries_failed: u64,
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Total peers in routing table
    pub routing_table_size: usize,
    /// Total records stored locally
    pub records_stored: usize,
    /// Average query duration
    pub avg_query_duration: Duration,
    /// Last bootstrap time
    pub last_bootstrap: Option<Instant>,
}

impl KadStats {
    pub fn success_rate(&self) -> f64 {
        if self.queries_initiated == 0 {
            0.0
        } else {
            self.queries_completed as f64 / self.queries_initiated as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kad_peer_id_distance() {
        let peer1 = KadPeerId::new(vec![0b10101010, 0b11001100]);
        let peer2 = KadPeerId::new(vec![0b11001100, 0b10101010]);
        
        let distance = peer1.distance(&peer2);
        let expected = [0b01100110, 0b01100110];
        assert_eq!(&distance.bytes[..2], &expected);
    }

    #[test]
    fn test_kad_distance_leading_zeros() {
        let distance = KadDistance {
            bytes: [0, 0, 0b00001111, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        assert_eq!(distance.leading_zeros(), 20); // 16 + 4 leading zeros
    }

    #[test]
    fn test_record_key_to_kad_peer_id() {
        let key = RecordKey::new(b"test-key".to_vec());
        let peer_id = key.to_kad_peer_id();
        
        // Should be deterministic
        let peer_id2 = key.to_kad_peer_id();
        assert_eq!(peer_id, peer_id2);
    }

    #[test]
    fn test_query_id_generation() {
        let id1 = QueryId::new();
        let id2 = QueryId::new();
        assert_ne!(id1, id2);
        assert!(id2.0 > id1.0);
    }
}