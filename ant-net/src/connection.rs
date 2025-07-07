// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Connection management abstractions for ant-net.
//!
//! This module provides high-level interfaces for managing network connections,
//! hiding the complexity of libp2p connection handling.

use crate::{
    types::{Addresses, ConnectionDirection, ConnectionState},
    AntNetError, ConnectionId, Multiaddr, PeerId, Result,
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    fmt,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

/// Abstract connection interface.
///
/// Represents an active connection to a peer, providing methods
/// for querying connection state and metadata.
#[async_trait]
pub trait Connection: Send + Sync + fmt::Debug {
    /// Get the connection ID.
    fn id(&self) -> ConnectionId;

    /// Get the remote peer ID.
    fn peer_id(&self) -> PeerId;

    /// Get the connection state.
    async fn state(&self) -> ConnectionState;

    /// Get the connection direction.
    fn direction(&self) -> ConnectionDirection;

    /// Get the local address used for this connection.
    fn local_addr(&self) -> Option<Multiaddr>;

    /// Get the remote address used for this connection.
    fn remote_addr(&self) -> Option<Multiaddr>;

    /// Get when the connection was established.
    fn established_at(&self) -> Instant;

    /// Check if the connection is still alive.
    async fn is_alive(&self) -> bool;

    /// Close the connection.
    async fn close(&mut self) -> Result<()>;
}

/// Connection metadata and state tracking.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Unique connection identifier.
    pub id: ConnectionId,
    /// Remote peer ID.
    pub peer_id: PeerId,
    /// Current connection state.
    pub state: ConnectionState,
    /// Connection direction.
    pub direction: ConnectionDirection,
    /// Local address used for this connection.
    pub local_addr: Option<Multiaddr>,
    /// Remote address used for this connection.
    pub remote_addr: Option<Multiaddr>,
    /// When the connection was established.
    pub established_at: Instant,
    /// Last activity on this connection.
    pub last_activity: Instant,
}

impl ConnectionInfo {
    /// Create new connection info.
    pub fn new(
        id: ConnectionId,
        peer_id: PeerId,
        direction: ConnectionDirection,
        local_addr: Option<Multiaddr>,
        remote_addr: Option<Multiaddr>,
    ) -> Self {
        let now = Instant::now();
        Self {
            id,
            peer_id,
            state: ConnectionState::Connecting,
            direction,
            local_addr,
            remote_addr,
            established_at: now,
            last_activity: now,
        }
    }

    /// Update the connection state.
    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
        self.last_activity = Instant::now();
    }

    /// Update the last activity timestamp.
    pub fn update_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if the connection is considered stale.
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }
}

/// Connection manager interface.
///
/// Provides high-level connection management functionality,
/// including connection tracking, lifecycle management, and cleanup.
#[async_trait]
pub trait ConnectionManager: Send + Sync + fmt::Debug {
    /// Get information about a specific connection.
    async fn connection_info(&self, id: ConnectionId) -> Option<ConnectionInfo>;

    /// Get all connections to a specific peer.
    async fn connections_to_peer(&self, peer_id: PeerId) -> Vec<ConnectionInfo>;

    /// Get all active connections.
    async fn all_connections(&self) -> Vec<ConnectionInfo>;

    /// Get the number of active connections.
    async fn connection_count(&self) -> usize;

    /// Check if connected to a specific peer.
    async fn is_connected(&self, peer_id: PeerId) -> bool;

    /// Initiate a connection to a peer.
    async fn dial_peer(&mut self, peer_id: PeerId, addresses: Addresses) -> Result<ConnectionId>;

    /// Close a specific connection.
    async fn close_connection(&mut self, id: ConnectionId) -> Result<()>;

    /// Close all connections to a peer.
    async fn close_peer_connections(&mut self, peer_id: PeerId) -> Result<usize>;

    /// Clean up stale connections.
    async fn cleanup_stale_connections(&mut self, timeout: Duration) -> usize;

    /// Get connection statistics.
    async fn connection_stats(&self) -> ConnectionStats;
}

/// Connection statistics.
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// Total number of active connections.
    pub total_connections: usize,
    /// Number of inbound connections.
    pub inbound_connections: usize,
    /// Number of outbound connections.
    pub outbound_connections: usize,
    /// Number of connected peers.
    pub connected_peers: usize,
    /// Average connection age.
    pub average_connection_age: Duration,
    /// Number of failed connections (recently).
    pub failed_connections: usize,
}

/// Default connection manager implementation.
#[derive(Debug)]
pub struct DefaultConnectionManager {
    /// Active connections indexed by connection ID.
    connections: RwLock<HashMap<ConnectionId, ConnectionInfo>>,
    /// Connections indexed by peer ID for fast lookup.
    peer_connections: RwLock<HashMap<PeerId, Vec<ConnectionId>>>,
    /// Connection failure tracking.
    failed_connections: RwLock<HashMap<PeerId, Vec<Instant>>>,
    /// Configuration options.
    config: ConnectionManagerConfig,
}

/// Configuration for the connection manager.
#[derive(Debug, Clone)]
pub struct ConnectionManagerConfig {
    /// Maximum number of connections per peer.
    pub max_connections_per_peer: usize,
    /// Connection idle timeout.
    pub idle_timeout: Duration,
    /// How long to track connection failures.
    pub failure_tracking_duration: Duration,
    /// Maximum number of connection failures to track per peer.
    pub max_failures_per_peer: usize,
}

impl Default for ConnectionManagerConfig {
    fn default() -> Self {
        Self {
            max_connections_per_peer: 3,
            idle_timeout: Duration::from_secs(300), // 5 minutes
            failure_tracking_duration: Duration::from_secs(3600), // 1 hour
            max_failures_per_peer: 10,
        }
    }
}

impl DefaultConnectionManager {
    /// Create a new connection manager with default configuration.
    pub fn new() -> Self {
        Self::with_config(ConnectionManagerConfig::default())
    }

    /// Create a new connection manager with custom configuration.
    pub fn with_config(config: ConnectionManagerConfig) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            peer_connections: RwLock::new(HashMap::new()),
            failed_connections: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Track a new connection.
    pub async fn track_connection(&self, info: ConnectionInfo) {
        let connection_id = info.id;
        let peer_id = info.peer_id;

        // Add to connections map
        self.connections.write().await.insert(connection_id, info);

        // Add to peer connections index
        self.peer_connections
            .write()
            .await
            .entry(peer_id)
            .or_default()
            .push(connection_id);
    }

    /// Update connection state.
    pub async fn update_connection_state(
        &self,
        id: ConnectionId,
        state: ConnectionState,
    ) -> Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.get_mut(&id) {
            info.set_state(state);
            Ok(())
        } else {
            Err(AntNetError::Connection(format!(
                "Connection {} not found",
                id
            )))
        }
    }

    /// Remove a connection from tracking.
    pub async fn remove_connection(&self, id: ConnectionId) -> Option<ConnectionInfo> {
        let removed = self.connections.write().await.remove(&id);

        if let Some(ref info) = removed {
            // Remove from peer connections index
            let mut peer_connections = self.peer_connections.write().await;
            if let Some(connections) = peer_connections.get_mut(&info.peer_id) {
                connections.retain(|&conn_id| conn_id != id);
                if connections.is_empty() {
                    peer_connections.remove(&info.peer_id);
                }
            }
        }

        removed
    }

    /// Record a connection failure.
    pub async fn record_failure(&self, peer_id: PeerId) {
        let now = Instant::now();
        let mut failures = self.failed_connections.write().await;
        
        let peer_failures = failures.entry(peer_id).or_default();
        peer_failures.push(now);

        // Keep only recent failures
        peer_failures.retain(|&failure_time| {
            now.duration_since(failure_time) < self.config.failure_tracking_duration
        });

        // Limit the number of tracked failures
        if peer_failures.len() > self.config.max_failures_per_peer {
            peer_failures.remove(0);
        }
    }

    /// Get recent failure count for a peer.
    pub async fn failure_count(&self, peer_id: PeerId) -> usize {
        let failures = self.failed_connections.read().await;
        failures.get(&peer_id).map(|f| f.len()).unwrap_or(0)
    }
}

impl Default for DefaultConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConnectionManager for DefaultConnectionManager {
    async fn connection_info(&self, id: ConnectionId) -> Option<ConnectionInfo> {
        self.connections.read().await.get(&id).cloned()
    }

    async fn connections_to_peer(&self, peer_id: PeerId) -> Vec<ConnectionInfo> {
        let connections = self.connections.read().await;
        let peer_connections = self.peer_connections.read().await;

        if let Some(connection_ids) = peer_connections.get(&peer_id) {
            connection_ids
                .iter()
                .filter_map(|id| connections.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    async fn all_connections(&self) -> Vec<ConnectionInfo> {
        self.connections.read().await.values().cloned().collect()
    }

    async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    async fn is_connected(&self, peer_id: PeerId) -> bool {
        let peer_connections = self.peer_connections.read().await;
        peer_connections.contains_key(&peer_id)
    }

    async fn dial_peer(&mut self, peer_id: PeerId, _addresses: Addresses) -> Result<ConnectionId> {
        // Check connection limits
        let current_connections = self.connections_to_peer(peer_id).await.len();
        if current_connections >= self.config.max_connections_per_peer {
            return Err(AntNetError::Connection(format!(
                "Maximum connections per peer ({}) reached for {}",
                self.config.max_connections_per_peer, peer_id
            )));
        }

        // This would integrate with the actual transport layer
        // For now, we'll return a placeholder
        todo!("Integrate with transport layer for actual dialing")
    }

    async fn close_connection(&mut self, id: ConnectionId) -> Result<()> {
        if let Some(info) = self.remove_connection(id).await {
            tracing::debug!("Closed connection {} to {}", id, info.peer_id);
            Ok(())
        } else {
            Err(AntNetError::Connection(format!(
                "Connection {} not found",
                id
            )))
        }
    }

    async fn close_peer_connections(&mut self, peer_id: PeerId) -> Result<usize> {
        let connection_infos = self.connections_to_peer(peer_id).await;
        let count = connection_infos.len();

        for info in connection_infos {
            self.remove_connection(info.id).await;
        }

        tracing::debug!("Closed {} connections to {}", count, peer_id);
        Ok(count)
    }

    async fn cleanup_stale_connections(&mut self, timeout: Duration) -> usize {
        let now = Instant::now();
        let connections = self.connections.read().await;
        
        let stale_connections: Vec<ConnectionId> = connections
            .values()
            .filter(|info| {
                matches!(info.state, ConnectionState::Connected) 
                    && now.duration_since(info.last_activity) > timeout
            })
            .map(|info| info.id)
            .collect();

        drop(connections); // Release the read lock

        let count = stale_connections.len();
        for id in stale_connections {
            let _ = self.remove_connection(id).await;
        }

        if count > 0 {
            tracing::debug!("Cleaned up {} stale connections", count);
        }

        count
    }

    async fn connection_stats(&self) -> ConnectionStats {
        let connections = self.connections.read().await;
        let now = Instant::now();
        
        let mut stats = ConnectionStats::default();
        stats.total_connections = connections.len();
        
        let mut total_age = Duration::ZERO;
        let mut peer_set = std::collections::HashSet::new();

        for info in connections.values() {
            match info.direction {
                ConnectionDirection::Inbound => stats.inbound_connections += 1,
                ConnectionDirection::Outbound => stats.outbound_connections += 1,
            }
            
            peer_set.insert(info.peer_id);
            total_age += now.duration_since(info.established_at);
        }

        stats.connected_peers = peer_set.len();
        
        if stats.total_connections > 0 {
            stats.average_connection_age = total_age / stats.total_connections as u32;
        }

        // Count recent failures
        let failures = self.failed_connections.read().await;
        stats.failed_connections = failures.values().map(|f| f.len()).sum();

        stats
    }
}