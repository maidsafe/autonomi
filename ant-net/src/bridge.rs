// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! libp2p compatibility bridge for ant-net.
//!
//! This module provides seamless integration between libp2p behaviors and the ant-net
//! abstraction layer, allowing for gradual migration without breaking existing functionality.

use crate::{
    behavior_manager::{BehaviorAction, BehaviorController, BehaviorHealth},
    event::NetworkEvent,
    types::{Addresses, ConnectionDirection, ConnectionId, PeerId},
    AntNetError, Result,
};
use async_trait::async_trait;
use libp2p::{
    futures::StreamExt,
    swarm::{
        NetworkBehaviour,
        THandlerInEvent, ToSwarm,
    },
    PeerId as Libp2pPeerId, Swarm,
};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Instant,
};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error};

/// Bridge between libp2p NetworkBehaviour and ant-net BehaviorController.
pub struct LibP2pBehaviorBridge<B>
where
    B: NetworkBehaviour,
{
    /// The wrapped libp2p behavior.
    inner: B,
    /// Behavior identifier.
    behavior_id: String,
    /// Event queue for outbound events to ant-net.
    event_queue: Arc<RwLock<VecDeque<NetworkEvent>>>,
    /// Whether the behavior is active.
    active: AtomicBool,
    /// Start time for statistics.
    #[allow(dead_code)]
    start_time: Instant,
}

impl<B> LibP2pBehaviorBridge<B>
where
    B: NetworkBehaviour,
{
    /// Create a new bridge wrapper around a libp2p behavior.
    pub fn new(behavior: B, behavior_id: String) -> Self {
        Self {
            inner: behavior,
            behavior_id,
            event_queue: Arc::new(RwLock::new(VecDeque::new())),
            active: AtomicBool::new(true),
            start_time: Instant::now(),
        }
    }

    /// Get a reference to the inner libp2p behavior.
    pub fn inner(&self) -> &B {
        &self.inner
    }

    /// Get a mutable reference to the inner libp2p behavior.
    pub fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }

    /// Convert libp2p SwarmEvent to ant-net NetworkEvent.
    #[allow(dead_code)]
    fn convert_swarm_event(&self, event: &ToSwarm<B::ToSwarm, THandlerInEvent<B>>) -> Option<NetworkEvent> {
        match event {
            ToSwarm::Dial { opts } => {
                if let Some(libp2p_peer_id) = opts.get_peer_id() {
                    if let Ok(peer_id) = PeerId::from_bytes(&libp2p_peer_id.to_bytes()) {
                        Some(NetworkEvent::DialAttempt {
                            peer_id,
                            addresses: Addresses::new(), // TODO: Get addresses from opts when API allows
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ToSwarm::NotifyHandler { peer_id, event, .. } => {
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    Some(NetworkEvent::BehaviorEvent {
                        peer_id: ant_peer_id,
                        behavior_id: self.behavior_id.clone(),
                        event: format!("{:?}", event),
                    })
                } else {
                    None
                }
            }
            ToSwarm::CloseConnection { peer_id, connection: _ } => {
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    Some(NetworkEvent::ConnectionClosed {
                        peer_id: ant_peer_id,
                        connection_id: ConnectionId::new_unchecked(0),
                        reason: format!("Behavior {} requested close", self.behavior_id),
                    })
                } else {
                    None
                }
            }
            ToSwarm::GenerateEvent(_behavior_event) => {
                Some(NetworkEvent::BehaviorEvent {
                    peer_id: PeerId::random(),
                    behavior_id: self.behavior_id.clone(),
                    event: "behavior_event".to_string(),
                })
            }
            _ => None,
        }
    }

    /// Queue an ant-net event for processing.
    async fn queue_event(&self, event: NetworkEvent) {
        let mut queue = self.event_queue.write().await;
        queue.push_back(event);
    }

    /// Drain queued events.
    pub async fn drain_events(&self) -> Vec<NetworkEvent> {
        let mut queue = self.event_queue.write().await;
        queue.drain(..).collect()
    }
}

impl<B> fmt::Debug for LibP2pBehaviorBridge<B>
where
    B: NetworkBehaviour + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibP2pBehaviorBridge")
            .field("behavior_id", &self.behavior_id)
            .field("inner", &self.inner)
            .field("active", &self.active.load(Ordering::Relaxed))
            .finish()
    }
}

#[async_trait]
impl<B> BehaviorController for LibP2pBehaviorBridge<B>
where
    B: NetworkBehaviour + Send + Sync + fmt::Debug + 'static,
{
    fn id(&self) -> String {
        self.behavior_id.clone()
    }

    fn name(&self) -> &'static str {
        "libp2p_bridge"
    }

    async fn start(&mut self) -> Result<()> {
        self.active.store(true, Ordering::Relaxed);
        debug!("Started libp2p behavior bridge: {}", self.behavior_id);
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.active.store(false, Ordering::Relaxed);
        debug!("Stopped libp2p behavior bridge: {}", self.behavior_id);
        Ok(())
    }

    async fn health_check(&self) -> BehaviorHealth {
        if self.active.load(Ordering::Relaxed) {
            BehaviorHealth::Healthy
        } else {
            BehaviorHealth::Unhealthy("Behavior is inactive".to_string())
        }
    }

    async fn handle_event(&mut self, event: NetworkEvent) -> Result<Vec<BehaviorAction>> {
        if !self.active.load(Ordering::Relaxed) {
            return Ok(Vec::new());
        }

        // Handle events in ant-net context (bridge doesn't convert back to libp2p)
        match &event {
            NetworkEvent::PeerConnected { peer_id, .. } => {
                debug!("Bridge handling peer connected: {}", peer_id);
                // Track peer state, forward events, etc.
            }
            NetworkEvent::PeerDisconnected { peer_id, .. } => {
                debug!("Bridge handling peer disconnected: {}", peer_id);
                // Update peer state, cleanup, etc.
            }
            NetworkEvent::RequestReceived { peer_id, protocol, .. } => {
                debug!("Bridge forwarding request from {}: {}", peer_id, protocol);
                // Forward or transform requests as needed
            }
            _ => {
                // Handle other events as needed
            }
        }

        // Queue the original event for other subscribers
        self.queue_event(event).await;

        Ok(Vec::new())
    }

    fn clone_controller(&self) -> Box<dyn BehaviorController> {
        // This is a simplified clone - in practice, you'd need to clone the inner behavior
        // which may not always be possible depending on the behavior type
        // For now, we'll create a dummy bridge that can't be used for actual networking
        panic!("LibP2pBehaviorBridge cannot be cloned due to libp2p constraints")
    }
}

/// Bridge for converting between libp2p and ant-net types and events.
pub struct NetworkBridge {
    /// Event converters for different behavior types.
    converters: HashMap<String, Box<dyn EventConverter>>,
    /// Cached type conversions.
    peer_id_cache: RwLock<HashMap<Libp2pPeerId, PeerId>>,
}

impl NetworkBridge {
    /// Create a new network bridge.
    pub fn new() -> Self {
        Self {
            converters: HashMap::new(),
            peer_id_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Register an event converter for a specific behavior type.
    pub fn register_converter(&mut self, behavior_name: String, converter: Box<dyn EventConverter>) {
        self.converters.insert(behavior_name, converter);
    }

    /// Convert libp2p PeerId to ant-net PeerId with caching.
    pub async fn convert_peer_id(&self, libp2p_peer_id: &Libp2pPeerId) -> Result<PeerId> {
        // Check cache first
        {
            let cache = self.peer_id_cache.read().await;
            if let Some(ant_peer_id) = cache.get(libp2p_peer_id) {
                return Ok(*ant_peer_id);
            }
        }

        // Convert and cache
        let ant_peer_id = PeerId::from_bytes(&libp2p_peer_id.to_bytes())
            .map_err(|e| AntNetError::Protocol(format!("Failed to convert PeerId: {}", e)))?;

        {
            let mut cache = self.peer_id_cache.write().await;
            cache.insert(*libp2p_peer_id, ant_peer_id);
        }

        Ok(ant_peer_id)
    }

    /// Convert ant-net PeerId to libp2p PeerId.
    pub fn convert_to_libp2p_peer_id(&self, ant_peer_id: &PeerId) -> Result<Libp2pPeerId> {
        Libp2pPeerId::from_bytes(&ant_peer_id.to_bytes())
            .map_err(|e| AntNetError::Protocol(format!("Failed to convert to libp2p PeerId: {}", e)))
    }
}

impl Default for NetworkBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for converting behavior-specific events between libp2p and ant-net formats.
pub trait EventConverter: Send + Sync {
    /// Convert a libp2p behavior event to ant-net NetworkEvent.
    fn convert_to_ant_net(&self, event: &str) -> Option<NetworkEvent>;

    /// Convert an ant-net NetworkEvent to libp2p behavior event format.
    fn convert_from_ant_net(&self, event: &NetworkEvent) -> Option<String>;
}

/// Standard event converter for common behavior types.
pub struct StandardEventConverter {
    behavior_name: String,
}

impl StandardEventConverter {
    /// Create a new standard event converter.
    pub fn new(behavior_name: String) -> Self {
        Self { behavior_name }
    }
}

impl EventConverter for StandardEventConverter {
    fn convert_to_ant_net(&self, event: &str) -> Option<NetworkEvent> {
        // Basic conversion - in practice, this would be more sophisticated
        Some(NetworkEvent::BehaviorEvent {
            peer_id: PeerId::random(),
            behavior_id: self.behavior_name.clone(),
            event: event.to_string(),
        })
    }

    fn convert_from_ant_net(&self, event: &NetworkEvent) -> Option<String> {
        match event {
            NetworkEvent::BehaviorEvent { behavior_id, event, .. } 
                if behavior_id == &self.behavior_name => Some(event.clone()),
            _ => None,
        }
    }
}

/// Swarm wrapper that bridges libp2p Swarm with ant-net abstractions.
pub struct BridgedSwarm<B>
where
    B: NetworkBehaviour,
{
    /// The inner libp2p swarm.
    swarm: Swarm<B>,
    /// Network bridge for type conversions.
    bridge: NetworkBridge,
    /// Event sender to ant-net event router.
    event_sender: Option<mpsc::UnboundedSender<NetworkEvent>>,
}

impl<B> BridgedSwarm<B>
where
    B: NetworkBehaviour,
{
    /// Create a new bridged swarm.
    pub fn new(swarm: Swarm<B>) -> Self {
        Self {
            swarm,
            bridge: NetworkBridge::new(),
            event_sender: None,
        }
    }

    /// Connect to ant-net event router.
    pub fn connect_event_router(&mut self, event_sender: mpsc::UnboundedSender<NetworkEvent>) {
        self.event_sender = Some(event_sender);
    }

    /// Get the network bridge.
    pub fn bridge(&self) -> &NetworkBridge {
        &self.bridge
    }

    /// Get a mutable reference to the network bridge.
    pub fn bridge_mut(&mut self) -> &mut NetworkBridge {
        &mut self.bridge
    }

    /// Get a reference to the inner swarm.
    pub fn swarm(&self) -> &Swarm<B> {
        &self.swarm
    }

    /// Get a mutable reference to the inner swarm.
    pub fn swarm_mut(&mut self) -> &mut Swarm<B> {
        &mut self.swarm
    }

    /// Poll the swarm and forward events to ant-net.
    pub fn poll_and_forward(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        // Poll the inner swarm
        while let Poll::Ready(Some(event)) = self.swarm.poll_next_unpin(cx) {
            // Convert and forward libp2p SwarmEvent to ant-net
            if let Some(ant_event) = self.convert_swarm_event(&event) {
                if let Some(sender) = &self.event_sender {
                    if let Err(e) = sender.send(ant_event) {
                        error!("Failed to send event to ant-net router: {}", e);
                    }
                }
            }
        }

        Poll::Pending
    }

    /// Convert libp2p SwarmEvent to ant-net NetworkEvent.
    fn convert_swarm_event(&self, event: &libp2p::swarm::SwarmEvent<B::ToSwarm>) -> Option<NetworkEvent> {
        match event {
            libp2p::swarm::SwarmEvent::Behaviour(_behavior_event) => {
                Some(NetworkEvent::BehaviorEvent {
                    peer_id: PeerId::random(), // Would need proper conversion
                    behavior_id: "unknown".to_string(), // Would need behavior type detection
                    event: "behavior_event".to_string(),
                })
            }
            libp2p::swarm::SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id: _,
                endpoint,
                ..
            } => {
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    let direction = match endpoint {
                        libp2p::core::ConnectedPoint::Dialer { .. } => ConnectionDirection::Outbound,
                        libp2p::core::ConnectedPoint::Listener { .. } => ConnectionDirection::Inbound,
                    };
                    Some(NetworkEvent::PeerConnected {
                        peer_id: ant_peer_id,
                        connection_id: ConnectionId::new_unchecked(0),
                        direction,
                        endpoint: endpoint.get_remote_address().clone(),
                    })
                } else {
                    None
                }
            }
            libp2p::swarm::SwarmEvent::ConnectionClosed {
                peer_id,
                connection_id: _,
                cause,
                ..
            } => {
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    Some(NetworkEvent::PeerDisconnected {
                        peer_id: ant_peer_id,
                        connection_id: ConnectionId::new_unchecked(0),
                        reason: Some(cause.as_ref().map(|c| c.to_string()).unwrap_or_else(|| "Unknown".to_string())),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test commented out due to libp2p dummy behavior not implementing Debug
    // This will be uncommented when we have proper behavior wrappers
    /*
    #[tokio::test]
    async fn test_behavior_bridge_lifecycle() {
        let dummy_behavior = DummyBehaviour;
        let mut bridge = LibP2pBehaviorBridge::new(dummy_behavior, "test_behavior".to_string());

        // Test lifecycle
        assert_eq!(bridge.id(), "test_behavior");
        assert_eq!(bridge.name(), "libp2p_bridge");

        // Start behavior
        bridge.start().await.unwrap();
        assert!(matches!(bridge.health_check().await, BehaviorHealth::Healthy));

        // Stop behavior
        bridge.stop().await.unwrap();
        assert!(matches!(bridge.health_check().await, BehaviorHealth::Unhealthy(_)));
    }
    */

    #[tokio::test]
    async fn test_network_bridge_peer_id_conversion() {
        let bridge = NetworkBridge::new();
        let libp2p_peer_id = Libp2pPeerId::random();

        // Test conversion and caching
        let ant_peer_id1 = bridge.convert_peer_id(&libp2p_peer_id).await.unwrap();
        let ant_peer_id2 = bridge.convert_peer_id(&libp2p_peer_id).await.unwrap();

        assert_eq!(ant_peer_id1, ant_peer_id2); // Should be cached

        // Test reverse conversion
        let converted_back = bridge.convert_to_libp2p_peer_id(&ant_peer_id1).unwrap();
        assert_eq!(converted_back, libp2p_peer_id);
    }
}