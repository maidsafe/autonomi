// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Behavior management and coordination for ant-net.
//!
//! This module provides sophisticated behavior lifecycle management, state coordination,
//! and cross-behavior communication for the ant-net networking stack.

use crate::{
    event::NetworkEvent,
    event_router::{EventRouter, EventSubscriber, RoutableEvent, SubscriberId, CorrelationId},
    types::{Addresses, PeerInfo},
    AntNetError, ConnectionId, PeerId, Result,
};
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, oneshot, RwLock},
    task::JoinHandle,
    time::interval,
};
use tracing::{debug, error, info, warn};

/// Unique identifier for behaviors.
pub type BehaviorId = String;

/// Health status of a behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviorHealth {
    /// Behavior is healthy and operating normally.
    Healthy,
    /// Behavior is experiencing minor issues but still functional.
    Degraded(String),
    /// Behavior is unhealthy and may not be functioning correctly.
    Unhealthy(String),
    /// Behavior has failed and needs to be restarted.
    Failed(String),
}

impl Default for BehaviorHealth {
    fn default() -> Self {
        BehaviorHealth::Healthy
    }
}

impl fmt::Display for BehaviorHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BehaviorHealth::Healthy => write!(f, "healthy"),
            BehaviorHealth::Degraded(reason) => write!(f, "degraded: {}", reason),
            BehaviorHealth::Unhealthy(reason) => write!(f, "unhealthy: {}", reason),
            BehaviorHealth::Failed(reason) => write!(f, "failed: {}", reason),
        }
    }
}

/// Behavior action results.
#[derive(Debug)]
pub enum BehaviorAction {
    /// Send a network event.
    SendEvent {
        /// The network event to send.
        event: NetworkEvent,
        /// Optional correlation ID for tracking.
        correlation_id: Option<CorrelationId>,
    },
    /// Dial a peer.
    DialPeer {
        /// The peer ID to dial.
        peer_id: PeerId,
        /// Known addresses for the peer.
        addresses: Addresses,
    },
    /// Close a connection.
    CloseConnection {
        /// The connection ID to close.
        connection_id: ConnectionId,
    },
    /// Update behavior configuration.
    UpdateConfig {
        /// Configuration key to update.
        key: String,
        /// New configuration value.
        value: String,
    },
    /// Request state from another behavior.
    RequestState {
        /// The target behavior to query.
        target_behavior: BehaviorId,
        /// The state request to send.
        request: StateRequest,
        /// Channel to send the response.
        response_channel: oneshot::Sender<StateResponse>,
    },
}

/// Inter-behavior state requests.
#[derive(Debug, Clone)]
pub enum StateRequest {
    /// Get closest peers from Kademlia.
    GetClosestPeers {
        /// Target address to find peers near.
        target: crate::NetworkAddress,
        /// Maximum number of peers to return.
        count: usize,
    },
    /// Get connection info.
    GetConnectionInfo {
        /// The peer ID to get connection info for.
        peer_id: PeerId,
    },
    /// Get routing table status.
    GetRoutingTableStatus,
    /// Custom state request.
    Custom {
        /// The type of custom request.
        request_type: String,
        /// Request payload data.
        data: bytes::Bytes,
    },
}

/// Inter-behavior state responses.
#[derive(Debug, Clone)]
pub enum StateResponse {
    /// Closest peers response.
    ClosestPeers(Vec<PeerInfo>),
    /// Connection info response.
    ConnectionInfo(Option<crate::connection::ConnectionInfo>),
    /// Routing table status response.
    RoutingTableStatus {
        /// Number of peers in routing table.
        peer_count: usize,
        /// Number of K-buckets in use.
        bucket_count: usize,
    },
    /// Custom state response.
    Custom {
        /// The type of custom response.
        response_type: String,
        /// Response payload data.
        data: bytes::Bytes,
    },
    /// Error response.
    Error(String),
}

/// Advanced behavior controller trait with lifecycle management and state coordination.
#[async_trait]
pub trait BehaviorController: Send + Sync + fmt::Debug {
    /// Get the unique behavior identifier.
    fn id(&self) -> BehaviorId;

    /// Get the behavior name for debugging.
    fn name(&self) -> &'static str;

    /// Initialize the behavior.
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    /// Start the behavior.
    async fn start(&mut self) -> Result<()> {
        Ok(())
    }

    /// Stop the behavior.
    async fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    /// Check behavior health.
    async fn health_check(&self) -> BehaviorHealth {
        BehaviorHealth::Healthy
    }

    /// Reset behavior state.
    async fn reset_state(&mut self) -> Result<()> {
        Ok(())
    }

    /// Handle an incoming network event.
    async fn handle_event(&mut self, event: NetworkEvent) -> Result<Vec<BehaviorAction>>;

    /// Handle an inter-behavior state request.
    async fn handle_state_request(&mut self, _request: StateRequest) -> StateResponse {
        StateResponse::Error("State request not supported".to_string())
    }

    /// Get behavior dependencies.
    fn dependencies(&self) -> Vec<BehaviorId> {
        Vec::new()
    }

    /// Check if this behavior is interested in the given event.
    fn is_interested(&self, event: &NetworkEvent) -> bool {
        // By default, interested in all events
        let _ = event;
        true
    }

    /// Get behavior configuration keys.
    fn config_keys(&self) -> Vec<String> {
        Vec::new()
    }

    /// Update behavior configuration.
    async fn update_config(&mut self, key: &str, value: &str) -> Result<()> {
        let _ = (key, value);
        Err(AntNetError::Configuration(
            "Configuration updates not supported".to_string(),
        ))
    }

    /// Clone the behavior controller.
    fn clone_controller(&self) -> Box<dyn BehaviorController>;
}

impl Clone for Box<dyn BehaviorController> {
    fn clone(&self) -> Self {
        self.clone_controller()
    }
}

/// Behavior statistics.
#[derive(Debug, Clone, Default)]
pub struct BehaviorStats {
    /// Total events processed.
    pub events_processed: u64,
    /// Total actions generated.
    pub actions_generated: u64,
    /// Average event processing latency.
    pub avg_processing_latency: Duration,
    /// Last health check result.
    pub last_health_check: BehaviorHealth,
    /// Behavior uptime.
    pub uptime: Duration,
    /// Error count.
    pub error_count: u64,
}

/// Internal behavior state tracking.
#[derive(Debug)]
struct BehaviorState {
    /// The behavior controller.
    controller: Box<dyn BehaviorController>,
    /// Event subscriber ID.
    subscriber_id: SubscriberId,
    /// Whether the behavior is enabled.
    enabled: AtomicBool,
    /// Behavior statistics.
    stats: RwLock<BehaviorStats>,
    /// When the behavior was started.
    started_at: Instant,
}

impl BehaviorState {
    fn new(controller: Box<dyn BehaviorController>, subscriber_id: SubscriberId) -> Self {
        Self {
            controller,
            subscriber_id,
            enabled: AtomicBool::new(true),
            stats: RwLock::new(BehaviorStats::default()),
            started_at: Instant::now(),
        }
    }

    async fn update_stats(&self, processing_latency: Duration, action_count: usize, error: bool) {
        let mut stats = self.stats.write().await;
        stats.events_processed += 1;
        stats.actions_generated += action_count as u64;
        
        if error {
            stats.error_count += 1;
        }

        // Update rolling average latency
        let total_events = stats.events_processed;
        stats.avg_processing_latency = (stats.avg_processing_latency * (total_events - 1) as u32
            + processing_latency) / total_events as u32;
        
        stats.uptime = self.started_at.elapsed();
    }
}

/// Configuration for the behavior manager.
#[derive(Debug, Clone)]
pub struct BehaviorManagerConfig {
    /// Health check interval.
    pub health_check_interval: Duration,
    /// Maximum processing time for a single event.
    pub max_event_processing_time: Duration,
    /// Maximum number of pending inter-behavior requests.
    pub max_pending_state_requests: usize,
    /// State request timeout.
    pub state_request_timeout: Duration,
}

impl Default for BehaviorManagerConfig {
    fn default() -> Self {
        Self {
            health_check_interval: Duration::from_secs(30),
            max_event_processing_time: Duration::from_secs(10),
            max_pending_state_requests: 1000,
            state_request_timeout: Duration::from_secs(5),
        }
    }
}

/// High-level behavior manager with sophisticated lifecycle and state coordination.
pub struct BehaviorManager {
    /// Event router for distributing events.
    event_router: Arc<EventRouter>,
    /// Managed behaviors.
    behaviors: Arc<RwLock<HashMap<BehaviorId, BehaviorState>>>,
    /// Behavior dependency graph.
    dependency_graph: Arc<RwLock<HashMap<BehaviorId, HashSet<BehaviorId>>>>,
    /// Configuration.
    config: BehaviorManagerConfig,
    /// Background task handles.
    tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
    /// Inter-behavior communication channels.
    state_request_sender: mpsc::Sender<(BehaviorId, StateRequest, oneshot::Sender<StateResponse>)>,
    /// Shutdown signal.
    shutdown_tx: Arc<RwLock<Option<tokio::sync::broadcast::Sender<()>>>>,
}

impl BehaviorManager {
    /// Create a new behavior manager.
    pub fn new(event_router: Arc<EventRouter>, config: BehaviorManagerConfig) -> Self {
        let (state_request_sender, state_request_receiver) = 
            mpsc::channel(config.max_pending_state_requests);

        let manager = Self {
            event_router,
            behaviors: Arc::new(RwLock::new(HashMap::new())),
            dependency_graph: Arc::new(RwLock::new(HashMap::new())),
            config,
            tasks: Arc::new(RwLock::new(Vec::new())),
            state_request_sender,
            shutdown_tx: Arc::new(RwLock::new(None)),
        };

        // Start background tasks
        manager.start_background_tasks(state_request_receiver);

        manager
    }

    /// Create a new behavior manager with default configuration.
    pub fn with_default_config(event_router: Arc<EventRouter>) -> Self {
        Self::new(event_router, BehaviorManagerConfig::default())
    }

    /// Start background management tasks.
    fn start_background_tasks(
        &self,
        mut state_request_receiver: mpsc::Receiver<(BehaviorId, StateRequest, oneshot::Sender<StateResponse>)>,
    ) {
        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(16);
        
        // Store shutdown sender
        {
            let mut shutdown_lock = self.shutdown_tx.blocking_write();
            *shutdown_lock = Some(shutdown_tx);
        }

        // Health monitoring task
        let health_task = {
            let behaviors = self.behaviors.clone();
            let config = self.config.clone();
            let mut shutdown_rx = shutdown_rx.resubscribe();

            tokio::spawn(async move {
                let mut interval = interval(config.health_check_interval);
                
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            Self::perform_health_checks(&behaviors).await;
                        }
                        _ = shutdown_rx.recv() => {
                            debug!("Health monitoring task shutting down");
                            break;
                        }
                    }
                }
            })
        };

        // Inter-behavior communication task
        let communication_task = {
            let behaviors = self.behaviors.clone();
            let config = self.config.clone();
            let mut shutdown_rx = shutdown_rx.resubscribe();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        Some((target_behavior, request, response_channel)) = state_request_receiver.recv() => {
                            Self::handle_state_request(target_behavior, request, response_channel, &behaviors, &config).await;
                        }
                        _ = shutdown_rx.recv() => {
                            debug!("Inter-behavior communication task shutting down");
                            break;
                        }
                    }
                }
            })
        };

        // Store task handles
        {
            let mut task_handles = self.tasks.blocking_write();
            *task_handles = vec![health_task, communication_task];
        }
    }

    /// Perform health checks on all behaviors.
    async fn perform_health_checks(behaviors: &Arc<RwLock<HashMap<BehaviorId, BehaviorState>>>) {
        let behavior_ids: Vec<_> = {
            let behaviors_lock = behaviors.read().await;
            behaviors_lock.keys().cloned().collect()
        };

        for behavior_id in behavior_ids {
            let health = {
                let mut behaviors_lock = behaviors.write().await;
                if let Some(behavior_state) = behaviors_lock.get_mut(&behavior_id) {
                    behavior_state.controller.health_check().await
                } else {
                    continue;
                }
            };

            // Update stats with health check result
            {
                let behaviors_lock = behaviors.read().await;
                if let Some(behavior_state) = behaviors_lock.get(&behavior_id) {
                    let mut stats = behavior_state.stats.write().await;
                    stats.last_health_check = health.clone();
                }
            }

            match health {
                BehaviorHealth::Healthy => {
                    debug!("Behavior {} is healthy", behavior_id);
                }
                BehaviorHealth::Degraded(reason) => {
                    warn!("Behavior {} is degraded: {}", behavior_id, reason);
                }
                BehaviorHealth::Unhealthy(reason) => {
                    error!("Behavior {} is unhealthy: {}", behavior_id, reason);
                }
                BehaviorHealth::Failed(reason) => {
                    error!("Behavior {} has failed: {}", behavior_id, reason);
                    // TODO: Implement behavior restart logic
                }
            }
        }
    }

    /// Handle inter-behavior state requests.
    async fn handle_state_request(
        target_behavior: BehaviorId,
        request: StateRequest,
        response_channel: oneshot::Sender<StateResponse>,
        behaviors: &Arc<RwLock<HashMap<BehaviorId, BehaviorState>>>,
        config: &BehaviorManagerConfig,
    ) {
        let response = tokio::time::timeout(
            config.state_request_timeout,
            async {
                let mut behaviors_lock = behaviors.write().await;
                if let Some(behavior_state) = behaviors_lock.get_mut(&target_behavior) {
                    behavior_state.controller.handle_state_request(request).await
                } else {
                    StateResponse::Error(format!("Behavior {} not found", target_behavior))
                }
            }
        ).await;

        let final_response = match response {
            Ok(resp) => resp,
            Err(_) => StateResponse::Error("Request timed out".to_string()),
        };

        if let Err(_) = response_channel.send(final_response) {
            warn!("Failed to send state response for behavior {}", target_behavior);
        }
    }

    /// Add a behavior to the manager.
    pub async fn add_behavior(&self, mut controller: Box<dyn BehaviorController>) -> Result<()> {
        let behavior_id = controller.id();
        
        // Check for duplicate IDs
        {
            let behaviors = self.behaviors.read().await;
            if behaviors.contains_key(&behavior_id) {
                return Err(AntNetError::Behavior(format!(
                    "Behavior with ID '{}' already exists",
                    behavior_id
                )));
            }
        }

        // Initialize the behavior
        controller.initialize().await?;

        // Create event subscriber for this behavior
        let behavior_subscriber = BehaviorEventSubscriber::new(
            behavior_id.clone(),
            self.behaviors.clone(),
            self.config.clone(),
        );

        let subscriber_id = self.event_router
            .subscribe(Box::new(behavior_subscriber))
            .await;

        // Create behavior state
        let behavior_state = BehaviorState::new(controller, subscriber_id);

        // Update dependency graph
        {
            let dependencies = behavior_state.controller.dependencies();
            let mut dep_graph = self.dependency_graph.write().await;
            dep_graph.insert(behavior_id.clone(), dependencies.into_iter().collect());
        }

        // Add to behaviors map
        {
            let mut behaviors = self.behaviors.write().await;
            behaviors.insert(behavior_id.clone(), behavior_state);
        }

        info!("Added behavior: {}", behavior_id);
        Ok(())
    }

    /// Remove a behavior from the manager.
    pub async fn remove_behavior(&self, behavior_id: &BehaviorId) -> Result<()> {
        let (subscriber_id, mut controller) = {
            let mut behaviors = self.behaviors.write().await;
            if let Some(behavior_state) = behaviors.remove(behavior_id) {
                (behavior_state.subscriber_id, behavior_state.controller)
            } else {
                return Err(AntNetError::Behavior(format!(
                    "Behavior '{}' not found",
                    behavior_id
                )));
            }
        };

        // Stop the behavior
        controller.stop().await?;

        // Unsubscribe from events
        self.event_router.unsubscribe(subscriber_id).await?;

        // Remove from dependency graph
        {
            let mut dep_graph = self.dependency_graph.write().await;
            dep_graph.remove(behavior_id);
        }

        info!("Removed behavior: {}", behavior_id);
        Ok(())
    }

    /// Start a behavior.
    pub async fn start_behavior(&self, behavior_id: &BehaviorId) -> Result<()> {
        let mut behaviors = self.behaviors.write().await;
        if let Some(behavior_state) = behaviors.get_mut(behavior_id) {
            behavior_state.controller.start().await?;
            behavior_state.enabled.store(true, Ordering::Relaxed);
            info!("Started behavior: {}", behavior_id);
            Ok(())
        } else {
            Err(AntNetError::Behavior(format!(
                "Behavior '{}' not found",
                behavior_id
            )))
        }
    }

    /// Stop a behavior.
    pub async fn stop_behavior(&self, behavior_id: &BehaviorId) -> Result<()> {
        let mut behaviors = self.behaviors.write().await;
        if let Some(behavior_state) = behaviors.get_mut(behavior_id) {
            behavior_state.controller.stop().await?;
            behavior_state.enabled.store(false, Ordering::Relaxed);
            info!("Stopped behavior: {}", behavior_id);
            Ok(())
        } else {
            Err(AntNetError::Behavior(format!(
                "Behavior '{}' not found",
                behavior_id
            )))
        }
    }

    /// Get behavior statistics.
    pub async fn behavior_stats(&self, behavior_id: &BehaviorId) -> Option<BehaviorStats> {
        let behaviors = self.behaviors.read().await;
        if let Some(behavior_state) = behaviors.get(behavior_id) {
            Some(behavior_state.stats.read().await.clone())
        } else {
            None
        }
    }

    /// Get all behavior IDs.
    pub async fn behavior_ids(&self) -> Vec<BehaviorId> {
        let behaviors = self.behaviors.read().await;
        behaviors.keys().cloned().collect()
    }

    /// Request state from a behavior.
    pub async fn request_state(
        &self,
        target_behavior: BehaviorId,
        request: StateRequest,
    ) -> Result<StateResponse> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.state_request_sender
            .send((target_behavior, request, response_tx))
            .await
            .map_err(|_| AntNetError::Behavior("State request channel closed".to_string()))?;

        response_rx.await.map_err(|_| {
            AntNetError::Behavior("Failed to receive state response".to_string())
        })
    }

    /// Shutdown the behavior manager.
    pub async fn shutdown(&self) -> Result<()> {
        // Stop all behaviors
        let behavior_ids: Vec<_> = {
            let behaviors = self.behaviors.read().await;
            behaviors.keys().cloned().collect()
        };

        for behavior_id in behavior_ids {
            let _ = self.stop_behavior(&behavior_id).await;
        }

        // Send shutdown signal
        if let Some(shutdown_tx) = self.shutdown_tx.read().await.as_ref() {
            let _ = shutdown_tx.send(());
        }

        // Wait for all tasks to complete
        let tasks = {
            let mut task_handles = self.tasks.write().await;
            std::mem::take(&mut *task_handles)
        };

        for task in tasks {
            task.abort();
            let _ = task.await;
        }

        info!("BehaviorManager shut down successfully");
        Ok(())
    }
}

/// Event subscriber that forwards events to a specific behavior.
struct BehaviorEventSubscriber {
    behavior_id: BehaviorId,
    behaviors: Arc<RwLock<HashMap<BehaviorId, BehaviorState>>>,
    config: BehaviorManagerConfig,
}

impl BehaviorEventSubscriber {
    fn new(
        behavior_id: BehaviorId,
        behaviors: Arc<RwLock<HashMap<BehaviorId, BehaviorState>>>,
        config: BehaviorManagerConfig,
    ) -> Self {
        Self {
            behavior_id,
            behaviors,
            config,
        }
    }
}

#[async_trait]
impl EventSubscriber for BehaviorEventSubscriber {
    async fn handle_event(&mut self, event: RoutableEvent) -> Result<()> {
        let start_time = Instant::now();
        let mut action_count = 0;
        let mut error = false;

        // Process the event
        let processing_result = tokio::time::timeout(
            self.config.max_event_processing_time,
            async {
                let mut behaviors = self.behaviors.write().await;
                if let Some(behavior_state) = behaviors.get_mut(&self.behavior_id) {
                    if !behavior_state.enabled.load(Ordering::Relaxed) {
                        return Ok(Vec::new());
                    }

                    behavior_state.controller.handle_event(event.event).await
                } else {
                    Ok(Vec::new())
                }
            }
        ).await;

        let actions = match processing_result {
            Ok(Ok(actions)) => {
                action_count = actions.len();
                actions
            }
            Ok(Err(e)) => {
                error = true;
                warn!(
                    "Behavior '{}' failed to process event: {}",
                    self.behavior_id, e
                );
                Vec::new()
            }
            Err(_) => {
                error = true;
                warn!(
                    "Behavior '{}' timed out processing event",
                    self.behavior_id
                );
                Vec::new()
            }
        };

        // Update statistics
        {
            let behaviors = self.behaviors.read().await;
            if let Some(behavior_state) = behaviors.get(&self.behavior_id) {
                behavior_state.update_stats(start_time.elapsed(), action_count, error).await;
            }
        }

        // Process actions (for now, just log them)
        for action in actions {
            debug!("Behavior '{}' generated action: {:?}", self.behavior_id, action);
            // TODO: Implement action processing
        }

        Ok(())
    }

    fn name(&self) -> &str {
        &self.behavior_id
    }

    fn is_interested(&self, event: &NetworkEvent) -> bool {
        // Check if the behavior is interested in this event
        let behaviors = self.behaviors.blocking_read();
        if let Some(behavior_state) = behaviors.get(&self.behavior_id) {
            behavior_state.controller.is_interested(event)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_router::EventRouter;

    #[derive(Debug)]
    struct TestBehavior {
        id: BehaviorId,
        event_count: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl TestBehavior {
        fn new(id: &str) -> (Self, Arc<std::sync::atomic::AtomicUsize>) {
            let event_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let behavior = Self {
                id: id.to_string(),
                event_count: event_count.clone(),
            };
            (behavior, event_count)
        }
    }

    #[async_trait]
    impl BehaviorController for TestBehavior {
        fn id(&self) -> BehaviorId {
            self.id.clone()
        }

        fn name(&self) -> &'static str {
            "test_behavior"
        }

        async fn handle_event(&mut self, _event: NetworkEvent) -> Result<Vec<BehaviorAction>> {
            self.event_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(vec![])
        }

        fn clone_controller(&self) -> Box<dyn BehaviorController> {
            Box::new(TestBehavior {
                id: self.id.clone(),
                event_count: self.event_count.clone(),
            })
        }
    }

    #[tokio::test]
    async fn test_behavior_management() {
        let event_router = Arc::new(EventRouter::with_default_config());
        let manager = BehaviorManager::with_default_config(event_router.clone());

        let (behavior, event_count) = TestBehavior::new("test_behavior_1");
        manager.add_behavior(Box::new(behavior)).await.unwrap();

        // Send an event
        let event = NetworkEvent::PeerConnected {
            peer_id: crate::PeerId::random(),
            connection_id: crate::ConnectionId::new_unchecked(0),
            direction: crate::types::ConnectionDirection::Outbound,
            endpoint: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        };

        event_router.route_event(event, None).unwrap();

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(event_count.load(std::sync::atomic::Ordering::Relaxed), 1);

        manager.shutdown().await.unwrap();
        event_router.shutdown().await.unwrap();
    }
}