# Phase 2: Implement iroh Transport Adapter

## Objective
Implement an iroh-based transport adapter that can work with the extracted Kademlia module, enabling Kademlia operations over iroh's networking layer.

## Prerequisites
- Phase 1 completed: Kademlia module extracted and abstracted
- Transport abstraction layer (`KademliaTransport` trait) defined
- Existing libp2p functionality still working via compatibility layer

## Tasks

### 1. Add iroh Dependencies

Update `ant-node/Cargo.toml`:
```toml
[dependencies]
# Enable iroh
iroh = { version = "0.28", features = ["discovery-n0", "metrics"] }
iroh-base = "0.28"
iroh-net = "0.28"

# Add serialization for Kad messages
serde = { version = "1.0", features = ["derive"] }
serde_bytes = "0.11"
postcard = { version = "1.0", features = ["use-std"] }
```

### 2. Define Kademlia Protocol for iroh

Create `ant-node/src/networking/iroh_adapter/mod.rs`:
```rust
pub mod transport;
pub mod protocol;
pub mod discovery;

// Kademlia ALPN (Application-Layer Protocol Negotiation) identifier
pub const KAD_ALPN: &[u8] = b"autonomi/kad/1.0.0";
```

### 3. Implement Kademlia Protocol Handler

Create `ant-node/src/networking/iroh_adapter/protocol.rs`:
```rust
use iroh::protocol::{ProtocolHandler, RouterBuilder};
use iroh::net::endpoint::{Connection, RecvStream, SendStream};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KadRequest {
    pub id: u64,  // Request ID for matching responses
    pub message: KadMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KadReply {
    pub id: u64,  // Matching request ID
    pub response: KadResponse,
}

#[derive(Clone)]
pub struct KadProtocol {
    routing_table: Arc<Mutex<RoutingTable>>,
    pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<KadResponse>>>>,
    next_request_id: Arc<AtomicU64>,
}

#[async_trait]
impl ProtocolHandler for KadProtocol {
    async fn accept(&self, connection: Connection) -> Result<()> {
        let peer_id = connection.remote_node_id();
        
        // Handle incoming streams
        while let Ok((send, recv)) = connection.accept_bi().await {
            let protocol = self.clone();
            tokio::spawn(async move {
                if let Err(e) = protocol.handle_stream(peer_id, send, recv).await {
                    warn!("Error handling Kad stream: {}", e);
                }
            });
        }
        
        Ok(())
    }
    
    async fn shutdown(&self) -> Result<()> {
        // Clean shutdown logic
        Ok(())
    }
}

impl KadProtocol {
    async fn handle_stream(
        &self,
        peer: NodeId,
        mut send: SendStream,
        mut recv: RecvStream,
    ) -> Result<()> {
        // Read request
        let request_bytes = recv.read_to_end(64 * 1024).await?; // 64KB max message
        let request: KadRequest = postcard::from_bytes(&request_bytes)?;
        
        // Process Kademlia message
        let response = self.process_kad_message(peer, request.message).await?;
        
        // Send reply
        let reply = KadReply {
            id: request.id,
            response,
        };
        let reply_bytes = postcard::to_stdvec(&reply)?;
        send.write_all(&reply_bytes).await?;
        send.finish()?;
        
        Ok(())
    }
}
```

### 4. Implement iroh Transport

Create `ant-node/src/networking/iroh_adapter/transport.rs`:
```rust
use crate::networking::kad::transport::{KademliaTransport, KadPeerId, KadMessage, KadResponse};
use iroh::net::endpoint::Endpoint;
use iroh::net::NodeId;

pub struct IrohTransport {
    endpoint: Endpoint,
    node_id: NodeId,
    // Mapping between transport-agnostic IDs and iroh NodeIds
    peer_mapping: Arc<Mutex<BiMap<KadPeerId, NodeId>>>,
}

impl IrohTransport {
    pub async fn new() -> Result<Self> {
        let endpoint = Endpoint::builder()
            .discovery_n0()
            .alpns(vec![KAD_ALPN.to_vec()])
            .bind()
            .await?;
        
        let node_id = endpoint.node_id();
        
        Ok(Self {
            endpoint,
            node_id,
            peer_mapping: Arc::new(Mutex::new(BiMap::new())),
        })
    }
    
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
    
    /// Convert KadPeerId to NodeId
    fn to_node_id(&self, peer: &KadPeerId) -> Result<NodeId> {
        // Try mapping first
        if let Some(node_id) = self.peer_mapping.lock().unwrap().get_by_left(peer) {
            return Ok(*node_id);
        }
        
        // Otherwise, try to parse as NodeId
        if peer.0.len() == 32 {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&peer.0);
            Ok(NodeId::from_bytes(&bytes)?)
        } else {
            Err(anyhow!("Invalid peer ID format"))
        }
    }
}

#[async_trait]
impl KademliaTransport for IrohTransport {
    type Error = anyhow::Error;
    
    async fn send_message(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, Self::Error> {
        let node_id = self.to_node_id(peer)?;
        
        // Connect to peer
        let conn = self.endpoint
            .connect(node_id, KAD_ALPN)
            .await?;
        
        // Open bidirectional stream
        let (mut send, mut recv) = conn.open_bi().await?;
        
        // Create request
        let request = KadRequest {
            id: self.next_request_id.fetch_add(1, Ordering::SeqCst),
            message,
        };
        
        // Serialize and send
        let request_bytes = postcard::to_stdvec(&request)?;
        send.write_all(&request_bytes).await?;
        send.finish()?;
        
        // Read response
        let response_bytes = recv.read_to_end(64 * 1024).await?;
        let reply: KadReply = postcard::from_bytes(&response_bytes)?;
        
        if reply.id != request.id {
            return Err(anyhow!("Response ID mismatch"));
        }
        
        Ok(reply.response)
    }
    
    async fn is_connected(&self, peer: &KadPeerId) -> bool {
        if let Ok(node_id) = self.to_node_id(peer) {
            // Check if we have an active connection
            // iroh maintains connection state internally
            true // Simplified - iroh will establish connection on demand
        } else {
            false
        }
    }
    
    fn local_peer_id(&self) -> KadPeerId {
        KadPeerId(self.node_id.as_bytes().to_vec())
    }
}
```

### 5. Implement Discovery Bridge

Create `ant-node/src/networking/iroh_adapter/discovery.rs`:
```rust
/// Bridge between Kademlia peer discovery and iroh discovery
pub struct DiscoveryBridge {
    /// Known peers from Kademlia
    kad_peers: Arc<Mutex<HashMap<KadPeerId, HashSet<SocketAddr>>>>,
    /// iroh discovery service
    discovery: Box<dyn Discovery>,
}

impl DiscoveryBridge {
    pub fn new() -> Self {
        Self {
            kad_peers: Arc::new(Mutex::new(HashMap::new())),
            discovery: Box::new(DnsDiscovery::n0()),
        }
    }
    
    /// Add peer addresses from Kademlia routing table
    pub fn add_kad_peer(&self, peer: KadPeerId, addrs: Vec<SocketAddr>) {
        self.kad_peers.lock().unwrap()
            .entry(peer)
            .or_insert_with(HashSet::new)
            .extend(addrs);
    }
}

#[async_trait]
impl Discovery for DiscoveryBridge {
    async fn resolve(&self, node_id: NodeId) -> Result<NodeAddr> {
        // First check Kademlia peers
        let kad_peer = KadPeerId(node_id.as_bytes().to_vec());
        if let Some(addrs) = self.kad_peers.lock().unwrap().get(&kad_peer) {
            if !addrs.is_empty() {
                return Ok(NodeAddr {
                    node_id,
                    relay_url: None,
                    direct_addresses: addrs.clone(),
                });
            }
        }
        
        // Fall back to n0 discovery
        self.discovery.resolve(node_id).await
    }
}
```

### 6. Integration with Kademlia Module

Create `ant-node/src/networking/kad/iroh_integration.rs`:
```rust
/// Integrate iroh transport with Kademlia behavior
pub struct IrohKademlia {
    transport: IrohTransport,
    kademlia: Kademlia<IrohTransport>,
    router: Router,
}

impl IrohKademlia {
    pub async fn new(config: KademliaConfig) -> Result<Self> {
        let transport = IrohTransport::new().await?;
        let endpoint = transport.endpoint().clone();
        
        // Create Kademlia instance with iroh transport
        let kademlia = Kademlia::with_transport(
            transport.clone(),
            transport.local_peer_id(),
            config,
        );
        
        // Set up protocol handler
        let protocol = KadProtocol::new(kademlia.clone());
        
        // Build router
        let router = Router::builder(endpoint)
            .accept(KAD_ALPN.to_vec(), Arc::new(protocol))
            .spawn()
            .await?;
        
        Ok(Self {
            transport,
            kademlia,
            router,
        })
    }
}
```

### 7. Testing Infrastructure

Create `ant-node/src/networking/iroh_adapter/tests.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_iroh_transport_basic() {
        // Create two nodes
        let node1 = IrohTransport::new().await.unwrap();
        let node2 = IrohTransport::new().await.unwrap();
        
        // Test message exchange
        let msg = KadMessage::FindNode { 
            key: vec![1, 2, 3, 4] 
        };
        
        let response = node1.send_message(
            &node2.local_peer_id(),
            msg,
        ).await.unwrap();
        
        // Verify response
        assert!(matches!(response, KadResponse::Nodes(_)));
    }
    
    #[tokio::test]
    async fn test_kad_over_iroh() {
        // Test full Kademlia operations over iroh
        let config = KademliaConfig::default();
        let mut kad1 = IrohKademlia::new(config.clone()).await.unwrap();
        let mut kad2 = IrohKademlia::new(config).await.unwrap();
        
        // Bootstrap kad2 from kad1
        kad2.add_address(&kad1.local_peer_id(), kad1.listen_addresses());
        
        // Perform Kademlia operations
        let key = vec![5, 6, 7, 8];
        kad1.put_record(key.clone(), vec![9, 10, 11, 12]);
        
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        let result = kad2.get_record(&key).await;
        assert!(result.is_ok());
    }
}
```

### 8. Configuration

Create `ant-node/src/networking/iroh_adapter/config.rs`:
```rust
#[derive(Debug, Clone)]
pub struct IrohConfig {
    /// Enable relay servers
    pub enable_relay: bool,
    /// Custom relay URLs (uses n0 defaults if empty)
    pub relay_urls: Vec<String>,
    /// Discovery configuration
    pub discovery: DiscoveryConfig,
    /// Concurrent connection limit
    pub max_connections: usize,
}

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Use n0 DNS discovery
    pub use_n0_dns: bool,
    /// Use Kademlia peer addresses
    pub use_kad_peers: bool,
    /// Custom discovery endpoints
    pub custom_endpoints: Vec<String>,
}

impl Default for IrohConfig {
    fn default() -> Self {
        Self {
            enable_relay: true,
            relay_urls: vec![],
            discovery: DiscoveryConfig {
                use_n0_dns: true,
                use_kad_peers: true,
                custom_endpoints: vec![],
            },
            max_connections: 1000,
        }
    }
}
```

### 9. Metrics and Monitoring

Add metrics collection for iroh transport:
```rust
pub struct IrohMetrics {
    connections_total: Counter,
    messages_sent: Counter,
    messages_received: Counter,
    connection_duration: Histogram,
    message_latency: Histogram,
}
```

## Validation Criteria

1. iroh transport successfully implements the `KademliaTransport` trait
2. Kademlia messages can be exchanged between nodes using iroh
3. Unit tests pass for basic iroh transport operations
4. Integration tests show Kademlia operations work over iroh
5. Performance metrics collected (latency, throughput)
6. No regression in existing libp2p functionality

## Notes

- This phase focuses on making iroh work with Kademlia, not optimization
- Keep both transports available for A/B testing
- Document any iroh-specific limitations or advantages discovered
- Prepare for dual-stack operation in the next phase

## Next Phase
Phase 3 will implement dual-stack networking to run both libp2p and iroh in parallel.
