// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! RequestResponse behavior wrapper for ant-net.
//!
//! This module provides an ant-net wrapper around the libp2p RequestResponse behavior,
//! enabling structured request/response communication between peers.

use crate::{
    behavior_manager::{BehaviorAction, BehaviorController, BehaviorHealth, StateRequest, StateResponse},
    event::{NetworkEvent, RequestId, ResponseChannel},
    types::{PeerId, ProtocolId},
    AntNetError, Result,
};
use async_trait::async_trait;
use bytes::Bytes;
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use tracing::{debug, info, warn, error};

/// Configuration for the RequestResponse behavior.
#[derive(Debug, Clone)]
pub struct RequestResponseConfig {
    /// Request timeout duration.
    pub request_timeout: Duration,
    /// Maximum number of concurrent requests.
    pub max_concurrent_requests: usize,
    /// Maximum request size in bytes.
    pub max_request_size: usize,
    /// Maximum response size in bytes.
    pub max_response_size: usize,
    /// Connection keep-alive duration.
    pub connection_keep_alive: Duration,
}

impl Default for RequestResponseConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            max_concurrent_requests: 100,
            max_request_size: 1024 * 1024, // 1MB
            max_response_size: 1024 * 1024, // 1MB
            connection_keep_alive: Duration::from_secs(10),
        }
    }
}

/// Represents a pending outbound request.
#[derive(Debug)]
pub struct PendingRequest {
    /// The request ID.
    pub request_id: RequestId,
    /// The target peer.
    pub peer_id: PeerId,
    /// The protocol used.
    pub protocol: ProtocolId,
    /// When the request was sent.
    pub sent_at: Instant,
    /// Response channel to send the result.
    pub response_channel: Option<oneshot::Sender<Result<Bytes>>>,
    /// Request timeout.
    pub timeout: Duration,
}

impl PendingRequest {
    /// Check if this request has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.sent_at.elapsed() >= self.timeout
    }

    /// Get remaining time until timeout.
    pub fn remaining_time(&self) -> Duration {
        self.timeout.saturating_sub(self.sent_at.elapsed())
    }
}

/// Represents an inbound request waiting for response.
#[derive(Debug)]
pub struct InboundRequest {
    /// The request ID.
    pub request_id: RequestId,
    /// The source peer.
    pub peer_id: PeerId,
    /// The protocol used.
    pub protocol: ProtocolId,
    /// The request data.
    pub data: Bytes,
    /// When the request was received.
    pub received_at: Instant,
    /// Response channel.
    pub response_channel: ResponseChannel,
}

/// Statistics for the RequestResponse behavior.
#[derive(Debug, Clone, Default)]
pub struct RequestResponseStats {
    /// Number of requests sent.
    pub requests_sent: u64,
    /// Number of requests received.
    pub requests_received: u64,
    /// Number of responses sent.
    pub responses_sent: u64,
    /// Number of responses received.
    pub responses_received: u64,
    /// Number of request timeouts.
    pub request_timeouts: u64,
    /// Number of request failures.
    pub request_failures: u64,
    /// Current number of pending requests.
    pub pending_requests: usize,
    /// Current number of inbound requests.
    pub inbound_requests: usize,
    /// Average request latency.
    pub avg_request_latency: Duration,
}

/// ant-net wrapper for RequestResponse behavior.
pub struct RequestResponseBehaviorWrapper {
    /// Behavior configuration.
    config: RequestResponseConfig,
    /// Pending outbound requests.
    pending_requests: HashMap<RequestId, PendingRequest>,
    /// Inbound requests waiting for response.
    inbound_requests: HashMap<RequestId, InboundRequest>,
    /// Request queue for rate limiting.
    #[allow(dead_code)]
    request_queue: VecDeque<(PeerId, ProtocolId, Bytes, oneshot::Sender<Result<Bytes>>)>,
    /// Statistics.
    stats: RequestResponseStats,
    /// Whether the behavior is active.
    active: bool,
    /// Last cleanup time.
    last_cleanup: Instant,
}

impl RequestResponseBehaviorWrapper {
    /// Create a new RequestResponse behavior wrapper.
    pub fn new(config: RequestResponseConfig) -> Self {
        Self {
            config,
            pending_requests: HashMap::new(),
            inbound_requests: HashMap::new(),
            request_queue: VecDeque::new(),
            stats: RequestResponseStats::default(),
            active: true,
            last_cleanup: Instant::now(),
        }
    }

    /// Create a new RequestResponse behavior with default configuration.
    pub fn with_default_config() -> Self {
        Self::new(RequestResponseConfig::default())
    }

    /// Send a request to a peer.
    pub async fn send_request(
        &mut self,
        peer_id: PeerId,
        protocol: ProtocolId,
        data: Bytes,
    ) -> Result<oneshot::Receiver<Result<Bytes>>> {
        if !self.active {
            return Err(AntNetError::Behavior("Behavior is not active".to_string()));
        }

        if data.len() > self.config.max_request_size {
            return Err(AntNetError::Protocol(format!(
                "Request size {} exceeds maximum {}",
                data.len(),
                self.config.max_request_size
            )));
        }

        if self.pending_requests.len() >= self.config.max_concurrent_requests {
            return Err(AntNetError::Protocol(
                "Maximum concurrent requests exceeded".to_string()
            ));
        }

        let request_id = RequestId::new();
        let (response_tx, response_rx) = oneshot::channel();

        let pending_request = PendingRequest {
            request_id: request_id.clone(),
            peer_id,
            protocol: protocol.clone(),
            sent_at: Instant::now(),
            response_channel: Some(response_tx),
            timeout: self.config.request_timeout,
        };

        self.pending_requests.insert(request_id.clone(), pending_request);
        self.stats.requests_sent += 1;
        self.stats.pending_requests = self.pending_requests.len();

        debug!(
            "Sending request {} to peer {} on protocol {}",
            request_id, peer_id, protocol
        );

        // In a real implementation, this would trigger the actual libp2p request
        // For now, we just track it in our pending requests

        Ok(response_rx)
    }

    /// Handle an incoming request.
    pub fn handle_incoming_request(
        &mut self,
        peer_id: PeerId,
        protocol: ProtocolId,
        data: Bytes,
        response_channel: ResponseChannel,
    ) -> RequestId {
        let request_id = RequestId::new();
        
        let inbound_request = InboundRequest {
            request_id: request_id.clone(),
            peer_id,
            protocol: protocol.clone(),
            data,
            received_at: Instant::now(),
            response_channel,
        };

        self.inbound_requests.insert(request_id.clone(), inbound_request);
        self.stats.requests_received += 1;
        self.stats.inbound_requests = self.inbound_requests.len();

        debug!(
            "Received request {} from peer {} on protocol {}",
            request_id, peer_id, protocol
        );

        request_id
    }

    /// Send a response to an inbound request.
    pub fn send_response(&mut self, request_id: &RequestId, response_data: Bytes) -> Result<()> {
        if response_data.len() > self.config.max_response_size {
            return Err(AntNetError::Protocol(format!(
                "Response size {} exceeds maximum {}",
                response_data.len(),
                self.config.max_response_size
            )));
        }

        if let Some(inbound_request) = self.inbound_requests.remove(request_id) {
            if let Err(_) = inbound_request.response_channel.send(response_data) {
                warn!("Failed to send response for request {}", request_id);
                return Err(AntNetError::Protocol("Failed to send response".to_string()));
            }

            self.stats.responses_sent += 1;
            self.stats.inbound_requests = self.inbound_requests.len();

            debug!("Sent response for request {}", request_id);
            Ok(())
        } else {
            Err(AntNetError::Protocol(format!(
                "Request {} not found",
                request_id
            )))
        }
    }

    /// Handle an incoming response.
    pub fn handle_incoming_response(
        &mut self,
        request_id: &RequestId,
        response_data: Bytes,
    ) -> Result<()> {
        if let Some(pending_request) = self.pending_requests.remove(request_id) {
            let latency = pending_request.sent_at.elapsed();
            
            // Update average latency
            let total_responses = self.stats.responses_received + 1;
            self.stats.avg_request_latency = (self.stats.avg_request_latency * self.stats.responses_received as u32 + latency) / total_responses as u32;
            
            self.stats.responses_received += 1;
            self.stats.pending_requests = self.pending_requests.len();

            if let Some(response_channel) = pending_request.response_channel {
                if let Err(_) = response_channel.send(Ok(response_data)) {
                    warn!("Failed to deliver response for request {}", request_id);
                }
            }

            debug!("Received response for request {} (latency: {:?})", request_id, latency);
            Ok(())
        } else {
            warn!("Received response for unknown request {}", request_id);
            Err(AntNetError::Protocol(format!(
                "Unknown request ID: {}",
                request_id
            )))
        }
    }

    /// Handle a request failure.
    pub fn handle_request_failure(&mut self, request_id: &RequestId, error: String) {
        if let Some(pending_request) = self.pending_requests.remove(request_id) {
            self.stats.request_failures += 1;
            self.stats.pending_requests = self.pending_requests.len();

            if let Some(response_channel) = pending_request.response_channel {
                let _ = response_channel.send(Err(AntNetError::Protocol(error.clone())));
            }

            warn!("Request {} failed: {}", request_id, error);
        }
    }

    /// Cleanup timed out requests.
    pub fn cleanup_timed_out_requests(&mut self) {
        let mut timed_out_requests = Vec::new();

        for (request_id, pending_request) in &self.pending_requests {
            if pending_request.is_timed_out() {
                timed_out_requests.push(request_id.clone());
            }
        }

        for request_id in timed_out_requests {
            if let Some(pending_request) = self.pending_requests.remove(&request_id) {
                self.stats.request_timeouts += 1;
                self.stats.pending_requests = self.pending_requests.len();

                if let Some(response_channel) = pending_request.response_channel {
                    let _ = response_channel.send(Err(AntNetError::Protocol(
                        "Request timed out".to_string()
                    )));
                }

                warn!("Request {} timed out", request_id);
            }
        }

        self.last_cleanup = Instant::now();
    }

    /// Get behavior statistics.
    pub fn stats(&self) -> &RequestResponseStats {
        &self.stats
    }

    /// Get pending request info.
    pub fn get_pending_request(&self, request_id: &RequestId) -> Option<&PendingRequest> {
        self.pending_requests.get(request_id)
    }

    /// Get all pending requests.
    pub fn get_all_pending_requests(&self) -> &HashMap<RequestId, PendingRequest> {
        &self.pending_requests
    }

    /// Cancel a pending request.
    pub fn cancel_request(&mut self, request_id: &RequestId) -> bool {
        if let Some(pending_request) = self.pending_requests.remove(request_id) {
            self.stats.pending_requests = self.pending_requests.len();

            if let Some(response_channel) = pending_request.response_channel {
                let _ = response_channel.send(Err(AntNetError::Protocol(
                    "Request cancelled".to_string()
                )));
            }

            debug!("Cancelled request {}", request_id);
            true
        } else {
            false
        }
    }
}

impl fmt::Debug for RequestResponseBehaviorWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestResponseBehaviorWrapper")
            .field("config", &self.config)
            .field("pending_requests", &self.pending_requests.len())
            .field("inbound_requests", &self.inbound_requests.len())
            .field("stats", &self.stats)
            .field("active", &self.active)
            .finish()
    }
}

#[async_trait]
impl BehaviorController for RequestResponseBehaviorWrapper {
    fn id(&self) -> String {
        "request_response".to_string()
    }

    fn name(&self) -> &'static str {
        "request_response"
    }

    async fn start(&mut self) -> Result<()> {
        self.active = true;
        info!("Started RequestResponse behavior");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.active = false;
        
        // Cancel all pending requests
        let pending_request_ids: Vec<_> = self.pending_requests.keys().cloned().collect();
        for request_id in pending_request_ids {
            self.cancel_request(&request_id);
        }

        info!("Stopped RequestResponse behavior");
        Ok(())
    }

    async fn health_check(&self) -> BehaviorHealth {
        if !self.active {
            return BehaviorHealth::Unhealthy("Behavior is inactive".to_string());
        }

        let pending_count = self.pending_requests.len();
        let max_requests = self.config.max_concurrent_requests;

        if pending_count > max_requests * 9 / 10 {
            BehaviorHealth::Degraded(format!(
                "High pending request count: {}/{}", 
                pending_count, 
                max_requests
            ))
        } else if self.stats.request_failures > 0 && 
                 self.stats.request_failures * 100 / (self.stats.requests_sent + 1) > 50 {
            BehaviorHealth::Degraded(format!(
                "High failure rate: {}/{} requests failed",
                self.stats.request_failures,
                self.stats.requests_sent
            ))
        } else {
            BehaviorHealth::Healthy
        }
    }

    async fn handle_event(&mut self, event: NetworkEvent) -> Result<Vec<BehaviorAction>> {
        if !self.active {
            return Ok(Vec::new());
        }

        // Periodic cleanup
        if self.last_cleanup.elapsed() >= Duration::from_secs(10) {
            self.cleanup_timed_out_requests();
        }

        let mut actions = Vec::new();

        match event {
            NetworkEvent::RequestReceived { peer_id, protocol, data, response_channel, .. } => {
                // Handle incoming request
                let request_id = self.handle_incoming_request(peer_id, protocol.clone(), data.clone(), response_channel);
                
                // Generate action to notify upper layers
                actions.push(BehaviorAction::SendEvent {
                    event: NetworkEvent::RequestReceived {
                        peer_id,
                        connection_id: crate::ConnectionId::new_unchecked(0), // Would be properly set in real implementation
                        protocol,
                        data,
                        response_channel: {
                            let (channel, _) = ResponseChannel::new();
                            channel
                        },
                    },
                    correlation_id: Some(request_id.0.parse().unwrap_or(0)),
                });
            }
            NetworkEvent::ResponseReceived { peer_id, data, request_id, .. } => {
                // Handle incoming response
                if let Err(e) = self.handle_incoming_response(&request_id, data) {
                    error!("Failed to handle response from {}: {}", peer_id, e);
                }
            }
            NetworkEvent::RequestTimeout { peer_id: _, request_id, .. } => {
                self.handle_request_failure(&request_id, "Request timed out".to_string());
            }
            NetworkEvent::RequestFailed { peer_id: _, request_id, error, .. } => {
                self.handle_request_failure(&request_id, error);
            }
            NetworkEvent::PeerDisconnected { peer_id, .. } => {
                // Cancel requests to disconnected peer
                let mut to_cancel = Vec::new();
                for (request_id, pending_request) in &self.pending_requests {
                    if pending_request.peer_id == peer_id {
                        to_cancel.push(request_id.clone());
                    }
                }
                
                for request_id in to_cancel {
                    self.handle_request_failure(&request_id, "Peer disconnected".to_string());
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
            StateRequest::Custom { request_type, data: _ } => {
                match request_type.as_str() {
                    "get_request_stats" => {
                        StateResponse::Custom {
                            response_type: "request_stats".to_string(),
                            data: format!(
                                "Sent: {}, Received: {}, Pending: {}, Failures: {}",
                                self.stats.requests_sent,
                                self.stats.requests_received,
                                self.stats.pending_requests,
                                self.stats.request_failures
                            ).into(),
                        }
                    }
                    "get_pending_count" => {
                        StateResponse::Custom {
                            response_type: "pending_count".to_string(),
                            data: self.stats.pending_requests.to_string().into(),
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
            NetworkEvent::RequestReceived { .. } |
            NetworkEvent::ResponseReceived { .. } |
            NetworkEvent::RequestTimeout { .. } |
            NetworkEvent::RequestFailed { .. } |
            NetworkEvent::PeerDisconnected { .. }
        )
    }

    fn config_keys(&self) -> Vec<String> {
        vec![
            "request_timeout".to_string(),
            "max_concurrent_requests".to_string(),
            "max_request_size".to_string(),
            "max_response_size".to_string(),
            "connection_keep_alive".to_string(),
        ]
    }

    async fn update_config(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "request_timeout" => {
                let timeout: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid timeout value".to_string()))?;
                self.config.request_timeout = Duration::from_secs(timeout);
                info!("Updated request_timeout to {} seconds", timeout);
            }
            "max_concurrent_requests" => {
                let max_requests: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid max_requests value".to_string()))?;
                self.config.max_concurrent_requests = max_requests;
                info!("Updated max_concurrent_requests to {}", max_requests);
            }
            "max_request_size" => {
                let max_size: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid max_size value".to_string()))?;
                self.config.max_request_size = max_size;
                info!("Updated max_request_size to {}", max_size);
            }
            "max_response_size" => {
                let max_size: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid max_size value".to_string()))?;
                self.config.max_response_size = max_size;
                info!("Updated max_response_size to {}", max_size);
            }
            "connection_keep_alive" => {
                let keep_alive: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid keep_alive value".to_string()))?;
                self.config.connection_keep_alive = Duration::from_secs(keep_alive);
                info!("Updated connection_keep_alive to {} seconds", keep_alive);
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
        Box::new(RequestResponseBehaviorWrapper::new(self.config.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_request_response_lifecycle() {
        let config = RequestResponseConfig::default();
        let mut behavior = RequestResponseBehaviorWrapper::new(config);

        // Test basic properties
        assert_eq!(behavior.id(), "request_response");
        assert_eq!(behavior.name(), "request_response");
        assert!(matches!(behavior.health_check().await, BehaviorHealth::Healthy));

        // Test lifecycle
        behavior.start().await.unwrap();
        assert!(behavior.active);

        behavior.stop().await.unwrap();
        assert!(!behavior.active);
    }

    #[tokio::test]
    async fn test_send_request() {
        let mut behavior = RequestResponseBehaviorWrapper::with_default_config();
        let peer_id = PeerId::random();
        let protocol = ProtocolId::from("test_protocol");
        let data = Bytes::from("test_request");

        // Send request
        let _response_rx = behavior.send_request(peer_id, protocol, data).await.unwrap();
        assert_eq!(behavior.stats().requests_sent, 1);
        assert_eq!(behavior.stats().pending_requests, 1);

        // Check pending request exists
        assert_eq!(behavior.get_all_pending_requests().len(), 1);
    }

    #[tokio::test]
    async fn test_handle_incoming_request() {
        let mut behavior = RequestResponseBehaviorWrapper::with_default_config();
        let peer_id = PeerId::random();
        let protocol = ProtocolId::from("test_protocol");
        let data = Bytes::from("test_request");
        let (response_channel, _) = ResponseChannel::new();

        // Handle incoming request
        let request_id = behavior.handle_incoming_request(peer_id, protocol, data.clone(), response_channel);
        assert_eq!(behavior.stats().requests_received, 1);
        assert_eq!(behavior.stats().inbound_requests, 1);

        // Send response
        let response_data = Bytes::from("test_response");
        behavior.send_response(&request_id, response_data).unwrap();
        assert_eq!(behavior.stats().responses_sent, 1);
        assert_eq!(behavior.stats().inbound_requests, 0);
    }

    #[tokio::test]
    async fn test_request_timeout() {
        let config = RequestResponseConfig {
            request_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let mut behavior = RequestResponseBehaviorWrapper::new(config);
        let peer_id = PeerId::random();
        let protocol = ProtocolId::from("test_protocol");
        let data = Bytes::from("test_request");

        // Send request with short timeout
        let _response_rx = behavior.send_request(peer_id, protocol, data).await.unwrap();
        assert_eq!(behavior.pending_requests.len(), 1);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Cleanup should remove timed out request
        behavior.cleanup_timed_out_requests();
        assert_eq!(behavior.pending_requests.len(), 0);
        assert_eq!(behavior.stats().request_timeouts, 1);
    }

    #[tokio::test]
    async fn test_max_concurrent_requests() {
        let config = RequestResponseConfig {
            max_concurrent_requests: 2,
            ..Default::default()
        };
        let mut behavior = RequestResponseBehaviorWrapper::new(config);
        let peer_id = PeerId::random();
        let protocol = ProtocolId::from("test_protocol");
        let data = Bytes::from("test_request");

        // Send two requests (should succeed)
        behavior.send_request(peer_id, protocol.clone(), data.clone()).await.unwrap();
        behavior.send_request(peer_id, protocol.clone(), data.clone()).await.unwrap();
        assert_eq!(behavior.pending_requests.len(), 2);

        // Try to send third request (should fail)
        assert!(behavior.send_request(peer_id, protocol, data).await.is_err());
        assert_eq!(behavior.pending_requests.len(), 2);
    }

    #[tokio::test]
    async fn test_request_response_flow() {
        let mut behavior = RequestResponseBehaviorWrapper::with_default_config();
        let peer_id = PeerId::random();
        let protocol = ProtocolId::from("test_protocol");
        let request_data = Bytes::from("test_request");
        let response_data = Bytes::from("test_response");

        // Send request
        let response_rx = behavior.send_request(peer_id, protocol, request_data).await.unwrap();
        assert_eq!(behavior.stats().requests_sent, 1);

        // Get the request ID from pending requests
        let request_id = behavior.pending_requests.keys().next().unwrap().clone();

        // Simulate receiving response
        behavior.handle_incoming_response(&request_id, response_data.clone()).unwrap();
        assert_eq!(behavior.stats().responses_received, 1);
        assert_eq!(behavior.pending_requests.len(), 0);

        // The response should be delivered to the channel
        let received_response = response_rx.await.unwrap().unwrap();
        assert_eq!(received_response, response_data);
    }

    #[tokio::test]
    async fn test_configuration_updates() {
        let mut behavior = RequestResponseBehaviorWrapper::with_default_config();

        // Test timeout configuration
        behavior.update_config("request_timeout", "60").await.unwrap();
        assert_eq!(behavior.config.request_timeout, Duration::from_secs(60));

        // Test max requests configuration
        behavior.update_config("max_concurrent_requests", "200").await.unwrap();
        assert_eq!(behavior.config.max_concurrent_requests, 200);

        // Test invalid configuration
        assert!(behavior.update_config("invalid_key", "value").await.is_err());
        assert!(behavior.update_config("request_timeout", "invalid").await.is_err());
    }
}