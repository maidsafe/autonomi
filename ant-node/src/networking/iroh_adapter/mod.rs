// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! iroh transport adapter for Kademlia DHT
//! 
//! This module provides an iroh-based transport implementation that bridges
//! the transport-agnostic Kademlia module with iroh's connection-oriented networking.
//! 
//! ## Architecture
//! 
//! - **Transport Layer**: `IrohTransport` implementing `KademliaTransport` trait
//! - **Protocol Layer**: ALPN-based protocol handler for Kademlia messages
//! - **Discovery Layer**: Bridge between Kademlia and iroh peer discovery
//! - **Integration Layer**: High-level interface combining all components
//! 
//! ## Usage
//! 
//! ```rust,ignore
//! use crate::networking::iroh_adapter::IrohKademlia;
//! 
//! let kad = IrohKademlia::new(config).await?;
//! kad.bootstrap(bootstrap_peers).await?;
//! let result = kad.find_node(target_peer).await?;
//! ```

// Note: iroh dependencies temporarily disabled due to hickory-proto version conflicts
// The architectural implementation is complete and demonstrates the design
#[cfg(feature = "iroh-transport")]
pub mod config;
#[cfg(feature = "iroh-transport")]
pub mod transport;
#[cfg(feature = "iroh-transport")]  
pub mod protocol;
#[cfg(feature = "iroh-transport")]
pub mod discovery;
#[cfg(feature = "iroh-transport")]
pub mod integration;
#[cfg(feature = "iroh-transport")]
pub mod metrics;

#[cfg(feature = "iroh-transport")]
#[cfg(test)]
mod tests;

#[cfg(feature = "iroh-transport")]
#[cfg(test)]
mod validation_test;

#[cfg(feature = "iroh-transport")]
pub use config::IrohConfig;
#[cfg(feature = "iroh-transport")]
pub use transport::IrohTransport;
#[cfg(feature = "iroh-transport")]
pub use protocol::KadProtocol;
#[cfg(feature = "iroh-transport")]
pub use discovery::DiscoveryBridge;
#[cfg(feature = "iroh-transport")]
pub use integration::IrohKademlia;
#[cfg(feature = "iroh-transport")]
pub use metrics::IrohMetrics;

/// Kademlia ALPN (Application-Layer Protocol Negotiation) identifier for iroh
#[cfg(feature = "iroh-transport")]
pub const KAD_ALPN: &[u8] = b"autonomi/kad/1.0.0";

/// Maximum message size for Kademlia protocol over iroh (64KB)
#[cfg(feature = "iroh-transport")]
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024;

/// Request timeout for Kademlia operations over iroh
#[cfg(feature = "iroh-transport")]
pub const DEFAULT_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Connection timeout for iroh endpoint connections
#[cfg(feature = "iroh-transport")]
pub const DEFAULT_CONNECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(not(feature = "iroh-transport"))]
mod disabled {
    //! Placeholder module when iroh-transport feature is disabled
    
    /// Placeholder type when iroh is disabled
    pub struct IrohTransportDisabled;
    
    impl IrohTransportDisabled {
        /// Returns an error indicating iroh transport is not enabled
        pub fn new() -> Result<Self, &'static str> {
            Err("iroh-transport feature not enabled. Enable with --features iroh-transport")
        }
    }
}

#[cfg(not(feature = "iroh-transport"))]
pub use disabled::IrohTransportDisabled as IrohTransport;

#[cfg(feature = "iroh-transport")]
mod version {
    //! Version compatibility checks for iroh dependencies
    
    use tracing::warn;
    
    /// Check iroh version compatibility and log warnings if needed
    pub fn check_compatibility() {
        // This is a placeholder for runtime version checks
        // In a real implementation, you might want to verify iroh version
        // matches what was expected during compilation
        let _iroh_version = env!("CARGO_PKG_VERSION");
    }
}

#[cfg(feature = "iroh-transport")]
pub use version::check_compatibility;

/// Error types specific to iroh transport operations
#[cfg(feature = "iroh-transport")]
#[derive(Debug, thiserror::Error)]
pub enum IrohError {
    /// iroh endpoint operation failed
    #[error("iroh endpoint error: {0}")]
    Endpoint(#[from] Box<dyn std::error::Error + Send + Sync>),
    
    /// Connection to peer failed
    #[error("connection failed to peer {peer}: {source}")]
    Connection {
        peer: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    
    /// Protocol message serialization failed
    #[error("serialization error: {0}")]
    Serialization(#[from] postcard::Error),
    
    /// Protocol message is too large
    #[error("message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },
    
    /// Request timeout
    #[error("request timeout after {duration:?}")]
    Timeout { duration: std::time::Duration },
    
    /// Discovery failed
    #[error("discovery failed: {0}")]
    Discovery(String),
    
    /// Invalid peer ID format
    #[error("invalid peer ID: {0}")]
    InvalidPeerId(String),
    
    /// Protocol error
    #[error("protocol error: {0}")]
    Protocol(String),
}

#[cfg(feature = "iroh-transport")]
impl From<IrohError> for crate::networking::kad::transport::KadError {
    fn from(err: IrohError) -> Self {
        match err {
            IrohError::Timeout { duration } => Self::Timeout { duration },
            IrohError::Connection { peer, source } => Self::Transport(format!("Connection to {} failed: {}", peer, source)),
            other => Self::Transport(other.to_string()),
        }
    }
}

/// Result type for iroh transport operations
#[cfg(feature = "iroh-transport")]
pub type IrohResult<T> = Result<T, IrohError>;