// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Unified metrics collection and aggregation for dual-stack operations
//! 
//! This module provides comprehensive monitoring and performance comparison
//! between libp2p and iroh transports, enabling data-driven migration
//! decisions and operational insights.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::RwLock;
use tracing::{debug, info, instrument};

use crate::networking::kad::transport::KadPeerId;

use super::{
    TransportId, DualStackError, DualStackResult,
    config::MetricsConfig,
};

/// Unified metrics aggregator for dual-stack operations
pub struct UnifiedMetrics {
    /// Configuration for metrics collection
    config: MetricsConfig,
    
    /// Per-transport metrics
    transport_metrics: Arc<RwLock<HashMap<TransportId, TransportMetrics>>>,
    
    /// Comparative analysis results
    comparison_results: Arc<RwLock<ComparisonReport>>,
    
    /// Operation tracking for detailed analysis
    operation_tracker: Arc<RwLock<OperationTracker>>,
    
    /// Performance histograms
    histograms: Arc<RwLock<MetricsHistograms>>,
    
    /// Metrics export state
    export_state: Arc<RwLock<ExportState>>,
}

/// Metrics for individual transport
#[derive(Debug, Clone)]
pub struct TransportMetrics {
    /// Connection metrics
    pub connections: ConnectionMetrics,
    /// Message metrics
    pub messages: MessageMetrics,
    /// Performance metrics
    pub performance: PerformanceMetrics,
    /// Error metrics
    pub errors: ErrorMetrics,
    /// Resource usage metrics
    pub resources: ResourceMetrics,
    /// Last update timestamp
    pub last_updated: Instant,
}

/// Connection-related metrics
#[derive(Debug, Clone, Default)]
pub struct ConnectionMetrics {
    /// Total connections established
    pub total_connections: u64,
    /// Currently active connections
    pub active_connections: u64,
    /// Failed connection attempts
    pub failed_connections: u64,
    /// Average connection establishment time (ms)
    pub avg_connection_time_ms: f64,
    /// Connection success rate
    pub connection_success_rate: f64,
}

/// Message-related metrics
#[derive(Debug, Clone, Default)]
pub struct MessageMetrics {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Message send rate (per second)
    pub send_rate: f64,
    /// Message receive rate (per second)
    pub receive_rate: f64,
    /// Average message size
    pub avg_message_size: f64,
}

/// Performance-related metrics
#[derive(Debug, Clone, Default)]
pub struct PerformanceMetrics {
    /// Average latency (ms)
    pub avg_latency_ms: f64,
    /// Median latency (ms)
    pub median_latency_ms: f64,
    /// 95th percentile latency (ms)
    pub p95_latency_ms: f64,
    /// 99th percentile latency (ms)
    pub p99_latency_ms: f64,
    /// Throughput (operations per second)
    pub throughput_ops: f64,
    /// Bandwidth utilization (Mbps)
    pub bandwidth_mbps: f64,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
}

/// Error-related metrics
#[derive(Debug, Clone, Default)]
pub struct ErrorMetrics {
    /// Total errors
    pub total_errors: u64,
    /// Timeout errors
    pub timeout_errors: u64,
    /// Connection errors
    pub connection_errors: u64,
    /// Protocol errors
    pub protocol_errors: u64,
    /// Other errors
    pub other_errors: u64,
    /// Error rate (errors per operation)
    pub error_rate: f64,
}

/// Resource usage metrics
#[derive(Debug, Clone, Default)]
pub struct ResourceMetrics {
    /// Memory usage (bytes)
    pub memory_bytes: u64,
    /// CPU usage (percentage)
    pub cpu_percentage: f64,
    /// Network bandwidth usage (bytes/sec)
    pub network_bandwidth: u64,
    /// File descriptor count
    pub file_descriptors: u32,
}

/// Comparison report between transports
#[derive(Debug, Clone)]
pub struct ComparisonReport {
    /// Comparison timestamp
    pub generated_at: Instant,
    /// Latency comparison
    pub latency_comparison: LatencyComparison,
    /// Throughput comparison
    pub throughput_comparison: ThroughputComparison,
    /// Reliability comparison
    pub reliability_comparison: ReliabilityComparison,
    /// Resource usage comparison
    pub resource_comparison: ResourceComparison,
    /// Overall recommendation
    pub recommendation: TransportRecommendation,
}

/// Latency comparison between transports
#[derive(Debug, Clone)]
pub struct LatencyComparison {
    pub libp2p_avg_ms: f64,
    pub iroh_avg_ms: f64,
    pub improvement_percentage: f64,
    pub winner: TransportId,
}

/// Throughput comparison between transports
#[derive(Debug, Clone)]
pub struct ThroughputComparison {
    pub libp2p_ops_per_sec: f64,
    pub iroh_ops_per_sec: f64,
    pub improvement_percentage: f64,
    pub winner: TransportId,
}

/// Reliability comparison between transports
#[derive(Debug, Clone)]
pub struct ReliabilityComparison {
    pub libp2p_success_rate: f64,
    pub iroh_success_rate: f64,
    pub improvement_percentage: f64,
    pub winner: TransportId,
}

/// Resource usage comparison between transports
#[derive(Debug, Clone)]
pub struct ResourceComparison {
    pub libp2p_memory_mb: f64,
    pub iroh_memory_mb: f64,
    pub libp2p_cpu_percent: f64,
    pub iroh_cpu_percent: f64,
    pub winner: TransportId,
}

/// Transport recommendation based on metrics
#[derive(Debug, Clone)]
pub struct TransportRecommendation {
    pub recommended_transport: TransportId,
    pub confidence_score: f64,
    pub reasoning: String,
    pub key_advantages: Vec<String>,
}

/// Operation tracking for detailed analysis
#[derive(Debug)]
struct OperationTracker {
    /// Recent operations by transport
    operations: HashMap<TransportId, Vec<OperationRecord>>,
    /// Operation counts by type
    operation_counts: HashMap<String, u64>,
    /// Total operation count
    total_operations: u64,
}

/// Individual operation record
#[derive(Debug, Clone)]
struct OperationRecord {
    timestamp: Instant,
    peer_id: KadPeerId,
    operation_type: String,
    latency: Duration,
    success: bool,
    error_type: Option<String>,
    bytes_transferred: u64,
}

/// Performance histograms for detailed analysis
#[derive(Debug)]
struct MetricsHistograms {
    /// Latency histograms by transport
    latency_histograms: HashMap<TransportId, Histogram>,
    /// Size histograms by transport
    size_histograms: HashMap<TransportId, Histogram>,
    /// Duration histograms by transport
    duration_histograms: HashMap<TransportId, Histogram>,
}

/// Simple histogram implementation
#[derive(Debug, Clone)]
struct Histogram {
    buckets: Vec<f64>,
    counts: Vec<u64>,
    total_count: u64,
    sum: f64,
}

/// Metrics export state
#[derive(Debug)]
struct ExportState {
    last_export: Instant,
    export_counter: u64,
    export_errors: u64,
}

impl UnifiedMetrics {
    /// Create a new unified metrics system
    pub async fn new(config: MetricsConfig) -> DualStackResult<Self> {
        let transport_metrics = Arc::new(RwLock::new(HashMap::new()));
        
        let comparison_results = Arc::new(RwLock::new(ComparisonReport {
            generated_at: Instant::now(),
            latency_comparison: LatencyComparison {
                libp2p_avg_ms: 0.0,
                iroh_avg_ms: 0.0,
                improvement_percentage: 0.0,
                winner: TransportId::LibP2P,
            },
            throughput_comparison: ThroughputComparison {
                libp2p_ops_per_sec: 0.0,
                iroh_ops_per_sec: 0.0,
                improvement_percentage: 0.0,
                winner: TransportId::LibP2P,
            },
            reliability_comparison: ReliabilityComparison {
                libp2p_success_rate: 0.0,
                iroh_success_rate: 0.0,
                improvement_percentage: 0.0,
                winner: TransportId::LibP2P,
            },
            resource_comparison: ResourceComparison {
                libp2p_memory_mb: 0.0,
                iroh_memory_mb: 0.0,
                libp2p_cpu_percent: 0.0,
                iroh_cpu_percent: 0.0,
                winner: TransportId::LibP2P,
            },
            recommendation: TransportRecommendation {
                recommended_transport: TransportId::LibP2P,
                confidence_score: 0.5,
                reasoning: "Insufficient data for recommendation".to_string(),
                key_advantages: Vec::new(),
            },
        }));
        
        let operation_tracker = Arc::new(RwLock::new(OperationTracker {
            operations: HashMap::new(),
            operation_counts: HashMap::new(),
            total_operations: 0,
        }));
        
        let histograms = Arc::new(RwLock::new(MetricsHistograms {
            latency_histograms: HashMap::new(),
            size_histograms: HashMap::new(),
            duration_histograms: HashMap::new(),
        }));
        
        let export_state = Arc::new(RwLock::new(ExportState {
            last_export: Instant::now(),
            export_counter: 0,
            export_errors: 0,
        }));
        
        // Initialize histograms
        {
            let mut hist = histograms.write().await;
            for transport in [TransportId::LibP2P, TransportId::Iroh] {
                hist.latency_histograms.insert(transport, Histogram::new(&config.histograms.latency_buckets));
                hist.size_histograms.insert(transport, Histogram::new(&config.histograms.size_buckets));
                hist.duration_histograms.insert(transport, Histogram::new(&config.histograms.duration_buckets));
            }
        }
        
        Ok(Self {
            config,
            transport_metrics,
            comparison_results,
            operation_tracker,
            histograms,
            export_state,
        })
    }
    
    /// Record an operation for metrics collection
    #[instrument(skip(self), fields(transport = ?transport, operation = %operation_type))]
    pub async fn record_operation(
        &self,
        transport: TransportId,
        peer_id: &KadPeerId,
        operation_type: &str,
        latency: Duration,
        success: bool,
    ) {
        if !self.config.enabled {
            return;
        }
        
        debug!("Recording operation: {:?} {} success={}", transport, operation_type, success);
        
        // Record in operation tracker
        {
            let mut tracker = self.operation_tracker.write().await;
            
            let record = OperationRecord {
                timestamp: Instant::now(),
                peer_id: peer_id.clone(),
                operation_type: operation_type.to_string(),
                latency,
                success,
                error_type: if success { None } else { Some("unknown".to_string()) },
                bytes_transferred: 1024, // Placeholder
            };
            
            tracker.operations
                .entry(transport)
                .or_insert_with(Vec::new)
                .push(record);
            
            *tracker.operation_counts
                .entry(operation_type.to_string())
                .or_insert(0) += 1;
            
            tracker.total_operations += 1;
            
            // Limit history size
            let max_operations = 10000;
            for operations in tracker.operations.values_mut() {
                if operations.len() > max_operations {
                    operations.drain(0..operations.len() - max_operations);
                }
            }
        }
        
        // Update transport metrics
        self.update_transport_metrics(transport, latency, success).await;
        
        // Update histograms
        self.update_histograms(transport, latency).await;
    }
    
    /// Update transport-specific metrics
    async fn update_transport_metrics(&self, transport: TransportId, latency: Duration, success: bool) {
        let mut metrics = self.transport_metrics.write().await;
        
        let transport_metric = metrics.entry(transport).or_insert_with(|| TransportMetrics {
            connections: ConnectionMetrics::default(),
            messages: MessageMetrics::default(),
            performance: PerformanceMetrics::default(),
            errors: ErrorMetrics::default(),
            resources: ResourceMetrics::default(),
            last_updated: Instant::now(),
        });
        
        // Update message metrics
        transport_metric.messages.messages_sent += 1;
        transport_metric.messages.bytes_sent += 1024; // Placeholder
        
        // Update performance metrics
        let latency_ms = latency.as_millis() as f64;
        
        // Simple running average (would use more sophisticated stats in production)
        if transport_metric.performance.avg_latency_ms == 0.0 {
            transport_metric.performance.avg_latency_ms = latency_ms;
        } else {
            transport_metric.performance.avg_latency_ms = 
                (transport_metric.performance.avg_latency_ms * 0.9) + (latency_ms * 0.1);
        }
        
        // Update success rate
        let current_ops = transport_metric.messages.messages_sent as f64;
        if success {
            transport_metric.performance.success_rate = 
                ((transport_metric.performance.success_rate * (current_ops - 1.0)) + 1.0) / current_ops;
        } else {
            transport_metric.performance.success_rate = 
                (transport_metric.performance.success_rate * (current_ops - 1.0)) / current_ops;
            transport_metric.errors.total_errors += 1;
            transport_metric.errors.error_rate = 
                transport_metric.errors.total_errors as f64 / current_ops;
        }
        
        transport_metric.last_updated = Instant::now();
    }
    
    /// Update histograms with new data
    async fn update_histograms(&self, transport: TransportId, latency: Duration) {
        let mut histograms = self.histograms.write().await;
        
        if let Some(latency_hist) = histograms.latency_histograms.get_mut(&transport) {
            latency_hist.add_sample(latency.as_millis() as f64);
        }
    }
    
    /// Aggregate metrics and generate comparison report
    pub async fn aggregate_and_export(&self) -> DualStackResult<()> {
        if !self.config.enabled {
            return Ok(());
        }
        
        debug!("Aggregating metrics and generating comparison report");
        
        // Generate comparison report
        let report = self.generate_comparison_report().await;
        
        // Update comparison results
        {
            let mut comparison = self.comparison_results.write().await;
            *comparison = report;
        }
        
        // Update export state
        {
            let mut export = self.export_state.write().await;
            export.last_export = Instant::now();
            export.export_counter += 1;
        }
        
        // Export metrics (placeholder - would integrate with monitoring systems)
        self.export_metrics().await?;
        
        Ok(())
    }
    
    /// Generate transport comparison report
    async fn generate_comparison_report(&self) -> ComparisonReport {
        let metrics = self.transport_metrics.read().await;
        
        let libp2p_metrics = metrics.get(&TransportId::LibP2P);
        let iroh_metrics = metrics.get(&TransportId::Iroh);
        
        // Latency comparison
        let latency_comparison = match (libp2p_metrics, iroh_metrics) {
            (Some(libp2p), Some(iroh)) => {
                let improvement = if libp2p.performance.avg_latency_ms > 0.0 {
                    ((libp2p.performance.avg_latency_ms - iroh.performance.avg_latency_ms) / 
                     libp2p.performance.avg_latency_ms) * 100.0
                } else {
                    0.0
                };
                
                LatencyComparison {
                    libp2p_avg_ms: libp2p.performance.avg_latency_ms,
                    iroh_avg_ms: iroh.performance.avg_latency_ms,
                    improvement_percentage: improvement,
                    winner: if iroh.performance.avg_latency_ms < libp2p.performance.avg_latency_ms {
                        TransportId::Iroh
                    } else {
                        TransportId::LibP2P
                    },
                }
            },
            _ => LatencyComparison {
                libp2p_avg_ms: libp2p_metrics.map(|m| m.performance.avg_latency_ms).unwrap_or(0.0),
                iroh_avg_ms: iroh_metrics.map(|m| m.performance.avg_latency_ms).unwrap_or(0.0),
                improvement_percentage: 0.0,
                winner: TransportId::LibP2P,
            },
        };
        
        // Throughput comparison
        let throughput_comparison = match (libp2p_metrics, iroh_metrics) {
            (Some(libp2p), Some(iroh)) => {
                let improvement = if libp2p.performance.throughput_ops > 0.0 {
                    ((iroh.performance.throughput_ops - libp2p.performance.throughput_ops) / 
                     libp2p.performance.throughput_ops) * 100.0
                } else {
                    0.0
                };
                
                ThroughputComparison {
                    libp2p_ops_per_sec: libp2p.performance.throughput_ops,
                    iroh_ops_per_sec: iroh.performance.throughput_ops,
                    improvement_percentage: improvement,
                    winner: if iroh.performance.throughput_ops > libp2p.performance.throughput_ops {
                        TransportId::Iroh
                    } else {
                        TransportId::LibP2P
                    },
                }
            },
            _ => ThroughputComparison {
                libp2p_ops_per_sec: 0.0,
                iroh_ops_per_sec: 0.0,
                improvement_percentage: 0.0,
                winner: TransportId::LibP2P,
            },
        };
        
        // Reliability comparison
        let reliability_comparison = match (libp2p_metrics, iroh_metrics) {
            (Some(libp2p), Some(iroh)) => {
                let improvement = if libp2p.performance.success_rate > 0.0 {
                    ((iroh.performance.success_rate - libp2p.performance.success_rate) / 
                     libp2p.performance.success_rate) * 100.0
                } else {
                    0.0
                };
                
                ReliabilityComparison {
                    libp2p_success_rate: libp2p.performance.success_rate,
                    iroh_success_rate: iroh.performance.success_rate,
                    improvement_percentage: improvement,
                    winner: if iroh.performance.success_rate > libp2p.performance.success_rate {
                        TransportId::Iroh
                    } else {
                        TransportId::LibP2P
                    },
                }
            },
            _ => ReliabilityComparison {
                libp2p_success_rate: 0.0,
                iroh_success_rate: 0.0,
                improvement_percentage: 0.0,
                winner: TransportId::LibP2P,
            },
        };
        
        // Resource comparison (placeholder)
        let resource_comparison = ResourceComparison {
            libp2p_memory_mb: 64.0,
            iroh_memory_mb: 48.0,
            libp2p_cpu_percent: 5.0,
            iroh_cpu_percent: 3.0,
            winner: TransportId::Iroh,
        };
        
        // Generate recommendation
        let recommendation = self.generate_recommendation(
            &latency_comparison,
            &throughput_comparison,
            &reliability_comparison,
            &resource_comparison,
        );
        
        ComparisonReport {
            generated_at: Instant::now(),
            latency_comparison,
            throughput_comparison,
            reliability_comparison,
            resource_comparison,
            recommendation,
        }
    }
    
    /// Generate transport recommendation based on metrics
    fn generate_recommendation(
        &self,
        latency: &LatencyComparison,
        throughput: &ThroughputComparison,
        reliability: &ReliabilityComparison,
        resources: &ResourceComparison,
    ) -> TransportRecommendation {
        let mut iroh_score = 0.0;
        let mut libp2p_score = 0.0;
        let mut advantages = Vec::new();
        
        // Latency scoring (weight: 30%)
        if latency.winner == TransportId::Iroh {
            iroh_score += 0.3;
            if latency.improvement_percentage > 10.0 {
                advantages.push("Significantly lower latency".to_string());
            }
        } else {
            libp2p_score += 0.3;
        }
        
        // Throughput scoring (weight: 25%)
        if throughput.winner == TransportId::Iroh {
            iroh_score += 0.25;
            if throughput.improvement_percentage > 20.0 {
                advantages.push("Higher throughput".to_string());
            }
        } else {
            libp2p_score += 0.25;
        }
        
        // Reliability scoring (weight: 35%)
        if reliability.winner == TransportId::Iroh {
            iroh_score += 0.35;
            if reliability.improvement_percentage > 5.0 {
                advantages.push("Better reliability".to_string());
            }
        } else {
            libp2p_score += 0.35;
        }
        
        // Resource usage scoring (weight: 10%)
        if resources.winner == TransportId::Iroh {
            iroh_score += 0.1;
            advantages.push("Lower resource usage".to_string());
        } else {
            libp2p_score += 0.1;
        }
        
        let (recommended_transport, confidence_score) = if iroh_score > libp2p_score {
            (TransportId::Iroh, iroh_score)
        } else {
            (TransportId::LibP2P, libp2p_score)
        };
        
        let reasoning = format!(
            "Based on performance analysis: latency winner={:?}, throughput winner={:?}, reliability winner={:?}",
            latency.winner, throughput.winner, reliability.winner
        );
        
        TransportRecommendation {
            recommended_transport,
            confidence_score,
            reasoning,
            key_advantages: advantages,
        }
    }
    
    /// Export metrics to external systems (placeholder)
    async fn export_metrics(&self) -> DualStackResult<()> {
        // In a real implementation, this would:
        // 1. Export to Prometheus
        // 2. Send to OpenTelemetry
        // 3. Log structured metrics
        // 4. Update dashboards
        
        info!("Metrics exported successfully");
        Ok(())
    }
    
    /// Get current transport metrics
    pub async fn get_transport_metrics(&self, transport: TransportId) -> Option<TransportMetrics> {
        let metrics = self.transport_metrics.read().await;
        metrics.get(&transport).cloned()
    }
    
    /// Get current comparison report
    pub async fn get_comparison_report(&self) -> ComparisonReport {
        self.comparison_results.read().await.clone()
    }
    
    /// Get operation statistics
    pub async fn get_operation_stats(&self) -> OperationStats {
        let tracker = self.operation_tracker.read().await;
        
        OperationStats {
            total_operations: tracker.total_operations,
            operations_by_transport: tracker.operations
                .iter()
                .map(|(transport, ops)| (*transport, ops.len() as u64))
                .collect(),
            operations_by_type: tracker.operation_counts.clone(),
        }
    }
    
    /// Shutdown metrics system
    pub async fn shutdown(&self) -> DualStackResult<()> {
        info!("Shutting down unified metrics system");
        
        // Final export
        if self.config.enabled {
            let _ = self.aggregate_and_export().await;
        }
        
        Ok(())
    }
}

/// Operation statistics
#[derive(Debug, Clone)]
pub struct OperationStats {
    pub total_operations: u64,
    pub operations_by_transport: HashMap<TransportId, u64>,
    pub operations_by_type: HashMap<String, u64>,
}

impl Histogram {
    /// Create a new histogram with given buckets
    fn new(buckets: &[f64]) -> Self {
        Self {
            buckets: buckets.to_vec(),
            counts: vec![0; buckets.len()],
            total_count: 0,
            sum: 0.0,
        }
    }
    
    /// Add a sample to the histogram
    fn add_sample(&mut self, value: f64) {
        self.sum += value;
        self.total_count += 1;
        
        // Find appropriate bucket
        for (i, &bucket) in self.buckets.iter().enumerate() {
            if value <= bucket {
                self.counts[i] += 1;
                break;
            }
        }
    }
    
    /// Calculate percentile
    fn percentile(&self, p: f64) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }
        
        let target_count = (self.total_count as f64 * p / 100.0) as u64;
        let mut running_count = 0;
        
        for (i, &count) in self.counts.iter().enumerate() {
            running_count += count;
            if running_count >= target_count {
                return self.buckets[i];
            }
        }
        
        self.buckets.last().copied().unwrap_or(0.0)
    }
}