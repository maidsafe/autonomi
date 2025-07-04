# Phase 3: Implement Dual-Stack Networking

## Objective
Implement a dual-stack networking layer that can run both libp2p and iroh transports simultaneously, enabling gradual migration and A/B testing while maintaining full backward compatibility with the existing network.

## Prerequisites
- Phase 1 completed: Kademlia module extracted with transport abstraction
- Phase 2 completed: iroh transport adapter implemented and tested
- Both transports can run Kademlia operations independently

## Tasks

### 1. Create Unified Network Interface

Create `ant-node/src/networking/dual_stack/mod.rs`:
```rust
pub mod bridge;
pub mod routing;
pub mod manager;
pub mod config;

use crate::networking::kad::transport::{KademliaTransport, KadPeerId};

/// Unified network interface that can use multiple transports
pub struct DualStackNetwork {
    /// Active transport mode
    mode: NetworkMode,
    /// libp2p network instance
    libp2p: Option<LibP2pNetwork>,
    /// iroh network instance  
    iroh: Option<IrohNetwork>,
    /// Message routing between transports
    router: MessageRouter,
    /// Peer information synchronization
    peer_sync: PeerSynchronizer,
    /// Metrics for both stacks
    metrics: DualStackMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NetworkMode {
    /// Only use libp2p
    LibP2pOnly,
    /// Only use iroh
    IrohOnly,
    /// Use both, prefer libp2p
    DualPreferLibP2p,
    /// Use both, prefer iroh
    DualPreferIroh,
    /// Use both, load balance
    DualLoadBalance,
}
```

### 2. Implement Transport Bridge

Create `ant-node/src/networking/dual_stack/bridge.rs`:
```rust
/// Bridge that can route Kademlia operations to either transport
pub struct TransportBridge {
    libp2p_transport: Arc<Libp2pTransport>,
    iroh_transport: Arc<IrohTransport>,
    routing_strategy: RoutingStrategy,
    peer_capabilities: Arc<Mutex<HashMap<KadPeerId, PeerCapabilities>>>,
}

#[derive(Debug, Clone)]
pub struct PeerCapabilities {
    /// Peer supports libp2p
    pub has_libp2p: bool,
    /// Peer supports iroh
    pub has_iroh: bool,
    /// Last successful contact via libp2p
    pub last_libp2p_contact: Option<Instant>,
    /// Last successful contact via iroh
    pub last_iroh_contact: Option<Instant>,
    /// Measured latency for each transport
    pub libp2p_latency: Option<Duration>,
    pub iroh_latency: Option<Duration>,
}

#[async_trait]
impl KademliaTransport for TransportBridge {
    type Error = BridgeError;
    
    async fn send_message(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, Self::Error> {
        // Determine which transport to use
        let transport = self.select_transport(peer, &message).await?;
        
        match transport {
            SelectedTransport::LibP2p => {
                self.send_via_libp2p(peer, message).await
            }
            SelectedTransport::Iroh => {
                self.send_via_iroh(peer, message).await
            }
            SelectedTransport::Both => {
                // Try both in parallel, return first success
                self.send_via_both(peer, message).await
            }
        }
    }
    
    async fn is_connected(&self, peer: &KadPeerId) -> bool {
        let libp2p_connected = self.libp2p_transport.is_connected(peer).await;
        let iroh_connected = self.iroh_transport.is_connected(peer).await;
        libp2p_connected || iroh_connected
    }
    
    fn local_peer_id(&self) -> KadPeerId {
        // Use a unified peer ID that works for both transports
        self.unified_peer_id.clone()
    }
}

impl TransportBridge {
    async fn select_transport(
        &self,
        peer: &KadPeerId,
        message: &KadMessage,
    ) -> Result<SelectedTransport> {
        let capabilities = self.peer_capabilities.lock().unwrap()
            .get(peer).cloned();
        
        match (&self.routing_strategy, capabilities) {
            (RoutingStrategy::PreferIroh, Some(cap)) if cap.has_iroh => {
                Ok(SelectedTransport::Iroh)
            }
            (RoutingStrategy::PreferLibP2p, Some(cap)) if cap.has_libp2p => {
                Ok(SelectedTransport::LibP2p)
            }
            (RoutingStrategy::LowestLatency, Some(cap)) => {
                // Choose based on measured latency
                match (cap.iroh_latency, cap.libp2p_latency) {
                    (Some(iroh), Some(libp2p)) => {
                        if iroh < libp2p {
                            Ok(SelectedTransport::Iroh)
                        } else {
                            Ok(SelectedTransport::LibP2p)
                        }
                    }
                    _ => Ok(SelectedTransport::Both)
                }
            }
            _ => Ok(SelectedTransport::Both)
        }
    }
    
    async fn send_via_both(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, BridgeError> {
        use futures::future::select;
        
        let msg1 = message.clone();
        let msg2 = message.clone();
        
        let libp2p_fut = self.send_via_libp2p(peer, msg1);
        let iroh_fut = self.send_via_iroh(peer, msg2);
        
        // Race both transports
        match select(Box::pin(libp2p_fut), Box::pin(iroh_fut)).await {
            Either::Left((Ok(response), _)) => {
                // libp2p succeeded first
                self.update_peer_capabilities(peer, true, None).await;
                Ok(response)
            }
            Either::Right((Ok(response), _)) => {
                // iroh succeeded first
                self.update_peer_capabilities(peer, false, Some(true)).await;
                Ok(response)
            }
            Either::Left((Err(e1), fut2)) => {
                // libp2p failed, wait for iroh
                match fut2.await {
                    Ok(response) => {
                        self.update_peer_capabilities(peer, false, Some(true)).await;
                        Ok(response)
                    }
                    Err(e2) => {
                        Err(BridgeError::BothTransportsFailed { 
                            libp2p: e1.to_string(), 
                            iroh: e2.to_string() 
                        })
                    }
                }
            }
            Either::Right((Err(e2), fut1)) => {
                // iroh failed, wait for libp2p
                match fut1.await {
                    Ok(response) => {
                        self.update_peer_capabilities(peer, true, Some(false)).await;
                        Ok(response)
                    }
                    Err(e1) => {
                        Err(BridgeError::BothTransportsFailed { 
                            libp2p: e1.to_string(), 
                            iroh: e2.to_string() 
                        })
                    }
                }
            }
        }
    }
}
```

### 3. Implement Peer Discovery Synchronization

Create `ant-node/src/networking/dual_stack/routing.rs`:
```rust
/// Synchronizes peer information between libp2p and iroh
pub struct PeerSynchronizer {
    /// Peers discovered via libp2p
    libp2p_peers: Arc<Mutex<HashMap<PeerId, PeerInfo>>>,
    /// Peers discovered via iroh
    iroh_peers: Arc<Mutex<HashMap<NodeId, PeerInfo>>>,
    /// Mapping between libp2p and iroh identifiers
    id_mapping: Arc<Mutex<BiMap<PeerId, NodeId>>>,
    /// Channel for peer events
    peer_events: mpsc::Sender<PeerEvent>,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub addresses: Vec<Multiaddr>,
    pub protocols: Vec<String>,
    pub last_seen: Instant,
    pub transport: Transport,
}

impl PeerSynchronizer {
    /// Called when libp2p discovers a peer
    pub async fn on_libp2p_peer_discovered(
        &self,
        peer: PeerId,
        addresses: Vec<Multiaddr>,
    ) {
        // Store in libp2p peers
        self.libp2p_peers.lock().unwrap().insert(peer, PeerInfo {
            addresses: addresses.clone(),
            protocols: vec!["libp2p-kad".to_string()],
            last_seen: Instant::now(),
            transport: Transport::LibP2p,
        });
        
        // Try to derive iroh NodeId if possible
        if let Some(node_id) = self.try_convert_to_node_id(&peer) {
            self.id_mapping.lock().unwrap().insert(peer, node_id);
            
            // Notify iroh transport about the peer
            let _ = self.peer_events.send(PeerEvent::DiscoveredViaLibP2p {
                peer_id: peer,
                node_id: Some(node_id),
                addresses,
            }).await;
        }
    }
    
    /// Called when iroh discovers a peer
    pub async fn on_iroh_peer_discovered(
        &self,
        node: NodeId,
        addr: NodeAddr,
    ) {
        // Store in iroh peers
        self.iroh_peers.lock().unwrap().insert(node, PeerInfo {
            addresses: addr.direct_addresses.iter()
                .map(|a| Multiaddr::from(*a))
                .collect(),
            protocols: vec!["iroh-kad".to_string()],
            last_seen: Instant::now(),
            transport: Transport::Iroh,
        });
        
        // Try to derive libp2p PeerId if possible
        if let Some(peer_id) = self.try_convert_to_peer_id(&node) {
            self.id_mapping.lock().unwrap().insert(peer_id, node);
            
            // Notify libp2p transport about the peer
            let _ = self.peer_events.send(PeerEvent::DiscoveredViaIroh {
                node_id: node,
                peer_id: Some(peer_id),
                addresses: addr.direct_addresses,
            }).await;
        }
    }
    
    /// Get unified view of all peers
    pub fn get_all_peers(&self) -> Vec<UnifiedPeerInfo> {
        let mut peers = Vec::new();
        let mapping = self.id_mapping.lock().unwrap();
        
        // Add libp2p peers
        for (peer_id, info) in self.libp2p_peers.lock().unwrap().iter() {
            peers.push(UnifiedPeerInfo {
                kad_peer_id: KadPeerId::from(peer_id.clone()),
                libp2p_id: Some(peer_id.clone()),
                iroh_id: mapping.get_by_left(peer_id).cloned(),
                addresses: info.addresses.clone(),
                capabilities: PeerCapabilities {
                    has_libp2p: true,
                    has_iroh: mapping.contains_left(peer_id),
                    last_libp2p_contact: Some(info.last_seen),
                    last_iroh_contact: None,
                    libp2p_latency: None,
                    iroh_latency: None,
                },
            });
        }
        
        // Add iroh-only peers
        for (node_id, info) in self.iroh_peers.lock().unwrap().iter() {
            if !mapping.contains_right(node_id) {
                peers.push(UnifiedPeerInfo {
                    kad_peer_id: KadPeerId(node_id.as_bytes().to_vec()),
                    libp2p_id: None,
                    iroh_id: Some(node_id.clone()),
                    addresses: info.addresses.clone(),
                    capabilities: PeerCapabilities {
                        has_libp2p: false,
                        has_iroh: true,
                        last_libp2p_contact: None,
                        last_iroh_contact: Some(info.last_seen),
                        libp2p_latency: None,
                        iroh_latency: None,
                    },
                });
            }
        }
        
        peers
    }
}
```

### 4. Implement Network Manager

Create `ant-node/src/networking/dual_stack/manager.rs`:
```rust
impl DualStackNetwork {
    pub async fn new(config: DualStackConfig) -> Result<Self> {
        let peer_sync = PeerSynchronizer::new();
        let metrics = DualStackMetrics::new();
        
        // Initialize transports based on mode
        let (libp2p, iroh) = match config.mode {
            NetworkMode::LibP2pOnly => {
                let libp2p = LibP2pNetwork::new(config.libp2p_config).await?;
                (Some(libp2p), None)
            }
            NetworkMode::IrohOnly => {
                let iroh = IrohNetwork::new(config.iroh_config).await?;
                (None, Some(iroh))
            }
            _ => {
                // Initialize both
                let libp2p = LibP2pNetwork::new(config.libp2p_config).await?;
                let iroh = IrohNetwork::new(config.iroh_config).await?;
                (Some(libp2p), Some(iroh))
            }
        };
        
        // Create message router
        let router = MessageRouter::new(
            libp2p.as_ref().map(|n| n.transport()),
            iroh.as_ref().map(|n| n.transport()),
            config.routing_strategy,
        );
        
        Ok(Self {
            mode: config.mode,
            libp2p,
            iroh,
            router,
            peer_sync,
            metrics,
        })
    }
    
    /// Start both network stacks
    pub async fn start(&mut self) -> Result<()> {
        // Start libp2p if available
        if let Some(ref mut libp2p) = self.libp2p {
            libp2p.start().await?;
            info!("Started libp2p network");
        }
        
        // Start iroh if available
        if let Some(ref mut iroh) = self.iroh {
            iroh.start().await?;
            info!("Started iroh network");
        }
        
        // Start synchronization tasks
        self.start_sync_tasks().await?;
        
        Ok(())
    }
    
    /// Handle network events from both stacks
    pub async fn handle_events(&mut self) -> Result<NetworkEvent> {
        loop {
            tokio::select! {
                // Handle libp2p events
                Some(event) = self.libp2p_events() => {
                    self.handle_libp2p_event(event).await?
                }
                
                // Handle iroh events
                Some(event) = self.iroh_events() => {
                    self.handle_iroh_event(event).await?
                }
                
                // Handle sync events
                Some(event) = self.sync_events() => {
                    self.handle_sync_event(event).await?
                }
            }
        }
    }
    
    /// Gradually shift traffic from libp2p to iroh
    pub async fn start_migration(&mut self, duration: Duration) -> Result<()> {
        info!("Starting gradual migration from libp2p to iroh over {:?}", duration);
        
        let start = Instant::now();
        let migration_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            
            loop {
                interval.tick().await;
                let elapsed = start.elapsed();
                let progress = (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);
                
                // Adjust routing preference based on progress
                let new_strategy = if progress < 0.25 {
                    RoutingStrategy::PreferLibP2p
                } else if progress < 0.75 {
                    RoutingStrategy::LoadBalance
                } else {
                    RoutingStrategy::PreferIroh
                };
                
                // Update routing strategy
                self.router.set_strategy(new_strategy).await;
                
                info!("Migration progress: {:.1}%, strategy: {:?}", 
                    progress * 100.0, new_strategy);
                
                if progress >= 1.0 {
                    break;
                }
            }
        });
        
        Ok(())
    }
}
```

### 5. Implement Metrics Collection

Create `ant-node/src/networking/dual_stack/metrics.rs`:
```rust
pub struct DualStackMetrics {
    // Transport usage metrics
    libp2p_requests: Counter,
    iroh_requests: Counter,
    
    // Success/failure rates
    libp2p_success: Counter,
    libp2p_failure: Counter,
    iroh_success: Counter,
    iroh_failure: Counter,
    
    // Latency histograms
    libp2p_latency: Histogram,
    iroh_latency: Histogram,
    
    // Peer counts
    libp2p_peers: Gauge,
    iroh_peers: Gauge,
    dual_stack_peers: Gauge,
    
    // Traffic metrics
    libp2p_bytes_sent: Counter,
    libp2p_bytes_recv: Counter,
    iroh_bytes_sent: Counter,
    iroh_bytes_recv: Counter,
}

impl DualStackMetrics {
    pub fn record_request(&self, transport: Transport, success: bool, latency: Duration) {
        match transport {
            Transport::LibP2p => {
                self.libp2p_requests.inc();
                if success {
                    self.libp2p_success.inc();
                } else {
                    self.libp2p_failure.inc();
                }
                self.libp2p_latency.observe(latency.as_secs_f64());
            }
            Transport::Iroh => {
                self.iroh_requests.inc();
                if success {
                    self.iroh_success.inc();
                } else {
                    self.iroh_failure.inc();
                }
                self.iroh_latency.observe(latency.as_secs_f64());
            }
        }
    }
    
    pub fn get_success_rate(&self, transport: Transport) -> f64 {
        match transport {
            Transport::LibP2p => {
                let total = self.libp2p_requests.get();
                let success = self.libp2p_success.get();
                if total > 0 { success as f64 / total as f64 } else { 0.0 }
            }
            Transport::Iroh => {
                let total = self.iroh_requests.get();
                let success = self.iroh_success.get();
                if total > 0 { success as f64 / total as f64 } else { 0.0 }
            }
        }
    }
    
    pub fn get_average_latency(&self, transport: Transport) -> Option<Duration> {
        match transport {
            Transport::LibP2p => self.libp2p_latency.mean()
                .map(|s| Duration::from_secs_f64(s)),
            Transport::Iroh => self.iroh_latency.mean()
                .map(|s| Duration::from_secs_f64(s)),
        }
    }
}
```

### 6. Configuration

Create `ant-node/src/networking/dual_stack/config.rs`:
```rust
#[derive(Debug, Clone)]
pub struct DualStackConfig {
    /// Operating mode
    pub mode: NetworkMode,
    /// Routing strategy for dual-stack mode
    pub routing_strategy: RoutingStrategy,
    /// libp2p configuration
    pub libp2p_config: LibP2pConfig,
    /// iroh configuration
    pub iroh_config: IrohConfig,
    /// Synchronization settings
    pub sync_config: SyncConfig,
    /// Migration settings
    pub migration_config: MigrationConfig,
}

#[derive(Debug, Clone, Copy)]
pub enum RoutingStrategy {
    /// Always prefer libp2p
    PreferLibP2p,
    /// Always prefer iroh
    PreferIroh,
    /// Choose based on lowest latency
    LowestLatency,
    /// Load balance between transports
    LoadBalance,
    /// Use success rate to choose
    HighestSuccessRate,
}

#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Sync peer information between transports
    pub sync_peers: bool,
    /// Sync interval
    pub sync_interval: Duration,
    /// Share addresses between transports
    pub share_addresses: bool,
}

#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Enable automatic migration
    pub auto_migrate: bool,
    /// Migration duration
    pub migration_duration: Duration,
    /// Minimum success rate before switching
    pub min_success_rate: f64,
    /// A/B test percentage (0-100)
    pub ab_test_percentage: u8,
}
```

### 7. Integration with Existing Code

Update `ant-node/src/networking/mod.rs`:
```rust
// Add dual-stack module
pub mod dual_stack;

// Update Network enum
pub enum Network {
    LibP2p(LibP2pNetwork),
    Iroh(IrohNetwork),
    DualStack(DualStackNetwork),
}

impl Network {
    pub async fn new(config: NetworkConfig) -> Result<Self> {
        match config.backend {
            NetworkBackend::LibP2p => {
                Ok(Network::LibP2p(LibP2pNetwork::new(config).await?))
            }
            NetworkBackend::Iroh => {
                Ok(Network::Iroh(IrohNetwork::new(config).await?))
            }
            NetworkBackend::DualStack => {
                Ok(Network::DualStack(DualStackNetwork::new(config).await?))
            }
        }
    }
}
```

### 8. Testing

Create comprehensive tests for dual-stack operation:
```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_dual_stack_basic_operation() {
        let config = DualStackConfig {
            mode: NetworkMode::DualLoadBalance,
            routing_strategy: RoutingStrategy::LoadBalance,
            ..Default::default()
        };
        
        let mut network = DualStackNetwork::new(config).await.unwrap();
        network.start().await.unwrap();
        
        // Test that both transports work
        // Test routing decisions
        // Test failover
    }
    
    #[tokio::test]
    async fn test_gradual_migration() {
        // Test migration from libp2p to iroh
        // Monitor metrics during migration
        // Verify no message loss
    }
    
    #[tokio::test]
    async fn test_peer_synchronization() {
        // Test that peers discovered on one transport
        // are available on the other
    }
}
```

## Validation Criteria

1. Both transports can run simultaneously without conflicts
2. Messages are routed correctly based on configuration
3. Peer information is synchronized between transports
4. Failover works when one transport fails
5. Metrics accurately track usage and performance
6. No regression in network functionality
7. Migration can be controlled and monitored

## Notes

- Monitor resource usage with both stacks running
- Test with various network conditions
- Document optimal routing strategies for different scenarios
- Prepare for production deployment strategies

## Next Phase
Phase 4 will focus on production deployment and monitoring of the dual-stack network.
