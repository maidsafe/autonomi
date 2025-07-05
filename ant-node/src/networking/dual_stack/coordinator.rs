// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Dual-stack transport coordinator
//! 
//! This module implements the core `DualStackTransport` that coordinates between
//! libp2p and iroh transports, providing intelligent routing, failover, and
//! gradual migration capabilities.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::sync::{RwLock, Mutex};
use tracing::{debug, error, info, warn, instrument};

use crate::networking::{
    kad::transport::{
        KademliaTransport, KadPeerId, KadAddress, KadMessage, KadResponse, KadError,
        PeerInfo, ConnectionStatus, QueryId, RecordKey, Record, QueryResult,
    },
    libp2p_compat::LibP2PTransport,
};

#[cfg(feature = "iroh-transport")]
use crate::networking::iroh_adapter::IrohTransport;

use super::{
    TransportId, DualStackError, DualStackResult, DualStackConfig,
    router::{TransportRouter, TransportChoice},
    migration::MigrationManager,
    metrics::UnifiedMetrics,
    failover::FailoverController,
    affinity::PeerAffinityTracker,
};

/// Primary dual-stack transport coordinator implementing KademliaTransport
/// 
/// This is the main entry point for dual-stack functionality, coordinating
/// between libp2p and iroh transports based on configuration policies,
/// performance metrics, and migration strategies.
pub struct DualStackTransport {
    /// Configuration for dual-stack behavior
    config: DualStackConfig,
    
    /// libp2p transport (always available)
    libp2p_transport: Arc<LibP2PTransport>,
    
    /// iroh transport (optional, feature-gated)
    #[cfg(feature = "iroh-transport")]
    iroh_transport: Option<Arc<IrohTransport>>,
    
    /// Transport routing engine
    router: Arc<TransportRouter>,
    
    /// Migration orchestration manager
    migration_manager: Arc<MigrationManager>,
    
    /// Unified metrics aggregation
    metrics: Arc<UnifiedMetrics>,
    
    /// Failover and redundancy controller
    failover_controller: Arc<FailoverController>,
    
    /// Per-peer transport affinity tracker
    affinity_tracker: Arc<PeerAffinityTracker>,
    
    /// Local peer information
    local_peer_id: KadPeerId,
    
    /// Current transport availability status
    transport_status: Arc<RwLock<TransportStatus>>,
    
    /// Operation tracking for performance analysis
    operation_tracker: Arc<Mutex<OperationTracker>>,
    
    /// Background task handles
    background_tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    
    /// Shutdown signal
    shutdown_signal: Arc<tokio::sync::Notify>,
}

/// Transport availability status
#[derive(Debug, Clone)]
struct TransportStatus {
    libp2p_available: bool,
    libp2p_healthy: bool,
    iroh_available: bool,
    iroh_healthy: bool,
    last_health_check: Instant,
}

/// Operation tracking for performance analysis
#[derive(Debug)]
struct OperationTracker {
    pending_operations: HashMap<QueryId, PendingOperation>,
    completed_operations: Vec<CompletedOperation>,
    operation_counter: u64,
}

/// Information about pending operations
#[derive(Debug)]
struct PendingOperation {
    query_id: QueryId,
    transport: TransportId,
    peer_id: KadPeerId,
    operation_type: OperationType,
    started_at: Instant,
    timeout: Duration,
}

/// Information about completed operations
#[derive(Debug, Clone)]
struct CompletedOperation {
    query_id: QueryId,
    transport: TransportId,
    peer_id: KadPeerId,
    operation_type: OperationType,
    started_at: Instant,
    completed_at: Instant,
    success: bool,
    error: Option<String>,
    latency: Duration,
}

/// Types of Kademlia operations
#[derive(Debug, Clone, Copy)]
enum OperationType {
    FindNode,
    FindValue,
    PutRecord,
    Bootstrap,
    Ping,
}

impl DualStackTransport {
    /// Create a new dual-stack transport coordinator
    pub async fn new(
        config: DualStackConfig,
        libp2p_transport: Arc<LibP2PTransport>,
        #[cfg(feature = "iroh-transport")] iroh_transport: Option<Arc<IrohTransport>>,
        local_peer_id: KadPeerId,
    ) -> DualStackResult<Self> {
        // Validate configuration
        config.validate()
            .map_err(|e| DualStackError::Configuration(e))?;
        
        info!("Initializing dual-stack transport coordinator");
        
        // Initialize core components
        let router = Arc::new(TransportRouter::new(config.routing.clone()).await?);
        let migration_manager = Arc::new(MigrationManager::new(config.migration.clone()).await?);
        let metrics = Arc::new(UnifiedMetrics::new(config.metrics.clone()).await?);
        let failover_controller = Arc::new(FailoverController::new(config.failover.clone()).await?);
        let affinity_tracker = Arc::new(PeerAffinityTracker::new(
            config.performance.history_size,
            config.performance.evaluation_interval,
        ).await?);
        
        // Initialize transport status
        let transport_status = Arc::new(RwLock::new(TransportStatus {
            libp2p_available: true,
            libp2p_healthy: true,
            #[cfg(feature = "iroh-transport")]
            iroh_available: iroh_transport.is_some(),
            #[cfg(not(feature = "iroh-transport"))]
            iroh_available: false,
            iroh_healthy: false,
            last_health_check: Instant::now(),
        }));
        
        let operation_tracker = Arc::new(Mutex::new(OperationTracker {
            pending_operations: HashMap::new(),
            completed_operations: Vec::new(),
            operation_counter: 0,
        }));
        
        let shutdown_signal = Arc::new(tokio::sync::Notify::new());
        
        let coordinator = Self {
            config,
            libp2p_transport,
            #[cfg(feature = "iroh-transport")]
            iroh_transport,
            router,
            migration_manager,
            metrics,
            failover_controller,
            affinity_tracker,
            local_peer_id,
            transport_status,
            operation_tracker,
            background_tasks: Arc::new(Mutex::new(Vec::new())),
            shutdown_signal,
        };
        
        // Start background tasks
        coordinator.start_background_tasks().await?;
        
        info!("Dual-stack transport coordinator initialized successfully");
        Ok(coordinator)
    }
    
    /// Start background maintenance tasks
    async fn start_background_tasks(&self) -> DualStackResult<()> {
        let mut tasks = self.background_tasks.lock().await;
        
        // Health monitoring task
        {
            let coordinator = self.clone();
            let handle = tokio::spawn(async move {
                coordinator.health_monitoring_task().await;
            });
            tasks.push(handle);
        }
        
        // Migration management task
        if self.config.migration.enable_migration {
            let coordinator = self.clone();
            let handle = tokio::spawn(async move {
                coordinator.migration_management_task().await;
            });
            tasks.push(handle);
        }
        
        // Metrics aggregation task
        {
            let coordinator = self.clone();
            let handle = tokio::spawn(async move {
                coordinator.metrics_aggregation_task().await;
            });
            tasks.push(handle);
        }
        
        // Operation cleanup task
        {
            let coordinator = self.clone();
            let handle = tokio::spawn(async move {
                coordinator.operation_cleanup_task().await;
            });
            tasks.push(handle);
        }
        
        debug!("Started {} background tasks", tasks.len());
        Ok(())
    }
    
    /// Select the optimal transport for a given operation
    #[instrument(skip(self), fields(peer_id = %peer_id))]
    async fn select_transport(
        &self,
        peer_id: &KadPeerId,
        operation_type: OperationType,
    ) -> DualStackResult<TransportId> {
        // Check transport availability
        let status = self.transport_status.read().await;
        
        let available_transports = {
            let mut transports = Vec::new();
            if status.libp2p_available && status.libp2p_healthy {
                transports.push(TransportId::LibP2P);
            }
            if status.iroh_available && status.iroh_healthy {
                transports.push(TransportId::Iroh);
            }
            transports
        };
        
        if available_transports.is_empty() {
            return Err(DualStackError::AllTransportsFailed {
                libp2p_error: if !status.libp2p_healthy { "unhealthy" } else { "unavailable" }.to_string(),
                iroh_error: if !status.iroh_healthy { "unhealthy" } else { "unavailable" }.to_string(),
            });
        }
        
        drop(status);
        
        // Use router to make intelligent selection
        let choice = self.router.select_transport(
            peer_id,
            &available_transports,
            operation_type.into(),
        ).await?;
        
        // Apply migration policies
        let final_choice = self.migration_manager.apply_migration_policy(
            peer_id,
            choice,
        ).await?;
        
        // Learn from affinity if enabled
        if self.config.advanced.feature_flags.peer_affinity {
            self.affinity_tracker.record_selection(peer_id, final_choice).await;
        }
        
        debug!("Selected transport {:?} for peer {} operation {:?}", 
               final_choice, peer_id, operation_type);
        
        Ok(final_choice)
    }
    
    /// Execute an operation with the selected transport
    #[instrument(skip(self, operation), fields(transport = ?transport, peer_id = %peer_id))]
    async fn execute_with_transport<F, T>(
        &self,
        transport: TransportId,
        peer_id: &KadPeerId,
        operation_type: OperationType,
        operation: F,
    ) -> DualStackResult<T>
    where
        F: FnOnce() -> tokio::task::JoinHandle<Result<T, KadError>> + Send + 'static,
        T: Send + 'static,
    {
        let start_time = Instant::now();
        
        // Create operation tracking
        let query_id = self.generate_query_id().await;
        let pending_op = PendingOperation {
            query_id,
            transport,
            peer_id: peer_id.clone(),
            operation_type,
            started_at: start_time,
            timeout: self.get_transport_timeout(transport),
        };
        
        // Track pending operation
        self.operation_tracker.lock().await
            .pending_operations.insert(query_id, pending_op);
        
        // Execute operation with timeout and failover
        let result = self.execute_with_failover(
            transport,
            peer_id,
            operation_type,
            operation,
        ).await;
        
        // Record completion
        let completed_at = Instant::now();
        let latency = completed_at.duration_since(start_time);
        
        let completed_op = CompletedOperation {
            query_id,
            transport,
            peer_id: peer_id.clone(),
            operation_type,
            started_at: start_time,
            completed_at,
            success: result.is_ok(),
            error: result.as_ref().err().map(|e| e.to_string()),
            latency,
        };
        
        // Update tracking
        {
            let mut tracker = self.operation_tracker.lock().await;
            tracker.pending_operations.remove(&query_id);
            tracker.completed_operations.push(completed_op.clone());
            
            // Limit history size
            if tracker.completed_operations.len() > self.config.performance.history_size {
                tracker.completed_operations.remove(0);
            }
        }
        
        // Update metrics
        self.metrics.record_operation(
            transport,
            peer_id,
            operation_type.into(),
            latency,
            result.is_ok(),
        ).await;
        
        // Update affinity tracker
        if self.config.advanced.feature_flags.peer_affinity {
            self.affinity_tracker.record_result(
                peer_id,
                transport,
                latency,
                result.is_ok(),
            ).await;
        }
        
        result
    }
    
    /// Execute operation with automatic failover
    async fn execute_with_failover<F, T>(
        &self,
        primary_transport: TransportId,
        peer_id: &KadPeerId,
        operation_type: OperationType,
        operation: F,
    ) -> DualStackResult<T>
    where
        F: FnOnce() -> tokio::task::JoinHandle<Result<T, KadError>> + Send + 'static,
        T: Send + 'static,
    {
        // First attempt with primary transport
        let timeout = self.get_transport_timeout(primary_transport);
        let handle = operation();
        
        match tokio::time::timeout(timeout, handle).await {
            Ok(Ok(result)) => {
                // Success - update health status
                self.failover_controller.record_success(primary_transport).await;
                return Ok(result);
            },
            Ok(Err(kad_error)) => {
                // Transport error - record failure and potentially failover
                self.failover_controller.record_failure(primary_transport, &kad_error).await;
                
                if self.config.failover.enabled {
                    if let Some(fallback_transport) = self.get_fallback_transport(primary_transport).await {
                        warn!("Primary transport {:?} failed, attempting failover to {:?}", 
                              primary_transport, fallback_transport);
                        
                        // Retry with fallback transport
                        // Note: This is a simplified failover - in practice would need to reconstruct operation
                        return Err(DualStackError::TransportUnavailable {
                            transport: primary_transport,
                            reason: kad_error.to_string(),
                        });
                    }
                }
                
                return Err(DualStackError::TransportUnavailable {
                    transport: primary_transport,
                    reason: kad_error.to_string(),
                });
            },
            Err(_) => {
                // Timeout
                self.failover_controller.record_timeout(primary_transport).await;
                
                return Err(DualStackError::TransportUnavailable {
                    transport: primary_transport,
                    reason: "Operation timeout".to_string(),
                });
            }
        }
    }
    
    /// Get fallback transport for failover
    async fn get_fallback_transport(&self, failed_transport: TransportId) -> Option<TransportId> {
        let status = self.transport_status.read().await;
        
        match failed_transport {
            TransportId::LibP2P => {
                if status.iroh_available && status.iroh_healthy {
                    Some(TransportId::Iroh)
                } else {
                    None
                }
            },
            TransportId::Iroh => {
                if status.libp2p_available && status.libp2p_healthy {
                    Some(TransportId::LibP2P)
                } else {
                    None
                }
            },
        }
    }
    
    /// Get transport-specific timeout
    fn get_transport_timeout(&self, transport: TransportId) -> Duration {
        match transport {
            TransportId::LibP2P => self.config.routing.transport_overrides.libp2p.request_timeout,
            TransportId::Iroh => self.config.routing.transport_overrides.iroh.request_timeout,
        }
    }
    
    /// Generate unique query ID
    async fn generate_query_id(&self) -> QueryId {
        let mut tracker = self.operation_tracker.lock().await;
        tracker.operation_counter += 1;
        QueryId::new(tracker.operation_counter)
    }
    
    /// Health monitoring background task
    async fn health_monitoring_task(&self) {
        let mut interval = tokio::time::interval(self.config.failover.health_check.interval);
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.perform_health_checks().await;
                },
                _ = self.shutdown_signal.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Perform health checks on all transports
    async fn perform_health_checks(&self) {
        debug!("Performing transport health checks");
        
        let mut status = self.transport_status.write().await;
        
        // Check libp2p health
        status.libp2p_healthy = self.check_transport_health(TransportId::LibP2P).await;
        
        // Check iroh health
        if status.iroh_available {
            status.iroh_healthy = self.check_transport_health(TransportId::Iroh).await;
        }
        
        status.last_health_check = Instant::now();
        
        debug!("Health check completed: libp2p={}, iroh={}", 
               status.libp2p_healthy, status.iroh_healthy);
    }
    
    /// Check health of a specific transport
    async fn check_transport_health(&self, transport: TransportId) -> bool {
        // Simple health check - could be enhanced with actual ping operations
        match transport {
            TransportId::LibP2P => {
                // libp2p is always considered healthy if available
                true
            },
            TransportId::Iroh => {
                #[cfg(feature = "iroh-transport")]
                {
                    // Check if iroh transport is responsive
                    self.iroh_transport.is_some()
                }
                #[cfg(not(feature = "iroh-transport"))]
                {
                    false
                }
            },
        }
    }
    
    /// Migration management background task
    async fn migration_management_task(&self) {
        let mut interval = tokio::time::interval(self.config.migration.rollout_interval);
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.migration_manager.update_migration_progress().await {
                        error!("Migration update failed: {}", e);
                    }
                },
                _ = self.shutdown_signal.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Metrics aggregation background task
    async fn metrics_aggregation_task(&self) {
        let mut interval = tokio::time::interval(self.config.metrics.export_interval);
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.metrics.aggregate_and_export().await {
                        error!("Metrics aggregation failed: {}", e);
                    }
                },
                _ = self.shutdown_signal.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Operation cleanup background task
    async fn operation_cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_minutes(5));
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.cleanup_stale_operations().await;
                },
                _ = self.shutdown_signal.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Clean up stale operations
    async fn cleanup_stale_operations(&self) {
        let now = Instant::now();
        let mut tracker = self.operation_tracker.lock().await;
        
        // Remove timed out pending operations
        let timed_out: Vec<_> = tracker.pending_operations
            .iter()
            .filter(|(_, op)| now.duration_since(op.started_at) > op.timeout)
            .map(|(id, _)| *id)
            .collect();
        
        for id in timed_out {
            if let Some(op) = tracker.pending_operations.remove(&id) {
                warn!("Removing timed out operation: {:?}", op);
            }
        }
        
        debug!("Cleaned up {} stale operations", tracker.pending_operations.len());
    }
    
    /// Shutdown the dual-stack coordinator
    pub async fn shutdown(&self) -> DualStackResult<()> {
        info!("Shutting down dual-stack transport coordinator");
        
        // Signal shutdown to background tasks
        self.shutdown_signal.notify_waiters();
        
        // Wait for tasks to complete
        let mut tasks = self.background_tasks.lock().await;
        for task in tasks.drain(..) {
            task.abort();
        }
        
        // Shutdown components
        if let Err(e) = self.metrics.shutdown().await {
            error!("Failed to shutdown metrics: {}", e);
        }
        
        if let Err(e) = self.migration_manager.shutdown().await {
            error!("Failed to shutdown migration manager: {}", e);
        }
        
        info!("Dual-stack transport coordinator shutdown complete");
        Ok(())
    }
    
    /// Get current transport status
    pub async fn get_transport_status(&self) -> TransportStatus {
        self.transport_status.read().await.clone()
    }
    
    /// Get operation statistics
    pub async fn get_operation_stats(&self) -> OperationStats {
        let tracker = self.operation_tracker.lock().await;
        
        OperationStats {
            pending_operations: tracker.pending_operations.len(),
            completed_operations: tracker.completed_operations.len(),
            total_operations: tracker.operation_counter,
        }
    }
}

/// Operation statistics
#[derive(Debug, Clone)]
pub struct OperationStats {
    pub pending_operations: usize,
    pub completed_operations: usize,
    pub total_operations: u64,
}

impl Clone for DualStackTransport {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            libp2p_transport: self.libp2p_transport.clone(),
            #[cfg(feature = "iroh-transport")]
            iroh_transport: self.iroh_transport.clone(),
            router: self.router.clone(),
            migration_manager: self.migration_manager.clone(),
            metrics: self.metrics.clone(),
            failover_controller: self.failover_controller.clone(),
            affinity_tracker: self.affinity_tracker.clone(),
            local_peer_id: self.local_peer_id.clone(),
            transport_status: self.transport_status.clone(),
            operation_tracker: self.operation_tracker.clone(),
            background_tasks: self.background_tasks.clone(),
            shutdown_signal: self.shutdown_signal.clone(),
        }
    }
}

impl From<OperationType> for &'static str {
    fn from(op_type: OperationType) -> Self {
        match op_type {
            OperationType::FindNode => "find_node",
            OperationType::FindValue => "find_value",
            OperationType::PutRecord => "put_record",
            OperationType::Bootstrap => "bootstrap",
            OperationType::Ping => "ping",
        }
    }
}

#[async_trait]
impl KademliaTransport for DualStackTransport {
    async fn find_node(&self, target: KadPeerId) -> Result<Vec<PeerInfo>, KadError> {
        let transport = self.select_transport(&target, OperationType::FindNode).await?;
        
        match transport {
            TransportId::LibP2P => {
                self.execute_with_transport(
                    transport,
                    &target,
                    OperationType::FindNode,
                    || {
                        let libp2p = self.libp2p_transport.clone();
                        let target_clone = target.clone();
                        tokio::spawn(async move {
                            libp2p.find_node(target_clone).await
                        })
                    },
                ).await
            },
            TransportId::Iroh => {
                #[cfg(feature = "iroh-transport")]
                {
                    if let Some(ref iroh) = self.iroh_transport {
                        self.execute_with_transport(
                            transport,
                            &target,
                            OperationType::FindNode,
                            || {
                                let iroh = iroh.clone();
                                let target_clone = target.clone();
                                tokio::spawn(async move {
                                    iroh.find_node(target_clone).await
                                })
                            },
                        ).await
                    } else {
                        Err(DualStackError::TransportUnavailable {
                            transport: TransportId::Iroh,
                            reason: "iroh transport not configured".to_string(),
                        }.into())
                    }
                }
                #[cfg(not(feature = "iroh-transport"))]
                {
                    Err(DualStackError::TransportUnavailable {
                        transport: TransportId::Iroh,
                        reason: "iroh transport not compiled".to_string(),
                    }.into())
                }
            },
        }
    }
    
    async fn find_value(&self, key: RecordKey) -> Result<QueryResult, KadError> {
        // Create a synthetic target peer ID from the key for routing
        let target_peer = KadPeerId::new(key.0.clone());
        let transport = self.select_transport(&target_peer, OperationType::FindValue).await?;
        
        match transport {
            TransportId::LibP2P => {
                self.execute_with_transport(
                    transport,
                    &target_peer,
                    OperationType::FindValue,
                    || {
                        let libp2p = self.libp2p_transport.clone();
                        let key_clone = key.clone();
                        tokio::spawn(async move {
                            libp2p.find_value(key_clone).await
                        })
                    },
                ).await
            },
            TransportId::Iroh => {
                #[cfg(feature = "iroh-transport")]
                {
                    if let Some(ref iroh) = self.iroh_transport {
                        self.execute_with_transport(
                            transport,
                            &target_peer,
                            OperationType::FindValue,
                            || {
                                let iroh = iroh.clone();
                                let key_clone = key.clone();
                                tokio::spawn(async move {
                                    iroh.find_value(key_clone).await
                                })
                            },
                        ).await
                    } else {
                        Err(DualStackError::TransportUnavailable {
                            transport: TransportId::Iroh,
                            reason: "iroh transport not configured".to_string(),
                        }.into())
                    }
                }
                #[cfg(not(feature = "iroh-transport"))]
                {
                    Err(DualStackError::TransportUnavailable {
                        transport: TransportId::Iroh,
                        reason: "iroh transport not compiled".to_string(),
                    }.into())
                }
            },
        }
    }
    
    async fn put_record(&self, record: Record) -> Result<(), KadError> {
        // Use record key to determine target peer for routing
        let target_peer = KadPeerId::new(record.key.0.clone());
        let transport = self.select_transport(&target_peer, OperationType::PutRecord).await?;
        
        match transport {
            TransportId::LibP2P => {
                self.execute_with_transport(
                    transport,
                    &target_peer,
                    OperationType::PutRecord,
                    || {
                        let libp2p = self.libp2p_transport.clone();
                        let record_clone = record.clone();
                        tokio::spawn(async move {
                            libp2p.put_record(record_clone).await
                        })
                    },
                ).await
            },
            TransportId::Iroh => {
                #[cfg(feature = "iroh-transport")]
                {
                    if let Some(ref iroh) = self.iroh_transport {
                        self.execute_with_transport(
                            transport,
                            &target_peer,
                            OperationType::PutRecord,
                            || {
                                let iroh = iroh.clone();
                                let record_clone = record.clone();
                                tokio::spawn(async move {
                                    iroh.put_record(record_clone).await
                                })
                            },
                        ).await
                    } else {
                        Err(DualStackError::TransportUnavailable {
                            transport: TransportId::Iroh,
                            reason: "iroh transport not configured".to_string(),
                        }.into())
                    }
                }
                #[cfg(not(feature = "iroh-transport"))]
                {
                    Err(DualStackError::TransportUnavailable {
                        transport: TransportId::Iroh,
                        reason: "iroh transport not compiled".to_string(),
                    }.into())
                }
            },
        }
    }
    
    async fn bootstrap(&self, bootstrap_peers: Vec<PeerInfo>) -> Result<(), KadError> {
        info!("Starting dual-stack bootstrap with {} peers", bootstrap_peers.len());
        
        // For bootstrap, we use both transports if available
        let mut results = Vec::new();
        
        // Bootstrap libp2p
        let libp2p_result = self.libp2p_transport.bootstrap(bootstrap_peers.clone()).await;
        results.push(("libp2p", libp2p_result));
        
        // Bootstrap iroh if available
        #[cfg(feature = "iroh-transport")]
        if let Some(ref iroh) = self.iroh_transport {
            let iroh_result = iroh.bootstrap(bootstrap_peers.clone()).await;
            results.push(("iroh", iroh_result));
        }
        
        // Return success if at least one transport succeeded
        let successes = results.iter().filter(|(_, result)| result.is_ok()).count();
        if successes > 0 {
            info!("Bootstrap succeeded on {} transports", successes);
            Ok(())
        } else {
            let errors: Vec<_> = results.into_iter()
                .map(|(name, result)| format!("{}={}", name, result.unwrap_err()))
                .collect();
            Err(KadError::QueryFailed {
                reason: format!("All bootstrap attempts failed: {}", errors.join(", "))
            })
        }
    }
    
    async fn get_routing_table_info(&self) -> Result<Vec<PeerInfo>, KadError> {
        // Return combined routing table from all transports
        let mut all_peers = Vec::new();
        
        // Get libp2p routing table
        if let Ok(libp2p_peers) = self.libp2p_transport.get_routing_table_info().await {
            all_peers.extend(libp2p_peers);
        }
        
        // Get iroh routing table if available
        #[cfg(feature = "iroh-transport")]
        if let Some(ref iroh) = self.iroh_transport {
            if let Ok(iroh_peers) = iroh.get_routing_table_info().await {
                all_peers.extend(iroh_peers);
            }
        }
        
        // Deduplicate by peer ID
        all_peers.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
        all_peers.dedup_by(|a, b| a.peer_id == b.peer_id);
        
        Ok(all_peers)
    }
    
    async fn local_peer_id(&self) -> KadPeerId {
        self.local_peer_id.clone()
    }
    
    async fn add_address(&self, peer_id: KadPeerId, address: KadAddress) -> Result<(), KadError> {
        // Add address to all available transports
        let mut results = Vec::new();
        
        results.push(self.libp2p_transport.add_address(peer_id.clone(), address.clone()).await);
        
        #[cfg(feature = "iroh-transport")]
        if let Some(ref iroh) = self.iroh_transport {
            results.push(iroh.add_address(peer_id.clone(), address.clone()).await);
        }
        
        // Return success if at least one transport succeeded
        if results.iter().any(|r| r.is_ok()) {
            Ok(())
        } else {
            results.into_iter().find(|r| r.is_err()).unwrap()
        }
    }
    
    async fn remove_peer(&self, peer_id: &KadPeerId) -> Result<(), KadError> {
        // Remove from all transports
        let mut results = Vec::new();
        
        results.push(self.libp2p_transport.remove_peer(peer_id).await);
        
        #[cfg(feature = "iroh-transport")]
        if let Some(ref iroh) = self.iroh_transport {
            results.push(iroh.remove_peer(peer_id).await);
        }
        
        // Return success if at least one transport succeeded
        if results.iter().any(|r| r.is_ok()) {
            Ok(())
        } else {
            results.into_iter().find(|r| r.is_err()).unwrap()
        }
    }
}