// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Kademlia protocol implementation for iroh transport
//! 
//! This module implements the ALPN-based protocol handler that enables
//! Kademlia DHT operations over iroh's bidirectional streaming connections.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot, RwLock, Mutex},
    time::timeout,
};
use tracing::{debug, error, info, trace, warn};

// Placeholder types for iroh networking (real iroh types would be used in production)
pub type NodeId = [u8; 32];
pub type Connection = std::marker::PhantomData<()>;
pub type RecvStream = std::marker::PhantomData<()>;
pub type SendStream = std::marker::PhantomData<()>;
pub type Endpoint = std::marker::PhantomData<()>;

use crate::networking::{
    kad::transport::{KadMessage, KadResponse, KadError, KadPeerId, QueryId},
    iroh_adapter::{
        config::{ProtocolConfig, SerializationFormat},
        IrohError, IrohResult, KAD_ALPN, MAX_MESSAGE_SIZE,
    },
};

/// Request message with correlation ID for iroh transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KadRequest {
    /// Unique request ID for matching responses
    pub id: u64,
    /// The actual Kademlia message
    pub message: KadMessage,
    /// Timestamp when request was created
    pub timestamp: u64,
    /// Request sender for routing responses
    pub sender: KadPeerId,
}

/// Response message with correlation ID for iroh transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KadReply {
    /// Request ID this response corresponds to
    pub id: u64,
    /// The actual Kademlia response
    pub response: Result<KadResponse, String>,
    /// Timestamp when response was created
    pub timestamp: u64,
    /// Response sender
    pub sender: KadPeerId,
}

/// Internal message for request tracking
#[derive(Debug)]
struct PendingRequest {
    sender: oneshot::Sender<Result<KadResponse, KadError>>,
    created_at: Instant,
    peer: KadPeerId,
}

/// Statistics for protocol operations
#[derive(Debug, Clone, Default)]
pub struct ProtocolStats {
    pub requests_sent: u64,
    pub responses_received: u64,
    pub requests_received: u64,
    pub responses_sent: u64,
    pub timeouts: u64,
    pub errors: u64,
    pub avg_latency_ms: f64,
    pub active_connections: usize,
}

/// Core protocol handler for Kademlia over iroh
#[derive(Clone)]
pub struct KadProtocol {
    /// Local node identifier
    local_node_id: NodeId,
    
    /// Local Kademlia peer ID
    local_kad_peer_id: KadPeerId,
    
    /// Configuration for protocol behavior
    config: ProtocolConfig,
    
    /// Pending outbound requests awaiting responses
    pending_requests: Arc<Mutex<HashMap<u64, PendingRequest>>>,
    
    /// Next request ID counter
    next_request_id: Arc<AtomicU64>,
    
    /// Message handler for incoming Kademlia messages
    message_handler: Arc<dyn MessageHandler + Send + Sync>,
    
    /// Protocol statistics
    stats: Arc<RwLock<ProtocolStats>>,
    
    /// Active connections tracker
    connections: Arc<RwLock<HashMap<NodeId, ConnectionInfo>>>,
    
    /// Channel for cleanup tasks
    cleanup_tx: mpsc::UnboundedSender<CleanupTask>,
}

/// Information about an active connection
#[derive(Debug, Clone)]
struct ConnectionInfo {
    peer_id: KadPeerId,
    established_at: Instant,
    last_activity: Instant,
    message_count: u64,
}

/// Cleanup task for background maintenance
#[derive(Debug)]
enum CleanupTask {
    TimeoutRequest(u64),
    CleanupConnection(NodeId),
    UpdateStats,
}

/// Handler for incoming Kademlia messages
#[async_trait]
pub trait MessageHandler {
    /// Process an incoming Kademlia message and return a response
    async fn handle_message(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, KadError>;
    
    /// Handle connection events
    async fn on_connection_established(&self, peer: &KadPeerId) {}
    
    /// Handle disconnection events
    async fn on_connection_lost(&self, peer: &KadPeerId) {}
}

impl KadProtocol {
    /// Create a new Kademlia protocol handler
    pub fn new(
        local_node_id: NodeId,
        local_kad_peer_id: KadPeerId,
        config: ProtocolConfig,
        message_handler: Arc<dyn MessageHandler + Send + Sync>,
    ) -> Self {
        let (cleanup_tx, cleanup_rx) = mpsc::unbounded_channel();
        
        let protocol = Self {
            local_node_id,
            local_kad_peer_id,
            config,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            next_request_id: Arc::new(AtomicU64::new(1)),
            message_handler,
            stats: Arc::new(RwLock::new(ProtocolStats::default())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            cleanup_tx,
        };
        
        // Start cleanup task
        let cleanup_protocol = protocol.clone();
        tokio::spawn(async move {
            cleanup_protocol.cleanup_task(cleanup_rx).await;
        });
        
        protocol
    }
    
    /// Send a Kademlia message to a peer and await response
    pub async fn send_request(
        &self,
        endpoint: &Endpoint,
        peer_node_id: NodeId,
        peer_kad_id: KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, KadError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let (response_tx, response_rx) = oneshot::channel();
        
        // Track the pending request
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id, PendingRequest {
                sender: response_tx,
                created_at: Instant::now(),
                peer: peer_kad_id.clone(),
            });
        }
        
        // Schedule timeout cleanup
        let cleanup_tx = self.cleanup_tx.clone();
        let timeout_duration = self.config.request_timeout;
        tokio::spawn(async move {
            tokio::time::sleep(timeout_duration).await;
            let _ = cleanup_tx.send(CleanupTask::TimeoutRequest(request_id));
        });
        
        // Attempt to send the request
        let send_result = self.send_request_internal(
            endpoint,
            peer_node_id,
            request_id,
            message,
        ).await;
        
        match send_result {
            Ok(()) => {
                // Update stats
                {
                    let mut stats = self.stats.write().await;
                    stats.requests_sent += 1;
                }
                
                // Wait for response or timeout
                match timeout(self.config.request_timeout, response_rx).await {
                    Ok(Ok(response)) => {
                        self.update_latency_stats(request_id).await;
                        response
                    },
                    Ok(Err(_)) => {
                        // Channel closed - request was cancelled
                        Err(KadError::QueryFailed { reason: "Request cancelled".to_string() })
                    },
                    Err(_) => {
                        // Timeout
                        let mut stats = self.stats.write().await;
                        stats.timeouts += 1;
                        Err(KadError::Timeout { duration: self.config.request_timeout })
                    }
                }
            },
            Err(e) => {
                // Remove the pending request since sending failed
                self.pending_requests.lock().await.remove(&request_id);
                let mut stats = self.stats.write().await;
                stats.errors += 1;
                Err(e)
            }
        }
    }
    
    /// Internal method to send a request over iroh (placeholder implementation)
    async fn send_request_internal(
        &self,
        _endpoint: &Endpoint,
        peer_node_id: NodeId,
        request_id: u64,
        message: KadMessage,
    ) -> Result<(), KadError> {
        // Create request
        let request = KadRequest {
            id: request_id,
            message,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            sender: self.local_kad_peer_id.clone(),
        };
        
        // Serialize the request
        let request_bytes = self.serialize_message(&request)
            .map_err(|e| KadError::Transport(format!("Serialization failed: {}", e)))?;
        
        // Check message size
        if request_bytes.len() > self.config.max_message_size {
            return Err(KadError::Transport(format!(
                "Message too large: {} bytes (max: {})",
                request_bytes.len(),
                self.config.max_message_size
            )));
        }
        
        // Placeholder for actual iroh connection logic
        debug!("Would send {} bytes to peer {:?}", request_bytes.len(), peer_node_id);
        
        // Update connection info
        self.update_connection_info(peer_node_id, &self.node_id_to_kad_peer_id(peer_node_id)).await;
        
        Ok(())
    }
    
    /// Handle an incoming connection and process requests (placeholder implementation)
    pub async fn handle_connection(&self, _connection: Connection) -> IrohResult<()> {
        // Placeholder implementation - in real iroh this would handle incoming connections
        debug!("Placeholder: would handle incoming connection");
        Ok(())
    }
    
    /// Handle an individual bidirectional stream (placeholder implementation)
    async fn handle_stream(
        &self,
        _peer_kad_id: KadPeerId,
        _send_stream: SendStream,
        _recv_stream: RecvStream,
    ) -> IrohResult<()> {
        // Placeholder implementation - in real iroh this would handle bidirectional streams
        debug!("Placeholder: would handle bidirectional stream");
        Ok(())
    }
    
    /// Serialize a message using the configured format
    fn serialize_message<T: Serialize>(&self, message: &T) -> Result<Vec<u8>, IrohError> {
        match self.config.serialization_format {
            SerializationFormat::Postcard => {
                postcard::to_stdvec(message).map_err(IrohError::Serialization)
            },
            SerializationFormat::Bincode => {
                bincode::serialize(message)
                    .map_err(|e| IrohError::Protocol(format!("Bincode serialization failed: {}", e)))
            },
            SerializationFormat::Json => {
                serde_json::to_vec(message)
                    .map_err(|e| IrohError::Protocol(format!("JSON serialization failed: {}", e)))
            },
        }
    }
    
    /// Deserialize a message using the configured format
    fn deserialize_message<T>(&self, bytes: &[u8]) -> Result<T, IrohError>
    where
        T: for<'de> Deserialize<'de>,
    {
        match self.config.serialization_format {
            SerializationFormat::Postcard => {
                postcard::from_bytes(bytes).map_err(IrohError::Serialization)
            },
            SerializationFormat::Bincode => {
                bincode::deserialize(bytes)
                    .map_err(|e| IrohError::Protocol(format!("Bincode deserialization failed: {}", e)))
            },
            SerializationFormat::Json => {
                serde_json::from_slice(bytes)
                    .map_err(|e| IrohError::Protocol(format!("JSON deserialization failed: {}", e)))
            },
        }
    }
    
    /// Convert iroh NodeId to KadPeerId
    fn node_id_to_kad_peer_id(&self, node_id: NodeId) -> KadPeerId {
        KadPeerId::new(node_id.as_bytes().to_vec())
    }
    
    /// Update connection information and statistics
    async fn update_connection_info(&self, node_id: NodeId, kad_peer_id: &KadPeerId) {
        let mut connections = self.connections.write().await;
        let now = Instant::now();
        
        connections
            .entry(node_id)
            .and_modify(|info| {
                info.last_activity = now;
                info.message_count += 1;
            })
            .or_insert_with(|| ConnectionInfo {
                peer_id: kad_peer_id.clone(),
                established_at: now,
                last_activity: now,
                message_count: 1,
            });
        
        // Update stats
        let mut stats = self.stats.write().await;
        stats.active_connections = connections.len();
    }
    
    /// Update latency statistics for a completed request
    async fn update_latency_stats(&self, request_id: u64) {
        if let Some(pending) = self.pending_requests.lock().await.remove(&request_id) {
            let latency = pending.created_at.elapsed();
            let mut stats = self.stats.write().await;
            stats.responses_received += 1;
            
            // Simple rolling average - in production might want more sophisticated stats
            let latency_ms = latency.as_millis() as f64;
            if stats.avg_latency_ms == 0.0 {
                stats.avg_latency_ms = latency_ms;
            } else {
                stats.avg_latency_ms = (stats.avg_latency_ms * 0.9) + (latency_ms * 0.1);
            }
        }
    }
    
    /// Clean up a connection
    async fn cleanup_connection(&self, node_id: NodeId) {
        self.connections.write().await.remove(&node_id);
        
        // Update stats
        let connections_count = self.connections.read().await.len();
        let mut stats = self.stats.write().await;
        stats.active_connections = connections_count;
    }
    
    /// Background cleanup task
    async fn cleanup_task(&self, mut cleanup_rx: mpsc::UnboundedReceiver<CleanupTask>) {
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(30));
        
        loop {
            tokio::select! {
                task = cleanup_rx.recv() => {
                    match task {
                        Some(CleanupTask::TimeoutRequest(request_id)) => {
                            if let Some(pending) = self.pending_requests.lock().await.remove(&request_id) {
                                let _ = pending.sender.send(Err(KadError::Timeout { 
                                    duration: self.config.request_timeout 
                                }));
                                debug!("Timed out request {}", request_id);
                            }
                        },
                        Some(CleanupTask::CleanupConnection(node_id)) => {
                            self.cleanup_connection(node_id).await;
                        },
                        Some(CleanupTask::UpdateStats) => {
                            // Periodic stats update
                        },
                        None => break,
                    }
                },
                _ = cleanup_interval.tick() => {
                    self.periodic_cleanup().await;
                }
            }
        }
    }
    
    /// Periodic cleanup of stale state
    async fn periodic_cleanup(&self) {
        let now = Instant::now();
        let stale_threshold = Duration::from_secs(300); // 5 minutes
        
        // Clean up stale connections
        let mut connections = self.connections.write().await;
        connections.retain(|_, info| {
            now.duration_since(info.last_activity) < stale_threshold
        });
        
        // Update connection count stats
        let connection_count = connections.len();
        drop(connections);
        
        let mut stats = self.stats.write().await;
        stats.active_connections = connection_count;
        
        debug!("Periodic cleanup completed, {} active connections", connection_count);
    }
    
    /// Get current protocol statistics
    pub async fn stats(&self) -> ProtocolStats {
        self.stats.read().await.clone()
    }
    
    /// Get information about active connections
    pub async fn connection_info(&self) -> HashMap<NodeId, ConnectionInfo> {
        self.connections.read().await.clone()
    }
}