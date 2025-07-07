// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Network event abstractions for ant-net.
//!
//! This module provides a unified event system that abstracts libp2p events
//! and provides a clean interface for handling network events.

use crate::{
    types::{Addresses, ConnectionDirection, ProtocolId},
    ConnectionId, Multiaddr, PeerId,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Unified network event type.
///
/// This enum abstracts all network events that can occur in the system,
/// providing a clean interface for event handling across all behaviors.
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// A new peer has been discovered.
    PeerDiscovered {
        /// The discovered peer ID.
        peer_id: PeerId,
        /// Known addresses for the peer.
        addresses: Addresses,
    },

    /// A peer connection has been established.
    PeerConnected {
        /// The connected peer ID.
        peer_id: PeerId,
        /// The connection ID.
        connection_id: ConnectionId,
        /// The connection direction.
        direction: ConnectionDirection,
        /// The endpoint used for the connection.
        endpoint: Multiaddr,
    },

    /// A peer connection has been closed.
    PeerDisconnected {
        /// The disconnected peer ID.
        peer_id: PeerId,
        /// The connection ID.
        connection_id: ConnectionId,
        /// Reason for disconnection.
        reason: Option<String>,
    },

    /// A request has been received from a peer.
    RequestReceived {
        /// The peer that sent the request.
        peer_id: PeerId,
        /// The connection ID.
        connection_id: ConnectionId,
        /// The protocol used.
        protocol: ProtocolId,
        /// The request data.
        data: Bytes,
        /// Channel to send the response.
        response_channel: ResponseChannel,
    },

    /// A response has been received for a previous request.
    ResponseReceived {
        /// The peer that sent the response.
        peer_id: PeerId,
        /// The connection ID.
        connection_id: ConnectionId,
        /// The protocol used.
        protocol: ProtocolId,
        /// The response data.
        data: Bytes,
        /// The original request ID.
        request_id: RequestId,
    },

    /// A request has timed out.
    RequestTimeout {
        /// The peer the request was sent to.
        peer_id: PeerId,
        /// The protocol used.
        protocol: ProtocolId,
        /// The request ID.
        request_id: RequestId,
    },

    /// An outbound request failed.
    RequestFailed {
        /// The peer the request was sent to.
        peer_id: PeerId,
        /// The protocol used.
        protocol: ProtocolId,
        /// The request ID.
        request_id: RequestId,
        /// The error that occurred.
        error: String,
    },

    /// A Kademlia query has completed.
    KademliaQueryResult {
        /// The query ID.
        query_id: String,
        /// The result of the query.
        result: KademliaQueryResult,
    },

    /// A new record has been stored in the Kademlia DHT.
    KademliaRecordStored {
        /// The key of the stored record.
        key: Bytes,
        /// The peer that stored the record.
        peer_id: PeerId,
    },

    /// A record lookup in Kademlia has completed.
    KademliaRecordFound {
        /// The key of the found record.
        key: Bytes,
        /// The record data.
        data: Bytes,
        /// The peer that provided the record.
        peer_id: Option<PeerId>,
    },

    /// Peer identification information has been received.
    PeerIdentified {
        /// The identified peer.
        peer_id: PeerId,
        /// The peer's protocol version.
        protocol_version: String,
        /// The peer's agent version.
        agent_version: String,
        /// The peer's supported protocols.
        protocols: Vec<ProtocolId>,
        /// The peer's listen addresses.
        listen_addresses: Vec<Multiaddr>,
        /// The peer's observed address.
        observed_address: Option<Multiaddr>,
    },

    /// Network connectivity has been determined.
    ConnectivityChanged {
        /// The new connectivity status.
        connectivity: Connectivity,
    },

    /// A relay reservation has been established.
    RelayReservationAccepted {
        /// The relay peer.
        relay_peer: PeerId,
        /// The reservation endpoint.
        endpoint: Multiaddr,
    },

    /// A relay reservation has been denied.
    RelayReservationDenied {
        /// The relay peer.
        relay_peer: PeerId,
        /// The reason for denial.
        reason: String,
    },

    /// UPnP port mapping has been established.
    UpnpMappingEstablished {
        /// The external address.
        external_address: Multiaddr,
        /// The internal address.
        internal_address: Multiaddr,
        /// The protocol (TCP/UDP).
        protocol: String,
    },

    /// A connection is being closed.
    ConnectionClosed {
        /// The peer whose connection is being closed.
        peer_id: PeerId,
        /// The connection ID.
        connection_id: ConnectionId,
        /// Reason for closing.
        reason: String,
    },

    /// A dial attempt is being made to a peer.
    DialAttempt {
        /// The peer being dialed.
        peer_id: PeerId,
        /// The addresses being tried.
        addresses: Addresses,
    },

    /// A behavior-specific event occurred.
    BehaviorEvent {
        /// The peer involved.
        peer_id: PeerId,
        /// The behavior that generated the event.
        behavior_id: String,
        /// The event data.
        event: String,
    },

    /// Request to identify a specific peer.
    RequestPeerIdentification {
        /// The peer to identify.
        peer_id: PeerId,
    },

    /// A generic network error has occurred.
    NetworkError {
        /// The error message.
        error: String,
        /// The peer involved (if any).
        peer_id: Option<PeerId>,
    },
}

/// Request identifier for tracking request/response pairs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(pub String);

impl RequestId {
    /// Generate a new random request ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Create a request ID from a string.
    pub fn from_string(id: String) -> Self {
        Self(id)
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Channel for sending responses to requests.
#[derive(Debug)]
pub struct ResponseChannel {
    sender: tokio::sync::oneshot::Sender<Bytes>,
}

impl Clone for ResponseChannel {
    fn clone(&self) -> Self {
        // We can't actually clone a oneshot sender, so this is a placeholder
        // In a real implementation, we'd need a different approach
        let (sender, _) = tokio::sync::oneshot::channel();
        Self { sender }
    }
}

impl ResponseChannel {
    /// Create a new response channel.
    pub fn new() -> (Self, tokio::sync::oneshot::Receiver<Bytes>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (Self { sender }, receiver)
    }

    /// Send a response through the channel.
    pub fn send(self, response: Bytes) -> Result<(), Bytes> {
        self.sender.send(response)
    }
}

/// Kademlia query result types.
#[derive(Debug, Clone)]
pub enum KademliaQueryResult {
    /// A successful get_closest_peers query.
    GetClosestPeers {
        /// The target key.
        key: Bytes,
        /// The closest peers found.
        peers: Vec<PeerId>,
    },

    /// A successful get_record query.
    GetRecord {
        /// The record key.
        key: Bytes,
        /// The record data.
        data: Option<Bytes>,
        /// The peers that had the record.
        providers: Vec<PeerId>,
    },

    /// A successful put_record query.
    PutRecord {
        /// The record key.
        key: Bytes,
        /// The peers that stored the record.
        stored_at: Vec<PeerId>,
    },

    /// A query that timed out.
    Timeout {
        /// The query that timed out.
        query_type: String,
        /// The target key.
        key: Bytes,
    },

    /// A query that failed.
    Error {
        /// The error message.
        error: String,
        /// The target key.
        key: Option<Bytes>,
    },
}

/// Network connectivity status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Connectivity {
    /// The node is not connected to any peers.
    NotConnected,
    /// The node is connected but behind NAT.
    ConnectedBehindNat,
    /// The node is connected and publicly reachable.
    ConnectedPublic,
    /// Connectivity status is unknown.
    Unknown,
}

impl std::fmt::Display for Connectivity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Connectivity::NotConnected => write!(f, "not connected"),
            Connectivity::ConnectedBehindNat => write!(f, "connected behind NAT"),
            Connectivity::ConnectedPublic => write!(f, "connected public"),
            Connectivity::Unknown => write!(f, "unknown"),
        }
    }
}

/// Event priority for processing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    /// Critical events that must be processed immediately.
    Critical = 0,
    /// High priority events (connections, protocols).
    High = 1,
    /// Normal priority events (discovery, identification).
    Normal = 2,
    /// Low priority events (metrics, logging).
    Low = 3,
}

impl NetworkEvent {
    /// Get the priority of this event.
    pub fn priority(&self) -> EventPriority {
        match self {
            NetworkEvent::PeerConnected { .. } 
            | NetworkEvent::PeerDisconnected { .. } 
            | NetworkEvent::ConnectionClosed { .. } => {
                EventPriority::Critical
            }
            NetworkEvent::RequestReceived { .. }
            | NetworkEvent::ResponseReceived { .. }
            | NetworkEvent::RequestTimeout { .. }
            | NetworkEvent::RequestFailed { .. }
            | NetworkEvent::DialAttempt { .. } => EventPriority::High,
            NetworkEvent::PeerDiscovered { .. }
            | NetworkEvent::PeerIdentified { .. }
            | NetworkEvent::KademliaQueryResult { .. }
            | NetworkEvent::KademliaRecordStored { .. }
            | NetworkEvent::KademliaRecordFound { .. }
            | NetworkEvent::RequestPeerIdentification { .. }
            | NetworkEvent::BehaviorEvent { .. } => EventPriority::Normal,
            NetworkEvent::ConnectivityChanged { .. }
            | NetworkEvent::RelayReservationAccepted { .. }
            | NetworkEvent::RelayReservationDenied { .. }
            | NetworkEvent::UpnpMappingEstablished { .. }
            | NetworkEvent::NetworkError { .. } => EventPriority::Low,
        }
    }

    /// Get the peer ID associated with this event, if any.
    pub fn peer_id(&self) -> Option<PeerId> {
        match self {
            NetworkEvent::PeerDiscovered { peer_id, .. }
            | NetworkEvent::PeerConnected { peer_id, .. }
            | NetworkEvent::PeerDisconnected { peer_id, .. }
            | NetworkEvent::RequestReceived { peer_id, .. }
            | NetworkEvent::ResponseReceived { peer_id, .. }
            | NetworkEvent::RequestTimeout { peer_id, .. }
            | NetworkEvent::RequestFailed { peer_id, .. }
            | NetworkEvent::KademliaRecordStored { peer_id, .. }
            | NetworkEvent::PeerIdentified { peer_id, .. }
            | NetworkEvent::ConnectionClosed { peer_id, .. }
            | NetworkEvent::DialAttempt { peer_id, .. }
            | NetworkEvent::BehaviorEvent { peer_id, .. }
            | NetworkEvent::RequestPeerIdentification { peer_id, .. }
            | NetworkEvent::RelayReservationAccepted {
                relay_peer: peer_id, ..
            }
            | NetworkEvent::RelayReservationDenied {
                relay_peer: peer_id, ..
            } => Some(*peer_id),
            NetworkEvent::KademliaRecordFound { peer_id, .. } => *peer_id,
            NetworkEvent::NetworkError { peer_id, .. } => *peer_id,
            _ => None,
        }
    }

    /// Check if this event indicates a connection change.
    pub fn is_connection_event(&self) -> bool {
        matches!(
            self,
            NetworkEvent::PeerConnected { .. } 
            | NetworkEvent::PeerDisconnected { .. }
            | NetworkEvent::ConnectionClosed { .. }
            | NetworkEvent::DialAttempt { .. }
        )
    }

    /// Check if this event is related to a protocol operation.
    pub fn is_protocol_event(&self) -> bool {
        matches!(
            self,
            NetworkEvent::RequestReceived { .. }
                | NetworkEvent::ResponseReceived { .. }
                | NetworkEvent::RequestTimeout { .. }
                | NetworkEvent::RequestFailed { .. }
        )
    }

    /// Check if this event is related to Kademlia DHT.
    pub fn is_kademlia_event(&self) -> bool {
        matches!(
            self,
            NetworkEvent::KademliaQueryResult { .. }
                | NetworkEvent::KademliaRecordStored { .. }
                | NetworkEvent::KademliaRecordFound { .. }
        )
    }
}

// Simple UUID implementation for event correlation
#[allow(dead_code)]
mod uuid {
    pub struct Uuid;
    
    impl Uuid {
        pub fn new_v4() -> Self {
            Self
        }
        
        pub fn to_string(&self) -> String {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                rng.gen::<u32>(),
                rng.gen::<u16>(),
                rng.gen::<u16>(),
                rng.gen::<u16>(),
                rng.gen::<u64>() & 0xffffffffffff
            )
        }
    }
}