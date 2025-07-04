// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Metrics and monitoring for iroh transport adapter
//! 
//! This module provides comprehensive metrics collection and monitoring
//! capabilities for the iroh transport integration.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::{RwLock, Mutex};
use tracing::{debug, info, warn};

use crate::networking::{
    kad::transport::{KadPeerId, QueryId},
    iroh_adapter::{
        config::MetricsConfig,
        protocol::ProtocolStats,
        transport::TransportStats,
        discovery::DiscoveryStats,
    },
};

/// Comprehensive metrics collector for iroh transport
#[derive(Clone)]
pub struct IrohMetrics {
    /// Configuration for metrics collection
    config: MetricsConfig,
    
    /// Connection metrics
    connection_metrics: Arc<RwLock<ConnectionMetrics>>,
    
    /// Message metrics
    message_metrics: Arc<RwLock<MessageMetrics>>,
    
    /// Latency histogram
    latency_histogram: Arc<RwLock<LatencyHistogram>>,
    
    /// Per-peer metrics
    peer_metrics: Arc<RwLock<HashMap<KadPeerId, PeerMetrics>>>,
    
    /// Query metrics
    query_metrics: Arc<RwLock<QueryMetrics>>,
    
    /// Error metrics
    error_metrics: Arc<RwLock<ErrorMetrics>>,
    
    /// System metrics
    system_metrics: Arc<RwLock<SystemMetrics>>,
    
    /// Start time for uptime calculation
    start_time: Instant,
    
    /// Last export time
    last_export: Arc<Mutex<Instant>>,
    
    /// Background task handles
    tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    
    /// Shutdown signal
    shutdown: Arc<tokio::sync::Notify>,
}

/// Connection-related metrics
#[derive(Debug, Clone, Default)]
pub struct ConnectionMetrics {
    pub total_connections: u64,
    pub active_connections: u64,
    pub failed_connections: u64,
    pub connection_attempts: u64,
    pub connections_per_second: f64,
    pub avg_connection_duration: Duration,
    pub max_concurrent_connections: u64,
    pub connection_timeouts: u64,
}

/// Message-related metrics
#[derive(Debug, Clone, Default)]
pub struct MessageMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub messages_per_second: f64,
    pub avg_message_size: u64,
    pub max_message_size: u64,
    pub message_send_failures: u64,
    pub message_receive_failures: u64,
}

/// Latency histogram for tracking response times
#[derive(Debug, Clone)]
pub struct LatencyHistogram {
    pub buckets: Vec<f64>,
    pub counts: Vec<u64>,
    pub total_samples: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub percentiles: HashMap<String, f64>, // "p50", "p95", "p99"
}

/// Per-peer metrics tracking
#[derive(Debug, Clone, Default)]
pub struct PeerMetrics {
    pub peer_id: KadPeerId,
    pub connections_established: u64,
    pub connections_failed: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub avg_latency: Duration,
    pub reliability_score: f32,
    pub last_seen: Option<Instant>,
    pub first_seen: Instant,
    pub error_count: u64,
}

/// Query performance metrics
#[derive(Debug, Clone, Default)]
pub struct QueryMetrics {
    pub total_queries: u64,
    pub successful_queries: u64,
    pub failed_queries: u64,
    pub timeout_queries: u64,
    pub avg_query_duration: Duration,
    pub queries_by_type: HashMap<String, u64>,
    pub query_success_rate: f64,
}

/// Error tracking metrics
#[derive(Debug, Clone, Default)]
pub struct ErrorMetrics {
    pub total_errors: u64,
    pub connection_errors: u64,
    pub protocol_errors: u64,
    pub serialization_errors: u64,
    pub discovery_errors: u64,
    pub timeout_errors: u64,
    pub errors_by_type: HashMap<String, u64>,
    pub error_rate: f64,
}

/// System-level metrics
#[derive(Debug, Clone, Default)]
pub struct SystemMetrics {
    pub uptime: Duration,
    pub memory_usage: u64,
    pub cpu_usage: f32,
    pub network_interfaces: u32,
    pub open_file_descriptors: u32,
    pub thread_count: u32,
}

/// Aggregated metrics for export
#[derive(Debug, Clone)]
pub struct AggregatedMetrics {
    pub timestamp: Instant,
    pub uptime: Duration,
    pub connections: ConnectionMetrics,
    pub messages: MessageMetrics,
    pub latency: LatencyHistogram,
    pub queries: QueryMetrics,
    pub errors: ErrorMetrics,
    pub system: SystemMetrics,
    pub top_peers: Vec<PeerMetrics>,
    pub discovery: DiscoveryStats,
    pub transport: TransportStats,
    pub protocol: ProtocolStats,
}

impl IrohMetrics {
    /// Create a new metrics collector
    pub fn new(config: MetricsConfig) -> Self {
        let metrics = Self {
            config,
            connection_metrics: Arc::new(RwLock::new(ConnectionMetrics::default())),
            message_metrics: Arc::new(RwLock::new(MessageMetrics::default())),
            latency_histogram: Arc::new(RwLock::new(LatencyHistogram::new(&[
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]))),
            peer_metrics: Arc::new(RwLock::new(HashMap::new())),
            query_metrics: Arc::new(RwLock::new(QueryMetrics::default())),
            error_metrics: Arc::new(RwLock::new(ErrorMetrics::default())),
            system_metrics: Arc::new(RwLock::new(SystemMetrics::default())),
            start_time: Instant::now(),
            last_export: Arc::new(Mutex::new(Instant::now())),
            tasks: Arc::new(Mutex::new(Vec::new())),
            shutdown: Arc::new(tokio::sync::Notify::new()),
        };
        
        if config.enabled {
            let metrics_clone = metrics.clone();
            tokio::spawn(async move {
                metrics_clone.start_background_tasks().await;
            });
        }
        
        metrics
    }
    
    /// Record a new connection
    pub async fn record_connection_established(&self, peer_id: &KadPeerId) {
        if !self.config.track_connections {
            return;
        }
        
        let mut conn_metrics = self.connection_metrics.write().await;
        conn_metrics.total_connections += 1;
        conn_metrics.active_connections += 1;
        conn_metrics.connection_attempts += 1;
        conn_metrics.max_concurrent_connections = 
            conn_metrics.max_concurrent_connections.max(conn_metrics.active_connections);
        
        // Update peer metrics
        if self.peer_metrics.read().await.len() < self.config.max_peer_metrics {
            let mut peer_metrics = self.peer_metrics.write().await;
            peer_metrics
                .entry(peer_id.clone())
                .and_modify(|metrics| {
                    metrics.connections_established += 1;
                    metrics.last_seen = Some(Instant::now());
                })
                .or_insert_with(|| PeerMetrics {
                    peer_id: peer_id.clone(),
                    connections_established: 1,
                    first_seen: Instant::now(),
                    last_seen: Some(Instant::now()),
                    ..Default::default()
                });
        }
    }
    
    /// Record a connection closure
    pub async fn record_connection_closed(&self, peer_id: &KadPeerId) {
        if !self.config.track_connections {
            return;
        }
        
        let mut conn_metrics = self.connection_metrics.write().await;
        conn_metrics.active_connections = conn_metrics.active_connections.saturating_sub(1);
    }
    
    /// Record a failed connection attempt
    pub async fn record_connection_failed(&self, peer_id: &KadPeerId) {
        if !self.config.track_connections {
            return;
        }
        
        let mut conn_metrics = self.connection_metrics.write().await;
        conn_metrics.failed_connections += 1;
        conn_metrics.connection_attempts += 1;
        
        // Update peer metrics
        if let Some(peer_metrics) = self.peer_metrics.write().await.get_mut(peer_id) {
            peer_metrics.connections_failed += 1;
            peer_metrics.error_count += 1;
        }
    }
    
    /// Record a message sent
    pub async fn record_message_sent(&self, peer_id: &KadPeerId, size: usize) {
        if !self.config.track_messages {
            return;
        }
        
        let mut msg_metrics = self.message_metrics.write().await;
        msg_metrics.messages_sent += 1;
        msg_metrics.bytes_sent += size as u64;
        msg_metrics.max_message_size = msg_metrics.max_message_size.max(size as u64);
        
        // Update peer metrics
        if let Some(peer_metrics) = self.peer_metrics.write().await.get_mut(peer_id) {
            peer_metrics.messages_sent += 1;
            peer_metrics.bytes_sent += size as u64;
            peer_metrics.last_seen = Some(Instant::now());
        }
    }
    
    /// Record a message received
    pub async fn record_message_received(&self, peer_id: &KadPeerId, size: usize) {
        if !self.config.track_messages {
            return;
        }
        
        let mut msg_metrics = self.message_metrics.write().await;
        msg_metrics.messages_received += 1;
        msg_metrics.bytes_received += size as u64;
        
        // Update peer metrics
        if let Some(peer_metrics) = self.peer_metrics.write().await.get_mut(peer_id) {
            peer_metrics.messages_received += 1;
            peer_metrics.bytes_received += size as u64;
            peer_metrics.last_seen = Some(Instant::now());
        }
    }
    
    /// Record request latency
    pub async fn record_latency(&self, peer_id: &KadPeerId, latency: Duration) {
        if !self.config.track_latency {
            return;
        }
        
        let latency_ms = latency.as_secs_f64() * 1000.0;
        self.latency_histogram.write().await.add_sample(latency_ms);
        
        // Update peer latency
        if let Some(peer_metrics) = self.peer_metrics.write().await.get_mut(peer_id) {
            let current_avg = peer_metrics.avg_latency.as_secs_f64();
            let new_avg = if current_avg == 0.0 {
                latency
            } else {
                Duration::from_secs_f64((current_avg * 0.9) + (latency.as_secs_f64() * 0.1))
            };
            peer_metrics.avg_latency = new_avg;
        }
    }
    
    /// Record a query started
    pub async fn record_query_started(&self, query_type: &str) {
        let mut query_metrics = self.query_metrics.write().await;
        query_metrics.total_queries += 1;
        *query_metrics.queries_by_type.entry(query_type.to_string()).or_insert(0) += 1;
    }
    
    /// Record a query completed successfully
    pub async fn record_query_success(&self, duration: Duration) {
        let mut query_metrics = self.query_metrics.write().await;
        query_metrics.successful_queries += 1;
        
        // Update average duration
        let current_avg = query_metrics.avg_query_duration.as_secs_f64();
        let new_avg = if current_avg == 0.0 {
            duration
        } else {
            Duration::from_secs_f64((current_avg * 0.9) + (duration.as_secs_f64() * 0.1))
        };
        query_metrics.avg_query_duration = new_avg;
        
        // Update success rate
        if query_metrics.total_queries > 0 {
            query_metrics.query_success_rate = 
                query_metrics.successful_queries as f64 / query_metrics.total_queries as f64;
        }
    }
    
    /// Record a query failure
    pub async fn record_query_failure(&self, error_type: &str) {
        let mut query_metrics = self.query_metrics.write().await;
        query_metrics.failed_queries += 1;
        
        if error_type == "timeout" {
            query_metrics.timeout_queries += 1;
        }
        
        // Update success rate
        if query_metrics.total_queries > 0 {
            query_metrics.query_success_rate = 
                query_metrics.successful_queries as f64 / query_metrics.total_queries as f64;
        }
        
        // Record error
        self.record_error(error_type).await;
    }
    
    /// Record an error
    pub async fn record_error(&self, error_type: &str) {
        let mut error_metrics = self.error_metrics.write().await;
        error_metrics.total_errors += 1;
        
        match error_type {
            "connection" => error_metrics.connection_errors += 1,
            "protocol" => error_metrics.protocol_errors += 1,
            "serialization" => error_metrics.serialization_errors += 1,
            "discovery" => error_metrics.discovery_errors += 1,
            "timeout" => error_metrics.timeout_errors += 1,
            _ => {}
        }
        
        *error_metrics.errors_by_type.entry(error_type.to_string()).or_insert(0) += 1;
        
        // Calculate error rate (errors per minute)
        let elapsed = self.start_time.elapsed().as_secs_f64() / 60.0;
        if elapsed > 0.0 {
            error_metrics.error_rate = error_metrics.total_errors as f64 / elapsed;
        }
    }
    
    /// Get current aggregated metrics
    pub async fn get_metrics(&self) -> AggregatedMetrics {
        let now = Instant::now();
        let uptime = now.duration_since(self.start_time);
        
        // Update system metrics
        self.update_system_metrics().await;
        
        // Get top peers by activity
        let top_peers = self.get_top_peers(10).await;
        
        AggregatedMetrics {
            timestamp: now,
            uptime,
            connections: self.connection_metrics.read().await.clone(),
            messages: self.message_metrics.read().await.clone(),
            latency: self.latency_histogram.read().await.clone(),
            queries: self.query_metrics.read().await.clone(),
            errors: self.error_metrics.read().await.clone(),
            system: self.system_metrics.read().await.clone(),
            top_peers,
            // These would be provided by external components
            discovery: DiscoveryStats::default(),
            transport: TransportStats::default(),
            protocol: ProtocolStats::default(),
        }
    }
    
    /// Get top peers by activity
    async fn get_top_peers(&self, limit: usize) -> Vec<PeerMetrics> {
        let peer_metrics = self.peer_metrics.read().await;
        let mut peers: Vec<_> = peer_metrics.values().cloned().collect();
        
        // Sort by total activity (messages + connections)
        peers.sort_by(|a, b| {
            let activity_a = a.messages_sent + a.messages_received + a.connections_established;
            let activity_b = b.messages_sent + b.messages_received + b.connections_established;
            activity_b.cmp(&activity_a)
        });
        
        peers.into_iter().take(limit).collect()
    }
    
    /// Update system metrics
    async fn update_system_metrics(&self) {
        let mut system_metrics = self.system_metrics.write().await;
        system_metrics.uptime = self.start_time.elapsed();
        
        // Note: In a real implementation, you'd use system libraries to get actual metrics
        // For now, we'll just update uptime
    }
    
    /// Start background tasks for metrics collection
    async fn start_background_tasks(&self) {
        let mut tasks = self.tasks.lock().await;
        
        // Periodic export task
        let metrics = self.clone();
        let handle = tokio::spawn(async move {
            metrics.export_task().await;
        });
        tasks.push(handle);
        
        // Cleanup task
        let metrics = self.clone();
        let handle = tokio::spawn(async move {
            metrics.cleanup_task().await;
        });
        tasks.push(handle);
        
        // Rate calculation task
        let metrics = self.clone();
        let handle = tokio::spawn(async move {
            metrics.rate_calculation_task().await;
        });
        tasks.push(handle);
    }
    
    /// Periodic export task
    async fn export_task(&self) {
        let mut interval = tokio::time::interval(self.config.export_interval);
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let metrics = self.get_metrics().await;
                    self.export_metrics(&metrics).await;
                    *self.last_export.lock().await = Instant::now();
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Cleanup task for old peer metrics
    async fn cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.cleanup_old_peers().await;
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Rate calculation task
    async fn rate_calculation_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(60)); // 1 minute
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.calculate_rates().await;
                },
                _ = self.shutdown.notified() => {
                    break;
                }
            }
        }
    }
    
    /// Export metrics (placeholder - in real implementation would send to monitoring system)
    async fn export_metrics(&self, metrics: &AggregatedMetrics) {
        info!("Exporting metrics: {} connections, {} messages, {:.2}ms avg latency", 
              metrics.connections.active_connections,
              metrics.messages.messages_sent + metrics.messages.messages_received,
              metrics.latency.percentiles.get("p50").unwrap_or(&0.0));
    }
    
    /// Clean up old peer metrics
    async fn cleanup_old_peers(&self) {
        let now = Instant::now();
        let stale_threshold = Duration::from_secs(3600); // 1 hour
        
        let mut peer_metrics = self.peer_metrics.write().await;
        peer_metrics.retain(|_, metrics| {
            if let Some(last_seen) = metrics.last_seen {
                now.duration_since(last_seen) < stale_threshold
            } else {
                now.duration_since(metrics.first_seen) < stale_threshold
            }
        });
        
        debug!("Cleaned up peer metrics, {} peers remaining", peer_metrics.len());
    }
    
    /// Calculate rates (messages/sec, connections/sec, etc.)
    async fn calculate_rates(&self) {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed < 1.0 {
            return;
        }
        
        // Update connection rate
        {
            let mut conn_metrics = self.connection_metrics.write().await;
            conn_metrics.connections_per_second = conn_metrics.total_connections as f64 / elapsed;
        }
        
        // Update message rate
        {
            let mut msg_metrics = self.message_metrics.write().await;
            let total_messages = msg_metrics.messages_sent + msg_metrics.messages_received;
            msg_metrics.messages_per_second = total_messages as f64 / elapsed;
            
            if total_messages > 0 {
                msg_metrics.avg_message_size = 
                    (msg_metrics.bytes_sent + msg_metrics.bytes_received) / total_messages;
            }
        }
    }
    
    /// Shutdown metrics collection
    pub async fn shutdown(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Shutting down metrics collection");
        
        // Signal shutdown to background tasks
        self.shutdown.notify_waiters();
        
        // Wait for tasks to complete
        let mut tasks = self.tasks.lock().await;
        for task in tasks.drain(..) {
            task.abort();
        }
        
        // Export final metrics
        let metrics = self.get_metrics().await;
        self.export_metrics(&metrics).await;
        
        Ok(())
    }
}

impl LatencyHistogram {
    fn new(buckets: &[f64]) -> Self {
        Self {
            buckets: buckets.to_vec(),
            counts: vec![0; buckets.len()],
            total_samples: 0,
            sum: 0.0,
            min: f64::INFINITY,
            max: 0.0,
            percentiles: HashMap::new(),
        }
    }
    
    fn add_sample(&mut self, value: f64) {
        self.total_samples += 1;
        self.sum += value;
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        
        // Find bucket for this value
        for (i, &bucket) in self.buckets.iter().enumerate() {
            if value <= bucket {
                self.counts[i] += 1;
                break;
            }
        }
        
        // Update percentiles periodically
        if self.total_samples % 100 == 0 {
            self.calculate_percentiles();
        }
    }
    
    fn calculate_percentiles(&mut self) {
        if self.total_samples == 0 {
            return;
        }
        
        let p50_target = self.total_samples / 2;
        let p95_target = (self.total_samples * 95) / 100;
        let p99_target = (self.total_samples * 99) / 100;
        
        let mut cumulative = 0;
        for (i, &count) in self.counts.iter().enumerate() {
            cumulative += count;
            
            if cumulative >= p50_target && !self.percentiles.contains_key("p50") {
                self.percentiles.insert("p50".to_string(), self.buckets[i]);
            }
            if cumulative >= p95_target && !self.percentiles.contains_key("p95") {
                self.percentiles.insert("p95".to_string(), self.buckets[i]);
            }
            if cumulative >= p99_target && !self.percentiles.contains_key("p99") {
                self.percentiles.insert("p99".to_string(), self.buckets[i]);
                break;
            }
        }
    }
}