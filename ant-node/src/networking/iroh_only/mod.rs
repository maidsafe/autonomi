//! Phase 5: Pure Iroh Networking Implementation
//! 
//! This module implements a pure iroh-based networking system that completely
//! replaces the dual-stack coordinator with a simplified, high-performance
//! iroh-only transport layer.
//! 
//! ## Migration from Phase 3
//! 
//! Phase 5 represents the final evolution of the Autonomi Network transport layer:
//! - **Phase 3**: Dual-stack with gradual migration (libp2p + iroh)
//! - **Phase 5**: Pure iroh with simplified architecture (iroh only)
//! 
//! This migration is only performed after Phase 3 has achieved 100% iroh adoption
//! and proven network stability across all deployment scenarios.
//! 
//! ## Architecture Overview
//! 
//! The Phase 5 implementation removes all libp2p dependencies and dual-stack
//! complexity, providing a clean, optimized iroh-only networking layer:
//! 
//! - **IrohTransport**: Primary transport implementing `KademliaTransport`
//! - **IrohConfig**: Simplified configuration without dual-stack complexity
//! - **IrohMetrics**: Focused metrics for iroh-only operations
//! - **IrohDiscovery**: Enhanced peer discovery leveraging iroh capabilities
//! 
//! ## Performance Benefits
//! 
//! By removing dual-stack overhead, Phase 5 provides:
//! - **Reduced Memory Usage**: No dual transport coordination
//! - **Lower Latency**: Direct iroh communication without routing decisions
//! - **Simplified Code Paths**: Cleaner, more maintainable codebase
//! - **Enhanced Features**: Full utilization of iroh's advanced capabilities
//! 
//! ## Usage
//! 
//! ```rust,ignore
//! use crate::networking::iroh_only::IrohTransport;
//! 
//! // Create pure iroh transport
//! let transport = IrohTransport::new(config).await?;
//! 
//! // Use with Kademlia - direct iroh communication
//! let kad = Kademlia::with_iroh(transport, kad_config)?;
//! ```
//! 
//! ## Migration Requirements
//! 
//! Phase 5 can only be deployed when:
//! 1. **100% Network Coverage**: All nodes support iroh transport
//! 2. **Proven Stability**: Extensive validation in production environments
//! 3. **Performance Validation**: iroh transport meets all SLA requirements
//! 4. **Rollback Plan**: Ability to revert to Phase 3 if issues arise

pub mod transport;
pub mod config;
pub mod metrics;
pub mod discovery;

#[cfg(test)]
mod tests;

// Public exports for iroh-only functionality
pub use transport::IrohTransport;
pub use config::{IrohConfig, IrohConfigBuilder};
pub use metrics::IrohMetrics;
pub use discovery::IrohDiscovery;

/// Result type for iroh-only operations
pub type IrohResult<T> = Result<T, IrohError>;

/// Error types specific to iroh-only operations
#[derive(Debug, thiserror::Error)]
pub enum IrohError {
    /// Transport operation failed
    #[error("Transport operation failed: {reason}")]
    TransportFailed { reason: String },
    
    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    /// Discovery operation failed
    #[error("Discovery failed: {0}")]
    Discovery(String),
    
    /// Metrics collection error
    #[error("Metrics error: {0}")]
    Metrics(String),
    
    /// Network connectivity error
    #[error("Network connectivity error: {0}")]
    Connectivity(String),
}

impl From<IrohError> for crate::networking::kad::transport::KadError {
    fn from(err: IrohError) -> Self {
        match err {
            IrohError::TransportFailed { .. } => {
                Self::Transport(err.to_string())
            },
            IrohError::Configuration(_) => {
                Self::Transport(err.to_string())
            },
            IrohError::Discovery(_) => {
                Self::QueryFailed { reason: err.to_string() }
            },
            IrohError::Metrics(_) => {
                Self::Transport(err.to_string())
            },
            IrohError::Connectivity(_) => {
                Self::Transport(err.to_string())
            },
        }
    }
}

/// Constants for iroh-only operation
pub mod constants {
    use std::time::Duration;
    
    /// Default timeout for iroh operations
    pub const DEFAULT_IROH_TIMEOUT: Duration = Duration::from_secs(30);
    
    /// Default connection pool size
    pub const DEFAULT_CONNECTION_POOL_SIZE: usize = 1000;
    
    /// Default health check interval
    pub const DEFAULT_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
    
    /// Maximum number of recent operations to track
    pub const MAX_OPERATION_HISTORY: usize = 10000;
    
    /// Default discovery refresh interval
    pub const DEFAULT_DISCOVERY_INTERVAL: Duration = Duration::from_minutes(5);
}

/// Version information for iroh-only implementation
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const IMPLEMENTATION_VERSION: &str = "phase5-v1.0.0";

/// Migration utilities for transitioning from Phase 3 to Phase 5
pub mod migration {
    use super::*;
    
    /// Validate that the network is ready for Phase 5 migration
    pub fn validate_phase5_readiness() -> Result<(), String> {
        // In a real implementation, this would:
        // 1. Check network-wide iroh adoption percentage
        // 2. Validate performance metrics meet SLA requirements
        // 3. Confirm no critical libp2p dependencies remain
        // 4. Verify rollback procedures are in place
        
        // For now, return placeholder validation
        Ok(())
    }
    
    /// Convert Phase 3 dual-stack configuration to Phase 5 iroh-only config
    pub fn migrate_config(
        dual_stack_config: &crate::networking::dual_stack::DualStackConfig
    ) -> IrohConfig {
        // Extract iroh-specific settings from dual-stack config
        IrohConfig::builder()
            .with_performance_settings(|perf| {
                perf.history_size = dual_stack_config.performance.history_size;
                perf.evaluation_interval = dual_stack_config.performance.evaluation_interval;
            })
            .with_metrics_settings(|metrics| {
                metrics.enabled = dual_stack_config.metrics.enabled;
                metrics.export_interval = dual_stack_config.metrics.export_interval;
            })
            .build()
    }
    
    /// Create migration report comparing Phase 3 vs Phase 5 performance
    pub fn create_migration_report() -> MigrationReport {
        MigrationReport {
            performance_improvement: 0.0, // To be measured
            memory_reduction: 0.0,        // To be measured
            latency_improvement: 0.0,     // To be measured
            code_complexity_reduction: 0.0, // To be measured
        }
    }
}

/// Migration report for Phase 3 to Phase 5 transition
#[derive(Debug, Clone)]
pub struct MigrationReport {
    pub performance_improvement: f64,
    pub memory_reduction: f64,
    pub latency_improvement: f64,
    pub code_complexity_reduction: f64,
}