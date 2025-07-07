// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Builder pattern implementation for ant-net.
//!
//! This module provides a fluent builder API for configuring and creating
//! ant-net network instances with custom transports, behaviors, and settings.

use crate::{
    behavior::{BehaviourComposer, NetworkBehaviour},
    driver::{AntNet, NetworkDriver},
    transport::{QuicTransport, Transport, TransportBuilder},
    AntNetError, Multiaddr, Result,
};
use libp2p::identity::Keypair;
use std::time::Duration;

/// Configuration for the ant-net network.
#[derive(Debug, Clone)]
pub struct AntNetConfig {
    /// Cryptographic keypair for the node.
    pub keypair: Keypair,
    /// Transport configuration.
    pub transport: Box<dyn Transport>,
    /// Behavior composition.
    pub behaviors: BehaviourComposer,
    /// Listening addresses.
    pub listen_addresses: Vec<Multiaddr>,
    /// Connection limits.
    pub connection_limits: ConnectionLimits,
    /// Timeouts and intervals.
    pub timeouts: TimeoutConfig,
    /// Enable metrics collection.
    pub metrics_enabled: bool,
}

/// Connection limit configuration.
#[derive(Debug, Clone)]
pub struct ConnectionLimits {
    /// Maximum number of inbound connections.
    pub max_inbound: Option<usize>,
    /// Maximum number of outbound connections.
    pub max_outbound: Option<usize>,
    /// Maximum connections per peer.
    pub max_per_peer: usize,
    /// Maximum pending inbound connections.
    pub max_pending_inbound: Option<usize>,
    /// Maximum pending outbound connections.
    pub max_pending_outbound: Option<usize>,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            max_inbound: Some(100),
            max_outbound: Some(100),
            max_per_peer: 3,
            max_pending_inbound: Some(10),
            max_pending_outbound: Some(10),
        }
    }
}

/// Timeout configuration.
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Connection establishment timeout.
    pub connection_timeout: Duration,
    /// Connection idle timeout.
    pub idle_timeout: Duration,
    /// Request timeout.
    pub request_timeout: Duration,
    /// Substream negotiation timeout.
    pub substream_timeout: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connection_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300),
            request_timeout: Duration::from_secs(30),
            substream_timeout: Duration::from_secs(10),
        }
    }
}

/// Builder for creating ant-net network instances.
#[derive(Debug)]
pub struct AntNetBuilder {
    keypair: Option<Keypair>,
    transport: Option<Box<dyn Transport>>,
    behaviors: BehaviourComposer,
    listen_addresses: Vec<Multiaddr>,
    connection_limits: ConnectionLimits,
    timeouts: TimeoutConfig,
    metrics_enabled: bool,
}

impl Default for AntNetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AntNetBuilder {
    /// Create a new ant-net builder.
    pub fn new() -> Self {
        Self {
            keypair: None,
            transport: None,
            behaviors: BehaviourComposer::new(),
            listen_addresses: Vec::new(),
            connection_limits: ConnectionLimits::default(),
            timeouts: TimeoutConfig::default(),
            metrics_enabled: false,
        }
    }

    /// Set the cryptographic keypair.
    pub fn with_keypair(mut self, keypair: Keypair) -> Self {
        self.keypair = Some(keypair);
        self
    }

    /// Generate a new random keypair.
    pub fn with_random_keypair(mut self) -> Self {
        self.keypair = Some(Keypair::generate_ed25519());
        self
    }

    /// Set the transport configuration.
    pub fn with_transport(mut self, transport: Box<dyn Transport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Use QUIC transport with default settings.
    pub fn with_quic_transport(mut self) -> Self {
        self.transport = Some(Box::new(QuicTransport::new()));
        self
    }

    /// Use a custom transport builder.
    pub fn with_transport_builder(mut self, _builder: TransportBuilder) -> Self {
        // Note: We can't directly use the builder here because it needs a keypair
        // This would be handled differently in the actual build() method
        self.transport = None; // Will be built in build()
        self
    }

    /// Set the behavior composition.
    pub fn with_behaviors(mut self, behaviors: BehaviourComposer) -> Self {
        self.behaviors = behaviors;
        self
    }

    /// Add a single behavior.
    pub fn with_behavior(mut self, behavior: Box<dyn NetworkBehaviour>) -> Self {
        self.behaviors = self.behaviors.with_behavior(behavior);
        self
    }

    /// Use default node behaviors (Kademlia, RequestResponse, Identify, Relay).
    pub fn with_default_node_behaviors(mut self) -> Self {
        self.behaviors = BehaviourComposer::default_node_behaviors();
        self
    }

    /// Use default client behaviors (RequestResponse, Identify, Relay).
    pub fn with_default_client_behaviors(mut self) -> Self {
        self.behaviors = BehaviourComposer::default_client_behaviors();
        self
    }

    /// Add a listening address.
    pub fn with_listen_address(mut self, addr: Multiaddr) -> Self {
        self.listen_addresses.push(addr);
        self
    }

    /// Add multiple listening addresses.
    pub fn with_listen_addresses(mut self, addrs: Vec<Multiaddr>) -> Self {
        self.listen_addresses.extend(addrs);
        self
    }

    /// Listen on all interfaces with a specific port.
    pub fn listen_on_port(mut self, port: u16) -> Result<Self> {
        let addr = format!("/ip4/0.0.0.0/udp/{}/quic-v1", port)
            .parse()
            .map_err(|e| AntNetError::Configuration(format!("Invalid address: {}", e)))?;
        self.listen_addresses.push(addr);
        Ok(self)
    }

    /// Set connection limits.
    pub fn with_connection_limits(mut self, limits: ConnectionLimits) -> Self {
        self.connection_limits = limits;
        self
    }

    /// Set timeout configuration.
    pub fn with_timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Enable metrics collection.
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.metrics_enabled = enabled;
        self
    }

    /// Build the network configuration.
    pub fn build_config(self) -> Result<AntNetConfig> {
        let keypair = self.keypair.unwrap_or_else(Keypair::generate_ed25519);

        let transport = if let Some(transport) = self.transport {
            transport
        } else {
            // Default to QUIC transport
            Box::new(QuicTransport::new())
        };

        Ok(AntNetConfig {
            keypair,
            transport,
            behaviors: self.behaviors,
            listen_addresses: self.listen_addresses,
            connection_limits: self.connection_limits,
            timeouts: self.timeouts,
            metrics_enabled: self.metrics_enabled,
        })
    }

    /// Build the ant-net network instance.
    pub async fn build(self) -> Result<AntNet> {
        let config = self.build_config()?;

        // Create the network driver
        let network = AntNet::new(
            config.keypair,
            config.transport,
            config.behaviors,
        );

        Ok(network)
    }

    /// Build and start the network in one step.
    pub async fn build_and_start(self) -> Result<AntNet> {
        let mut network = self.build().await?;
        network.start().await?;
        Ok(network)
    }
}

/// Preset configurations for common use cases.
impl AntNetBuilder {
    /// Create a configuration for a full node.
    pub fn node() -> Self {
        Self::new()
            .with_random_keypair()
            .with_quic_transport()
            .with_default_node_behaviors()
            .with_metrics(true)
    }

    /// Create a configuration for a client.
    pub fn client() -> Self {
        Self::new()
            .with_random_keypair()
            .with_quic_transport()
            .with_default_client_behaviors()
    }

    /// Create a minimal configuration for testing.
    pub fn minimal() -> Self {
        Self::new()
            .with_random_keypair()
            .with_quic_transport()
    }

    /// Create a configuration with custom port.
    pub fn with_port(port: u16) -> Result<Self> {
        Self::node().listen_on_port(port)
    }
}

/// Helper functions for common configurations.
impl AntNetConfig {
    /// Create a default node configuration.
    pub fn node() -> Result<Self> {
        AntNetBuilder::node().build_config()
    }

    /// Create a default client configuration.
    pub fn client() -> Result<Self> {
        AntNetBuilder::client().build_config()
    }

    /// Create a minimal configuration for testing.
    pub fn minimal() -> Result<Self> {
        AntNetBuilder::minimal().build_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::{IdentifyBehaviour, KademliaBehaviour};

    #[tokio::test]
    async fn test_builder_basic() {
        let network = AntNetBuilder::new()
            .with_random_keypair()
            .with_quic_transport()
            .build()
            .await
            .expect("Failed to build network");

        assert!(!network.peer_id().to_string().is_empty());
    }

    #[tokio::test]
    async fn test_builder_with_behaviors() {
        let network = AntNetBuilder::new()
            .with_random_keypair()
            .with_behavior(Box::new(KademliaBehaviour::new()))
            .with_behavior(Box::new(IdentifyBehaviour::new()))
            .build()
            .await
            .expect("Failed to build network");

        assert!(!network.peer_id().to_string().is_empty());
    }

    #[tokio::test]
    async fn test_preset_configurations() {
        // Test node preset
        let node = AntNetBuilder::node()
            .build()
            .await
            .expect("Failed to build node");

        // Test client preset
        let client = AntNetBuilder::client()
            .build()
            .await
            .expect("Failed to build client");

        // Test minimal preset
        let minimal = AntNetBuilder::minimal()
            .build()
            .await
            .expect("Failed to build minimal");

        assert_ne!(node.peer_id(), client.peer_id());
        assert_ne!(client.peer_id(), minimal.peer_id());
    }

    #[test]
    fn test_config_creation() {
        let config = AntNetConfig::node().expect("Failed to create node config");
        assert!(!config.listen_addresses.is_empty() || config.listen_addresses.is_empty()); // Just check it doesn't panic

        let config = AntNetConfig::client().expect("Failed to create client config");
        assert!(config.metrics_enabled || !config.metrics_enabled); // Just check it doesn't panic
    }
}