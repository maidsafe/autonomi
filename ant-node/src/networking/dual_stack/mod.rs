// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Phase 3: Dual-Stack Networking Implementation
//! 
//! This module implements a dual-stack networking system that can run both
//! libp2p and iroh transports simultaneously, enabling gradual migration,
//! A/B testing, and optimal transport selection.
//! 
//! ## Architecture Overview
//! 
//! The dual-stack implementation consists of several key components:
//! 
//! - **DualStackTransport**: Primary coordinator implementing `KademliaTransport`
//! - **TransportRouter**: Intelligent routing engine for transport selection
//! - **MigrationManager**: Gradual migration orchestration and policies  
//! - **UnifiedMetrics**: Aggregated monitoring across both transports
//! - **FailoverController**: Automatic redundancy and recovery management
//! - **PeerAffinityTracker**: Per-peer transport preference learning
//! 
//! ## Usage
//! 
//! ```rust,ignore
//! use crate::networking::dual_stack::DualStackTransport;
//! 
//! // Create dual-stack transport with both libp2p and iroh
//! let transport = DualStackTransport::new(libp2p_transport, iroh_transport, config).await?;
//! 
//! // Use with Kademlia - automatically routes to optimal transport
//! let kad = Kademlia::with_dual_stack(transport, kad_config)?;
//! ```
//! 
//! ## Migration Strategy
//! 
//! The dual-stack system supports gradual migration from libp2p to iroh:
//! 
//! 1. **Conservative Phase** (0-25%): Handpicked stable peers, extensive monitoring
//! 2. **Validation Phase** (25-50%): Broader rollout with automatic rollback
//! 3. **Optimization Phase** (50-75%): Performance tuning and policy refinement  
//! 4. **Completion Phase** (75-100%): Full migration with libp2p backup

#[cfg(feature = "dual-stack")]
pub mod coordinator;

#[cfg(feature = "dual-stack")]
pub mod router;

#[cfg(feature = "dual-stack")]
pub mod migration;

#[cfg(feature = "dual-stack")]
pub mod metrics;

#[cfg(feature = "dual-stack")]
pub mod failover;

#[cfg(feature = "dual-stack")]
pub mod affinity;

#[cfg(feature = "dual-stack")]
pub mod config;

#[cfg(feature = "dual-stack")]
pub mod utils;

#[cfg(feature = "dual-stack")]
pub mod testing;

#[cfg(feature = "dual-stack")]
#[cfg(test)]
mod tests;

// Public exports for dual-stack functionality
#[cfg(feature = "dual-stack")]
pub use coordinator::DualStackTransport;

#[cfg(feature = "dual-stack")]
pub use router::{TransportRouter, RoutingPolicy, TransportChoice};

#[cfg(feature = "dual-stack")]
pub use migration::{MigrationManager, MigrationPolicy, MigrationPhase};

#[cfg(feature = "dual-stack")]
pub use metrics::{UnifiedMetrics, TransportMetrics, ComparisonReport};

#[cfg(feature = "dual-stack")]
pub use failover::{FailoverController, FailoverStats};

#[cfg(feature = "dual-stack")]
pub use affinity::{PeerAffinityTracker, AffinityStats};

#[cfg(feature = "dual-stack")]
pub use config::{DualStackConfig, DualStackConfigBuilder};

#[cfg(feature = "dual-stack")]
pub use testing::{ABTestingFramework, ABTestConfig, TestConfig, ExperimentStatus};

/// Transport identification for dual-stack operations
#[cfg(feature = "dual-stack")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportId {
    /// libp2p transport (legacy/fallback)
    LibP2P,
    /// iroh transport (target/optimized)
    Iroh,
}

impl TransportId {
    /// Get human-readable name for the transport
    pub fn name(&self) -> &'static str {
        match self {
            TransportId::LibP2P => "libp2p",
            TransportId::Iroh => "iroh",
        }
    }
    
    /// Check if this is the preferred modern transport
    pub fn is_modern(&self) -> bool {
        matches!(self, TransportId::Iroh)
    }
    
    /// Check if this is the legacy fallback transport
    pub fn is_legacy(&self) -> bool {
        matches!(self, TransportId::LibP2P)
    }
}

/// Result type for dual-stack operations
#[cfg(feature = "dual-stack")]
pub type DualStackResult<T> = Result<T, DualStackError>;

/// Error types specific to dual-stack operations
#[cfg(feature = "dual-stack")]
#[derive(Debug, thiserror::Error)]
pub enum DualStackError {
    /// Both transports failed for an operation
    #[error("All transports failed: libp2p={libp2p_error}, iroh={iroh_error}")]
    AllTransportsFailed {
        libp2p_error: String,
        iroh_error: String,
    },
    
    /// Transport not available (not configured or failed)
    #[error("Transport {transport:?} not available: {reason}")]
    TransportUnavailable {
        transport: TransportId,
        reason: String,
    },
    
    /// Migration operation failed
    #[error("Migration failed: {reason}")]
    MigrationFailed { reason: String },
    
    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    /// Routing decision failed
    #[error("Routing failed: {0}")]
    Routing(String),
    
    /// Metrics collection error
    #[error("Metrics error: {0}")]
    Metrics(String),
    
    /// Failover operation failed
    #[error("Failover failed: {0}")]
    Failover(String),
}

#[cfg(feature = "dual-stack")]
impl From<DualStackError> for crate::networking::kad::transport::KadError {
    fn from(err: DualStackError) -> Self {
        match err {
            DualStackError::AllTransportsFailed { .. } => {
                Self::Transport(err.to_string())
            },
            DualStackError::TransportUnavailable { .. } => {
                Self::Transport(err.to_string())
            },
            DualStackError::MigrationFailed { .. } => {
                Self::QueryFailed { reason: err.to_string() }
            },
            DualStackError::Configuration(_) => {
                Self::Transport(err.to_string())
            },
            DualStackError::Routing(_) => {
                Self::Transport(err.to_string())
            },
            DualStackError::Metrics(_) => {
                Self::Transport(err.to_string())
            },
            DualStackError::Failover(_) => {
                Self::Transport(err.to_string())
            },
        }
    }
}

/// Constants for dual-stack operation
#[cfg(feature = "dual-stack")]
pub mod constants {
    use std::time::Duration;
    
    /// Default timeout for transport selection decisions
    pub const DEFAULT_ROUTING_TIMEOUT: Duration = Duration::from_millis(100);
    
    /// Default timeout for failover operations
    pub const DEFAULT_FAILOVER_TIMEOUT: Duration = Duration::from_secs(5);
    
    /// Default migration rollout percentage (conservative start)
    pub const DEFAULT_MIGRATION_PERCENTAGE: f32 = 0.05; // 5%
    
    /// Maximum number of recent operations to track for performance
    pub const MAX_PERFORMANCE_HISTORY: usize = 1000;
    
    /// Default health check interval
    pub const DEFAULT_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
    
    /// Default peer affinity cache size
    pub const DEFAULT_AFFINITY_CACHE_SIZE: usize = 10000;
    
    /// Minimum operations required before trusting affinity scores
    pub const MIN_AFFINITY_OPERATIONS: usize = 5;
}

// Re-export utility functions
#[cfg(feature = "dual-stack")]
pub use utils::*;

#[cfg(not(feature = "dual-stack"))]
mod disabled {
    //! Placeholder module when dual-stack feature is disabled
    
    /// Placeholder type when dual-stack is disabled
    pub struct DualStackDisabled;
    
    impl DualStackDisabled {
        /// Returns an error indicating dual-stack is not enabled
        pub fn new() -> Result<Self, &'static str> {
            Err("dual-stack feature not enabled. Enable with --features dual-stack")
        }
    }
}

#[cfg(not(feature = "dual-stack"))]
pub use disabled::DualStackDisabled as DualStackTransport;

/// Version information for dual-stack implementation
#[cfg(feature = "dual-stack")]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "dual-stack")]
pub const IMPLEMENTATION_VERSION: &str = "phase3-v1.0.0";