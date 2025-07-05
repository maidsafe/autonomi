//! Enhanced metrics collection for pure iroh networking
//! 
//! This module provides optimized metrics collection focused specifically
//! on iroh transport performance, removing dual-stack complexity while
//! adding iroh-specific insights.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::{RwLock, Mutex};
use tracing::{debug, error, info};

use crate::networking::kad::transport::KadPeerId;

use super::{IrohError, IrohResult, MetricsConfig};

/// Enhanced metrics collector for iroh-only transport
pub struct IrohMetrics {
    /// Configuration for metrics collection
    config: MetricsConfig,
    
    /// Operation metrics by peer
    peer_metrics: Arc<RwLock<HashMap<KadPeerId, PeerMetrics>>>,
    
    /// Global transport metrics
    global_metrics: Arc<RwLock<GlobalMetrics>>,
    
    /// Performance histograms
    histograms: Arc<Mutex<PerformanceHistograms>>,
    
    /// Export state
    export_state: Arc<Mutex<ExportState>>,
}

/// Metrics for individual peers
#[derive(Debug, Clone)]
pub struct PeerMetrics {
    /// Total operations performed
    pub total_operations: u64,
    /// Successful operations
    pub successful_operations: u64,
    /// Total bytes transferred
    pub total_bytes_transferred: u64,
    /// Average latency
    pub average_latency: Duration,
    /// Last operation timestamp
    pub last_operation: Instant,
    /// Connection status
    pub connection_status: ConnectionStatus,
    /// Operation type breakdown
    pub operation_breakdown: HashMap<String, OperationStats>,
}

/// Global transport metrics
#[derive(Debug, Clone)]
pub struct GlobalMetrics {
    /// Total connections established
    pub total_connections: u64,
    /// Active connections
    pub active_connections: u64,
    /// Connection pool utilization
    pub pool_utilization: f64,
    /// Overall success rate
    pub success_rate: f64,
    /// Network throughput (operations/second)
    pub throughput: f64,
    /// Memory usage
    pub memory_usage: usize,
    /// Last update timestamp
    pub last_updated: Instant,
}

/// Connection status for metrics
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Degraded,
    Unknown,
}

/// Statistics for specific operation types
#[derive(Debug, Clone)]
pub struct OperationStats {
    pub count: u64,
    pub success_count: u64,
    pub total_latency: Duration,
    pub bytes_transferred: u64,
    pub last_performed: Instant,
}

/// Performance histograms for detailed analysis
#[derive(Debug)]
pub struct PerformanceHistograms {
    /// Latency distribution (milliseconds)
    pub latency_histogram: Histogram,
    /// Size distribution (bytes)
    pub size_histogram: Histogram,
    /// Duration distribution (seconds)
    pub duration_histogram: Histogram,
    /// Bandwidth distribution (mbps)
    pub bandwidth_histogram: Histogram,
}

/// Histogram implementation for metrics
#[derive(Debug)]
pub struct Histogram {
    buckets: Vec<f64>,
    counts: Vec<u64>,
    total_count: u64,
    sum: f64,
}

/// Export state tracking
#[derive(Debug)]
struct ExportState {
    last_export: Instant,
    export_count: u64,
    last_export_duration: Duration,
}

impl IrohMetrics {
    /// Create new iroh metrics collector
    pub async fn new(config: MetricsConfig) -> IrohResult<Self> {
        info!("Initializing iroh metrics collector");
        
        let histograms = PerformanceHistograms {
            latency_histogram: Histogram::new(config.histograms.latency_buckets.clone()),
            size_histogram: Histogram::new(config.histograms.size_buckets.clone()),
            duration_histogram: Histogram::new(config.histograms.duration_buckets.clone()),
            bandwidth_histogram: Histogram::new(config.histograms.bandwidth_buckets.clone()),
        };
        
        Ok(Self {
            config,
            peer_metrics: Arc::new(RwLock::new(HashMap::new())),
            global_metrics: Arc::new(RwLock::new(GlobalMetrics::default())),
            histograms: Arc::new(Mutex::new(histograms)),
            export_state: Arc::new(Mutex::new(ExportState {
                last_export: Instant::now(),
                export_count: 0,
                last_export_duration: Duration::ZERO,
            })),
        })
    }
    
    /// Record an operation for metrics collection
    pub async fn record_operation(
        &self,
        peer_id: &KadPeerId,
        operation_type: &str,
        latency: Duration,
        success: bool,
    ) {
        if !self.config.enabled {
            return;
        }
        
        // Update peer metrics
        if self.config.per_peer_metrics {
            let mut peer_metrics = self.peer_metrics.write().await;
            let peer_metric = peer_metrics
                .entry(peer_id.clone())
                .or_insert_with(|| PeerMetrics::new());
            
            peer_metric.record_operation(operation_type, latency, success, 0).await;
        }
        
        // Update global metrics
        {
            let mut global = self.global_metrics.write().await;
            global.record_operation(latency, success).await;
        }
        
        // Update histograms
        if self.config.performance_profiling {
            let mut histograms = self.histograms.lock().await;
            histograms.latency_histogram.record(latency.as_millis() as f64);
            histograms.duration_histogram.record(latency.as_secs_f64());
        }
    }
    
    /// Record connection event
    pub async fn record_connection_event(&self, peer_id: &KadPeerId, connected: bool) {
        if !self.config.enabled {
            return;
        }
        
        // Update peer connection status
        if self.config.per_peer_metrics {
            let mut peer_metrics = self.peer_metrics.write().await;
            if let Some(peer_metric) = peer_metrics.get_mut(peer_id) {
                peer_metric.connection_status = if connected {
                    ConnectionStatus::Connected
                } else {
                    ConnectionStatus::Disconnected
                };
            }
        }
        
        // Update global connection count
        let mut global = self.global_metrics.write().await;
        if connected {
            global.total_connections += 1;
            global.active_connections += 1;
        } else {
            global.active_connections = global.active_connections.saturating_sub(1);
        }
        global.last_updated = Instant::now();
    }
    
    /// Record data transfer metrics
    pub async fn record_data_transfer(&self, peer_id: &KadPeerId, bytes: usize) {
        if !self.config.enabled {
            return;
        }
        
        // Update peer bytes transferred
        if self.config.per_peer_metrics {
            let mut peer_metrics = self.peer_metrics.write().await;
            if let Some(peer_metric) = peer_metrics.get_mut(peer_id) {
                peer_metric.total_bytes_transferred += bytes as u64;
            }
        }
        
        // Update histograms
        if self.config.performance_profiling {
            let mut histograms = self.histograms.lock().await;
            histograms.size_histogram.record(bytes as f64);
        }
    }
    
    /// Get current metrics snapshot
    pub async fn get_metrics_snapshot(&self) -> MetricsSnapshot {
        let global = self.global_metrics.read().await.clone();
        let peer_count = self.peer_metrics.read().await.len();
        
        let histograms = if self.config.performance_profiling {
            Some(self.histograms.lock().await.get_snapshot())
        } else {
            None
        };
        
        MetricsSnapshot {
            global,
            peer_count,
            histograms,
            timestamp: Instant::now(),
        }
    }
    
    /// Collect and export metrics
    pub async fn collect_and_export(&self) -> IrohResult<()> {
        if !self.config.enabled {
            return Ok(());
        }
        
        let start_time = Instant::now();
        debug!("Collecting and exporting iroh metrics");
        
        // Get metrics snapshot
        let snapshot = self.get_metrics_snapshot().await;
        
        // Export metrics (placeholder implementation)
        self.export_metrics(snapshot).await?;
        
        // Update export state
        let export_duration = start_time.elapsed();
        let mut export_state = self.export_state.lock().await;
        export_state.last_export = Instant::now();
        export_state.export_count += 1;
        export_state.last_export_duration = export_duration;
        
        debug!("Metrics export completed in {:?}", export_duration);
        Ok(())
    }
    
    /// Export metrics to configured endpoints
    async fn export_metrics(&self, snapshot: MetricsSnapshot) -> IrohResult<()> {
        // TODO: Implement actual metrics export (Prometheus, OpenTelemetry, etc.)
        debug!("Exporting metrics snapshot: {:?}", snapshot);
        Ok(())
    }
    
    /// Clean up old metrics data
    pub async fn cleanup_old_metrics(&self) {
        let retention_threshold = Instant::now() - self.config.retention_duration;
        
        // Clean up peer metrics
        if self.config.per_peer_metrics {
            let mut peer_metrics = self.peer_metrics.write().await;
            peer_metrics.retain(|_, metric| metric.last_operation > retention_threshold);
        }
        
        debug!("Cleaned up old metrics data");
    }
    
    /// Shutdown metrics collection
    pub async fn shutdown(&self) -> IrohResult<()> {
        info!("Shutting down iroh metrics collector");
        
        // Final metrics export
        if let Err(e) = self.collect_and_export().await {
            error!("Failed final metrics export: {}", e);
        }
        
        info!("Iroh metrics collector shutdown complete");
        Ok(())
    }
}

impl PeerMetrics {
    fn new() -> Self {
        Self {
            total_operations: 0,
            successful_operations: 0,
            total_bytes_transferred: 0,
            average_latency: Duration::ZERO,
            last_operation: Instant::now(),
            connection_status: ConnectionStatus::Unknown,
            operation_breakdown: HashMap::new(),
        }
    }
    
    async fn record_operation(&mut self, operation_type: &str, latency: Duration, success: bool, bytes: u64) {
        self.total_operations += 1;
        if success {
            self.successful_operations += 1;
        }
        self.total_bytes_transferred += bytes;
        self.last_operation = Instant::now();
        
        // Update average latency with exponential moving average
        let alpha = 0.1;
        if self.total_operations == 1 {
            self.average_latency = latency;
        } else {
            self.average_latency = Duration::from_nanos(
                ((1.0 - alpha) * self.average_latency.as_nanos() as f64 +
                 alpha * latency.as_nanos() as f64) as u64
            );
        }
        
        // Update operation breakdown
        let op_stats = self.operation_breakdown
            .entry(operation_type.to_string())
            .or_insert_with(|| OperationStats {
                count: 0,
                success_count: 0,
                total_latency: Duration::ZERO,
                bytes_transferred: 0,
                last_performed: Instant::now(),
            });
        
        op_stats.count += 1;
        if success {
            op_stats.success_count += 1;
        }
        op_stats.total_latency += latency;
        op_stats.bytes_transferred += bytes;
        op_stats.last_performed = Instant::now();
    }
}

impl Default for GlobalMetrics {
    fn default() -> Self {
        Self {
            total_connections: 0,
            active_connections: 0,
            pool_utilization: 0.0,
            success_rate: 1.0,
            throughput: 0.0,
            memory_usage: 0,
            last_updated: Instant::now(),
        }
    }
}

impl GlobalMetrics {
    async fn record_operation(&mut self, latency: Duration, success: bool) {
        // Update success rate with exponential moving average
        let alpha = 0.01;
        self.success_rate = (1.0 - alpha) * self.success_rate + alpha * if success { 1.0 } else { 0.0 };
        self.last_updated = Instant::now();
    }
}

impl Histogram {
    fn new(buckets: Vec<f64>) -> Self {
        let mut sorted_buckets = buckets;
        sorted_buckets.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        Self {
            counts: vec![0; sorted_buckets.len()],
            buckets: sorted_buckets,
            total_count: 0,
            sum: 0.0,
        }
    }
    
    fn record(&mut self, value: f64) {
        self.total_count += 1;
        self.sum += value;
        
        for (i, &bucket) in self.buckets.iter().enumerate() {
            if value <= bucket {
                self.counts[i] += 1;
                break;
            }
        }
    }
    
    fn get_percentile(&self, percentile: f64) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }
        
        let target_count = (percentile * self.total_count as f64) as u64;
        let mut cumulative = 0;
        
        for (i, &count) in self.counts.iter().enumerate() {
            cumulative += count;
            if cumulative >= target_count {
                return self.buckets[i];
            }
        }
        
        self.buckets.last().copied().unwrap_or(0.0)
    }
}

impl PerformanceHistograms {
    fn get_snapshot(&self) -> HistogramSnapshot {
        HistogramSnapshot {
            latency_p50: self.latency_histogram.get_percentile(0.5),
            latency_p95: self.latency_histogram.get_percentile(0.95),
            latency_p99: self.latency_histogram.get_percentile(0.99),
            size_p50: self.size_histogram.get_percentile(0.5),
            size_p95: self.size_histogram.get_percentile(0.95),
            duration_p50: self.duration_histogram.get_percentile(0.5),
            duration_p95: self.duration_histogram.get_percentile(0.95),
            bandwidth_avg: self.bandwidth_histogram.sum / self.bandwidth_histogram.total_count as f64,
        }
    }
}

/// Snapshot of current metrics
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub global: GlobalMetrics,
    pub peer_count: usize,
    pub histograms: Option<HistogramSnapshot>,
    pub timestamp: Instant,
}

/// Snapshot of histogram metrics
#[derive(Debug, Clone)]
pub struct HistogramSnapshot {
    pub latency_p50: f64,
    pub latency_p95: f64,
    pub latency_p99: f64,
    pub size_p50: f64,
    pub size_p95: f64,
    pub duration_p50: f64,
    pub duration_p95: f64,
    pub bandwidth_avg: f64,
}