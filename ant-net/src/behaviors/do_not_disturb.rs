// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! DoNotDisturb behavior wrapper for ant-net.
//!
//! This module provides an ant-net wrapper around the DoNotDisturb behavior,
//! enabling nodes to request temporary exclusion from certain network operations.

use crate::{
    behavior_manager::{BehaviorAction, BehaviorController, BehaviorHealth, StateRequest, StateResponse},
    event::NetworkEvent,
    types::PeerId,
    AntNetError, Result,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

/// Maximum duration for do-not-disturb requests (5 minutes).
pub const MAX_DO_NOT_DISTURB_DURATION: Duration = Duration::from_secs(5 * 60);

/// Configuration for the DoNotDisturb behavior.
#[derive(Debug, Clone)]
pub struct DoNotDisturbConfig {
    /// Default duration for do-not-disturb requests.
    pub default_duration: Duration,
    /// Maximum number of concurrent DND entries.
    pub max_entries: usize,
    /// Cleanup interval for expired entries.
    pub cleanup_interval: Duration,
    /// Whether to auto-accept DND requests.
    pub auto_accept: bool,
}

impl Default for DoNotDisturbConfig {
    fn default() -> Self {
        Self {
            default_duration: Duration::from_secs(60),
            max_entries: 1000,
            cleanup_interval: Duration::from_secs(30),
            auto_accept: true,
        }
    }
}

/// Messages exchanged in the DoNotDisturb protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DoNotDisturbMessage {
    /// Request to be added to the remote peer's do-not-disturb list.
    Request {
        /// Duration in seconds for which the sender should not be disturbed.
        duration: u64,
    },
    /// Response to a do-not-disturb request.
    Response {
        /// Whether the request was accepted.
        accepted: bool,
        /// Optional reason if rejected.
        reason: Option<String>,
    },
}

/// Information about a peer in the do-not-disturb list.
#[derive(Debug, Clone)]
pub struct DoNotDisturbEntry {
    /// The peer ID.
    pub peer_id: PeerId,
    /// When the entry was created.
    pub created_at: Instant,
    /// When the entry expires.
    pub expires_at: Instant,
    /// The requested duration.
    pub duration: Duration,
}

impl DoNotDisturbEntry {
    /// Create a new DND entry.
    pub fn new(peer_id: PeerId, duration: Duration) -> Self {
        let now = Instant::now();
        Self {
            peer_id,
            created_at: now,
            expires_at: now + duration,
            duration,
        }
    }

    /// Check if this entry has expired.
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    /// Get remaining time until expiration.
    pub fn remaining_time(&self) -> Duration {
        self.expires_at.saturating_duration_since(Instant::now())
    }
}

/// Statistics for the DoNotDisturb behavior.
#[derive(Debug, Clone, Default)]
pub struct DoNotDisturbStats {
    /// Number of DND requests sent.
    pub requests_sent: u64,
    /// Number of DND requests received.
    pub requests_received: u64,
    /// Number of requests accepted.
    pub requests_accepted: u64,
    /// Number of requests rejected.
    pub requests_rejected: u64,
    /// Current number of active DND entries.
    pub active_entries: usize,
    /// Total number of expired entries cleaned up.
    pub expired_entries_cleaned: u64,
}

/// ant-net wrapper for DoNotDisturb behavior.
pub struct DoNotDisturbBehaviorWrapper {
    /// Behavior configuration.
    config: DoNotDisturbConfig,
    /// Active do-not-disturb entries.
    dnd_entries: HashMap<PeerId, DoNotDisturbEntry>,
    /// Peers we've requested DND from.
    outbound_requests: HashMap<PeerId, Instant>,
    /// Statistics.
    stats: DoNotDisturbStats,
    /// Last cleanup time.
    last_cleanup: Instant,
    /// Whether the behavior is active.
    active: bool,
}

impl DoNotDisturbBehaviorWrapper {
    /// Create a new DoNotDisturb behavior wrapper.
    pub fn new(config: DoNotDisturbConfig) -> Self {
        Self {
            config,
            dnd_entries: HashMap::new(),
            outbound_requests: HashMap::new(),
            stats: DoNotDisturbStats::default(),
            last_cleanup: Instant::now(),
            active: true,
        }
    }

    /// Create a new DoNotDisturb behavior with default configuration.
    pub fn with_default_config() -> Self {
        Self::new(DoNotDisturbConfig::default())
    }

    /// Check if a peer is in the do-not-disturb list.
    pub fn is_peer_in_dnd(&self, peer_id: &PeerId) -> bool {
        if let Some(entry) = self.dnd_entries.get(peer_id) {
            !entry.is_expired()
        } else {
            false
        }
    }

    /// Add a peer to the do-not-disturb list.
    pub fn add_peer_to_dnd(&mut self, peer_id: PeerId, duration: Duration) -> bool {
        let capped_duration = duration.min(MAX_DO_NOT_DISTURB_DURATION);
        
        if self.dnd_entries.len() >= self.config.max_entries {
            self.cleanup_expired_entries();
            if self.dnd_entries.len() >= self.config.max_entries {
                warn!("DND list is full, rejecting request from {}", peer_id);
                return false;
            }
        }

        let entry = DoNotDisturbEntry::new(peer_id, capped_duration);
        self.dnd_entries.insert(peer_id, entry);
        self.stats.active_entries = self.dnd_entries.len();
        
        info!("Added peer {} to DND list for {:?}", peer_id, capped_duration);
        true
    }

    /// Remove a peer from the do-not-disturb list.
    pub fn remove_peer_from_dnd(&mut self, peer_id: &PeerId) -> bool {
        if self.dnd_entries.remove(peer_id).is_some() {
            self.stats.active_entries = self.dnd_entries.len();
            info!("Removed peer {} from DND list", peer_id);
            true
        } else {
            false
        }
    }

    /// Get all active DND entries.
    pub fn get_active_entries(&self) -> Vec<&DoNotDisturbEntry> {
        self.dnd_entries
            .values()
            .filter(|entry| !entry.is_expired())
            .collect()
    }

    /// Request DND status from a peer.
    pub fn request_dnd_from_peer(&mut self, peer_id: PeerId, duration: Duration) {
        self.outbound_requests.insert(peer_id, Instant::now());
        self.stats.requests_sent += 1;
        debug!("Requesting DND from peer {} for {:?}", peer_id, duration);
    }

    /// Handle incoming DND request.
    pub fn handle_dnd_request(&mut self, peer_id: PeerId, duration: Duration) -> DoNotDisturbMessage {
        self.stats.requests_received += 1;

        if !self.config.auto_accept {
            self.stats.requests_rejected += 1;
            return DoNotDisturbMessage::Response {
                accepted: false,
                reason: Some("Auto-accept disabled".to_string()),
            };
        }

        let capped_duration = duration.min(MAX_DO_NOT_DISTURB_DURATION);
        if self.add_peer_to_dnd(peer_id, capped_duration) {
            self.stats.requests_accepted += 1;
            DoNotDisturbMessage::Response {
                accepted: true,
                reason: None,
            }
        } else {
            self.stats.requests_rejected += 1;
            DoNotDisturbMessage::Response {
                accepted: false,
                reason: Some("DND list full".to_string()),
            }
        }
    }

    /// Cleanup expired entries.
    pub fn cleanup_expired_entries(&mut self) {
        let initial_count = self.dnd_entries.len();
        self.dnd_entries.retain(|_, entry| !entry.is_expired());
        let removed_count = initial_count - self.dnd_entries.len();
        
        if removed_count > 0 {
            self.stats.expired_entries_cleaned += removed_count as u64;
            self.stats.active_entries = self.dnd_entries.len();
            debug!("Cleaned up {} expired DND entries", removed_count);
        }
        
        self.last_cleanup = Instant::now();
    }

    /// Perform periodic maintenance.
    pub fn periodic_maintenance(&mut self) {
        if self.last_cleanup.elapsed() >= self.config.cleanup_interval {
            self.cleanup_expired_entries();
        }

        // Clean up old outbound requests (older than 5 minutes)
        let cutoff = Instant::now() - Duration::from_secs(300);
        self.outbound_requests.retain(|_, &mut created_at| created_at > cutoff);
    }

    /// Get behavior statistics.
    pub fn stats(&self) -> &DoNotDisturbStats {
        &self.stats
    }

    /// Clear all DND entries.
    pub fn clear_all_entries(&mut self) {
        let count = self.dnd_entries.len();
        self.dnd_entries.clear();
        self.stats.active_entries = 0;
        info!("Cleared all {} DND entries", count);
    }
}

impl fmt::Debug for DoNotDisturbBehaviorWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DoNotDisturbBehaviorWrapper")
            .field("config", &self.config)
            .field("active_entries", &self.dnd_entries.len())
            .field("stats", &self.stats)
            .field("active", &self.active)
            .finish()
    }
}

#[async_trait]
impl BehaviorController for DoNotDisturbBehaviorWrapper {
    fn id(&self) -> String {
        "do_not_disturb".to_string()
    }

    fn name(&self) -> &'static str {
        "do_not_disturb"
    }

    async fn start(&mut self) -> Result<()> {
        self.active = true;
        info!("Started DoNotDisturb behavior");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.active = false;
        info!("Stopped DoNotDisturb behavior");
        Ok(())
    }

    async fn health_check(&self) -> BehaviorHealth {
        if !self.active {
            return BehaviorHealth::Unhealthy("Behavior is inactive".to_string());
        }

        let active_entries = self.get_active_entries().len();
        if active_entries > self.config.max_entries * 9 / 10 {
            BehaviorHealth::Degraded(format!(
                "DND list nearly full: {}/{}", 
                active_entries, 
                self.config.max_entries
            ))
        } else if self.last_cleanup.elapsed() > self.config.cleanup_interval * 2 {
            BehaviorHealth::Degraded("Cleanup overdue".to_string())
        } else {
            BehaviorHealth::Healthy
        }
    }

    async fn handle_event(&mut self, event: NetworkEvent) -> Result<Vec<BehaviorAction>> {
        if !self.active {
            return Ok(Vec::new());
        }

        // Perform periodic maintenance
        self.periodic_maintenance();

        let actions = Vec::new();

        match event {
            NetworkEvent::PeerConnected { peer_id, .. } => {
                // Check if we have a pending outbound request to this peer
                if self.outbound_requests.contains_key(&peer_id) {
                    debug!("Peer {} connected, could send DND request", peer_id);
                    // In a real implementation, we'd send the DND request here
                }
            }
            NetworkEvent::PeerDisconnected { peer_id, .. } => {
                // Remove from DND list when peer disconnects
                if self.remove_peer_from_dnd(&peer_id) {
                    debug!("Removed disconnected peer {} from DND list", peer_id);
                }
                // Clean up pending requests
                self.outbound_requests.remove(&peer_id);
            }
            NetworkEvent::RequestReceived { peer_id, data, response_channel, .. } => {
                // Try to decode as DND message
                if let Ok(dnd_message) = rmp_serde::from_slice::<DoNotDisturbMessage>(&data) {
                    match dnd_message {
                        DoNotDisturbMessage::Request { duration } => {
                            let duration = Duration::from_secs(duration);
                            let response = self.handle_dnd_request(peer_id, duration);
                            
                            // Send response
                            if let Ok(response_data) = rmp_serde::to_vec(&response) {
                                let _ = response_channel.send(response_data.into());
                            }
                        }
                        DoNotDisturbMessage::Response { accepted, reason } => {
                            if accepted {
                                info!("DND request accepted by peer {}", peer_id);
                            } else {
                                warn!("DND request rejected by peer {}: {:?}", peer_id, reason);
                            }
                            self.outbound_requests.remove(&peer_id);
                        }
                    }
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
            StateRequest::Custom { request_type, data } => {
                match request_type.as_str() {
                    "is_peer_in_dnd" => {
                        if let Ok(peer_id_bytes) = PeerId::from_bytes(&data) {
                            let in_dnd = self.is_peer_in_dnd(&peer_id_bytes);
                            StateResponse::Custom {
                                response_type: "peer_dnd_status".to_string(),
                                data: if in_dnd { b"true".to_vec().into() } else { b"false".to_vec().into() },
                            }
                        } else {
                            StateResponse::Error("Invalid peer ID".to_string())
                        }
                    }
                    "get_dnd_stats" => {
                        StateResponse::Custom {
                            response_type: "dnd_stats".to_string(),
                            data: format!("Active: {}, Requests: {}/{}", 
                                self.stats.active_entries,
                                self.stats.requests_received,
                                self.stats.requests_sent
                            ).into(),
                        }
                    }
                    _ => StateResponse::Error("Unknown request type".to_string()),
                }
            }
            _ => StateResponse::Error("Unsupported state request".to_string()),
        }
    }

    fn is_interested(&self, event: &NetworkEvent) -> bool {
        matches!(event,
            NetworkEvent::PeerConnected { .. } |
            NetworkEvent::PeerDisconnected { .. } |
            NetworkEvent::RequestReceived { .. }
        )
    }

    fn config_keys(&self) -> Vec<String> {
        vec![
            "default_duration".to_string(),
            "max_entries".to_string(),
            "cleanup_interval".to_string(),
            "auto_accept".to_string(),
        ]
    }

    async fn update_config(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "default_duration" => {
                let duration: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid duration value".to_string()))?;
                self.config.default_duration = Duration::from_secs(duration);
                info!("Updated default_duration to {} seconds", duration);
            }
            "max_entries" => {
                let max_entries: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid max_entries value".to_string()))?;
                self.config.max_entries = max_entries;
                info!("Updated max_entries to {}", max_entries);
            }
            "cleanup_interval" => {
                let interval: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid interval value".to_string()))?;
                self.config.cleanup_interval = Duration::from_secs(interval);
                info!("Updated cleanup_interval to {} seconds", interval);
            }
            "auto_accept" => {
                let auto_accept: bool = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid boolean value".to_string()))?;
                self.config.auto_accept = auto_accept;
                info!("Updated auto_accept to {}", auto_accept);
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
        Box::new(DoNotDisturbBehaviorWrapper::new(self.config.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ConnectionDirection;

    #[tokio::test]
    async fn test_dnd_behavior_lifecycle() {
        let config = DoNotDisturbConfig::default();
        let mut behavior = DoNotDisturbBehaviorWrapper::new(config);

        // Test basic properties
        assert_eq!(behavior.id(), "do_not_disturb");
        assert_eq!(behavior.name(), "do_not_disturb");
        assert!(matches!(behavior.health_check().await, BehaviorHealth::Healthy));

        // Test lifecycle
        behavior.start().await.unwrap();
        assert!(behavior.active);

        behavior.stop().await.unwrap();
        assert!(!behavior.active);
    }

    #[tokio::test]
    async fn test_dnd_entry_management() {
        let mut behavior = DoNotDisturbBehaviorWrapper::with_default_config();
        let peer_id = PeerId::random();
        let duration = Duration::from_secs(60);

        // Initially not in DND
        assert!(!behavior.is_peer_in_dnd(&peer_id));

        // Add to DND
        assert!(behavior.add_peer_to_dnd(peer_id, duration));
        assert!(behavior.is_peer_in_dnd(&peer_id));
        assert_eq!(behavior.stats().active_entries, 1);

        // Remove from DND
        assert!(behavior.remove_peer_from_dnd(&peer_id));
        assert!(!behavior.is_peer_in_dnd(&peer_id));
        assert_eq!(behavior.stats().active_entries, 0);
    }

    #[tokio::test]
    async fn test_dnd_request_handling() {
        let mut behavior = DoNotDisturbBehaviorWrapper::with_default_config();
        let peer_id = PeerId::random();
        let duration = Duration::from_secs(30);

        // Handle DND request
        let response = behavior.handle_dnd_request(peer_id, duration);
        match response {
            DoNotDisturbMessage::Response { accepted, .. } => {
                assert!(accepted);
            }
            _ => panic!("Expected Response message"),
        }

        // Verify peer was added to DND list
        assert!(behavior.is_peer_in_dnd(&peer_id));
        assert_eq!(behavior.stats().requests_received, 1);
        assert_eq!(behavior.stats().requests_accepted, 1);
    }

    #[tokio::test]
    async fn test_dnd_max_entries() {
        let config = DoNotDisturbConfig {
            max_entries: 2,
            ..Default::default()
        };
        let mut behavior = DoNotDisturbBehaviorWrapper::new(config);

        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let peer3 = PeerId::random();
        let duration = Duration::from_secs(60);

        // Add two peers (should succeed)
        assert!(behavior.add_peer_to_dnd(peer1, duration));
        assert!(behavior.add_peer_to_dnd(peer2, duration));

        // Try to add third peer (should fail)
        assert!(!behavior.add_peer_to_dnd(peer3, duration));
        assert_eq!(behavior.stats().active_entries, 2);
    }

    #[tokio::test]
    async fn test_dnd_entry_expiration() {
        let mut behavior = DoNotDisturbBehaviorWrapper::with_default_config();
        let peer_id = PeerId::random();
        let short_duration = Duration::from_millis(10);

        // Add peer with very short duration
        behavior.add_peer_to_dnd(peer_id, short_duration);
        assert!(behavior.is_peer_in_dnd(&peer_id));

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Should no longer be in DND after cleanup
        behavior.cleanup_expired_entries();
        assert!(!behavior.is_peer_in_dnd(&peer_id));
        assert_eq!(behavior.stats().active_entries, 0);
        assert!(behavior.stats().expired_entries_cleaned > 0);
    }

    #[tokio::test]
    async fn test_dnd_configuration() {
        let mut behavior = DoNotDisturbBehaviorWrapper::with_default_config();

        // Test configuration updates
        assert!(behavior.config_keys().contains(&"auto_accept".to_string()));
        
        behavior.update_config("auto_accept", "false").await.unwrap();
        assert!(!behavior.config.auto_accept);

        behavior.update_config("max_entries", "500").await.unwrap();
        assert_eq!(behavior.config.max_entries, 500);

        // Test invalid configuration
        assert!(behavior.update_config("invalid_key", "value").await.is_err());
        assert!(behavior.update_config("max_entries", "invalid").await.is_err());
    }

    #[tokio::test]
    async fn test_dnd_event_handling() {
        let mut behavior = DoNotDisturbBehaviorWrapper::with_default_config();
        behavior.start().await.unwrap();

        let peer_id = PeerId::random();

        // Test peer connection event
        let connect_event = NetworkEvent::PeerConnected {
            peer_id,
            connection_id: crate::ConnectionId::new_unchecked(0),
            direction: ConnectionDirection::Outbound,
            endpoint: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        };

        let actions = behavior.handle_event(connect_event).await.unwrap();
        assert!(actions.is_empty()); // No actions for basic connection

        // Test peer disconnection with cleanup
        behavior.add_peer_to_dnd(peer_id, Duration::from_secs(60));
        assert!(behavior.is_peer_in_dnd(&peer_id));

        let disconnect_event = NetworkEvent::PeerDisconnected {
            peer_id,
            connection_id: crate::ConnectionId::new_unchecked(0),
            reason: Some("Test disconnect".to_string()),
        };

        behavior.handle_event(disconnect_event).await.unwrap();
        assert!(!behavior.is_peer_in_dnd(&peer_id));
    }
}