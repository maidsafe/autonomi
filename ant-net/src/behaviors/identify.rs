// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Identify behavior wrapper for ant-net.
//!
//! This module provides an ant-net wrapper around libp2p's Identify behavior,
//! enabling peer discovery and capability exchange within the ant-net abstraction layer.

use crate::{
    behavior_manager::{BehaviorAction, BehaviorController, BehaviorHealth, StateRequest, StateResponse},
    event::NetworkEvent,
    types::PeerId,
    AntNetError, Result,
};
use async_trait::async_trait;
use libp2p::{
    identify::{Behaviour as IdentifyBehaviour, Config as IdentifyConfig, Event as IdentifyEvent},
    Multiaddr, PeerId as Libp2pPeerId,
};
use std::{
    collections::HashMap,
    fmt,
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

/// Configuration for the Identify behavior.
#[derive(Debug, Clone)]
pub struct IdentifyBehaviorConfig {
    /// The interval at which identification requests are sent.
    pub identify_interval: Duration,
    /// Whether to push identification info to newly connected peers.
    pub push_listen_addr_updates: bool,
    /// Whether to cache identification info for peers.
    pub cache_size: usize,
}

impl Default for IdentifyBehaviorConfig {
    fn default() -> Self {
        Self {
            identify_interval: Duration::from_secs(30),
            push_listen_addr_updates: true,
            cache_size: 1000,
        }
    }
}

/// ant-net wrapper for libp2p Identify behavior.
pub struct IdentifyBehaviorWrapper {
    /// The inner libp2p Identify behavior.
    #[allow(dead_code)]
    inner: IdentifyBehaviour,
    /// Behavior configuration.
    config: IdentifyBehaviorConfig,
    /// Local public key for recreating the behavior.
    local_public_key: libp2p::identity::PublicKey,
    /// Cached peer information.
    peer_cache: HashMap<PeerId, PeerInfo>,
    /// When the behavior was last active.
    last_activity: Instant,
    /// Statistics.
    stats: IdentifyStats,
}

/// Information about a discovered peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer's protocol version.
    pub protocol_version: String,
    /// Peer's agent version.
    pub agent_version: String,
    /// Peer's listen addresses.
    pub listen_addresses: Vec<Multiaddr>,
    /// Supported protocols.
    pub protocols: Vec<String>,
    /// When this info was last updated.
    pub updated_at: Instant,
}

/// Statistics for the Identify behavior.
#[derive(Debug, Clone, Default)]
pub struct IdentifyStats {
    /// Number of peers identified.
    pub peers_identified: u64,
    /// Number of identify requests sent.
    pub requests_sent: u64,
    /// Number of identify responses received.
    pub responses_received: u64,
    /// Number of errors encountered.
    pub errors: u64,
}

impl IdentifyBehaviorWrapper {
    /// Create a new Identify behavior wrapper.
    pub fn new(local_public_key: libp2p::identity::PublicKey, config: IdentifyBehaviorConfig) -> Self {
        let identify_config = IdentifyConfig::new("autonomi/1.0.0".to_string(), local_public_key.clone())
            .with_interval(config.identify_interval)
            .with_push_listen_addr_updates(config.push_listen_addr_updates);

        let inner = IdentifyBehaviour::new(identify_config);

        Self {
            inner,
            config,
            local_public_key,
            peer_cache: HashMap::new(),
            last_activity: Instant::now(),
            stats: IdentifyStats::default(),
        }
    }

    /// Get peer information from cache.
    pub fn get_peer_info(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peer_cache.get(peer_id)
    }

    /// Get all cached peer information.
    pub fn get_all_peers(&self) -> &HashMap<PeerId, PeerInfo> {
        &self.peer_cache
    }

    /// Get behavior statistics.
    pub fn stats(&self) -> &IdentifyStats {
        &self.stats
    }

    /// Clear the peer cache.
    pub fn clear_cache(&mut self) {
        self.peer_cache.clear();
        debug!("Cleared identify peer cache");
    }

    /// Remove stale entries from the cache.
    pub fn cleanup_cache(&mut self, max_age: Duration) {
        let now = Instant::now();
        let initial_len = self.peer_cache.len();
        
        self.peer_cache.retain(|_, info| {
            now.duration_since(info.updated_at) < max_age
        });

        let removed = initial_len - self.peer_cache.len();
        if removed > 0 {
            debug!("Removed {} stale entries from identify cache", removed);
        }
    }

    /// Convert libp2p Identify event to ant-net NetworkEvent.
    #[allow(dead_code)]
    fn convert_event(&mut self, event: IdentifyEvent) -> Option<NetworkEvent> {
        self.last_activity = Instant::now();

        match event {
            IdentifyEvent::Received { peer_id, info, .. } => {
                self.stats.responses_received += 1;
                self.stats.peers_identified += 1;

                // Convert libp2p PeerId to ant-net PeerId
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    // Cache the peer info
                    let peer_info = PeerInfo {
                        protocol_version: info.protocol_version,
                        agent_version: info.agent_version,
                        listen_addresses: info.listen_addrs,
                        protocols: info.protocols.into_iter().map(|p| p.to_string()).collect(),
                        updated_at: Instant::now(),
                    };

                    // Manage cache size
                    if self.peer_cache.len() >= self.config.cache_size {
                        self.cleanup_cache(Duration::from_secs(300)); // 5 minutes
                    }

                    self.peer_cache.insert(ant_peer_id, peer_info.clone());

                    info!("Identified peer {}: {} protocols", ant_peer_id, peer_info.protocols.len());

                    Some(NetworkEvent::PeerIdentified {
                        peer_id: ant_peer_id,
                        protocol_version: peer_info.protocol_version,
                        agent_version: peer_info.agent_version,
                        listen_addresses: peer_info.listen_addresses,
                        protocols: peer_info.protocols.into_iter()
                            .map(|p| crate::types::ProtocolId::from(p))
                            .collect(),
                        observed_address: None, // Would need to extract from identify info
                    })
                } else {
                    warn!("Failed to convert libp2p PeerId to ant-net PeerId");
                    None
                }
            }
            IdentifyEvent::Sent { peer_id, .. } => {
                self.stats.requests_sent += 1;
                
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    debug!("Sent identify request to peer {}", ant_peer_id);
                    
                    Some(NetworkEvent::BehaviorEvent {
                        peer_id: ant_peer_id,
                        behavior_id: "identify".to_string(),
                        event: "IdentifyRequestSent".to_string(),
                    })
                } else {
                    None
                }
            }
            IdentifyEvent::Pushed { peer_id, .. } => {
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    debug!("Pushed identify info to peer {}", ant_peer_id);
                    
                    Some(NetworkEvent::BehaviorEvent {
                        peer_id: ant_peer_id,
                        behavior_id: "identify".to_string(),
                        event: "IdentifyInfoPushed".to_string(),
                    })
                } else {
                    None
                }
            }
            IdentifyEvent::Error { peer_id, error, .. } => {
                self.stats.errors += 1;
                
                if let Ok(ant_peer_id) = PeerId::from_bytes(&peer_id.to_bytes()) {
                    warn!("Identify error with peer {}: {}", ant_peer_id, error);
                    
                    Some(NetworkEvent::BehaviorEvent {
                        peer_id: ant_peer_id,
                        behavior_id: "identify".to_string(),
                        event: format!("IdentifyError: {}", error),
                    })
                } else {
                    None
                }
            }
        }
    }
}

impl fmt::Debug for IdentifyBehaviorWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdentifyBehaviorWrapper")
            .field("config", &self.config)
            .field("cached_peers", &self.peer_cache.len())
            .field("stats", &self.stats)
            .finish()
    }
}

#[async_trait]
impl BehaviorController for IdentifyBehaviorWrapper {
    fn id(&self) -> String {
        "identify".to_string()
    }

    fn name(&self) -> &'static str {
        "identify"
    }

    async fn health_check(&self) -> BehaviorHealth {
        let since_last_activity = self.last_activity.elapsed();
        
        if since_last_activity > Duration::from_secs(300) { // 5 minutes
            BehaviorHealth::Degraded(format!(
                "No activity for {} seconds", 
                since_last_activity.as_secs()
            ))
        } else if self.stats.errors > 0 && self.stats.errors * 100 / (self.stats.requests_sent + 1) > 50 {
            BehaviorHealth::Degraded(format!(
                "High error rate: {}/{} requests failed",
                self.stats.errors,
                self.stats.requests_sent
            ))
        } else {
            BehaviorHealth::Healthy
        }
    }

    async fn handle_event(&mut self, event: NetworkEvent) -> Result<Vec<BehaviorAction>> {
        let actions = Vec::new();

        match event {
            NetworkEvent::PeerConnected { peer_id, .. } => {
                // Automatically identify newly connected peers
                if let Ok(_libp2p_peer_id) = Libp2pPeerId::from_bytes(&peer_id.to_bytes()) {
                    // Note: In a real implementation, we'd need to inject this into the swarm
                    debug!("Would send identify request to newly connected peer: {}", peer_id);
                }
            }
            NetworkEvent::RequestPeerIdentification { peer_id } => {
                // Manual identification request
                if let Ok(_libp2p_peer_id) = Libp2pPeerId::from_bytes(&peer_id.to_bytes()) {
                    debug!("Received manual identify request for peer: {}", peer_id);
                    // Would inject identify request into swarm
                }
            }
            _ => {
                // Not interested in other events
            }
        }

        Ok(actions)
    }

    async fn handle_state_request(&mut self, request: StateRequest) -> StateResponse {
        match request {
            StateRequest::GetClosestPeers { target: _, count } => {
                // Return closest peers from our cache based on simple heuristic
                let mut peers: Vec<_> = self.peer_cache
                    .keys()
                    .take(count)
                    .map(|peer_id| crate::types::PeerInfo {
                        peer_id: *peer_id,
                        addresses: crate::types::Addresses::new(), // Would need proper address tracking
                    })
                    .collect();
                
                peers.truncate(count);
                StateResponse::ClosestPeers(peers)
            }
            StateRequest::Custom { request_type, .. } if request_type == "get_peer_info" => {
                // Custom request to get cached peer info
                let peer_count = self.peer_cache.len();
                StateResponse::Custom {
                    response_type: "peer_info".to_string(),
                    data: format!("Cached {} peers", peer_count).into(),
                }
            }
            _ => StateResponse::Error("Unsupported state request".to_string()),
        }
    }

    fn is_interested(&self, event: &NetworkEvent) -> bool {
        matches!(event, 
            NetworkEvent::PeerConnected { .. } |
            NetworkEvent::PeerDisconnected { .. } |
            NetworkEvent::RequestPeerIdentification { .. }
        )
    }

    fn config_keys(&self) -> Vec<String> {
        vec![
            "identify_interval".to_string(),
            "push_listen_addr_updates".to_string(),
            "cache_size".to_string(),
        ]
    }

    async fn update_config(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "identify_interval" => {
                let interval: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid interval value".to_string()))?;
                self.config.identify_interval = Duration::from_secs(interval);
                info!("Updated identify interval to {} seconds", interval);
            }
            "push_listen_addr_updates" => {
                let push: bool = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid boolean value".to_string()))?;
                self.config.push_listen_addr_updates = push;
                info!("Updated push_listen_addr_updates to {}", push);
            }
            "cache_size" => {
                let size: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid cache size".to_string()))?;
                self.config.cache_size = size;
                info!("Updated cache size to {}", size);
                
                // Trim cache if necessary
                if self.peer_cache.len() > size {
                    self.cleanup_cache(Duration::from_secs(0)); // Remove oldest entries
                }
            }
            _ => {
                return Err(AntNetError::Configuration(format!(
                    "Unknown configuration key: {}", key
                )));
            }
        }
        Ok(())
    }

    fn clone_controller(&self) -> Box<dyn BehaviorController> {
        // Create a new instance with the same configuration but fresh behavior
        Box::new(IdentifyBehaviorWrapper::new(
            self.local_public_key.clone(),
            self.config.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ConnectionDirection;

    #[tokio::test]
    async fn test_identify_behavior_lifecycle() {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let config = IdentifyBehaviorConfig::default();
        let mut behavior = IdentifyBehaviorWrapper::new(keypair.public(), config);

        // Test basic properties
        assert_eq!(behavior.id(), "identify");
        assert_eq!(behavior.name(), "identify");
        assert!(matches!(behavior.health_check().await, BehaviorHealth::Healthy));

        // Test interest in events
        let connect_event = NetworkEvent::PeerConnected {
            peer_id: PeerId::random(),
            connection_id: crate::ConnectionId::new_unchecked(0),
            direction: ConnectionDirection::Outbound,
            endpoint: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        };
        assert!(behavior.is_interested(&connect_event));

        // Test configuration
        assert!(behavior.config_keys().contains(&"identify_interval".to_string()));
        behavior.update_config("identify_interval", "60").await.unwrap();
        assert_eq!(behavior.config.identify_interval, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_peer_cache_management() {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let config = IdentifyBehaviorConfig {
            cache_size: 2,
            ..Default::default()
        };
        let mut behavior = IdentifyBehaviorWrapper::new(keypair.public(), config);

        // Add peers to cache
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let peer3 = PeerId::random();

        behavior.peer_cache.insert(peer1, PeerInfo {
            protocol_version: "1.0".to_string(),
            agent_version: "test".to_string(),
            listen_addresses: vec![],
            protocols: vec!["test".to_string()],
            updated_at: Instant::now(),
        });

        behavior.peer_cache.insert(peer2, PeerInfo {
            protocol_version: "1.0".to_string(),
            agent_version: "test".to_string(),
            listen_addresses: vec![],
            protocols: vec!["test".to_string()],
            updated_at: Instant::now(),
        });

        assert_eq!(behavior.peer_cache.len(), 2);
        assert!(behavior.get_peer_info(&peer1).is_some());
        assert!(behavior.get_peer_info(&peer2).is_some());
        assert!(behavior.get_peer_info(&peer3).is_none());
    }
}