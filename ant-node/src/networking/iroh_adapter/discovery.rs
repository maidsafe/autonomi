// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Discovery bridge between Kademlia and iroh discovery systems
//! 
//! This module provides integration between Kademlia's peer discovery and
//! iroh's built-in discovery mechanisms, creating a unified discovery system.

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::sync::{RwLock, Mutex};
use tracing::{debug, error, info, trace, warn};

// Note: iroh dependencies temporarily disabled due to version conflicts
// use iroh_net::{
//     discovery::{Discovery, DiscoveryItem},
//     NodeAddr, NodeId,
// };

// Placeholder types for compilation (real iroh types would be used in production)
pub type NodeId = [u8; 32];
pub struct NodeAddr {
    node_id: NodeId,
    addresses: Vec<SocketAddr>,
}

impl NodeAddr {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            addresses: Vec::new(),
        }
    }
    
    pub fn with_direct_addresses(mut self, addresses: Vec<SocketAddr>) -> Self {
        self.addresses = addresses;
        self
    }
    
    pub fn direct_addresses(&self) -> &[SocketAddr] {
        &self.addresses
    }
}

pub struct DiscoveryItem;

#[async_trait]
pub trait Discovery {
    type Item;
    async fn resolve(&self, node_id: NodeId) -> anyhow::Result<NodeAddr>;
}

use crate::networking::{
    kad::transport::{KadPeerId, KadAddress},
    iroh_adapter::{
        config::DiscoveryConfig,
        IrohError, IrohResult,
    },
};

/// Bridge between Kademlia peer discovery and iroh discovery
pub struct DiscoveryBridge {
    /// Configuration for discovery behavior
    config: DiscoveryConfig,
    
    /// Known peers from Kademlia routing table
    kad_peers: Arc<RwLock<HashMap<KadPeerId, PeerAddressInfo>>>,
    
    /// Cached discovered addresses from iroh discovery
    discovery_cache: Arc<RwLock<HashMap<NodeId, CachedDiscoveryInfo>>>,
    
    /// Mapping between KadPeerId and NodeId for efficient lookups
    peer_mapping: Arc<RwLock<HashMap<KadPeerId, NodeId>>>,
    
    /// Reverse mapping from NodeId to KadPeerId
    node_mapping: Arc<RwLock<HashMap<NodeId, KadPeerId>>>,
    
    /// iroh's n0 DNS discovery service (if enabled)
    #[cfg(feature = "discovery-n0")]
    n0_discovery: Option<Arc<iroh_net::discovery::dns::DnsDiscovery>>,
    
    /// Custom discovery endpoints
    custom_endpoints: Vec<String>,
    
    /// Discovery statistics
    stats: Arc<RwLock<DiscoveryStats>>,
    
    /// Background task handles
    tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    
    /// Shutdown signal
    shutdown: Arc<tokio::sync::Notify>,
}

/// Information about peer addresses from Kademlia
#[derive(Debug, Clone)]
struct PeerAddressInfo {
    addresses: HashSet<SocketAddr>,
    last_updated: Instant,
    source: AddressSource,
    reliability_score: f32,
}

/// Source of peer address information
#[derive(Debug, Clone)]
enum AddressSource {
    /// From Kademlia routing table
    Kademlia,
    /// From iroh discovery
    IrohDiscovery,
    /// From manual configuration
    Manual,
    /// From DHT bootstrap
    Bootstrap,
}

/// Cached discovery information from iroh
#[derive(Debug, Clone)]
struct CachedDiscoveryInfo {
    node_addr: NodeAddr,
    discovered_at: Instant,
    success_count: u32,
    failure_count: u32,
    last_success: Option<Instant>,
    last_failure: Option<Instant>,
}

/// Discovery statistics
#[derive(Debug, Clone, Default)]
pub struct DiscoveryStats {
    pub kad_peers_tracked: usize,
    pub discovery_cache_size: usize,
    pub discovery_queries: u64,
    pub discovery_successes: u64,
    pub discovery_failures: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub n0_queries: u64,
    pub n0_successes: u64,
    pub custom_endpoint_queries: u64,
}

impl DiscoveryBridge {
    /// Create a new discovery bridge
    pub fn new(config: DiscoveryConfig) -> Self {
        let custom_endpoints = config.custom_endpoints.clone();
        #[cfg(feature = "discovery-n0")]
        let use_n0_dns = config.use_n0_dns;
        
        let bridge = Self {
            config,
            kad_peers: Arc::new(RwLock::new(HashMap::new())),
            discovery_cache: Arc::new(RwLock::new(HashMap::new())),
            peer_mapping: Arc::new(RwLock::new(HashMap::new())),
            node_mapping: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "discovery-n0")]
            n0_discovery: if use_n0_dns {
                Some(Arc::new(iroh_net::discovery::dns::DnsDiscovery::n0()))
            } else {
                None
            },
            custom_endpoints,
            stats: Arc::new(RwLock::new(DiscoveryStats::default())),
            tasks: Arc::new(Mutex::new(Vec::new())),
            shutdown: Arc::new(tokio::sync::Notify::new()),
        };
        
        // Start background tasks
        let bridge_clone = bridge.clone();
        tokio::spawn(async move {
            bridge_clone.start_background_tasks().await;
        });
        
        bridge
    }
    
    /// Add peer addresses from Kademlia routing table
    pub async fn add_kad_peer(&self, kad_peer_id: KadPeerId, addresses: Vec<KadAddress>) {
        let socket_addrs: HashSet<SocketAddr> = addresses
            .into_iter()
            .filter_map(|addr| addr.socket_addr())
            .collect();
        
        if socket_addrs.is_empty() {
            return;
        }
        
        let peer_info = PeerAddressInfo {
            addresses: socket_addrs,
            last_updated: Instant::now(),
            source: AddressSource::Kademlia,
            reliability_score: 1.0, // Default reliability
        };
        
        // Convert KadPeerId to NodeId if possible
        if let Ok(node_id) = self.kad_peer_id_to_node_id(&kad_peer_id) {
            self.peer_mapping.write().await.insert(kad_peer_id.clone(), node_id);
            self.node_mapping.write().await.insert(node_id, kad_peer_id.clone());
        }
        
        self.kad_peers.write().await.insert(kad_peer_id.clone(), peer_info);
        
        // Update stats
        let mut stats = self.stats.write().await;
        stats.kad_peers_tracked = self.kad_peers.read().await.len();
        
        debug!("Added Kademlia peer {:?} with {} addresses", kad_peer_id, socket_addrs.len());
    }
    
    /// Remove a peer from tracking
    pub async fn remove_kad_peer(&self, kad_peer_id: &KadPeerId) {
        self.kad_peers.write().await.remove(kad_peer_id);
        
        if let Some(node_id) = self.peer_mapping.write().await.remove(kad_peer_id) {
            self.node_mapping.write().await.remove(&node_id);
            self.discovery_cache.write().await.remove(&node_id);
        }
        
        // Update stats
        let mut stats = self.stats.write().await;
        stats.kad_peers_tracked = self.kad_peers.read().await.len();
        stats.discovery_cache_size = self.discovery_cache.read().await.len();
        
        debug!("Removed Kademlia peer {:?}", kad_peer_id);
    }
    
    /// Update reliability score for a peer based on success/failure
    pub async fn update_peer_reliability(&self, kad_peer_id: &KadPeerId, success: bool) {
        if let Some(peer_info) = self.kad_peers.write().await.get_mut(kad_peer_id) {
            if success {
                peer_info.reliability_score = (peer_info.reliability_score * 0.9 + 0.1).min(1.0);
            } else {
                peer_info.reliability_score = (peer_info.reliability_score * 0.9).max(0.1);
            }
            peer_info.last_updated = Instant::now();
        }
    }
    
    /// Get all known addresses for a peer
    pub async fn get_peer_addresses(&self, kad_peer_id: &KadPeerId) -> Vec<SocketAddr> {
        if let Some(peer_info) = self.kad_peers.read().await.get(kad_peer_id) {
            peer_info.addresses.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }
    
    /// Get discovery statistics
    pub async fn stats(&self) -> DiscoveryStats {
        self.stats.read().await.clone()
    }
    
    /// Start background tasks for discovery maintenance
    async fn start_background_tasks(&self) {
        let mut tasks = self.tasks.lock().await;
        
        // Cache cleanup task
        if self.config.cache_discovered_peers {
            let bridge = self.clone();
            let handle = tokio::spawn(async move {
                bridge.cache_cleanup_task().await;
            });
            tasks.push(handle);
        }
        
        // Periodic discovery task
        if let Some(interval) = self.config.periodic_discovery_interval {
            let bridge = self.clone();
            let handle = tokio::spawn(async move {
                bridge.periodic_discovery_task(interval).await;
            });
            tasks.push(handle);
        }
        
        // Stats update task
        let bridge = self.clone();
        let handle = tokio::spawn(async move {
            bridge.stats_update_task().await;
        });
        tasks.push(handle);
    }
    
    /// Cache cleanup task
    async fn cache_cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.cleanup_expired_cache().await;
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Periodic discovery task
    async fn periodic_discovery_task(&self, interval: Duration) {
        let mut timer = tokio::time::interval(interval);
        
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    self.periodic_discovery().await;
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Stats update task
    async fn stats_update_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.update_stats().await;
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Clean up expired cache entries
    async fn cleanup_expired_cache(&self) {
        let now = Instant::now();
        let ttl = self.config.peer_cache_ttl;
        
        let mut cache = self.discovery_cache.write().await;
        let mut to_remove = Vec::new();
        
        for (node_id, info) in cache.iter() {
            if now.duration_since(info.discovered_at) > ttl {
                to_remove.push(*node_id);
            }
        }
        
        for node_id in to_remove {
            cache.remove(&node_id);
        }
        
        debug!("Cache cleanup removed {} expired entries", cache.len());
    }
    
    /// Perform periodic discovery for known peers
    async fn periodic_discovery(&self) {
        // Discover addresses for peers that need refreshing
        let peers_to_discover: Vec<_> = {
            let kad_peers = self.kad_peers.read().await;
            let now = Instant::now();
            
            kad_peers
                .iter()
                .filter(|(_, info)| {
                    now.duration_since(info.last_updated) > Duration::from_secs(300) // 5 minutes
                })
                .filter_map(|(kad_peer_id, _)| {
                    // Try to get NodeId for this peer
                    if let Ok(node_id) = self.kad_peer_id_to_node_id(kad_peer_id) {
                        Some((kad_peer_id.clone(), node_id))
                    } else {
                        None
                    }
                })
                .take(10) // Limit concurrent discoveries
                .collect()
        };
        
        for (kad_peer_id, node_id) in peers_to_discover {
            let bridge = self.clone();
            tokio::spawn(async move {
                if let Ok(node_addr) = bridge.resolve_via_discovery(node_id).await {
                    // Update cached info
                    let cached_info = CachedDiscoveryInfo {
                        node_addr: node_addr.clone(),
                        discovered_at: Instant::now(),
                        success_count: 1,
                        failure_count: 0,
                        last_success: Some(Instant::now()),
                        last_failure: None,
                    };
                    
                    bridge.discovery_cache.write().await.insert(node_id, cached_info);
                    
                    // Update Kademlia peer addresses
                    let addresses: Vec<KadAddress> = node_addr
                        .direct_addresses()
                        .iter()
                        .map(|addr| KadAddress::new("quic".to_string(), addr.to_string()))
                        .collect();
                    
                    if !addresses.is_empty() {
                        bridge.add_kad_peer(kad_peer_id, addresses).await;
                    }
                }
            });
        }
    }
    
    /// Update discovery statistics
    async fn update_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.kad_peers_tracked = self.kad_peers.read().await.len();
        stats.discovery_cache_size = self.discovery_cache.read().await.len();
    }
    
    /// Resolve peer via discovery services
    async fn resolve_via_discovery(&self, node_id: NodeId) -> IrohResult<NodeAddr> {
        let mut stats = self.stats.write().await;
        stats.discovery_queries += 1;
        drop(stats);
        
        // Try n0 discovery first if enabled
        #[cfg(feature = "discovery-n0")]
        if let Some(ref n0_discovery) = self.n0_discovery {
            let mut stats = self.stats.write().await;
            stats.n0_queries += 1;
            drop(stats);
            
            match n0_discovery.resolve(node_id).await {
                Ok(node_addr) => {
                    let mut stats = self.stats.write().await;
                    stats.discovery_successes += 1;
                    stats.n0_successes += 1;
                    return Ok(node_addr);
                },
                Err(e) => {
                    warn!("n0 discovery failed for {:?}: {}", node_id, e);
                }
            }
        }
        
        // Try custom endpoints
        for endpoint in &self.custom_endpoints {
            let mut stats = self.stats.write().await;
            stats.custom_endpoint_queries += 1;
            drop(stats);
            
            // This is a placeholder - in a real implementation, you'd query custom endpoints
            debug!("Querying custom endpoint {} for {:?}", endpoint, node_id);
        }
        
        let mut stats = self.stats.write().await;
        stats.discovery_failures += 1;
        
        Err(IrohError::Discovery(format!("No discovery method succeeded for {:?}", node_id)))
    }
    
    /// Convert KadPeerId to NodeId
    fn kad_peer_id_to_node_id(&self, kad_peer_id: &KadPeerId) -> IrohResult<NodeId> {
        if kad_peer_id.0.len() != 32 {
            return Err(IrohError::InvalidPeerId(format!(
                "Invalid peer ID length: {} (expected 32)",
                kad_peer_id.0.len()
            )));
        }
        
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&kad_peer_id.0);
        
        Ok(bytes)
    }
    
    /// Convert NodeId to KadPeerId
    fn node_id_to_kad_peer_id(&self, node_id: NodeId) -> KadPeerId {
        KadPeerId::new(node_id.to_vec())
    }
    
    /// Shutdown the discovery bridge
    pub async fn shutdown(&self) -> IrohResult<()> {
        info!("Shutting down discovery bridge");
        
        // Signal shutdown to background tasks
        self.shutdown.notify_waiters();
        
        // Wait for tasks to complete
        let mut tasks = self.tasks.lock().await;
        for task in tasks.drain(..) {
            task.abort();
        }
        
        Ok(())
    }
}

impl Clone for DiscoveryBridge {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            kad_peers: self.kad_peers.clone(),
            discovery_cache: self.discovery_cache.clone(),
            peer_mapping: self.peer_mapping.clone(),
            node_mapping: self.node_mapping.clone(),
            #[cfg(feature = "discovery-n0")]
            n0_discovery: self.n0_discovery.clone(),
            custom_endpoints: self.custom_endpoints.clone(),
            stats: self.stats.clone(),
            tasks: self.tasks.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

#[async_trait]
impl Discovery for DiscoveryBridge {
    type Item = DiscoveryItem;
    
    async fn resolve(&self, node_id: NodeId) -> anyhow::Result<NodeAddr> {
        debug!("Resolving addresses for node: {:?}", node_id);
        
        // First check if we have this peer in our Kademlia peers
        let kad_peer_id = self.node_id_to_kad_peer_id(node_id);
        
        if let Some(peer_info) = self.kad_peers.read().await.get(&kad_peer_id) {
            let mut stats = self.stats.write().await;
            stats.cache_hits += 1;
            
            debug!("Found peer in Kademlia cache: {:?}", kad_peer_id);
            return Ok(NodeAddr::new(node_id).with_direct_addresses(
                peer_info.addresses.iter().cloned().collect::<Vec<_>>()
            ));
        }
        
        // Check discovery cache
        if self.config.cache_discovered_peers {
            if let Some(cached_info) = self.discovery_cache.read().await.get(&node_id) {
                let age = Instant::now().duration_since(cached_info.discovered_at);
                if age < self.config.peer_cache_ttl {
                    let mut stats = self.stats.write().await;
                    stats.cache_hits += 1;
                    
                    debug!("Found peer in discovery cache: {:?}", node_id);
                    return Ok(cached_info.node_addr.clone());
                }
            }
        }
        
        let mut stats = self.stats.write().await;
        stats.cache_misses += 1;
        drop(stats);
        
        // Fall back to discovery services
        match self.resolve_via_discovery(node_id).await {
            Ok(node_addr) => {
                // Cache the result if caching is enabled
                if self.config.cache_discovered_peers {
                    let cached_info = CachedDiscoveryInfo {
                        node_addr: node_addr.clone(),
                        discovered_at: Instant::now(),
                        success_count: 1,
                        failure_count: 0,
                        last_success: Some(Instant::now()),
                        last_failure: None,
                    };
                    
                    // Check cache size limit
                    let mut cache = self.discovery_cache.write().await;
                    if cache.len() >= self.config.max_cached_peers {
                        // Remove oldest entry
                        if let Some((oldest_id, _)) = cache
                            .iter()
                            .min_by_key(|(_, info)| info.discovered_at)
                            .map(|(id, info)| (*id, info.clone()))
                        {
                            cache.remove(&oldest_id);
                        }
                    }
                    
                    cache.insert(node_id, cached_info);
                }
                
                Ok(node_addr)
            },
            Err(e) => {
                // Update failure stats
                if let Some(mut cached_info) = self.discovery_cache.write().await.get_mut(&node_id) {
                    cached_info.failure_count += 1;
                    cached_info.last_failure = Some(Instant::now());
                }
                
                Err(anyhow::anyhow!("Discovery failed: {}", e))
            }
        }
    }
}