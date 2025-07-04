// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Configuration for iroh transport adapter
//! 
//! This module provides configuration structures for all aspects of the iroh
//! transport integration, including networking, discovery, and protocol settings.

use std::time::Duration;
use serde::{Deserialize, Serialize};

use crate::networking::kad::transport::KadConfig;

/// Main configuration for iroh transport adapter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohConfig {
    /// Network configuration
    pub network: NetworkConfig,
    
    /// Discovery configuration
    pub discovery: DiscoveryConfig,
    
    /// Protocol configuration
    pub protocol: ProtocolConfig,
    
    /// Metrics and monitoring
    pub metrics: MetricsConfig,
    
    /// Kademlia DHT configuration
    pub kademlia: KadConfig,
}

/// Network-level configuration for iroh transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Enable relay servers for NAT traversal
    pub enable_relay: bool,
    
    /// Custom relay URLs (uses iroh n0 defaults if empty)
    pub relay_urls: Vec<String>,
    
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    
    /// Connection timeout duration
    pub connection_timeout: Duration,
    
    /// Keep-alive interval for connections
    pub keep_alive_interval: Duration,
    
    /// Enable STUN for NAT discovery
    pub enable_stun: bool,
    
    /// Custom STUN servers (uses defaults if empty)
    pub stun_servers: Vec<String>,
    
    /// Enable UPnP for automatic port forwarding
    pub enable_upnp: bool,
    
    /// Bind to specific addresses (empty = bind to all)
    pub bind_addresses: Vec<std::net::SocketAddr>,
    
    /// Enable IPv6 support
    pub enable_ipv6: bool,
}

/// Discovery configuration for peer finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Use n0 DNS discovery service
    pub use_n0_dns: bool,
    
    /// Use Kademlia peer addresses for discovery
    pub use_kad_peers: bool,
    
    /// Custom discovery endpoints
    pub custom_endpoints: Vec<String>,
    
    /// Discovery timeout
    pub discovery_timeout: Duration,
    
    /// Cache discovered peers
    pub cache_discovered_peers: bool,
    
    /// Peer cache TTL
    pub peer_cache_ttl: Duration,
    
    /// Maximum cached peers per node
    pub max_cached_peers: usize,
    
    /// Periodic discovery interval (None = disabled)
    pub periodic_discovery_interval: Option<Duration>,
}

/// Protocol-level configuration for Kademlia over iroh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfig {
    /// Request timeout for individual operations
    pub request_timeout: Duration,
    
    /// Maximum message size in bytes
    pub max_message_size: usize,
    
    /// Enable message compression
    pub enable_compression: bool,
    
    /// Message serialization format
    pub serialization_format: SerializationFormat,
    
    /// Number of retry attempts for failed requests
    pub max_retries: usize,
    
    /// Backoff strategy for retries
    pub retry_backoff: BackoffStrategy,
    
    /// Enable message deduplication
    pub enable_deduplication: bool,
    
    /// Deduplication cache size
    pub dedup_cache_size: usize,
    
    /// Deduplication cache TTL
    pub dedup_cache_ttl: Duration,
}

/// Message serialization format options
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SerializationFormat {
    /// Postcard (compact binary)
    Postcard,
    /// Bincode (fast binary)
    Bincode,
    /// JSON (human readable, debugging)
    Json,
}

/// Retry backoff strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed { delay: Duration },
    /// Exponential backoff with optional jitter
    Exponential { 
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
        jitter: bool,
    },
    /// Linear backoff
    Linear {
        initial_delay: Duration,
        increment: Duration,
        max_delay: Duration,
    },
}

/// Metrics and monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,
    
    /// Metrics export interval
    pub export_interval: Duration,
    
    /// Track connection metrics
    pub track_connections: bool,
    
    /// Track message metrics
    pub track_messages: bool,
    
    /// Track latency histograms
    pub track_latency: bool,
    
    /// Maximum number of peer metrics to track
    pub max_peer_metrics: usize,
    
    /// Histogram bucket configuration
    pub latency_buckets: Vec<f64>,
}

impl Default for IrohConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            discovery: DiscoveryConfig::default(),
            protocol: ProtocolConfig::default(),
            metrics: MetricsConfig::default(),
            kademlia: KadConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enable_relay: true,
            relay_urls: vec![], // Use iroh defaults
            max_connections: 1000,
            connection_timeout: Duration::from_secs(10),
            keep_alive_interval: Duration::from_secs(30),
            enable_stun: true,
            stun_servers: vec![], // Use iroh defaults
            enable_upnp: true,
            bind_addresses: vec![], // Bind to all interfaces
            enable_ipv6: true,
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            use_n0_dns: true,
            use_kad_peers: true,
            custom_endpoints: vec![],
            discovery_timeout: Duration::from_secs(5),
            cache_discovered_peers: true,
            peer_cache_ttl: Duration::from_secs(300), // 5 minutes
            max_cached_peers: 1000,
            periodic_discovery_interval: Some(Duration::from_secs(60)), // 1 minute
        }
    }
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            max_message_size: 64 * 1024, // 64KB
            enable_compression: false, // Keep simple for now
            serialization_format: SerializationFormat::Postcard,
            max_retries: 3,
            retry_backoff: BackoffStrategy::Exponential {
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(5),
                multiplier: 2.0,
                jitter: true,
            },
            enable_deduplication: true,
            dedup_cache_size: 10000,
            dedup_cache_ttl: Duration::from_secs(60),
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            export_interval: Duration::from_secs(60),
            track_connections: true,
            track_messages: true,
            track_latency: true,
            max_peer_metrics: 1000,
            latency_buckets: vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
        }
    }
}

impl SerializationFormat {
    /// Get the MIME type for this serialization format
    pub fn mime_type(&self) -> &'static str {
        match self {
            SerializationFormat::Postcard => "application/postcard",
            SerializationFormat::Bincode => "application/bincode",
            SerializationFormat::Json => "application/json",
        }
    }
    
    /// Check if this format is human readable
    pub fn is_human_readable(&self) -> bool {
        matches!(self, SerializationFormat::Json)
    }
    
    /// Get the typical compression ratio for this format
    pub fn compression_ratio(&self) -> f32 {
        match self {
            SerializationFormat::Postcard => 0.7, // Compact
            SerializationFormat::Bincode => 0.8,  // Efficient
            SerializationFormat::Json => 1.5,     // Verbose
        }
    }
}

impl BackoffStrategy {
    /// Calculate the delay for a given retry attempt
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        match self {
            BackoffStrategy::Fixed { delay } => *delay,
            BackoffStrategy::Exponential { 
                initial_delay, 
                max_delay, 
                multiplier, 
                jitter 
            } => {
                let base_delay = initial_delay.as_millis() as f64 * multiplier.powi(attempt as i32);
                let mut delay = Duration::from_millis(base_delay as u64).min(*max_delay);
                
                if *jitter {
                    use rand::Rng;
                    let jitter_factor = rand::thread_rng().gen_range(0.5..1.5);
                    delay = Duration::from_millis((delay.as_millis() as f64 * jitter_factor) as u64);
                }
                
                delay
            },
            BackoffStrategy::Linear { 
                initial_delay, 
                increment, 
                max_delay 
            } => {
                let total_increment = increment.as_millis() as u64 * attempt as u64;
                let delay = initial_delay.as_millis() as u64 + total_increment;
                Duration::from_millis(delay).min(*max_delay)
            },
        }
    }
}

/// Builder for IrohConfig with method chaining
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
    
    /// Configure network settings
    pub fn with_network<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut NetworkConfig),
    {
        configure(&mut self.config.network);
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
    
    /// Configure protocol settings
    pub fn with_protocol<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut ProtocolConfig),
    {
        configure(&mut self.config.protocol);
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
    
    /// Configure Kademlia settings
    pub fn with_kademlia<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut KadConfig),
    {
        configure(&mut self.config.kademlia);
        self
    }
    
    /// Build the final configuration
    pub fn build(self) -> IrohConfig {
        self.config
    }
}

impl Default for IrohConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Preset configurations for common use cases
impl IrohConfig {
    /// Configuration optimized for local development and testing
    pub fn local_development() -> Self {
        IrohConfigBuilder::new()
            .with_network(|net| {
                net.enable_relay = false;
                net.enable_stun = false;
                net.enable_upnp = false;
                net.max_connections = 100;
                net.bind_addresses = vec!["127.0.0.1:0".parse().unwrap()];
            })
            .with_discovery(|disc| {
                disc.use_n0_dns = false;
                disc.discovery_timeout = Duration::from_secs(1);
                disc.peer_cache_ttl = Duration::from_secs(30);
            })
            .with_protocol(|proto| {
                proto.request_timeout = Duration::from_secs(5);
                proto.serialization_format = SerializationFormat::Json; // For debugging
            })
            .build()
    }
    
    /// Configuration optimized for production deployment
    pub fn production() -> Self {
        IrohConfigBuilder::new()
            .with_network(|net| {
                net.max_connections = 5000;
                net.keep_alive_interval = Duration::from_secs(60);
            })
            .with_discovery(|disc| {
                disc.periodic_discovery_interval = Some(Duration::from_secs(300)); // 5 minutes
                disc.max_cached_peers = 5000;
            })
            .with_protocol(|proto| {
                proto.enable_compression = true;
                proto.serialization_format = SerializationFormat::Postcard;
            })
            .build()
    }
    
    /// Configuration for resource-constrained environments
    pub fn minimal() -> Self {
        IrohConfigBuilder::new()
            .with_network(|net| {
                net.max_connections = 50;
                net.enable_ipv6 = false;
            })
            .with_discovery(|disc| {
                disc.cache_discovered_peers = false;
                disc.max_cached_peers = 100;
                disc.periodic_discovery_interval = None;
            })
            .with_metrics(|metrics| {
                metrics.enabled = false;
            })
            .build()
    }
    
    /// Validate configuration for consistency and safety
    pub fn validate(&self) -> Result<(), String> {
        if self.network.max_connections == 0 {
            return Err("max_connections must be greater than 0".to_string());
        }
        
        if self.protocol.max_message_size == 0 {
            return Err("max_message_size must be greater than 0".to_string());
        }
        
        if self.protocol.max_message_size > 10 * 1024 * 1024 {
            return Err("max_message_size should not exceed 10MB".to_string());
        }
        
        if self.discovery.max_cached_peers == 0 && self.discovery.cache_discovered_peers {
            return Err("max_cached_peers must be greater than 0 when caching is enabled".to_string());
        }
        
        if self.protocol.max_retries > 10 {
            return Err("max_retries should not exceed 10".to_string());
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config_validation() {
        let config = IrohConfig::default();
        assert!(config.validate().is_ok());
    }
    
    #[test]
    fn test_config_builder() {
        let config = IrohConfigBuilder::new()
            .with_network(|net| {
                net.max_connections = 500;
            })
            .with_discovery(|disc| {
                disc.use_n0_dns = false;
            })
            .build();
        
        assert_eq!(config.network.max_connections, 500);
        assert!(!config.discovery.use_n0_dns);
        assert!(config.validate().is_ok());
    }
    
    #[test]
    fn test_preset_configs() {
        let local = IrohConfig::local_development();
        assert!(!local.network.enable_relay);
        assert!(local.validate().is_ok());
        
        let prod = IrohConfig::production();
        assert_eq!(prod.network.max_connections, 5000);
        assert!(prod.validate().is_ok());
        
        let minimal = IrohConfig::minimal();
        assert_eq!(minimal.network.max_connections, 50);
        assert!(minimal.validate().is_ok());
    }
    
    #[test]
    fn test_backoff_strategy() {
        let exponential = BackoffStrategy::Exponential {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            multiplier: 2.0,
            jitter: false,
        };
        
        assert_eq!(exponential.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(exponential.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(exponential.delay_for_attempt(2), Duration::from_millis(400));
    }
    
    #[test]
    fn test_serialization_format() {
        assert_eq!(SerializationFormat::Postcard.mime_type(), "application/postcard");
        assert!(SerializationFormat::Json.is_human_readable());
        assert!(!SerializationFormat::Bincode.is_human_readable());
    }
}