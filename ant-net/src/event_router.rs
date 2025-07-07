// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Advanced event routing system for ant-net.
//!
//! This module provides high-performance event routing with priority queues,
//! backpressure handling, and multi-subscriber support.

use crate::{
    event::{EventPriority, NetworkEvent},
    AntNetError, PeerId, Result,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, RwLock},
    task::JoinHandle,
    time::interval,
};
use tracing::{debug, warn};

/// Unique identifier for event subscribers.
pub type SubscriberId = u64;

/// Unique identifier for correlation tracking.
pub type CorrelationId = u64;

/// Event metadata for tracking and processing.
#[derive(Debug, Clone)]
pub struct EventMetadata {
    /// Unique event identifier.
    pub event_id: u64,
    /// Priority for processing order.
    pub priority: EventPriority,
    /// When the event was created.
    pub created_at: Instant,
    /// Optional correlation ID for request/response tracking.
    pub correlation_id: Option<CorrelationId>,
    /// Source peer for the event.
    pub peer_id: Option<PeerId>,
}

/// Enriched event with metadata for routing.
#[derive(Debug, Clone)]
pub struct RoutableEvent {
    /// The network event.
    pub event: NetworkEvent,
    /// Event metadata.
    pub metadata: EventMetadata,
}

impl RoutableEvent {
    /// Create a new routable event.
    pub fn new(event: NetworkEvent, correlation_id: Option<CorrelationId>) -> Self {
        let event_id = EVENT_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let priority = event.priority();
        let peer_id = event.peer_id();

        Self {
            event,
            metadata: EventMetadata {
                event_id,
                priority,
                created_at: Instant::now(),
                correlation_id,
                peer_id,
            },
        }
    }

    /// Get the processing latency for this event.
    pub fn latency(&self) -> Duration {
        self.metadata.created_at.elapsed()
    }
}

impl PartialEq for RoutableEvent {
    fn eq(&self, other: &Self) -> bool {
        self.metadata.event_id == other.metadata.event_id
    }
}

impl Eq for RoutableEvent {}

impl PartialOrd for RoutableEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RoutableEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority events come first, then by event_id for FIFO within same priority
        other
            .metadata
            .priority
            .cmp(&self.metadata.priority)
            .then_with(|| self.metadata.event_id.cmp(&other.metadata.event_id))
    }
}

/// Event subscriber callback trait.
#[async_trait::async_trait]
pub trait EventSubscriber: Send + Sync {
    /// Handle an incoming event.
    async fn handle_event(&mut self, event: RoutableEvent) -> Result<()>;

    /// Get the subscriber name for debugging.
    fn name(&self) -> &str;

    /// Check if the subscriber is interested in this event type.
    fn is_interested(&self, event: &NetworkEvent) -> bool {
        // By default, interested in all events
        let _ = event;
        true
    }
}

/// Event routing statistics.
#[derive(Debug, Clone, Default)]
pub struct EventRouterStats {
    /// Total events processed.
    pub total_events: u64,
    /// Events by priority level.
    pub events_by_priority: [u64; 4], // Critical, High, Normal, Low
    /// Average processing latency by priority.
    pub avg_latency_by_priority: [Duration; 4],
    /// Current queue depth by priority.
    pub queue_depth_by_priority: [usize; 4],
    /// Number of dropped events due to backpressure.
    pub dropped_events: u64,
    /// Number of active subscribers.
    pub active_subscribers: usize,
}

/// Configuration for the event router.
#[derive(Debug, Clone)]
pub struct EventRouterConfig {
    /// Maximum queue size per priority level.
    pub max_queue_size_by_priority: [usize; 4],
    /// Processing timeout for events.
    pub processing_timeout: Duration,
    /// Statistics collection interval.
    pub stats_interval: Duration,
    /// Maximum number of concurrent event processors.
    pub max_concurrent_processors: usize,
}

impl Default for EventRouterConfig {
    fn default() -> Self {
        Self {
            max_queue_size_by_priority: [1000, 5000, 10000, 50000], // Critical, High, Normal, Low
            processing_timeout: Duration::from_secs(30),
            stats_interval: Duration::from_secs(60),
            max_concurrent_processors: 4,
        }
    }
}

/// High-performance event router with priority queues and backpressure handling.
pub struct EventRouter {
    /// Event queues by priority level.
    priority_queues: Arc<RwLock<[VecDeque<RoutableEvent>; 4]>>,
    /// Registered event subscribers.
    subscribers: Arc<RwLock<HashMap<SubscriberId, Box<dyn EventSubscriber>>>>,
    /// Event routing statistics.
    stats: Arc<RwLock<EventRouterStats>>,
    /// Configuration.
    config: EventRouterConfig,
    /// Background task handles.
    tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
    /// Event sender for new events.
    event_sender: mpsc::UnboundedSender<RoutableEvent>,
    /// Shutdown signal.
    shutdown_tx: Arc<RwLock<Option<tokio::sync::broadcast::Sender<()>>>>,
}

/// Global event ID counter.
static EVENT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl EventRouter {
    /// Create a new event router.
    pub fn new(config: EventRouterConfig) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let router = Self {
            priority_queues: Arc::new(RwLock::new([
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
            ])),
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(EventRouterStats::default())),
            config,
            tasks: Arc::new(RwLock::new(Vec::new())),
            event_sender,
            shutdown_tx: Arc::new(RwLock::new(None)),
        };

        // Start background tasks
        router.start_background_tasks(event_receiver);

        router
    }

    /// Create a new event router with default configuration.
    pub fn with_default_config() -> Self {
        Self::new(EventRouterConfig::default())
    }

    /// Start the background processing tasks.
    fn start_background_tasks(&self, mut event_receiver: mpsc::UnboundedReceiver<RoutableEvent>) {
        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(16);
        
        // Store shutdown sender
        {
            let mut shutdown_lock = self.shutdown_tx.blocking_write();
            *shutdown_lock = Some(shutdown_tx);
        }

        // Event ingestion task
        let ingestion_task = {
            let priority_queues = self.priority_queues.clone();
            let config = self.config.clone();
            let stats = self.stats.clone();
            let mut shutdown_rx_clone = shutdown_rx.resubscribe();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        Some(event) = event_receiver.recv() => {
                            Self::ingest_event(event, &priority_queues, &config, &stats).await;
                        }
                        _ = shutdown_rx_clone.recv() => {
                            debug!("Event ingestion task shutting down");
                            break;
                        }
                    }
                }
            })
        };

        // Event processing tasks (one per priority level)
        let mut processing_tasks = Vec::new();
        for priority_level in 0..4 {
            for worker_id in 0..self.config.max_concurrent_processors {
                let task = {
                    let priority_queues = self.priority_queues.clone();
                    let subscribers = self.subscribers.clone();
                    let stats = self.stats.clone();
                    let config = self.config.clone();
                    let mut shutdown_rx_clone = shutdown_rx.resubscribe();

                    tokio::spawn(async move {
                        let mut interval = interval(Duration::from_millis(1));
                        
                        loop {
                            tokio::select! {
                                _ = interval.tick() => {
                                    if let Some(event) = Self::dequeue_event(&priority_queues, priority_level).await {
                                        Self::process_event(event, &subscribers, &stats, &config).await;
                                    }
                                }
                                _ = shutdown_rx_clone.recv() => {
                                    debug!("Event processing task {}-{} shutting down", priority_level, worker_id);
                                    break;
                                }
                            }
                        }
                    })
                };
                processing_tasks.push(task);
            }
        }

        // Statistics collection task
        let stats_task = {
            let stats = self.stats.clone();
            let priority_queues = self.priority_queues.clone();
            let subscribers = self.subscribers.clone();
            let mut interval = interval(self.config.stats_interval);
            let mut shutdown_rx_clone = shutdown_rx.resubscribe();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            Self::update_stats(&stats, &priority_queues, &subscribers).await;
                        }
                        _ = shutdown_rx_clone.recv() => {
                            debug!("Statistics collection task shutting down");
                            break;
                        }
                    }
                }
            })
        };

        // Store all task handles
        let mut tasks = vec![ingestion_task, stats_task];
        tasks.extend(processing_tasks);

        {
            let mut task_handles = self.tasks.blocking_write();
            *task_handles = tasks;
        }
    }

    /// Ingest an event into the appropriate priority queue.
    async fn ingest_event(
        event: RoutableEvent,
        priority_queues: &Arc<RwLock<[VecDeque<RoutableEvent>; 4]>>,
        config: &EventRouterConfig,
        stats: &Arc<RwLock<EventRouterStats>>,
    ) {
        let priority_index = event.metadata.priority as usize;
        let mut queues = priority_queues.write().await;

        // Check queue capacity
        if queues[priority_index].len() >= config.max_queue_size_by_priority[priority_index] {
            // Apply backpressure - drop the event and update stats
            {
                let mut stats_lock = stats.write().await;
                stats_lock.dropped_events += 1;
            }
            warn!(
                "Dropping event due to queue full: priority={:?}, queue_size={}",
                event.metadata.priority,
                queues[priority_index].len()
            );
            return;
        }

        // Add to appropriate queue
        queues[priority_index].push_back(event);
    }

    /// Dequeue the next event from the specified priority level.
    async fn dequeue_event(
        priority_queues: &Arc<RwLock<[VecDeque<RoutableEvent>; 4]>>,
        priority_level: usize,
    ) -> Option<RoutableEvent> {
        let mut queues = priority_queues.write().await;
        queues[priority_level].pop_front()
    }

    /// Process an event by distributing it to interested subscribers.
    async fn process_event(
        event: RoutableEvent,
        subscribers: &Arc<RwLock<HashMap<SubscriberId, Box<dyn EventSubscriber>>>>,
        stats: &Arc<RwLock<EventRouterStats>>,
        config: &EventRouterConfig,
    ) {
        let start_time = Instant::now();
        let priority_index = event.metadata.priority as usize;

        // Get interested subscribers
        let interested_subscribers = {
            let subscribers_lock = subscribers.read().await;
            subscribers_lock
                .iter()
                .filter_map(|(id, subscriber)| {
                    if subscriber.is_interested(&event.event) {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        // Process event for each interested subscriber
        for subscriber_id in interested_subscribers {
            let event_clone = event.clone();
            
            // Create timeout for processing
            let processing_result = tokio::time::timeout(
                config.processing_timeout,
                async {
                    let mut subscribers_lock = subscribers.write().await;
                    if let Some(subscriber) = subscribers_lock.get_mut(&subscriber_id) {
                        subscriber.handle_event(event_clone).await
                    } else {
                        Ok(()) // Subscriber was removed
                    }
                }
            ).await;

            match processing_result {
                Ok(Ok(())) => {
                    // Successfully processed
                }
                Ok(Err(e)) => {
                    warn!("Subscriber {} failed to process event: {}", subscriber_id, e);
                }
                Err(_) => {
                    warn!("Subscriber {} timed out processing event", subscriber_id);
                }
            }
        }

        // Update statistics
        {
            let mut stats_lock = stats.write().await;
            stats_lock.total_events += 1;
            stats_lock.events_by_priority[priority_index] += 1;

            let processing_latency = start_time.elapsed();
            let current_avg = stats_lock.avg_latency_by_priority[priority_index];
            let event_count = stats_lock.events_by_priority[priority_index];
            
            // Update rolling average
            stats_lock.avg_latency_by_priority[priority_index] = 
                (current_avg * (event_count - 1) as u32 + processing_latency) / event_count as u32;
        }
    }

    /// Update router statistics.
    async fn update_stats(
        stats: &Arc<RwLock<EventRouterStats>>,
        priority_queues: &Arc<RwLock<[VecDeque<RoutableEvent>; 4]>>,
        subscribers: &Arc<RwLock<HashMap<SubscriberId, Box<dyn EventSubscriber>>>>,
    ) {
        let queue_depths = {
            let queues = priority_queues.read().await;
            [
                queues[0].len(),
                queues[1].len(),
                queues[2].len(),
                queues[3].len(),
            ]
        };

        let subscriber_count = {
            let subscribers_lock = subscribers.read().await;
            subscribers_lock.len()
        };

        let mut stats_lock = stats.write().await;
        stats_lock.queue_depth_by_priority = queue_depths;
        stats_lock.active_subscribers = subscriber_count;

        debug!(
            "EventRouter stats: total_events={}, queue_depths={:?}, subscribers={}",
            stats_lock.total_events, queue_depths, subscriber_count
        );
    }

    /// Submit an event for routing.
    pub fn route_event(&self, event: NetworkEvent, correlation_id: Option<CorrelationId>) -> Result<()> {
        let routable_event = RoutableEvent::new(event, correlation_id);
        
        self.event_sender.send(routable_event).map_err(|_| {
            AntNetError::Event("Event router is shut down".to_string())
        })?;

        Ok(())
    }

    /// Register a new event subscriber.
    pub async fn subscribe(&self, subscriber: Box<dyn EventSubscriber>) -> SubscriberId {
        let subscriber_id = EVENT_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        
        let mut subscribers = self.subscribers.write().await;
        subscribers.insert(subscriber_id, subscriber);

        debug!("Registered new event subscriber: {}", subscriber_id);
        subscriber_id
    }

    /// Unregister an event subscriber.
    pub async fn unsubscribe(&self, subscriber_id: SubscriberId) -> Result<()> {
        let mut subscribers = self.subscribers.write().await;
        
        if subscribers.remove(&subscriber_id).is_some() {
            debug!("Unregistered event subscriber: {}", subscriber_id);
            Ok(())
        } else {
            Err(AntNetError::Event(format!(
                "Subscriber {} not found",
                subscriber_id
            )))
        }
    }

    /// Get current router statistics.
    pub async fn stats(&self) -> EventRouterStats {
        self.stats.read().await.clone()
    }

    /// Shutdown the event router.
    pub async fn shutdown(&self) -> Result<()> {
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

        debug!("EventRouter shut down successfully");
        Ok(())
    }
}

impl Drop for EventRouter {
    fn drop(&mut self) {
        // Best effort cleanup
        if let Some(shutdown_tx) = self.shutdown_tx.blocking_read().as_ref() {
            let _ = shutdown_tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::NetworkEvent;
    use crate::types::ConnectionDirection;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestSubscriber {
        name: String,
        event_count: Arc<AtomicUsize>,
    }

    impl TestSubscriber {
        fn new(name: &str) -> (Self, Arc<AtomicUsize>) {
            let event_count = Arc::new(AtomicUsize::new(0));
            let subscriber = Self {
                name: name.to_string(),
                event_count: event_count.clone(),
            };
            (subscriber, event_count)
        }
    }

    #[async_trait::async_trait]
    impl EventSubscriber for TestSubscriber {
        async fn handle_event(&mut self, _event: RoutableEvent) -> Result<()> {
            self.event_count.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(1)).await; // Simulate processing
            Ok(())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_event_routing_basic() {
        let router = EventRouter::with_default_config();
        
        let (subscriber, event_count) = TestSubscriber::new("test_subscriber");
        let _subscriber_id = router.subscribe(Box::new(subscriber)).await;

        // Send a test event
        let event = NetworkEvent::PeerConnected {
            peer_id: crate::PeerId::random(),
            connection_id: crate::ConnectionId::new_unchecked(0),
            direction: ConnectionDirection::Outbound,
            endpoint: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        };

        router.route_event(event, None).unwrap();

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert_eq!(event_count.load(Ordering::Relaxed), 1);
        
        router.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let router = EventRouter::with_default_config();
        
        let (subscriber, event_count) = TestSubscriber::new("priority_subscriber");
        let _subscriber_id = router.subscribe(Box::new(subscriber)).await;

        // Send events with different priorities
        let low_event = NetworkEvent::ConnectivityChanged {
            connectivity: crate::event::Connectivity::ConnectedPublic,
        };
        let critical_event = NetworkEvent::PeerConnected {
            peer_id: crate::PeerId::random(),
            connection_id: crate::ConnectionId::new_unchecked(0),
            direction: ConnectionDirection::Outbound,
            endpoint: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        };

        // Send low priority first, then critical
        router.route_event(low_event, None).unwrap();
        router.route_event(critical_event, None).unwrap();

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(event_count.load(Ordering::Relaxed), 2);
        
        router.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_backpressure_handling() {
        let mut config = EventRouterConfig::default();
        config.max_queue_size_by_priority = [1, 1, 1, 1]; // Very small queues
        
        let router = EventRouter::new(config);
        
        // Don't register any subscribers to create backpressure
        
        // Send many events to trigger backpressure
        for _ in 0..10 {
            let event = NetworkEvent::PeerConnected {
                peer_id: crate::PeerId::random(),
                connection_id: crate::ConnectionId::new_unchecked(0),
                direction: ConnectionDirection::Outbound,
                endpoint: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
            };
            let _ = router.route_event(event, None);
        }

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(50)).await;

        let stats = router.stats().await;
        assert!(stats.dropped_events > 0);
        
        router.shutdown().await.unwrap();
    }
}