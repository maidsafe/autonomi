// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Protocol handling abstractions for ant-net.
//!
//! This module provides high-level interfaces for handling network protocols,
//! particularly request/response patterns and custom protocol implementations.

use crate::{
    event::{NetworkEvent, RequestId, ResponseChannel},
    types::ProtocolId,
    AntNetError, ConnectionId, PeerId, Result,
};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, RwLock};

/// Abstract request/response protocol interface.
///
/// This trait provides a high-level interface for request/response communication,
/// hiding the complexity of libp2p request/response handling.
#[async_trait]
pub trait RequestResponse: Send + Sync + fmt::Debug {
    /// Send a request to a peer and wait for a response.
    async fn send_request(
        &mut self,
        peer_id: PeerId,
        protocol: ProtocolId,
        request: Bytes,
        timeout: Duration,
    ) -> Result<Bytes>;

    /// Send a request without waiting for a response.
    async fn send_request_fire_and_forget(
        &mut self,
        peer_id: PeerId,
        protocol: ProtocolId,
        request: Bytes,
    ) -> Result<RequestId>;

    /// Send a response to a previous request.
    async fn send_response(
        &mut self,
        channel: ResponseChannel,
        response: Bytes,
    ) -> Result<()>;

    /// Register a protocol handler.
    async fn register_protocol(
        &mut self,
        protocol: ProtocolId,
        handler: Box<dyn ProtocolHandler>,
    ) -> Result<()>;

    /// Unregister a protocol handler.
    async fn unregister_protocol(&mut self, protocol: ProtocolId) -> Result<()>;

    /// Get protocol statistics.
    async fn protocol_stats(&self) -> ProtocolStats;
}

/// Protocol handler interface.
///
/// Implementations of this trait handle incoming requests for specific protocols.
#[async_trait]
pub trait ProtocolHandler: Send + Sync + fmt::Debug {
    /// Handle an incoming request.
    async fn handle_request(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        request: Bytes,
    ) -> Result<Bytes>;

    /// Get the protocol ID this handler manages.
    fn protocol_id(&self) -> ProtocolId;

    /// Clone the protocol handler.
    fn clone_handler(&self) -> Box<dyn ProtocolHandler>;
}

impl Clone for Box<dyn ProtocolHandler> {
    fn clone(&self) -> Self {
        self.clone_handler()
    }
}

/// Request context for tracking pending requests.
#[derive(Debug)]
struct PendingRequest {
    /// The peer the request was sent to.
    peer_id: PeerId,
    /// The protocol used.
    protocol: ProtocolId,
    /// When the request was sent.
    sent_at: Instant,
    /// Timeout for the request.
    timeout: Duration,
    /// Channel to send the response.
    response_sender: oneshot::Sender<Result<Bytes>>,
}

/// Protocol operation statistics.
#[derive(Debug, Clone, Default)]
pub struct ProtocolStats {
    /// Total number of requests sent.
    pub requests_sent: u64,
    /// Total number of responses received.
    pub responses_received: u64,
    /// Total number of requests received.
    pub requests_received: u64,
    /// Total number of responses sent.
    pub responses_sent: u64,
    /// Number of request timeouts.
    pub request_timeouts: u64,
    /// Number of request failures.
    pub request_failures: u64,
    /// Average request latency.
    pub average_request_latency: Duration,
    /// Currently pending requests.
    pub pending_requests: usize,
}

/// Default request/response implementation.
#[derive(Debug)]
pub struct DefaultRequestResponse {
    /// Pending outbound requests.
    pending_requests: RwLock<HashMap<RequestId, PendingRequest>>,
    /// Registered protocol handlers.
    protocol_handlers: RwLock<HashMap<ProtocolId, Box<dyn ProtocolHandler>>>,
    /// Protocol statistics.
    stats: RwLock<ProtocolStats>,
    /// Event sender for outbound requests.
    #[allow(dead_code)]
    event_sender: mpsc::UnboundedSender<NetworkEvent>,
}

impl DefaultRequestResponse {
    /// Create a new request/response handler.
    pub fn new(event_sender: mpsc::UnboundedSender<NetworkEvent>) -> Self {
        Self {
            pending_requests: RwLock::new(HashMap::new()),
            protocol_handlers: RwLock::new(HashMap::new()),
            stats: RwLock::new(ProtocolStats::default()),
            event_sender,
        }
    }

    /// Handle an incoming request event.
    pub async fn handle_incoming_request(
        &self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        protocol: ProtocolId,
        request_data: Bytes,
        response_channel: ResponseChannel,
    ) -> Result<()> {
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.requests_received += 1;
        }

        // Find the appropriate handler
        let handler = {
            let handlers = self.protocol_handlers.read().await;
            handlers.get(&protocol).cloned()
        };

        if let Some(mut handler) = handler {
            // Handle the request
            match handler.handle_request(peer_id, connection_id, request_data).await {
                Ok(response) => {
                    // Send the response
                    if response_channel.send(response).is_ok() {
                        let mut stats = self.stats.write().await;
                        stats.responses_sent += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Protocol handler for {} failed: {}",
                        protocol,
                        e
                    );
                    // Send an error response (empty for now)
                    let _ = response_channel.send(Bytes::new());
                }
            }
        } else {
            tracing::warn!("No handler registered for protocol: {}", protocol);
            // Send an error response
            let _ = response_channel.send(Bytes::new());
        }

        Ok(())
    }

    /// Handle an incoming response event.
    pub async fn handle_incoming_response(
        &self,
        peer_id: PeerId,
        protocol: ProtocolId,
        response_data: Bytes,
        request_id: RequestId,
    ) -> Result<()> {
        let pending_request = {
            let mut pending = self.pending_requests.write().await;
            pending.remove(&request_id)
        };

        if let Some(pending) = pending_request {
            // Verify the response matches the request
            if pending.peer_id == peer_id && pending.protocol == protocol {
                // Update stats
                {
                    let mut stats = self.stats.write().await;
                    stats.responses_received += 1;
                    
                    let latency = pending.sent_at.elapsed();
                    let total_latency = stats.average_request_latency * stats.responses_received as u32;
                    stats.average_request_latency = (total_latency + latency) / (stats.responses_received as u32);
                }

                // Send the response to the waiting request
                let _ = pending.response_sender.send(Ok(response_data));
            } else {
                tracing::warn!(
                    "Response mismatch: expected from {} on {}, got from {} on {}",
                    pending.peer_id, pending.protocol, peer_id, protocol
                );
            }
        } else {
            tracing::warn!("Received response for unknown request: {}", request_id);
        }

        Ok(())
    }

    /// Handle a request timeout.
    pub async fn handle_request_timeout(&self, request_id: RequestId) -> Result<()> {
        let pending_request = {
            let mut pending = self.pending_requests.write().await;
            pending.remove(&request_id)
        };

        if let Some(pending) = pending_request {
            // Update stats
            {
                let mut stats = self.stats.write().await;
                stats.request_timeouts += 1;
            }

            // Notify the waiting request
            let _ = pending.response_sender.send(Err(AntNetError::Protocol(
                "Request timed out".to_string(),
            )));
        }

        Ok(())
    }

    /// Clean up expired requests.
    pub async fn cleanup_expired_requests(&self) -> usize {
        let now = Instant::now();
        let mut expired_requests = Vec::new();

        {
            let pending = self.pending_requests.read().await;
            for (id, request) in pending.iter() {
                if now.duration_since(request.sent_at) > request.timeout {
                    expired_requests.push(id.clone());
                }
            }
        }

        let count = expired_requests.len();
        for id in expired_requests {
            self.handle_request_timeout(id).await.ok();
        }

        count
    }
}

#[async_trait]
impl RequestResponse for DefaultRequestResponse {
    async fn send_request(
        &mut self,
        peer_id: PeerId,
        protocol: ProtocolId,
        _request: Bytes,
        timeout: Duration,
    ) -> Result<Bytes> {
        let request_id = RequestId::new();
        let (response_sender, response_receiver) = oneshot::channel();

        // Store the pending request
        {
            let mut pending = self.pending_requests.write().await;
            pending.insert(
                request_id.clone(),
                PendingRequest {
                    peer_id,
                    protocol: protocol.clone(),
                    sent_at: Instant::now(),
                    timeout,
                    response_sender,
                },
            );
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.requests_sent += 1;
        }

        // Send the request event (this would be handled by the network driver)
        // For now, we'll just simulate success
        tracing::debug!(
            "Sending request {} to {} on protocol {}",
            request_id,
            peer_id,
            protocol
        );

        // Wait for the response
        match tokio::time::timeout(timeout, response_receiver).await {
            Ok(Ok(Ok(response))) => Ok(response),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err(AntNetError::Protocol("Response channel closed".to_string())),
            Err(_) => {
                // Handle timeout
                self.handle_request_timeout(request_id).await?;
                Err(AntNetError::Protocol("Request timed out".to_string()))
            }
        }
    }

    async fn send_request_fire_and_forget(
        &mut self,
        peer_id: PeerId,
        protocol: ProtocolId,
        _request: Bytes,
    ) -> Result<RequestId> {
        let request_id = RequestId::new();

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.requests_sent += 1;
        }

        // Send the request event
        tracing::debug!(
            "Sending fire-and-forget request {} to {} on protocol {}",
            request_id,
            peer_id,
            protocol
        );

        Ok(request_id)
    }

    async fn send_response(
        &mut self,
        channel: ResponseChannel,
        response: Bytes,
    ) -> Result<()> {
        channel.send(response).map_err(|_| {
            AntNetError::Protocol("Failed to send response".to_string())
        })?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.responses_sent += 1;
        }

        Ok(())
    }

    async fn register_protocol(
        &mut self,
        protocol: ProtocolId,
        handler: Box<dyn ProtocolHandler>,
    ) -> Result<()> {
        let mut handlers = self.protocol_handlers.write().await;
        handlers.insert(protocol.clone(), handler);
        tracing::debug!("Registered protocol handler for: {}", protocol);
        Ok(())
    }

    async fn unregister_protocol(&mut self, protocol: ProtocolId) -> Result<()> {
        let mut handlers = self.protocol_handlers.write().await;
        if handlers.remove(&protocol).is_some() {
            tracing::debug!("Unregistered protocol handler for: {}", protocol);
            Ok(())
        } else {
            Err(AntNetError::Protocol(format!(
                "Protocol {} not registered",
                protocol
            )))
        }
    }

    async fn protocol_stats(&self) -> ProtocolStats {
        let mut stats = self.stats.read().await.clone();
        stats.pending_requests = self.pending_requests.read().await.len();
        stats
    }
}

/// A simple echo protocol handler for testing.
#[derive(Debug, Clone)]
pub struct EchoProtocolHandler {
    protocol_id: ProtocolId,
}

impl EchoProtocolHandler {
    /// Create a new echo protocol handler.
    pub fn new() -> Self {
        Self {
            protocol_id: ProtocolId::new("echo"),
        }
    }
}

impl Default for EchoProtocolHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolHandler for EchoProtocolHandler {
    async fn handle_request(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        request: Bytes,
    ) -> Result<Bytes> {
        // Simply echo the request back
        Ok(request)
    }

    fn protocol_id(&self) -> ProtocolId {
        self.protocol_id.clone()
    }

    fn clone_handler(&self) -> Box<dyn ProtocolHandler> {
        Box::new(self.clone())
    }
}

/// Message codec trait for serializing/deserializing protocol messages.
pub trait MessageCodec<T>: Send + Sync + fmt::Debug {
    /// Encode a message to bytes.
    fn encode(&self, message: &T) -> Result<Bytes>;

    /// Decode bytes to a message.
    fn decode(&self, data: Bytes) -> Result<T>;
}

/// CBOR message codec implementation.
#[derive(Debug, Clone)]
pub struct CborCodec<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Default for CborCodec<T> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> CborCodec<T> {
    /// Create a new CBOR codec.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> MessageCodec<T> for CborCodec<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync + fmt::Debug,
{
    fn encode(&self, message: &T) -> Result<Bytes> {
        let encoded = serde_cbor::to_vec(message)
            .map_err(|e| AntNetError::Serialization(e.to_string()))?;
        Ok(Bytes::from(encoded))
    }

    fn decode(&self, data: Bytes) -> Result<T> {
        let decoded = serde_cbor::from_slice(&data)
            .map_err(|e| AntNetError::Serialization(e.to_string()))?;
        Ok(decoded)
    }
}

/// JSON message codec implementation.
#[derive(Debug, Clone)]
pub struct JsonCodec<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Default for JsonCodec<T> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> JsonCodec<T> {
    /// Create a new JSON codec.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> MessageCodec<T> for JsonCodec<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync + fmt::Debug,
{
    fn encode(&self, message: &T) -> Result<Bytes> {
        let encoded = serde_json::to_vec(message)
            .map_err(|e| AntNetError::Serialization(e.to_string()))?;
        Ok(Bytes::from(encoded))
    }

    fn decode(&self, data: Bytes) -> Result<T> {
        let decoded = serde_json::from_slice(&data)
            .map_err(|e| AntNetError::Serialization(e.to_string()))?;
        Ok(decoded)
    }
}

// Simple CBOR serialization stub
#[allow(dead_code)]
mod serde_cbor {
    use super::*;
    
    pub fn to_vec<T: Serialize>(_value: &T) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
        Err("serde_cbor not available".into())
    }
    
    pub fn from_slice<T: for<'de> Deserialize<'de>>(_slice: &[u8]) -> std::result::Result<T, Box<dyn std::error::Error>> {
        Err("serde_cbor not available".into())
    }
}