// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! High-level integration layer for iroh Kademlia transport
//! 
//! This module provides the `IrohKademlia` struct which combines all components
//! into a unified interface for Kademlia DHT operations over iroh networking.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use tracing::{debug, error, info, warn};
use tokio::sync::{RwLock, oneshot};

use crate::networking::{
    kad::{
        transport::{
            KadPeerId, KadMessage, KadResponse, KadError, PeerInfo, 
            RecordKey, Record, QueryId, QueryResult,
        },
        behaviour::{Kademlia, KademliaHandle, RoutingTableInfo},
        record_store::MemoryRecordStore,
    },
    iroh_adapter::{
        config::IrohConfig,
        transport::IrohTransport,
        discovery::DiscoveryBridge,
        metrics::IrohMetrics,
        protocol::MessageHandler,
        IrohError, IrohResult,
    },
};

/// Integrated Kademlia DHT implementation using iroh transport
pub struct IrohKademlia {
    /// Configuration
    config: IrohConfig,
    
    /// iroh transport implementation
    transport: Arc<IrohTransport>,
    
    /// Kademlia behavior instance
    kademlia: Kademlia<IrohTransport, MemoryRecordStore>,
    
    /// Handle for Kademlia operations
    kademlia_handle: KademliaHandle,
    
    /// Discovery service
    discovery: Arc<DiscoveryBridge>,
    
    /// Metrics collector
    metrics: Arc<IrohMetrics>,
    
    /// Local peer information
    local_peer_info: Arc<RwLock<LocalPeerInfo>>,
    
    /// Bootstrap peers
    bootstrap_peers: Arc<RwLock<Vec<PeerInfo>>>,
    
    /// Running state
    running: Arc<RwLock<bool>>,
    
    /// Background task handles
    tasks: Arc<RwLock<Vec<tokio::task::JoinHandle<()>>>>,
}

/// Local peer information
#[derive(Debug, Clone)]
struct LocalPeerInfo {
    peer_id: KadPeerId,
    listen_addresses: Vec<String>,
    started_at: Instant,
    status: NodeStatus,
}

/// Status of the local node
#[derive(Debug, Clone, PartialEq)]
enum NodeStatus {
    Initializing,
    Bootstrapping,
    Ready,
    Degraded,
    ShuttingDown,
}

/// Integration statistics combining all component stats
#[derive(Debug, Clone)]
pub struct IntegrationStats {
    pub uptime: Duration,
    pub status: String,
    pub local_peer_id: KadPeerId,
    pub listen_addresses: Vec<String>,
    pub bootstrap_peers: usize,
    pub routing_table_size: usize,
    pub active_connections: usize,
    pub total_queries: u64,
    pub successful_queries: u64,
    pub error_rate: f64,
    pub avg_latency_ms: f64,
}

impl IrohKademlia {
    /// Create a new IrohKademlia instance
    pub async fn new(config: IrohConfig) -> IrohResult<Self> {
        info!("Creating IrohKademlia with config");
        
        // Create metrics collector
        let metrics = Arc::new(IrohMetrics::new(config.metrics.clone()));
        
        // Create discovery service
        let discovery = Arc::new(DiscoveryBridge::new(config.discovery.clone()));
        
        // Create message handler that integrates with our Kademlia instance
        let message_handler = {
            let metrics = metrics.clone();
            let discovery = discovery.clone();
            
            move |peer_id: KadPeerId, message: KadMessage| {
                let metrics = metrics.clone();
                let discovery = discovery.clone();
                
                Box::pin(async move {
                    let start_time = Instant::now();
                    
                    // Record incoming message
                    metrics.record_message_received(&peer_id, 0).await; // Size unknown at this level
                    
                    // Process the message (this is simplified - in real implementation,
                    // this would delegate to the actual Kademlia behavior)
                    let response = Self::handle_kademlia_message(&peer_id, message, &discovery).await;
                    
                    // Record latency
                    let latency = start_time.elapsed();
                    metrics.record_latency(&peer_id, latency).await;
                    
                    // Record success/failure
                    match &response {
                        Ok(_) => metrics.record_query_success(latency).await,
                        Err(e) => {
                            let error_type = match e {
                                KadError::Timeout { .. } => "timeout",
                                KadError::Transport(_) => "transport",
                                KadError::Protocol(_) => "protocol",
                                _ => "unknown",
                            };
                            metrics.record_query_failure(error_type).await;
                        }
                    }
                    
                    response
                })
            }
        };
        
        // Create iroh transport
        let transport = Arc::new(IrohTransport::new(config.clone(), message_handler).await?);
        
        // Create Kademlia instance
        let kademlia = Kademlia::with_memory_store(
            transport.clone(),
            config.kademlia.clone(),
        ).map_err(|e| IrohError::Protocol(format!("Failed to create Kademlia: {}", e)))?;
        
        let kademlia_handle = kademlia.handle();
        
        // Set up local peer info
        let local_peer_info = Arc::new(RwLock::new(LocalPeerInfo {
            peer_id: transport.local_peer_id(),
            listen_addresses: transport.listen_addresses()
                .into_iter()
                .map(|addr| addr.to_string())
                .collect(),
            started_at: Instant::now(),
            status: NodeStatus::Initializing,
        }));
        
        let instance = Self {
            config,
            transport,
            kademlia,
            kademlia_handle,
            discovery,
            metrics,
            local_peer_info,
            bootstrap_peers: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(RwLock::new(false)),
            tasks: Arc::new(RwLock::new(Vec::new())),
        };
        
        info!("IrohKademlia created successfully with peer ID: {}", 
              instance.local_peer_id().await);
        
        Ok(instance)
    }
    
    /// Start the IrohKademlia node
    pub async fn start(&self) -> IrohResult<()> {
        info!("Starting IrohKademlia node");
        
        // Update status
        {
            let mut peer_info = self.local_peer_info.write().await;
            peer_info.status = NodeStatus::Bootstrapping;
        }
        
        *self.running.write().await = true;
        
        // Start background tasks
        self.start_background_tasks().await?;
        
        // Perform initial bootstrap if we have bootstrap peers
        let bootstrap_peers = self.bootstrap_peers.read().await.clone();
        if !bootstrap_peers.is_empty() {
            info!("Performing initial bootstrap with {} peers", bootstrap_peers.len());
            self.bootstrap_internal(bootstrap_peers).await?;
        } else {
            warn!("No bootstrap peers configured - node may have limited connectivity");
        }
        
        // Update status to ready
        {
            let mut peer_info = self.local_peer_info.write().await;
            peer_info.status = NodeStatus::Ready;
        }
        
        info!("IrohKademlia node started successfully");
        Ok(())
    }
    
    /// Stop the IrohKademlia node
    pub async fn stop(&self) -> IrohResult<()> {
        info!("Stopping IrohKademlia node");
        
        // Update status
        {
            let mut peer_info = self.local_peer_info.write().await;
            peer_info.status = NodeStatus::ShuttingDown;
        }
        
        *self.running.write().await = false;
        
        // Stop background tasks
        let mut tasks = self.tasks.write().await;
        for task in tasks.drain(..) {
            task.abort();
        }
        
        // Shutdown components
        self.discovery.shutdown().await?;
        self.metrics.shutdown().await
            .map_err(|e| IrohError::Protocol(format!("Metrics shutdown failed: {}", e)))?;
        
        info!("IrohKademlia node stopped");
        Ok(())
    }
    
    /// Add bootstrap peers
    pub async fn add_bootstrap_peers(&self, peers: Vec<PeerInfo>) {
        info!("Adding {} bootstrap peers", peers.len());
        
        // Add to discovery
        for peer in &peers {
            self.discovery.add_kad_peer(peer.peer_id.clone(), peer.addresses.clone()).await;
        }
        
        // Store for future use
        self.bootstrap_peers.write().await.extend(peers);
    }
    
    /// Perform bootstrap operation
    pub async fn bootstrap(&self) -> IrohResult<()> {
        let bootstrap_peers = self.bootstrap_peers.read().await.clone();
        if bootstrap_peers.is_empty() {
            return Err(IrohError::Protocol("No bootstrap peers available".to_string()));
        }
        
        self.bootstrap_internal(bootstrap_peers).await
    }
    
    /// Internal bootstrap implementation
    async fn bootstrap_internal(&self, peers: Vec<PeerInfo>) -> IrohResult<()> {
        debug!("Bootstrapping with {} peers", peers.len());
        
        // Add peers to Kademlia
        for peer in peers {
            if let Err(e) = self.kademlia_handle.add_peer(peer.clone()).await {
                warn!("Failed to add bootstrap peer {:?}: {}", peer.peer_id, e);
            }
        }
        
        // Perform bootstrap query
        let (tx, rx) = oneshot::channel();
        match self.kademlia_handle.bootstrap(tx).await {
            Ok(_) => {
                match rx.await {
                    Ok(Ok(_)) => {
                        info!("Bootstrap completed successfully");
                        Ok(())
                    },
                    Ok(Err(e)) => {
                        warn!("Bootstrap failed: {}", e);
                        Err(IrohError::Protocol(format!("Bootstrap failed: {}", e)))
                    },
                    Err(_) => {
                        Err(IrohError::Protocol("Bootstrap response channel closed".to_string()))
                    }
                }
            },
            Err(e) => {
                error!("Failed to initiate bootstrap: {}", e);
                Err(IrohError::Protocol(format!("Bootstrap initiation failed: {}", e)))
            }
        }
    }
    
    /// Find the closest peers to a target
    pub async fn find_node(&self, target: KadPeerId) -> IrohResult<Vec<PeerInfo>> {
        debug!("Finding node: {:?}", target);
        
        self.metrics.record_query_started("find_node").await;
        let start_time = Instant::now();
        
        match self.kademlia_handle.find_node(target.clone()).await {
            Ok(peers) => {
                let duration = start_time.elapsed();
                self.metrics.record_query_success(duration).await;
                
                info!("Found {} peers for target {:?}", peers.len(), target);
                Ok(peers)
            },
            Err(e) => {
                self.metrics.record_query_failure("find_node").await;
                self.metrics.record_error("query").await;
                
                warn!("Find node failed for {:?}: {}", target, e);
                Err(IrohError::Protocol(format!("Find node failed: {}", e)))
            }
        }
    }
    
    /// Store a record in the DHT
    pub async fn put_record(&self, key: RecordKey, value: Vec<u8>) -> IrohResult<()> {
        debug!("Storing record with key: {:?}", key);
        
        self.metrics.record_query_started("put_record").await;
        let start_time = Instant::now();
        
        let record = Record::new(key.clone(), value);
        
        match self.kademlia_handle.put_record(record).await {
            Ok(_) => {
                let duration = start_time.elapsed();
                self.metrics.record_query_success(duration).await;
                
                info!("Successfully stored record with key: {:?}", key);
                Ok(())
            },
            Err(e) => {
                self.metrics.record_query_failure("put_record").await;
                self.metrics.record_error("query").await;
                
                warn!("Put record failed for key {:?}: {}", key, e);
                Err(IrohError::Protocol(format!("Put record failed: {}", e)))
            }
        }
    }
    
    /// Retrieve a record from the DHT
    pub async fn get_record(&self, key: RecordKey) -> IrohResult<Option<Vec<u8>>> {
        debug!("Retrieving record with key: {:?}", key);
        
        self.metrics.record_query_started("get_record").await;
        let start_time = Instant::now();
        
        match self.kademlia_handle.find_value(key.clone()).await {
            Ok(record_opt) => {
                let duration = start_time.elapsed();
                self.metrics.record_query_success(duration).await;
                
                match record_opt {
                    Some(record) => {
                        info!("Successfully retrieved record with key: {:?}", key);
                        Ok(Some(record.value))
                    },
                    None => {
                        debug!("Record not found for key: {:?}", key);
                        Ok(None)
                    }
                }
            },
            Err(e) => {
                self.metrics.record_query_failure("get_record").await;
                self.metrics.record_error("query").await;
                
                warn!("Get record failed for key {:?}: {}", key, e);
                Err(IrohError::Protocol(format!("Get record failed: {}", e)))
            }
        }
    }
    
    /// Get local peer ID
    pub async fn local_peer_id(&self) -> KadPeerId {
        self.local_peer_info.read().await.peer_id.clone()
    }
    
    /// Get listen addresses
    pub async fn listen_addresses(&self) -> Vec<String> {
        self.local_peer_info.read().await.listen_addresses.clone()
    }
    
    /// Get routing table information
    pub async fn routing_table_info(&self) -> IrohResult<RoutingTableInfo> {
        let (tx, rx) = oneshot::channel();
        
        // This is simplified - in a real implementation, we'd need to expose this from KademliaHandle
        // For now, we'll return a placeholder
        let info = RoutingTableInfo {
            bucket_sizes: vec![],
            total_peers: 0,
            local_peer_id: self.local_peer_id().await,
            closest_peers: vec![],
        };
        
        Ok(info)
    }
    
    /// Get integration statistics
    pub async fn stats(&self) -> IrohResult<IntegrationStats> {
        let peer_info = self.local_peer_info.read().await;
        let metrics = self.metrics.get_metrics().await;
        let transport_stats = self.transport.stats().await;
        let discovery_stats = self.discovery.stats().await;
        
        Ok(IntegrationStats {
            uptime: peer_info.started_at.elapsed(),
            status: format!("{:?}", peer_info.status),
            local_peer_id: peer_info.peer_id.clone(),
            listen_addresses: peer_info.listen_addresses.clone(),
            bootstrap_peers: self.bootstrap_peers.read().await.len(),
            routing_table_size: discovery_stats.kad_peers_tracked,
            active_connections: transport_stats.active_connections,
            total_queries: metrics.queries.total_queries,
            successful_queries: metrics.queries.successful_queries,
            error_rate: metrics.errors.error_rate,
            avg_latency_ms: metrics.latency.percentiles.get("p50").unwrap_or(&0.0) * 1000.0,
        })
    }
    
    /// Check if the node is ready to serve requests
    pub async fn is_ready(&self) -> bool {
        let peer_info = self.local_peer_info.read().await;
        matches!(peer_info.status, NodeStatus::Ready)
    }
    
    /// Start background tasks
    async fn start_background_tasks(&self) -> IrohResult<()> {
        let mut tasks = self.tasks.write().await;
        
        // Status monitoring task
        let instance = self.clone();
        let handle = tokio::spawn(async move {
            instance.status_monitoring_task().await;
        });
        tasks.push(handle);
        
        // Health check task
        let instance = self.clone();
        let handle = tokio::spawn(async move {
            instance.health_check_task().await;
        });
        tasks.push(handle);
        
        // Discovery sync task
        let instance = self.clone();
        let handle = tokio::spawn(async move {
            instance.discovery_sync_task().await;
        });
        tasks.push(handle);
        
        Ok(())
    }
    
    /// Status monitoring background task
    async fn status_monitoring_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        
        while *self.running.read().await {
            tokio::select! {
                _ = interval.tick() => {
                    self.update_node_status().await;
                },
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if !*self.running.read().await {
                        break;
                    }
                }
            }
        }
    }
    
    /// Health check background task
    async fn health_check_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        
        while *self.running.read().await {
            tokio::select! {
                _ = interval.tick() => {
                    self.perform_health_check().await;
                },
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if !*self.running.read().await {
                        break;
                    }
                }
            }
        }
    }
    
    /// Discovery sync background task
    async fn discovery_sync_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(120));
        
        while *self.running.read().await {
            tokio::select! {
                _ = interval.tick() => {
                    self.sync_discovery_state().await;
                },
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if !*self.running.read().await {
                        break;
                    }
                }
            }
        }
    }
    
    /// Update node status based on health metrics
    async fn update_node_status(&self) {
        let stats = match self.stats().await {
            Ok(stats) => stats,
            Err(_) => return,
        };
        
        let mut peer_info = self.local_peer_info.write().await;
        
        // Determine status based on metrics
        let new_status = if stats.active_connections == 0 && stats.uptime > Duration::from_secs(60) {
            NodeStatus::Degraded
        } else if stats.error_rate > 10.0 { // More than 10 errors per minute
            NodeStatus::Degraded
        } else if stats.routing_table_size < 5 && stats.uptime > Duration::from_secs(300) {
            NodeStatus::Degraded
        } else {
            NodeStatus::Ready
        };
        
        if peer_info.status != new_status {
            info!("Node status changed from {:?} to {:?}", peer_info.status, new_status);
            peer_info.status = new_status;
        }
    }
    
    /// Perform periodic health checks
    async fn perform_health_check(&self) {
        debug!("Performing health check");
        
        // Check if we can perform basic DHT operations
        let local_peer_id = self.local_peer_id().await;
        
        // Try to find ourselves (should succeed if DHT is healthy)
        if let Err(e) = self.find_node(local_peer_id.clone()).await {
            warn!("Health check failed - cannot find self: {}", e);
            self.metrics.record_error("health_check").await;
        }
    }
    
    /// Sync discovery state with routing table
    async fn sync_discovery_state(&self) {
        debug!("Syncing discovery state");
        
        // This would sync discovered peers with the Kademlia routing table
        // For now, this is a placeholder
    }
    
    /// Handle incoming Kademlia messages (simplified implementation)
    async fn handle_kademlia_message(
        peer_id: &KadPeerId,
        message: KadMessage,
        _discovery: &DiscoveryBridge,
    ) -> Result<KadResponse, KadError> {
        debug!("Handling message from {:?}: {:?}", peer_id, message);
        
        // This is a simplified implementation - in a real system, this would
        // delegate to the actual Kademlia behavior
        match message {
            KadMessage::FindNode { target, .. } => {
                Ok(KadResponse::Nodes {
                    closer_peers: vec![],
                    requester: target,
                })
            },
            KadMessage::FindValue { key, .. } => {
                Ok(KadResponse::Value {
                    record: None,
                    closer_peers: vec![],
                    requester: key.to_kad_peer_id(),
                })
            },
            KadMessage::PutValue { requester, .. } => {
                Ok(KadResponse::Ack { requester })
            },
            KadMessage::GetProviders { key, .. } => {
                Ok(KadResponse::Providers {
                    key,
                    providers: vec![],
                    closer_peers: vec![],
                    requester: peer_id.clone(),
                })
            },
            KadMessage::AddProvider { requester, .. } => {
                Ok(KadResponse::Ack { requester })
            },
            KadMessage::Ping { requester } => {
                Ok(KadResponse::Ack { requester })
            },
        }
    }
}

impl Clone for IrohKademlia {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            transport: self.transport.clone(),
            kademlia: self.kademlia.clone(), // Note: This assumes Kademlia implements Clone
            kademlia_handle: self.kademlia_handle.clone(),
            discovery: self.discovery.clone(),
            metrics: self.metrics.clone(),
            local_peer_info: self.local_peer_info.clone(),
            bootstrap_peers: self.bootstrap_peers.clone(),
            running: self.running.clone(),
            tasks: self.tasks.clone(),
        }
    }
}