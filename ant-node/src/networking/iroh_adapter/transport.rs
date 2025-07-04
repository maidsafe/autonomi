// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! iroh transport implementation for Kademlia DHT
//! 
//! This module provides the `IrohTransport` which implements the `KademliaTransport`
//! trait, enabling Kademlia operations over iroh's networking layer.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::sync::{RwLock, Mutex};
use tracing::{debug, error, info, trace, warn};

// Placeholder types for iroh networking (real iroh types would be used in production)
use crate::networking::iroh_adapter::discovery::{NodeId, NodeAddr, DiscoveryItem, Discovery};

pub type Connection = std::marker::PhantomData<()>;
pub type Endpoint = std::marker::PhantomData<()>;
pub type RelayUrl = String;

use crate::networking::{
    kad::transport::{
        KademliaTransport, KadPeerId, KadAddress, KadMessage, KadResponse, KadError,
        PeerInfo, ConnectionStatus, QueryId, RecordKey, Record,
    },
    iroh_adapter::{
        config::{IrohConfig, NetworkConfig},
        protocol::{KadProtocol, MessageHandler},
        discovery::DiscoveryBridge,
        IrohError, IrohResult, KAD_ALPN,
    },
};

/// Mapping between transport-agnostic peer IDs and iroh NodeIds
type PeerMapping = Arc<RwLock<BiMap<KadPeerId, NodeId>>>;

/// Bidirectional map for efficient peer ID conversion
#[derive(Debug, Clone, Default)]
struct BiMap<K, V>
where
    K: Clone + std::hash::Hash + Eq,
    V: Clone + std::hash::Hash + Eq,
{
    forward: HashMap<K, V>,
    reverse: HashMap<V, K>,
}

impl<K, V> BiMap<K, V>
where
    K: Clone + std::hash::Hash + Eq,
    V: Clone + std::hash::Hash + Eq,
{
    fn new() -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
        }
    }
    
    fn insert(&mut self, key: K, value: V) {
        // Remove any existing mappings
        if let Some(old_value) = self.forward.remove(&key) {
            self.reverse.remove(&old_value);
        }
        if let Some(old_key) = self.reverse.remove(&value) {
            self.forward.remove(&old_key);
        }
        
        // Insert new mapping
        self.forward.insert(key.clone(), value.clone());
        self.reverse.insert(value, key);
    }
    
    fn get_by_left(&self, key: &K) -> Option<&V> {
        self.forward.get(key)
    }
    
    fn get_by_right(&self, value: &V) -> Option<&K> {
        self.reverse.get(value)
    }
    
    fn remove_by_left(&mut self, key: &K) -> Option<V> {
        if let Some(value) = self.forward.remove(key) {
            self.reverse.remove(&value);
            Some(value)
        } else {
            None
        }
    }
    
    fn remove_by_right(&mut self, value: &V) -> Option<K> {
        if let Some(key) = self.reverse.remove(value) {
            self.forward.remove(&key);
            Some(key)
        } else {
            None
        }
    }
}

/// iroh transport implementation for Kademlia DHT
pub struct IrohTransport {
    /// iroh networking endpoint
    endpoint: Endpoint,
    
    /// Local iroh node ID
    node_id: NodeId,
    
    /// Local Kademlia peer ID
    kad_peer_id: KadPeerId,
    
    /// Configuration
    config: IrohConfig,
    
    /// Mapping between Kademlia peer IDs and iroh NodeIds
    peer_mapping: PeerMapping,
    
    /// Known peer addresses and information
    peer_info: Arc<RwLock<HashMap<KadPeerId, PeerInfo>>>,
    
    /// Active connections to peers
    connections: Arc<RwLock<HashMap<NodeId, ConnectionInfo>>>,
    
    /// Kademlia protocol handler
    protocol: KadProtocol,
    
    /// Discovery service
    discovery: Arc<dyn Discovery + Send + Sync>,
    
    /// Transport statistics
    stats: Arc<RwLock<TransportStats>>,
    
    /// Connection pool for reusing connections
    connection_pool: Arc<Mutex<HashMap<NodeId, Arc<Connection>>>>,
    
    /// Shutdown signal
    shutdown: Arc<tokio::sync::Notify>,
}

/// Information about an active connection
#[derive(Debug, Clone)]
struct ConnectionInfo {
    connection: Arc<Connection>,
    established_at: Instant,
    last_used: Instant,
    message_count: u64,
    kad_peer_id: KadPeerId,
}

/// Transport statistics
#[derive(Debug, Clone, Default)]
pub struct TransportStats {
    pub connections_established: u64,
    pub connections_failed: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub discovery_queries: u64,
    pub discovery_successes: u64,
    pub active_connections: usize,
}

/// Simple message handler that delegates to user-provided handler
struct DelegatingMessageHandler {
    handler: Arc<dyn Fn(KadPeerId, KadMessage) -> futures::future::BoxFuture<'static, Result<KadResponse, KadError>> + Send + Sync>,
}

#[async_trait]
impl MessageHandler for DelegatingMessageHandler {
    async fn handle_message(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, KadError> {
        (self.handler)(peer.clone(), message).await
    }
}

impl IrohTransport {
    /// Create a new iroh transport
    pub async fn new(
        config: IrohConfig,
        message_handler: impl Fn(KadPeerId, KadMessage) -> futures::future::BoxFuture<'static, Result<KadResponse, KadError>> + Send + Sync + 'static,
    ) -> IrohResult<Self> {
        info!("Creating iroh transport with config: {:?}", config);
        
        // Build iroh endpoint
        let endpoint = Self::build_endpoint(&config.network).await?;
        let node_id = endpoint.node_id();
        let kad_peer_id = Self::node_id_to_kad_peer_id(node_id);
        
        info!("iroh endpoint created with NodeId: {:?}", node_id);
        
        // Create discovery service
        let discovery = Self::create_discovery(&config)?;
        
        // Create protocol handler
        let delegating_handler = Arc::new(DelegatingMessageHandler {
            handler: Arc::new(message_handler),
        });
        
        let protocol = KadProtocol::new(
            node_id,
            kad_peer_id.clone(),
            config.protocol.clone(),
            delegating_handler,
        );
        
        let transport = Self {
            endpoint,
            node_id,
            kad_peer_id,
            config,
            peer_mapping: Arc::new(RwLock::new(BiMap::new())),
            peer_info: Arc::new(RwLock::new(HashMap::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            protocol,
            discovery,
            stats: Arc::new(RwLock::new(TransportStats::default())),
            connection_pool: Arc::new(Mutex::new(HashMap::new())),
            shutdown: Arc::new(tokio::sync::Notify::new()),
        };
        
        // Start background tasks
        transport.start_background_tasks().await;
        
        Ok(transport)
    }
    
    /// Build iroh endpoint with configuration
    async fn build_endpoint(config: &NetworkConfig) -> IrohResult<Endpoint> {
        let mut builder = Endpoint::builder();
        
        // Configure relay if enabled
        if config.enable_relay {
            if config.relay_urls.is_empty() {
                builder = builder.relay_mode(RelayMode::Default);
            } else {
                let relay_urls: Result<Vec<RelayUrl>, _> = config
                    .relay_urls
                    .iter()
                    .map(|url| url.parse())
                    .collect();
                
                match relay_urls {
                    Ok(urls) => {
                        builder = builder.relay_mode(RelayMode::Custom(urls));
                    },
                    Err(e) => {
                        warn!("Invalid relay URLs, using default: {}", e);
                        builder = builder.relay_mode(RelayMode::Default);
                    }
                }
            }
        } else {
            builder = builder.relay_mode(RelayMode::Disabled);
        }
        
        // Configure discovery
        #[cfg(feature = "discovery-n0")]
        if config.use_n0_dns {
            builder = builder.discovery_n0();
        }
        
        // Set ALPN protocols
        builder = builder.alpns(vec![KAD_ALPN.to_vec()]);
        
        // Configure bind addresses
        if !config.bind_addresses.is_empty() {
            for addr in &config.bind_addresses {
                builder = builder.bind_addr(*addr);
            }
        }
        
        // Build and bind endpoint
        let endpoint = builder
            .bind()
            .await
            .map_err(|e| IrohError::Endpoint(Box::new(e)))?;
        
        Ok(endpoint)
    }
    
    /// Create discovery service
    fn create_discovery(config: &IrohConfig) -> IrohResult<Arc<dyn Discovery + Send + Sync>> {
        // For now, use a simple discovery bridge
        // In a full implementation, this would integrate with iroh's discovery system
        let bridge = DiscoveryBridge::new(config.discovery.clone());
        Ok(Arc::new(bridge))
    }
    
    /// Start background tasks for the transport
    async fn start_background_tasks(&self) {
        let transport = self.clone();
        tokio::spawn(async move {
            transport.connection_acceptor().await;
        });
        
        let transport = self.clone();
        tokio::spawn(async move {
            transport.connection_cleanup_task().await;
        });
        
        let transport = self.clone();
        tokio::spawn(async move {
            transport.stats_update_task().await;
        });
    }
    
    /// Accept incoming connections
    async fn connection_acceptor(&self) {
        while let Ok(connection) = self.endpoint.accept().await {
            let protocol = self.protocol.clone();
            let transport = self.clone();
            
            tokio::spawn(async move {
                let peer_node_id = connection.remote_node_id();
                let kad_peer_id = Self::node_id_to_kad_peer_id(peer_node_id);
                
                // Track the connection
                transport.track_connection(peer_node_id, kad_peer_id.clone(), connection.clone()).await;
                
                // Handle the connection
                if let Err(e) = protocol.handle_connection(connection).await {
                    warn!("Error handling connection from {:?}: {}", peer_node_id, e);
                }
                
                // Clean up when done
                transport.untrack_connection(peer_node_id).await;
            });
        }
    }
    
    /// Periodic cleanup of stale connections
    async fn connection_cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.cleanup_stale_connections().await;
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Periodic statistics update
    async fn stats_update_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.update_connection_stats().await;
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Track a new connection
    async fn track_connection(&self, node_id: NodeId, kad_peer_id: KadPeerId, connection: Connection) {
        let connection_info = ConnectionInfo {
            connection: Arc::new(connection),
            established_at: Instant::now(),
            last_used: Instant::now(),
            message_count: 0,
            kad_peer_id: kad_peer_id.clone(),
        };
        
        self.connections.write().await.insert(node_id, connection_info);
        self.peer_mapping.write().await.insert(kad_peer_id, node_id);
        
        let mut stats = self.stats.write().await;
        stats.connections_established += 1;
        stats.active_connections = self.connections.read().await.len();
    }
    
    /// Remove tracking for a connection
    async fn untrack_connection(&self, node_id: NodeId) {
        if let Some(info) = self.connections.write().await.remove(&node_id) {
            self.peer_mapping.write().await.remove_by_right(&node_id);
        }
        
        let mut stats = self.stats.write().await;
        stats.active_connections = self.connections.read().await.len();
    }
    
    /// Clean up stale connections
    async fn cleanup_stale_connections(&self) {
        let stale_threshold = Duration::from_secs(300); // 5 minutes
        let now = Instant::now();
        
        let mut to_remove = Vec::new();
        {
            let connections = self.connections.read().await;
            for (node_id, info) in connections.iter() {
                if now.duration_since(info.last_used) > stale_threshold {
                    to_remove.push(*node_id);
                }
            }
        }
        
        for node_id in to_remove {
            self.untrack_connection(node_id).await;
            debug!("Cleaned up stale connection to {:?}", node_id);
        }
    }
    
    /// Update connection statistics
    async fn update_connection_stats(&self) {
        let connections_count = self.connections.read().await.len();
        let mut stats = self.stats.write().await;
        stats.active_connections = connections_count;
    }
    
    /// Convert NodeId to KadPeerId
    fn node_id_to_kad_peer_id(node_id: NodeId) -> KadPeerId {
        KadPeerId::new(node_id.as_bytes().to_vec())
    }
    
    /// Convert KadPeerId to NodeId
    fn kad_peer_id_to_node_id(kad_peer_id: &KadPeerId) -> IrohResult<NodeId> {
        if kad_peer_id.0.len() != 32 {
            return Err(IrohError::InvalidPeerId(format!(
                "Invalid peer ID length: {} (expected 32)",
                kad_peer_id.0.len()
            )));
        }
        
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&kad_peer_id.0);
        
        NodeId::from_bytes(&bytes)
            .map_err(|e| IrohError::InvalidPeerId(format!("Invalid NodeId bytes: {}", e)))
    }
    
    /// Get or create a connection to a peer
    async fn get_or_connect(&self, kad_peer_id: &KadPeerId) -> Result<Arc<Connection>, KadError> {
        // First check if we already have a connection
        if let Some(node_id) = self.peer_mapping.read().await.get_by_left(kad_peer_id) {
            if let Some(info) = self.connections.read().await.get(node_id) {
                // Update last used time
                if let Some(mut info) = self.connections.write().await.get_mut(node_id) {
                    info.last_used = Instant::now();
                    info.message_count += 1;
                }
                return Ok(info.connection.clone());
            }
        }
        
        // Need to establish a new connection
        let node_id = Self::kad_peer_id_to_node_id(kad_peer_id)
            .map_err(|e| KadError::Transport(e.to_string()))?;
        
        // Try to discover the peer's addresses
        let node_addr = self.discover_peer_addresses(node_id).await?;
        
        // Establish connection
        let connection = self.endpoint
            .connect(node_addr, KAD_ALPN)
            .await
            .map_err(|e| KadError::Transport(format!("Connection failed: {}", e)))?;
        
        // Track the new connection
        self.track_connection(node_id, kad_peer_id.clone(), connection.clone()).await;
        
        Ok(Arc::new(connection))
    }
    
    /// Discover addresses for a peer
    async fn discover_peer_addresses(&self, node_id: NodeId) -> Result<NodeAddr, KadError> {
        let mut stats = self.stats.write().await;
        stats.discovery_queries += 1;
        drop(stats);
        
        // First check if we have cached peer info
        let kad_peer_id = Self::node_id_to_kad_peer_id(node_id);
        if let Some(peer_info) = self.peer_info.read().await.get(&kad_peer_id) {
            // Convert addresses to iroh format
            let addrs: Vec<std::net::SocketAddr> = peer_info.addresses
                .iter()
                .filter_map(|addr| addr.socket_addr())
                .collect();
            
            if !addrs.is_empty() {
                let mut stats = self.stats.write().await;
                stats.discovery_successes += 1;
                
                return Ok(NodeAddr::new(node_id).with_direct_addresses(addrs));
            }
        }
        
        // Use discovery service
        match self.discovery.resolve(node_id).await {
            Ok(node_addr) => {
                let mut stats = self.stats.write().await;
                stats.discovery_successes += 1;
                Ok(node_addr)
            },
            Err(e) => {
                Err(KadError::Transport(format!("Discovery failed: {}", e)))
            }
        }
    }
    
    /// Get transport statistics
    pub async fn stats(&self) -> TransportStats {
        self.stats.read().await.clone()
    }
    
    /// Shutdown the transport
    pub async fn shutdown(&self) -> IrohResult<()> {
        info!("Shutting down iroh transport");
        
        // Signal shutdown to background tasks
        self.shutdown.notify_waiters();
        
        // Close all connections
        let connections: Vec<_> = self.connections.read().await.keys().cloned().collect();
        for node_id in connections {
            self.untrack_connection(node_id).await;
        }
        
        // Close endpoint
        self.endpoint.close().await.map_err(|e| IrohError::Endpoint(Box::new(e)))?;
        
        Ok(())
    }
}

// Implement Clone for IrohTransport (needed for background tasks)
impl Clone for IrohTransport {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            node_id: self.node_id,
            kad_peer_id: self.kad_peer_id.clone(),
            config: self.config.clone(),
            peer_mapping: self.peer_mapping.clone(),
            peer_info: self.peer_info.clone(),
            connections: self.connections.clone(),
            protocol: self.protocol.clone(),
            discovery: self.discovery.clone(),
            stats: self.stats.clone(),
            connection_pool: self.connection_pool.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

#[async_trait]
impl KademliaTransport for IrohTransport {
    type Error = KadError;
    
    fn local_peer_id(&self) -> KadPeerId {
        self.kad_peer_id.clone()
    }
    
    fn listen_addresses(&self) -> Vec<KadAddress> {
        // Get local addresses from iroh endpoint
        let bound_sockets = self.endpoint.bound_sockets();
        bound_sockets
            .into_iter()
            .map(|socket_addr| KadAddress::new("quic".to_string(), socket_addr.to_string()))
            .collect()
    }
    
    async fn send_request(&self, peer: &KadPeerId, message: KadMessage) -> Result<KadResponse, Self::Error> {
        trace!("Sending request to peer {:?}: {:?}", peer, message);
        
        // Get NodeId for the peer
        let node_id = Self::kad_peer_id_to_node_id(peer)
            .map_err(|e| KadError::Transport(e.to_string()))?;
        
        // Send via protocol handler
        let response = self.protocol
            .send_request(&self.endpoint, node_id, peer.clone(), message)
            .await?;
        
        // Update stats
        let mut stats = self.stats.write().await;
        stats.messages_sent += 1;
        
        Ok(response)
    }
    
    async fn send_message(&self, peer: &KadPeerId, message: KadMessage) -> Result<(), Self::Error> {
        // For fire-and-forget, we can just send without waiting for response
        // This is a simplified implementation
        let _ = self.send_request(peer, message).await?;
        Ok(())
    }
    
    async fn is_connected(&self, peer: &KadPeerId) -> bool {
        if let Some(node_id) = self.peer_mapping.read().await.get_by_left(peer) {
            self.connections.read().await.contains_key(node_id)
        } else {
            false
        }
    }
    
    async fn dial_peer(&self, peer: &KadPeerId, addresses: &[KadAddress]) -> Result<(), Self::Error> {
        debug!("Dialing peer {:?} with addresses: {:?}", peer, addresses);
        
        // Convert addresses to socket addresses
        let socket_addrs: Vec<std::net::SocketAddr> = addresses
            .iter()
            .filter_map(|addr| addr.socket_addr())
            .collect();
        
        if socket_addrs.is_empty() {
            return Err(KadError::Transport("No valid socket addresses provided".to_string()));
        }
        
        let node_id = Self::kad_peer_id_to_node_id(peer)
            .map_err(|e| KadError::Transport(e.to_string()))?;
        
        let node_addr = NodeAddr::new(node_id).with_direct_addresses(socket_addrs);
        
        // Attempt to connect
        let connection = self.endpoint
            .connect(node_addr, KAD_ALPN)
            .await
            .map_err(|e| KadError::Transport(format!("Dial failed: {}", e)))?;
        
        // Track the connection
        self.track_connection(node_id, peer.clone(), connection).await;
        
        Ok(())
    }
    
    async fn add_peer_addresses(&self, peer: &KadPeerId, addresses: Vec<KadAddress>) -> Result<(), Self::Error> {
        let peer_info = PeerInfo {
            peer_id: peer.clone(),
            addresses,
            connection_status: ConnectionStatus::Unknown,
            last_seen: Some(Instant::now()),
        };
        
        self.peer_info.write().await.insert(peer.clone(), peer_info);
        
        debug!("Added addresses for peer {:?}", peer);
        Ok(())
    }
    
    async fn remove_peer(&self, peer: &KadPeerId) -> Result<(), Self::Error> {
        // Remove from peer info
        self.peer_info.write().await.remove(peer);
        
        // Remove from mapping and close connection if exists
        if let Some(node_id) = self.peer_mapping.write().await.remove_by_left(peer) {
            self.untrack_connection(node_id).await;
        }
        
        debug!("Removed peer {:?}", peer);
        Ok(())
    }
    
    async fn peer_info(&self, peer: &KadPeerId) -> Option<PeerInfo> {
        self.peer_info.read().await.get(peer).cloned()
    }
    
    async fn connected_peers(&self) -> Vec<KadPeerId> {
        self.connections
            .read()
            .await
            .values()
            .map(|info| info.kad_peer_id.clone())
            .collect()
    }
    
    async fn start_listening(&mut self) -> Result<(), Self::Error> {
        // iroh endpoint starts listening automatically when created
        info!("iroh transport listening on: {:?}", self.listen_addresses());
        Ok(())
    }
    
    async fn shutdown(&mut self) -> Result<(), Self::Error> {
        self.shutdown().await.map_err(|e| KadError::Transport(e.to_string()))
    }
}