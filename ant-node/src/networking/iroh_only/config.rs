//! Configuration management for pure iroh networking
//! 
//! This module provides simplified configuration for iroh-only transport,
//! removing the complexity of dual-stack coordination while enabling
//! full utilization of iroh's advanced capabilities.

use std::time::Duration;
use serde::{Deserialize, Serialize};

use super::constants::*;

/// Comprehensive configuration for pure iroh networking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohConfig {
    /// Connection management configuration
    pub connection_pool: ConnectionPoolConfig,
    
    /// Discovery and peer management
    pub discovery: DiscoveryConfig,
    
    /// Performance monitoring and optimization
    pub performance: PerformanceConfig,
    
    /// Metrics collection and export
    pub metrics: MetricsConfig,
    
    /// Advanced operational settings
    pub advanced: AdvancedConfig,
}

/// Connection pool configuration optimized for iroh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    /// Maximum number of concurrent connections
    pub max_size: usize,
    
    /// Minimum number of persistent connections
    pub min_size: usize,
    
    /// Connection idle timeout before cleanup
    pub idle_timeout: Duration,
    
    /// Pool maintenance interval
    pub maintenance_interval: Duration,
    
    /// Connection establishment timeout
    pub connection_timeout: Duration,
    
    /// Maximum connection reuse count
    pub max_reuse_count: u32,
    
    /// Connection lifetime limit
    pub max_lifetime: Duration,
}

/// Discovery configuration leveraging iroh capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Enable enhanced iroh discovery features
    pub enhanced_discovery: bool,
    
    /// Peer discovery refresh interval
    pub refresh_interval: Duration,
    
    /// Discovery timeout for individual operations
    pub discovery_timeout: Duration,
    
    /// Maximum number of peers to track
    pub max_tracked_peers: usize,
    
    /// Peer information cache TTL
    pub peer_cache_ttl: Duration,
    
    /// Bootstrap peer configuration
    pub bootstrap: BootstrapConfig,
}

/// Bootstrap configuration for iroh networking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfig {
    /// Maximum bootstrap attempts
    pub max_attempts: u32,
    
    /// Timeout for bootstrap operations
    pub timeout: Duration,
    
    /// Retry interval for failed bootstrap
    pub retry_interval: Duration,
    
    /// Minimum successful bootstraps required
    pub min_successful: usize,
}

/// Performance monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Enable performance optimization
    pub enabled: bool,
    
    /// Operation history size for analysis
    pub history_size: usize,
    
    /// Performance evaluation interval
    pub evaluation_interval: Duration,
    
    /// Latency optimization settings
    pub latency: LatencyConfig,
    
    /// Bandwidth optimization settings
    pub bandwidth: BandwidthConfig,
    
    /// Connection optimization settings
    pub connection: ConnectionOptimizationConfig,
}

/// Latency optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyConfig {
    /// Target latency for operations
    pub target_ms: f64,
    
    /// Latency degradation threshold for alerts
    pub degradation_threshold: f64,
    
    /// Enable latency-based connection selection
    pub enable_optimization: bool,
    
    /// Latency measurement window
    pub measurement_window: Duration,
}

/// Bandwidth optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthConfig {
    /// Target bandwidth utilization
    pub target_utilization: f32,
    
    /// Enable bandwidth monitoring
    pub enable_monitoring: bool,
    
    /// Large transfer optimization threshold
    pub large_transfer_threshold: usize,
    
    /// Bandwidth measurement interval
    pub measurement_interval: Duration,
}

/// Connection optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionOptimizationConfig {
    /// Enable connection multiplexing
    pub enable_multiplexing: bool,
    
    /// Connection warmup strategy
    pub warmup_strategy: WarmupStrategy,
    
    /// Connection health monitoring
    pub health_monitoring: HealthMonitoringConfig,
}

/// Connection warmup strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WarmupStrategy {
    /// No warmup (connect on demand)
    None,
    
    /// Lazy warmup (warm up on first use)
    Lazy,
    
    /// Proactive warmup (maintain warm connections)
    Proactive { target_count: usize },
}

/// Health monitoring configuration for connections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMonitoringConfig {
    /// Enable health monitoring
    pub enabled: bool,
    
    /// Health check interval
    pub check_interval: Duration,
    
    /// Health check timeout
    pub check_timeout: Duration,
    
    /// Failure threshold before marking unhealthy
    pub failure_threshold: u32,
    
    /// Recovery threshold before marking healthy
    pub recovery_threshold: u32,
}

/// Metrics collection configuration
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
    
    /// Enable performance profiling
    pub performance_profiling: bool,
    
    /// Histogram configuration
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
    
    /// Bandwidth histogram buckets (mbps)
    pub bandwidth_buckets: Vec<f64>,
}

/// Advanced operational configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedConfig {
    /// Enable experimental iroh features
    pub experimental_features: bool,
    
    /// Detailed logging configuration
    pub logging: LoggingConfig,
    
    /// Feature flags for runtime control
    pub feature_flags: FeatureFlagsConfig,
    
    /// Resource limits
    pub limits: ResourceLimitsConfig,
    
    /// Security settings
    pub security: SecurityConfig,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level for iroh operations
    pub level: String,
    
    /// Enable structured logging
    pub structured: bool,
    
    /// Enable performance logging
    pub performance: bool,
    
    /// Enable connection logging
    pub connection_events: bool,
    
    /// Enable discovery logging
    pub discovery_events: bool,
}

/// Feature flags configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagsConfig {
    /// Enable connection pooling
    pub connection_pooling: bool,
    
    /// Enable performance optimization
    pub performance_optimization: bool,
    
    /// Enable enhanced discovery
    pub enhanced_discovery: bool,
    
    /// Enable predictive connection management
    pub predictive_connections: bool,
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

/// Security configuration for iroh transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable transport encryption
    pub encryption_enabled: bool,
    
    /// Peer authentication settings
    pub peer_authentication: PeerAuthConfig,
    
    /// Rate limiting configuration
    pub rate_limiting: RateLimitConfig,
}

/// Peer authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAuthConfig {
    /// Enable peer authentication
    pub enabled: bool,
    
    /// Authentication timeout
    pub timeout: Duration,
    
    /// Maximum authentication attempts
    pub max_attempts: u32,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Enable rate limiting
    pub enabled: bool,
    
    /// Operations per second limit
    pub operations_per_second: u32,
    
    /// Burst size limit
    pub burst_size: u32,
    
    /// Rate limit window
    pub window: Duration,
}

impl Default for IrohConfig {
    fn default() -> Self {
        Self {
            connection_pool: ConnectionPoolConfig::default(),
            discovery: DiscoveryConfig::default(),
            performance: PerformanceConfig::default(),
            metrics: MetricsConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_size: DEFAULT_CONNECTION_POOL_SIZE,
            min_size: 10,
            idle_timeout: Duration::from_minutes(5),
            maintenance_interval: Duration::from_minutes(1),
            connection_timeout: Duration::from_secs(10),
            max_reuse_count: 1000,
            max_lifetime: Duration::from_hours(2),
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            enhanced_discovery: true,
            refresh_interval: DEFAULT_DISCOVERY_INTERVAL,
            discovery_timeout: Duration::from_secs(10),
            max_tracked_peers: 10000,
            peer_cache_ttl: Duration::from_hours(1),
            bootstrap: BootstrapConfig::default(),
        }
    }
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            timeout: Duration::from_secs(30),
            retry_interval: Duration::from_secs(5),
            min_successful: 3,
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_size: MAX_OPERATION_HISTORY,
            evaluation_interval: Duration::from_minutes(5),
            latency: LatencyConfig::default(),
            bandwidth: BandwidthConfig::default(),
            connection: ConnectionOptimizationConfig::default(),
        }
    }
}

impl Default for LatencyConfig {
    fn default() -> Self {
        Self {
            target_ms: 50.0,
            degradation_threshold: 0.20,
            enable_optimization: true,
            measurement_window: Duration::from_minutes(5),
        }
    }
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            target_utilization: 0.80,
            enable_monitoring: true,
            large_transfer_threshold: 1024 * 1024, // 1MB
            measurement_interval: Duration::from_secs(30),
        }
    }
}

impl Default for ConnectionOptimizationConfig {
    fn default() -> Self {
        Self {
            enable_multiplexing: true,
            warmup_strategy: WarmupStrategy::Lazy,
            health_monitoring: HealthMonitoringConfig::default(),
        }
    }
}

impl Default for HealthMonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval: DEFAULT_HEALTH_CHECK_INTERVAL,
            check_timeout: Duration::from_secs(5),
            failure_threshold: 3,
            recovery_threshold: 2,
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            export_interval: Duration::from_secs(60),
            retention_duration: Duration::from_hours(24),
            per_peer_metrics: false, // Can be expensive
            performance_profiling: true,
            histograms: HistogramConfig::default(),
        }
    }
}

impl Default for HistogramConfig {
    fn default() -> Self {
        Self {
            latency_buckets: vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0],
            size_buckets: vec![100.0, 1000.0, 10000.0, 100000.0, 1000000.0, 10000000.0],
            duration_buckets: vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0],
            bandwidth_buckets: vec![0.1, 1.0, 10.0, 100.0, 1000.0],
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
            security: SecurityConfig::default(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            structured: true,
            performance: true,
            connection_events: false,
            discovery_events: false,
        }
    }
}

impl Default for FeatureFlagsConfig {
    fn default() -> Self {
        Self {
            connection_pooling: true,
            performance_optimization: true,
            enhanced_discovery: true,
            predictive_connections: false, // Experimental
        }
    }
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 512 * 1024 * 1024, // 512MB
            max_concurrent_operations: 2000,
            max_cache_entries: 50000,
            max_metrics_history: MAX_OPERATION_HISTORY,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            encryption_enabled: true,
            peer_authentication: PeerAuthConfig::default(),
            rate_limiting: RateLimitConfig::default(),
        }
    }
}

impl Default for PeerAuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout: Duration::from_secs(10),
            max_attempts: 3,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            operations_per_second: 1000,
            burst_size: 100,
            window: Duration::from_secs(1),
        }
    }
}

/// Builder pattern for IrohConfig
pub struct IrohConfigBuilder {
    config: IrohConfig,
}

impl IrohConfigBuilder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: IrohConfig::default(),
        }
    }
    
    /// Configure connection pool settings
    pub fn with_connection_pool<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut ConnectionPoolConfig),
    {
        configure(&mut self.config.connection_pool);
        self
    }
    
    /// Configure discovery settings
    pub fn with_discovery<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut DiscoveryConfig),
    {
        configure(&mut self.config.discovery);
        self
    }
    
    /// Configure performance settings
    pub fn with_performance_settings<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut PerformanceConfig),
    {
        configure(&mut self.config.performance);
        self
    }
    
    /// Configure metrics settings
    pub fn with_metrics_settings<F>(mut self, configure: F) -> Self
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
    pub fn build(self) -> IrohConfig {
        self.config
    }
}

impl IrohConfig {
    /// Create a configuration builder
    pub fn builder() -> IrohConfigBuilder {
        IrohConfigBuilder::new()
    }
    
    /// Create a development configuration optimized for testing
    pub fn development() -> Self {
        Self::builder()
            .with_connection_pool(|pool| {
                pool.max_size = 100;
                pool.maintenance_interval = Duration::from_secs(30);
            })
            .with_metrics_settings(|metrics| {
                metrics.per_peer_metrics = true; // OK for development
                metrics.export_interval = Duration::from_secs(10);
            })
            .with_advanced(|advanced| {
                advanced.experimental_features = true;
                advanced.logging.level = "debug".to_string();
                advanced.logging.connection_events = true;
                advanced.logging.discovery_events = true;
            })
            .build()
    }
    
    /// Create a production configuration optimized for performance
    pub fn production() -> Self {
        Self::builder()
            .with_connection_pool(|pool| {
                pool.max_size = 2000;
                pool.max_lifetime = Duration::from_hours(4);
            })
            .with_performance_settings(|performance| {
                performance.enabled = true;
                performance.history_size = 50000;
            })
            .with_metrics_settings(|metrics| {
                metrics.enabled = true;
                metrics.per_peer_metrics = false; // Too expensive for production
                metrics.performance_profiling = true;
            })
            .with_advanced(|advanced| {
                advanced.experimental_features = false;
                advanced.feature_flags.predictive_connections = false;
            })
            .build()
    }
    
    /// Create a high-performance configuration for load testing
    pub fn high_performance() -> Self {
        Self::builder()
            .with_connection_pool(|pool| {
                pool.max_size = 5000;
                pool.min_size = 100;
                pool.maintenance_interval = Duration::from_secs(10);
            })
            .with_performance_settings(|performance| {
                performance.enabled = true;
                performance.latency.target_ms = 25.0;
                performance.bandwidth.target_utilization = 0.95;
            })
            .with_advanced(|advanced| {
                advanced.feature_flags.connection_pooling = true;
                advanced.feature_flags.performance_optimization = true;
                advanced.feature_flags.predictive_connections = true;
            })
            .build()
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate connection pool settings
        if self.connection_pool.max_size == 0 {
            return Err("Connection pool max_size must be positive".to_string());
        }
        
        if self.connection_pool.min_size > self.connection_pool.max_size {
            return Err("Connection pool min_size cannot exceed max_size".to_string());
        }
        
        // Validate performance settings
        if self.performance.latency.target_ms <= 0.0 {
            return Err("Target latency must be positive".to_string());
        }
        
        if self.performance.bandwidth.target_utilization <= 0.0 || 
           self.performance.bandwidth.target_utilization > 1.0 {
            return Err("Bandwidth target utilization must be between 0.0 and 1.0".to_string());
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