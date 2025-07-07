// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Network behavior abstractions for ant-net.
//!
//! This module provides high-level interfaces for composing network behaviors,
//! hiding the complexity of libp2p NetworkBehaviour trait implementations.

use crate::{event::NetworkEvent, AntNetError, Result};
use async_trait::async_trait;
use std::fmt;

/// Abstract network behavior interface.
///
/// This trait abstracts libp2p's NetworkBehaviour trait, allowing behaviors
/// to be composed and managed through a clean interface.
#[async_trait]
pub trait NetworkBehaviour: Send + Sync + fmt::Debug {
    /// Handle an incoming network event.
    async fn handle_event(&mut self, event: NetworkEvent) -> Result<()>;

    /// Get the behavior name for debugging.
    fn name(&self) -> &'static str;

    /// Check if this behavior is enabled.
    fn is_enabled(&self) -> bool {
        true
    }

    /// Enable or disable this behavior.
    fn set_enabled(&mut self, enabled: bool);

    /// Clone the behavior configuration.
    fn clone_behavior(&self) -> Box<dyn NetworkBehaviour>;
}

impl Clone for Box<dyn NetworkBehaviour> {
    fn clone(&self) -> Self {
        self.clone_behavior()
    }
}

/// Kademlia DHT behavior configuration.
#[derive(Debug, Clone)]
pub struct KademliaBehaviour {
    enabled: bool,
    // We'll add more configuration options as needed
}

impl Default for KademliaBehaviour {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl KademliaBehaviour {
    /// Create a new Kademlia behavior.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl NetworkBehaviour for KademliaBehaviour {
    async fn handle_event(&mut self, event: NetworkEvent) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        match event {
            NetworkEvent::PeerConnected { peer_id, .. } => {
                tracing::debug!("Kademlia: Peer connected: {peer_id}");
                // Handle peer connection for Kademlia routing table
                Ok(())
            }
            NetworkEvent::PeerDisconnected { peer_id, .. } => {
                tracing::debug!("Kademlia: Peer disconnected: {peer_id}");
                // Handle peer disconnection for Kademlia routing table
                Ok(())
            }
            _ => Ok(()), // Ignore other events
        }
    }

    fn name(&self) -> &'static str {
        "kademlia"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn clone_behavior(&self) -> Box<dyn NetworkBehaviour> {
        Box::new(self.clone())
    }
}

/// Request/Response behavior configuration.
#[derive(Debug, Clone)]
pub struct RequestResponseBehaviour {
    enabled: bool,
    // We'll add protocol configurations as needed
}

impl Default for RequestResponseBehaviour {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl RequestResponseBehaviour {
    /// Create a new Request/Response behavior.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl NetworkBehaviour for RequestResponseBehaviour {
    async fn handle_event(&mut self, event: NetworkEvent) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        match event {
            NetworkEvent::RequestReceived { .. } => {
                tracing::debug!("RequestResponse: Request received");
                // Handle incoming request
                Ok(())
            }
            NetworkEvent::ResponseReceived { .. } => {
                tracing::debug!("RequestResponse: Response received");
                // Handle incoming response
                Ok(())
            }
            _ => Ok(()), // Ignore other events
        }
    }

    fn name(&self) -> &'static str {
        "request-response"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn clone_behavior(&self) -> Box<dyn NetworkBehaviour> {
        Box::new(self.clone())
    }
}

/// Identify behavior configuration.
#[derive(Debug, Clone)]
pub struct IdentifyBehaviour {
    enabled: bool,
}

impl Default for IdentifyBehaviour {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl IdentifyBehaviour {
    /// Create a new Identify behavior.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl NetworkBehaviour for IdentifyBehaviour {
    async fn handle_event(&mut self, event: NetworkEvent) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        match event {
            NetworkEvent::PeerConnected { peer_id, .. } => {
                tracing::debug!("Identify: Identifying peer: {peer_id}");
                // Trigger identification for newly connected peer
                Ok(())
            }
            _ => Ok(()), // Ignore other events
        }
    }

    fn name(&self) -> &'static str {
        "identify"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn clone_behavior(&self) -> Box<dyn NetworkBehaviour> {
        Box::new(self.clone())
    }
}

/// Relay behavior configuration.
#[derive(Debug, Clone)]
pub struct RelayBehaviour {
    enabled: bool,
    is_relay_server: bool,
}

impl Default for RelayBehaviour {
    fn default() -> Self {
        Self {
            enabled: true,
            is_relay_server: false,
        }
    }
}

impl RelayBehaviour {
    /// Create a new Relay behavior as a client.
    pub fn client() -> Self {
        Self {
            enabled: true,
            is_relay_server: false,
        }
    }

    /// Create a new Relay behavior as a server.
    pub fn server() -> Self {
        Self {
            enabled: true,
            is_relay_server: true,
        }
    }
}

#[async_trait]
impl NetworkBehaviour for RelayBehaviour {
    async fn handle_event(&mut self, event: NetworkEvent) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        match event {
            NetworkEvent::PeerConnected { peer_id, .. } => {
                if self.is_relay_server {
                    tracing::debug!("Relay server: Peer connected: {peer_id}");
                } else {
                    tracing::debug!("Relay client: Connected via relay: {peer_id}");
                }
                Ok(())
            }
            _ => Ok(()), // Ignore other events
        }
    }

    fn name(&self) -> &'static str {
        "relay"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn clone_behavior(&self) -> Box<dyn NetworkBehaviour> {
        Box::new(self.clone())
    }
}

/// Behavior composer for building complex network behaviors.
#[derive(Debug, Clone)]
pub struct BehaviourComposer {
    behaviors: Vec<Box<dyn NetworkBehaviour>>,
}

impl Default for BehaviourComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl BehaviourComposer {
    /// Create a new behavior composer.
    pub fn new() -> Self {
        Self {
            behaviors: Vec::new(),
        }
    }

    /// Add a behavior to the composer.
    pub fn with_behavior(mut self, behavior: Box<dyn NetworkBehaviour>) -> Self {
        self.behaviors.push(behavior);
        self
    }

    /// Add Kademlia DHT behavior.
    pub fn with_kademlia(self) -> Self {
        self.with_behavior(Box::new(KademliaBehaviour::new()))
    }

    /// Add Request/Response behavior.
    pub fn with_request_response(self) -> Self {
        self.with_behavior(Box::new(RequestResponseBehaviour::new()))
    }

    /// Add Identify behavior.
    pub fn with_identify(self) -> Self {
        self.with_behavior(Box::new(IdentifyBehaviour::new()))
    }

    /// Add Relay client behavior.
    pub fn with_relay_client(self) -> Self {
        self.with_behavior(Box::new(RelayBehaviour::client()))
    }

    /// Add Relay server behavior.
    pub fn with_relay_server(self) -> Self {
        self.with_behavior(Box::new(RelayBehaviour::server()))
    }

    /// Build a default behavior set for nodes.
    pub fn default_node_behaviors() -> Self {
        Self::new()
            .with_kademlia()
            .with_request_response()
            .with_identify()
            .with_relay_client()
    }

    /// Build a default behavior set for clients.
    pub fn default_client_behaviors() -> Self {
        Self::new()
            .with_request_response()
            .with_identify()
            .with_relay_client()
    }

    /// Get all configured behaviors.
    pub fn behaviors(&self) -> &[Box<dyn NetworkBehaviour>] {
        &self.behaviors
    }

    /// Get all configured behaviors mutably.
    pub fn behaviors_mut(&mut self) -> &mut [Box<dyn NetworkBehaviour>] {
        &mut self.behaviors
    }

    /// Handle an event across all behaviors.
    pub async fn handle_event(&mut self, event: NetworkEvent) -> Result<()> {
        for behavior in &mut self.behaviors {
            if let Err(e) = behavior.handle_event(event.clone()).await {
                tracing::warn!(
                    "Behavior '{}' failed to handle event: {}",
                    behavior.name(),
                    e
                );
                // Continue processing other behaviors even if one fails
            }
        }
        Ok(())
    }

    /// Enable or disable a behavior by name.
    pub fn set_behavior_enabled(&mut self, name: &str, enabled: bool) -> Result<()> {
        for behavior in &mut self.behaviors {
            if behavior.name() == name {
                behavior.set_enabled(enabled);
                return Ok(());
            }
        }
        Err(AntNetError::Behavior(format!(
            "Behavior '{}' not found",
            name
        )))
    }
}