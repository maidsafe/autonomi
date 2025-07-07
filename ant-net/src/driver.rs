// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Network driver implementation for ant-net.
//!
//! This module provides the main network driver that coordinates all networking
//! functionality, including transport, behaviors, connections, and protocols.

use crate::{
    behavior::BehaviourComposer,
    connection::{ConnectionManager, DefaultConnectionManager},
    event::NetworkEvent,
    protocol::{DefaultRequestResponse, RequestResponse},
    transport::Transport,
    types::Addresses,
    AntNetError, ConnectionId, Multiaddr, PeerId, Result,
};
use async_trait::async_trait;
use libp2p::identity::Keypair;
use std::{fmt, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, RwLock},
    task::JoinHandle,
    time::interval,
};

/// Main network driver interface.
///
/// The network driver coordinates all networking functionality and provides
/// a high-level interface for network operations.
#[async_trait]
pub trait NetworkDriver: Send + Sync + fmt::Debug {
    /// Start the network driver.
    async fn start(&mut self) -> Result<()>;

    /// Stop the network driver.
    async fn stop(&mut self) -> Result<()>;

    /// Check if the driver is running.
    async fn is_running(&self) -> bool;

    /// Get the local peer ID.
    fn peer_id(&self) -> PeerId;

    /// Get the local listening addresses.
    async fn local_addresses(&self) -> Vec<Multiaddr>;

    /// Connect to a peer.
    async fn connect_peer(&mut self, peer_id: PeerId, addresses: Addresses) -> Result<ConnectionId>;

    /// Disconnect from a peer.
    async fn disconnect_peer(&mut self, peer_id: PeerId) -> Result<()>;

    /// Send a request to a peer.
    async fn send_request(
        &mut self,
        peer_id: PeerId,
        protocol: crate::types::ProtocolId,
        request: bytes::Bytes,
        timeout: Duration,
    ) -> Result<bytes::Bytes>;

    /// Get network statistics.
    async fn network_stats(&self) -> NetworkStats;
}

/// Network statistics.
#[derive(Debug, Clone, Default)]
pub struct NetworkStats {
    /// Number of connected peers.
    pub connected_peers: usize,
    /// Total number of connections.
    pub total_connections: usize,
    /// Number of inbound connections.
    pub inbound_connections: usize,
    /// Number of outbound connections.
    pub outbound_connections: usize,
    /// Network uptime.
    pub uptime: Duration,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
}

/// Main ant-net network driver implementation.
#[derive(Debug)]
pub struct AntNet {
    /// The local peer ID.
    peer_id: PeerId,
    /// The cryptographic keypair.
    keypair: Keypair,
    /// Transport configuration.
    transport: Box<dyn Transport>,
    /// Behavior composer.
    behaviors: BehaviourComposer,
    /// Connection manager.
    connection_manager: Arc<RwLock<DefaultConnectionManager>>,
    /// Request/response handler.
    request_response: Arc<RwLock<DefaultRequestResponse>>,
    /// Event sender.
    event_sender: mpsc::UnboundedSender<NetworkEvent>,
    /// Event receiver.
    event_receiver: Arc<RwLock<Option<mpsc::UnboundedReceiver<NetworkEvent>>>>,
    /// Background task handles.
    tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
    /// Whether the driver is running.
    running: Arc<RwLock<bool>>,
    /// Local listening addresses.
    local_addresses: Arc<RwLock<Vec<Multiaddr>>>,
    /// Network statistics.
    stats: Arc<RwLock<NetworkStats>>,
}

impl AntNet {
    /// Create a new ant-net driver.
    pub fn new(
        keypair: Keypair,
        transport: Box<dyn Transport>,
        behaviors: BehaviourComposer,
    ) -> Self {
        let peer_id = PeerId::from(keypair.public());
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let connection_manager = Arc::new(RwLock::new(DefaultConnectionManager::new()));
        let request_response = Arc::new(RwLock::new(DefaultRequestResponse::new(event_sender.clone())));

        Self {
            peer_id,
            keypair,
            transport,
            behaviors,
            connection_manager,
            request_response,
            event_sender,
            event_receiver: Arc::new(RwLock::new(Some(event_receiver))),
            tasks: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(RwLock::new(false)),
            local_addresses: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(NetworkStats::default())),
        }
    }

    /// Get the connection manager.
    pub fn connection_manager(&self) -> Arc<RwLock<DefaultConnectionManager>> {
        self.connection_manager.clone()
    }

    /// Get the request/response handler.
    pub fn request_response(&self) -> Arc<RwLock<DefaultRequestResponse>> {
        self.request_response.clone()
    }

    /// Get the event sender.
    pub fn event_sender(&self) -> mpsc::UnboundedSender<NetworkEvent> {
        self.event_sender.clone()
    }

    /// Handle a network event.
    #[allow(dead_code)]
    async fn handle_event(&mut self, event: NetworkEvent) -> Result<()> {
        tracing::debug!("Handling network event: {:?}", event);

        match &event {
            NetworkEvent::PeerConnected { peer_id, connection_id, .. } => {
                // Update connection tracking
                // This would be implemented with actual connection info
                tracing::info!("Peer connected: {} via {}", peer_id, connection_id);
            }
            NetworkEvent::PeerDisconnected { peer_id, connection_id, .. } => {
                // Update connection tracking
                self.connection_manager.write().await.remove_connection(*connection_id).await;
                tracing::info!("Peer disconnected: {} via {}", peer_id, connection_id);
            }
            NetworkEvent::RequestReceived { 
                peer_id, 
                connection_id: _, 
                protocol, 
                data: _, 
                response_channel: _ 
            } => {
                // Handle incoming request
                // TODO: Implement proper response channel handling
                // The response_channel can't be cloned, so we need to restructure this
                tracing::debug!("Received request from {} on protocol {}", peer_id, protocol);
            }
            NetworkEvent::ResponseReceived { 
                peer_id, 
                connection_id: _,
                protocol, 
                data, 
                request_id 
            } => {
                // Handle incoming response
                self.request_response
                    .read()
                    .await
                    .handle_incoming_response(
                        *peer_id,
                        protocol.clone(),
                        data.clone(),
                        request_id.clone(),
                    )
                    .await?;
            }
            _ => {
                // Handle other events
            }
        }

        // Forward the event to behaviors
        let mut behaviors = self.behaviors.clone();
        if let Err(e) = behaviors.handle_event(event).await {
            tracing::warn!("Error handling event in behaviors: {}", e);
        }

        Ok(())
    }

    /// Start the event processing loop.
    async fn start_event_loop(&mut self) -> Result<()> {
        let mut event_receiver = {
            let mut receiver_lock = self.event_receiver.write().await;
            receiver_lock.take().ok_or_else(|| {
                AntNetError::Driver("Event loop already started".to_string())
            })?
        };

        let _event_sender = self.event_sender.clone();
        
        // Clone what we need for the event loop
        let connection_manager = self.connection_manager.clone();
        let _request_response = self.request_response.clone();
        let mut behaviors = self.behaviors.clone();

        // Start the main event processing task
        let event_task = tokio::spawn(async move {
            while let Some(event) = event_receiver.recv().await {
                tracing::trace!("Processing event: {:?}", event);

                // Handle the event through behaviors
                if let Err(e) = behaviors.handle_event(event.clone()).await {
                    tracing::warn!("Error handling event in behaviors: {}", e);
                }

                // Handle specific event types
                match event {
                    NetworkEvent::PeerConnected { .. } => {
                        // Connection tracking would be handled here
                    }
                    NetworkEvent::PeerDisconnected { connection_id, .. } => {
                        // Remove from connection tracking
                        connection_manager.write().await.remove_connection(connection_id).await;
                    }
                    NetworkEvent::RequestReceived { .. } => {
                        // Request handling would be done here
                    }
                    NetworkEvent::ResponseReceived { .. } => {
                        // Response handling would be done here
                    }
                    _ => {}
                }
            }
            tracing::info!("Event loop terminated");
        });

        // Start cleanup task
        let cleanup_task = {
            let connection_manager = self.connection_manager.clone();
            let request_response = self.request_response.clone();
            
            tokio::spawn(async move {
                let mut cleanup_interval = interval(Duration::from_secs(30));
                
                loop {
                    cleanup_interval.tick().await;
                    
                    // Clean up stale connections
                    let stale_count = connection_manager
                        .write()
                        .await
                        .cleanup_stale_connections(Duration::from_secs(300))
                        .await;
                    
                    if stale_count > 0 {
                        tracing::debug!("Cleaned up {} stale connections", stale_count);
                    }
                    
                    // Clean up expired requests
                    let expired_count = request_response
                        .read()
                        .await
                        .cleanup_expired_requests()
                        .await;
                    
                    if expired_count > 0 {
                        tracing::debug!("Cleaned up {} expired requests", expired_count);
                    }
                }
            })
        };

        // Store task handles
        {
            let mut tasks = self.tasks.write().await;
            tasks.push(event_task);
            tasks.push(cleanup_task);
        }

        Ok(())
    }

    /// Stop all background tasks.
    async fn stop_tasks(&mut self) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        
        // Abort all tasks
        for task in tasks.drain(..) {
            task.abort();
        }

        Ok(())
    }
}

#[async_trait]
impl NetworkDriver for AntNet {
    async fn start(&mut self) -> Result<()> {
        {
            let running = self.running.read().await;
            if *running {
                return Err(AntNetError::Driver("Driver already running".to_string()));
            }
        }

        tracing::info!("Starting AntNet driver for peer {}", self.peer_id);

        // Build the transport
        #[cfg(feature = "metrics")]
        let mut metrics = crate::transport::MetricsRegistries::new();
        
        let _transport = self.transport.build(
            &self.keypair,
            #[cfg(feature = "metrics")]
            &mut metrics,
        )?;

        // Start the event loop
        self.start_event_loop().await?;

        {
            let mut running = self.running.write().await;
            *running = true;
        }
        tracing::info!("AntNet driver started successfully");

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        {
            let running = self.running.read().await;
            if !*running {
                return Err(AntNetError::Driver("Driver not running".to_string()));
            }
        }

        tracing::info!("Stopping AntNet driver");

        // Stop all background tasks
        self.stop_tasks().await?;

        {
            let mut running = self.running.write().await;
            *running = false;
        }
        tracing::info!("AntNet driver stopped");

        Ok(())
    }

    async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    async fn local_addresses(&self) -> Vec<Multiaddr> {
        self.local_addresses.read().await.clone()
    }

    async fn connect_peer(&mut self, peer_id: PeerId, addresses: Addresses) -> Result<ConnectionId> {
        if !self.is_running().await {
            return Err(AntNetError::Driver("Driver not running".to_string()));
        }

        // Use the connection manager to initiate the connection
        self.connection_manager
            .write()
            .await
            .dial_peer(peer_id, addresses)
            .await
    }

    async fn disconnect_peer(&mut self, peer_id: PeerId) -> Result<()> {
        if !self.is_running().await {
            return Err(AntNetError::Driver("Driver not running".to_string()));
        }

        // Close all connections to the peer
        let count = self.connection_manager
            .write()
            .await
            .close_peer_connections(peer_id)
            .await?;

        tracing::debug!("Disconnected {} connections from peer {}", count, peer_id);
        Ok(())
    }

    async fn send_request(
        &mut self,
        peer_id: PeerId,
        protocol: crate::types::ProtocolId,
        request: bytes::Bytes,
        timeout: Duration,
    ) -> Result<bytes::Bytes> {
        if !self.is_running().await {
            return Err(AntNetError::Driver("Driver not running".to_string()));
        }

        // Use the request/response handler
        self.request_response
            .write()
            .await
            .send_request(peer_id, protocol, request, timeout)
            .await
    }

    async fn network_stats(&self) -> NetworkStats {
        let mut stats = self.stats.read().await.clone();

        // Update connection stats
        let connection_stats = self.connection_manager.read().await.connection_stats().await;
        stats.connected_peers = connection_stats.connected_peers;
        stats.total_connections = connection_stats.total_connections;
        stats.inbound_connections = connection_stats.inbound_connections;
        stats.outbound_connections = connection_stats.outbound_connections;

        // Update protocol stats
        let _protocol_stats = self.request_response.read().await.protocol_stats().await;
        // Protocol stats would be incorporated into network stats

        stats
    }
}