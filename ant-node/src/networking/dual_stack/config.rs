// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Configuration management for dual-stack networking
//! 
//! This module provides comprehensive configuration for the dual-stack system,
//! enabling fine-grained control over transport selection, migration policies,
//! failover behavior, and performance optimization.

use std::time::Duration;
use serde::{Deserialize, Serialize};

use super::{TransportId, constants::*};

/// Comprehensive configuration for dual-stack networking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualStackConfig {
    /// Transport selection and routing configuration
    pub routing: RoutingConfig,
    
    /// Migration orchestration settings
    pub migration: MigrationConfig,
    
    /// Failover and redundancy configuration
    pub failover: FailoverConfig,
    
    /// Performance monitoring and optimization
    pub performance: PerformanceConfig,
    
    /// Metrics collection and aggregation
    pub metrics: MetricsConfig,
    
    /// Advanced operational settings
    pub advanced: AdvancedConfig,
}

/// Transport routing and selection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Default transport for new connections
    pub default_transport: TransportId,
    
    /// Enable intelligent routing based on performance
    pub enable_intelligent_routing: bool,
    
    /// Timeout for routing decisions
    pub routing_timeout: Duration,
    
    /// Prefer modern transport (iroh) when capabilities are equal
    pub prefer_modern_transport: bool,
    
    /// Load balancing strategy
    pub load_balancing: LoadBalancingStrategy,
    
    /// Per-transport configuration overrides
    pub transport_overrides: TransportOverrides,
}

/// Load balancing strategies for dual-stack operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadBalancingStrategy {
    /// Round-robin between available transports
    RoundRobin,
    
    /// Route to least loaded transport
    LeastLoaded,
    
    /// Route based on performance metrics
    PerformanceBased,
    
    /// Weighted distribution (libp2p_weight, iroh_weight)
    Weighted { libp2p_weight: f32, iroh_weight: f32 },
    
    /// Prefer specific transport unless unavailable
    PreferredWithFallback { preferred: TransportId },
}

/// Per-transport configuration overrides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportOverrides {
    /// libp2p-specific settings
    pub libp2p: TransportSettings,
    
    /// iroh-specific settings
    pub iroh: TransportSettings,
}

/// Settings for individual transports
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportSettings {
    /// Enable this transport
    pub enabled: bool,
    
    /// Maximum concurrent connections
    pub max_connections: usize,
    
    /// Connection timeout
    pub connection_timeout: Duration,
    
    /// Request timeout
    pub request_timeout: Duration,
    
    /// Maximum retries for failed operations
    pub max_retries: u32,
    
    /// Backoff strategy for retries
    pub retry_backoff: BackoffStrategy,
}

/// Backoff strategies for retry operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed { delay: Duration },
    
    /// Exponential backoff with optional jitter
    Exponential { initial: Duration, max: Duration, jitter: bool },
    
    /// Linear increase in delay
    Linear { initial: Duration, increment: Duration },
}

/// Migration orchestration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationConfig {
    /// Enable gradual migration from libp2p to iroh
    pub enable_migration: bool,
    
    /// Current migration percentage (0.0 = all libp2p, 1.0 = all iroh)
    pub migration_percentage: f32,
    
    /// Migration strategy
    pub strategy: MigrationStrategy,
    
    /// Rollout velocity (percentage increase per interval)
    pub rollout_velocity: f32,
    
    /// Rollout interval
    pub rollout_interval: Duration,
    
    /// Automatic rollback triggers
    pub rollback_triggers: RollbackTriggers,
    
    /// Canary deployment settings
    pub canary: CanaryConfig,
}

/// Migration strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MigrationStrategy {
    /// Gradual percentage-based rollout
    Percentage,
    
    /// Cohort-based testing (divide peers into groups)
    Cohort { total_cohorts: u32, active_cohorts: u32 },
    
    /// Geographic rollout (region by region)
    Geographic { regions: Vec<String> },
    
    /// Feature flag controlled
    FeatureFlag { flag_name: String },
}

/// Automatic rollback triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackTriggers {
    /// Enable automatic rollback
    pub enabled: bool,
    
    /// Error rate threshold (triggers rollback)
    pub error_rate_threshold: f32,
    
    /// Latency degradation threshold (percentage increase)
    pub latency_degradation_threshold: f32,
    
    /// Connection failure threshold
    pub connection_failure_threshold: f32,
    
    /// Evaluation window for triggers
    pub evaluation_window: Duration,
}

/// Canary deployment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryConfig {
    /// Enable canary deployments
    pub enabled: bool,
    
    /// Percentage of traffic for canary
    pub canary_percentage: f32,
    
    /// Canary evaluation duration
    pub evaluation_duration: Duration,
    
    /// Success criteria for canary promotion
    pub success_criteria: CanarySuccessCriteria,
}

/// Success criteria for canary promotion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanarySuccessCriteria {
    /// Minimum success rate required
    pub min_success_rate: f32,
    
    /// Maximum acceptable latency increase
    pub max_latency_increase: f32,
    
    /// Minimum number of operations for statistical significance
    pub min_operations: u64,
}

/// Failover and redundancy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    /// Enable automatic failover
    pub enabled: bool,
    
    /// Failover timeout
    pub timeout: Duration,
    
    /// Health check configuration
    pub health_check: HealthCheckConfig,
    
    /// Circuit breaker settings
    pub circuit_breaker: CircuitBreakerConfig,
    
    /// Retry policy for failed operations
    pub retry_policy: RetryPolicyConfig,
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Health check interval
    pub interval: Duration,
    
    /// Health check timeout
    pub timeout: Duration,
    
    /// Number of consecutive failures before marking unhealthy
    pub failure_threshold: u32,
    
    /// Number of consecutive successes before marking healthy
    pub recovery_threshold: u32,
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Enable circuit breaker
    pub enabled: bool,
    
    /// Failure rate threshold to open circuit
    pub failure_rate_threshold: f32,
    
    /// Minimum number of requests before calculating failure rate
    pub min_requests: u32,
    
    /// How long to wait before trying to close circuit
    pub recovery_timeout: Duration,
    
    /// Half-open state request limit
    pub half_open_requests: u32,
}

/// Retry policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicyConfig {
    /// Maximum number of retries
    pub max_retries: u32,
    
    /// Initial retry delay
    pub initial_delay: Duration,
    
    /// Maximum retry delay
    pub max_delay: Duration,
    
    /// Backoff multiplier
    pub backoff_multiplier: f32,
    
    /// Add jitter to prevent thundering herd
    pub jitter: bool,
}

/// Performance monitoring and optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Enable performance tracking
    pub enabled: bool,
    
    /// Performance history size
    pub history_size: usize,
    
    /// Performance evaluation interval
    pub evaluation_interval: Duration,
    
    /// Latency optimization settings
    pub latency: LatencyConfig,
    
    /// Bandwidth optimization settings
    pub bandwidth: BandwidthConfig,
    
    /// Connection optimization settings
    pub connection: ConnectionConfig,
}

/// Latency optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyConfig {
    /// Target latency (prefer transports under this threshold)
    pub target_ms: f64,
    
    /// Latency degradation threshold
    pub degradation_threshold: f64,
    
    /// Enable latency-based routing
    pub enable_routing: bool,
}

/// Bandwidth optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthConfig {
    /// Target bandwidth utilization
    pub target_utilization: f32,
    
    /// Enable bandwidth-based load balancing
    pub enable_load_balancing: bool,
    
    /// Large transfer threshold (prefer iroh for larger transfers)
    pub large_transfer_threshold: usize,
}

/// Connection optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Connection pool settings
    pub pool: ConnectionPoolConfig,
    
    /// Connection reuse settings
    pub reuse: ConnectionReuseConfig,
}

/// Connection pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    /// Maximum pool size per transport
    pub max_size: usize,
    
    /// Minimum pool size per transport
    pub min_size: usize,
    
    /// Connection idle timeout
    pub idle_timeout: Duration,
    
    /// Pool cleanup interval
    pub cleanup_interval: Duration,
}

/// Connection reuse configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionReuseConfig {
    /// Enable connection reuse
    pub enabled: bool,
    
    /// Maximum reuse count per connection
    pub max_reuse_count: u32,
    
    /// Connection lifetime
    pub max_lifetime: Duration,
}

/// Metrics collection and aggregation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,
    
    /// Metrics export interval
    pub export_interval: Duration,
    
    /// Metrics retention duration
    pub retention_duration: Duration,
    
    /// Enable detailed per-peer metrics
    pub per_peer_metrics: bool,
    
    /// Enable transport comparison metrics
    pub comparison_metrics: bool,
    
    /// Histogram bucket configuration
    pub histograms: HistogramConfig,
}

/// Histogram configuration for metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramConfig {
    /// Latency histogram buckets (milliseconds)
    pub latency_buckets: Vec<f64>,
    
    /// Size histogram buckets (bytes)
    pub size_buckets: Vec<f64>,
    
    /// Duration histogram buckets (seconds)
    pub duration_buckets: Vec<f64>,
}

/// Advanced operational configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedConfig {
    /// Enable experimental features
    pub experimental_features: bool,
    
    /// Detailed logging configuration
    pub logging: LoggingConfig,
    
    /// Feature flags for runtime control
    pub feature_flags: FeatureFlagsConfig,
    
    /// Resource limits
    pub limits: ResourceLimitsConfig,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level for dual-stack operations
    pub level: String,
    
    /// Enable structured logging
    pub structured: bool,
    
    /// Enable performance logging
    pub performance: bool,
    
    /// Enable routing decision logging
    pub routing_decisions: bool,
}

/// Feature flags configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagsConfig {
    /// Enable intelligent routing
    pub intelligent_routing: bool,
    
    /// Enable automatic migration
    pub auto_migration: bool,
    
    /// Enable peer affinity learning
    pub peer_affinity: bool,
    
    /// Enable predictive routing
    pub predictive_routing: bool,
}

/// Resource limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimitsConfig {
    /// Maximum memory usage (bytes)
    pub max_memory_bytes: usize,
    
    /// Maximum concurrent operations
    pub max_concurrent_operations: usize,
    
    /// Maximum cached entries
    pub max_cache_entries: usize,
    
    /// Maximum metrics history
    pub max_metrics_history: usize,
}

impl Default for DualStackConfig {
    fn default() -> Self {
        Self {
            routing: RoutingConfig::default(),
            migration: MigrationConfig::default(),
            failover: FailoverConfig::default(),
            performance: PerformanceConfig::default(),
            metrics: MetricsConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            default_transport: TransportId::LibP2P, // Conservative default
            enable_intelligent_routing: true,
            routing_timeout: DEFAULT_ROUTING_TIMEOUT,
            prefer_modern_transport: true,
            load_balancing: LoadBalancingStrategy::PerformanceBased,
            transport_overrides: TransportOverrides::default(),
        }
    }
}

impl Default for TransportOverrides {
    fn default() -> Self {
        Self {
            libp2p: TransportSettings {
                enabled: true,
                max_connections: 1000,
                connection_timeout: Duration::from_secs(10),
                request_timeout: Duration::from_secs(30),
                max_retries: 3,
                retry_backoff: BackoffStrategy::Exponential {
                    initial: Duration::from_millis(100),
                    max: Duration::from_secs(30),
                    jitter: true,
                },
            },
            iroh: TransportSettings {
                enabled: false, // Disabled by default for safety
                max_connections: 1000,
                connection_timeout: Duration::from_secs(5),
                request_timeout: Duration::from_secs(30),
                max_retries: 3,
                retry_backoff: BackoffStrategy::Exponential {
                    initial: Duration::from_millis(50),
                    max: Duration::from_secs(15),
                    jitter: true,
                },
            },
        }
    }
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            enable_migration: false, // Disabled by default
            migration_percentage: DEFAULT_MIGRATION_PERCENTAGE,
            strategy: MigrationStrategy::Percentage,
            rollout_velocity: 0.01, // 1% per interval
            rollout_interval: Duration::from_hours(1),
            rollback_triggers: RollbackTriggers::default(),
            canary: CanaryConfig::default(),
        }
    }
}

impl Default for RollbackTriggers {
    fn default() -> Self {
        Self {
            enabled: true,
            error_rate_threshold: 0.05, // 5% error rate
            latency_degradation_threshold: 0.20, // 20% latency increase
            connection_failure_threshold: 0.10, // 10% connection failures
            evaluation_window: Duration::from_minutes(10),
        }
    }
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            canary_percentage: 0.01, // 1% canary traffic
            evaluation_duration: Duration::from_minutes(30),
            success_criteria: CanarySuccessCriteria {
                min_success_rate: 0.95,
                max_latency_increase: 0.10,
                min_operations: 100,
            },
        }
    }
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout: DEFAULT_FAILOVER_TIMEOUT,
            health_check: HealthCheckConfig::default(),
            circuit_breaker: CircuitBreakerConfig::default(),
            retry_policy: RetryPolicyConfig::default(),
        }
    }
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_HEALTH_CHECK_INTERVAL,
            timeout: Duration::from_secs(5),
            failure_threshold: 3,
            recovery_threshold: 2,
        }
    }
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            failure_rate_threshold: 0.5, // 50% failure rate
            min_requests: 10,
            recovery_timeout: Duration::from_secs(30),
            half_open_requests: 5,
        }
    }
}

impl Default for RetryPolicyConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_size: MAX_PERFORMANCE_HISTORY,
            evaluation_interval: Duration::from_minutes(5),
            latency: LatencyConfig::default(),
            bandwidth: BandwidthConfig::default(),
            connection: ConnectionConfig::default(),
        }
    }
}

impl Default for LatencyConfig {
    fn default() -> Self {
        Self {
            target_ms: 100.0,
            degradation_threshold: 0.20,
            enable_routing: true,
        }
    }
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            target_utilization: 0.80,
            enable_load_balancing: true,
            large_transfer_threshold: 1024 * 1024, // 1MB
        }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            pool: ConnectionPoolConfig::default(),
            reuse: ConnectionReuseConfig::default(),
        }
    }
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_size: 100,
            min_size: 10,
            idle_timeout: Duration::from_minutes(5),
            cleanup_interval: Duration::from_minutes(1),
        }
    }
}

impl Default for ConnectionReuseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_reuse_count: 100,
            max_lifetime: Duration::from_hours(1),
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            export_interval: Duration::from_secs(60),
            retention_duration: Duration::from_hours(24),
            per_peer_metrics: false, // Expensive, disabled by default
            comparison_metrics: true,
            histograms: HistogramConfig::default(),
        }
    }
}

impl Default for HistogramConfig {
    fn default() -> Self {
        Self {
            latency_buckets: vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0],
            size_buckets: vec![100.0, 1000.0, 10000.0, 100000.0, 1000000.0, 10000000.0],
            duration_buckets: vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0],
        }
    }
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            experimental_features: false,
            logging: LoggingConfig::default(),
            feature_flags: FeatureFlagsConfig::default(),
            limits: ResourceLimitsConfig::default(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            structured: true,
            performance: false,
            routing_decisions: false,
        }
    }
}

impl Default for FeatureFlagsConfig {
    fn default() -> Self {
        Self {
            intelligent_routing: true,
            auto_migration: false,
            peer_affinity: true,
            predictive_routing: false,
        }
    }
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_concurrent_operations: 1000,
            max_cache_entries: DEFAULT_AFFINITY_CACHE_SIZE,
            max_metrics_history: MAX_PERFORMANCE_HISTORY,
        }
    }
}

/// Builder pattern for DualStackConfig
pub struct DualStackConfigBuilder {
    config: DualStackConfig,
}

impl DualStackConfigBuilder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: DualStackConfig::default(),
        }
    }
    
    /// Configure routing settings
    pub fn with_routing<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut RoutingConfig),
    {
        configure(&mut self.config.routing);
        self
    }
    
    /// Configure migration settings
    pub fn with_migration<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut MigrationConfig),
    {
        configure(&mut self.config.migration);
        self
    }
    
    /// Configure failover settings
    pub fn with_failover<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut FailoverConfig),
    {
        configure(&mut self.config.failover);
        self
    }
    
    /// Configure performance settings
    pub fn with_performance<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut PerformanceConfig),
    {
        configure(&mut self.config.performance);
        self
    }
    
    /// Configure metrics settings
    pub fn with_metrics<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut MetricsConfig),
    {
        configure(&mut self.config.metrics);
        self
    }
    
    /// Configure advanced settings
    pub fn with_advanced<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut AdvancedConfig),
    {
        configure(&mut self.config.advanced);
        self
    }
    
    /// Build the final configuration
    pub fn build(self) -> DualStackConfig {
        self.config
    }
}

impl DualStackConfig {
    /// Create a configuration builder
    pub fn builder() -> DualStackConfigBuilder {
        DualStackConfigBuilder::new()
    }
    
    /// Create a local development configuration
    pub fn local_development() -> Self {
        Self::builder()
            .with_routing(|routing| {
                routing.default_transport = TransportId::LibP2P;
                routing.enable_intelligent_routing = false;
                routing.prefer_modern_transport = false;
            })
            .with_migration(|migration| {
                migration.enable_migration = false;
            })
            .with_failover(|failover| {
                failover.enabled = false;
            })
            .with_metrics(|metrics| {
                metrics.per_peer_metrics = true; // OK for local dev
                metrics.export_interval = Duration::from_secs(10);
            })
            .with_advanced(|advanced| {
                advanced.experimental_features = true;
                advanced.logging.level = "debug".to_string();
                advanced.logging.routing_decisions = true;
            })
            .build()
    }
    
    /// Create a production configuration
    pub fn production() -> Self {
        Self::builder()
            .with_routing(|routing| {
                routing.default_transport = TransportId::LibP2P; // Conservative
                routing.enable_intelligent_routing = true;
                routing.prefer_modern_transport = true;
                routing.load_balancing = LoadBalancingStrategy::PerformanceBased;
            })
            .with_migration(|migration| {
                migration.enable_migration = true;
                migration.migration_percentage = 0.01; // Very conservative
                migration.rollout_velocity = 0.005; // 0.5% per hour
                migration.rollback_triggers.enabled = true;
                migration.canary.enabled = true;
            })
            .with_failover(|failover| {
                failover.enabled = true;
                failover.circuit_breaker.enabled = true;
            })
            .with_performance(|performance| {
                performance.enabled = true;
                performance.history_size = 10000;
            })
            .with_metrics(|metrics| {
                metrics.enabled = true;
                metrics.per_peer_metrics = false; // Too expensive for production
                metrics.comparison_metrics = true;
            })
            .build()
    }
    
    /// Create a testing configuration for A/B experiments
    pub fn testing() -> Self {
        Self::builder()
            .with_routing(|routing| {
                routing.load_balancing = LoadBalancingStrategy::Weighted {
                    libp2p_weight: 0.5,
                    iroh_weight: 0.5,
                };
            })
            .with_migration(|migration| {
                migration.enable_migration = true;
                migration.strategy = MigrationStrategy::Cohort {
                    total_cohorts: 10,
                    active_cohorts: 5,
                };
                migration.canary.enabled = true;
                migration.canary.canary_percentage = 0.10; // 10% for testing
            })
            .with_metrics(|metrics| {
                metrics.enabled = true;
                metrics.per_peer_metrics = true;
                metrics.comparison_metrics = true;
                metrics.export_interval = Duration::from_secs(30);
            })
            .with_advanced(|advanced| {
                advanced.experimental_features = true;
                advanced.logging.performance = true;
                advanced.logging.routing_decisions = true;
            })
            .build()
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate migration percentage
        if self.migration.migration_percentage < 0.0 || self.migration.migration_percentage > 1.0 {
            return Err("Migration percentage must be between 0.0 and 1.0".to_string());
        }
        
        // Validate rollout velocity
        if self.migration.rollout_velocity < 0.0 || self.migration.rollout_velocity > 1.0 {
            return Err("Rollout velocity must be between 0.0 and 1.0".to_string());
        }
        
        // Validate canary percentage
        if self.migration.canary.canary_percentage < 0.0 || self.migration.canary.canary_percentage > 1.0 {
            return Err("Canary percentage must be between 0.0 and 1.0".to_string());
        }
        
        // Validate threshold values
        if self.migration.rollback_triggers.error_rate_threshold < 0.0 || 
           self.migration.rollback_triggers.error_rate_threshold > 1.0 {
            return Err("Error rate threshold must be between 0.0 and 1.0".to_string());
        }
        
        // Validate weighted load balancing
        if let LoadBalancingStrategy::Weighted { libp2p_weight, iroh_weight } = self.routing.load_balancing {
            if libp2p_weight < 0.0 || iroh_weight < 0.0 {
                return Err("Load balancing weights must be non-negative".to_string());
            }
            if libp2p_weight + iroh_weight == 0.0 {
                return Err("At least one load balancing weight must be positive".to_string());
            }
        }
        
        // Validate resource limits
        if self.advanced.limits.max_memory_bytes == 0 {
            return Err("Maximum memory bytes must be positive".to_string());
        }
        
        if self.advanced.limits.max_concurrent_operations == 0 {
            return Err("Maximum concurrent operations must be positive".to_string());
        }
        
        Ok(())
    }
}