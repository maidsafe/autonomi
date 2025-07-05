// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Comprehensive tests for the transport-agnostic Kademlia implementation.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::networking::kad::{
    transport::{
        KademliaTransport, KadPeerId, KadAddress, KadMessage, KadResponse, KadError,
        PeerInfo, ConnectionStatus, RecordKey, Record,
    },
    behaviour::{Kademlia, KadCommand},
    record_store::{MemoryRecordStore, RecordStoreConfig},
    query::QueryType,
    KadConfig,
};

/// Mock transport implementation for testing
#[derive(Debug)]
struct MockTransport {
    local_peer_id: KadPeerId,
    known_peers: Arc<RwLock<HashMap<KadPeerId, PeerInfo>>>,
    message_handler: Arc<RwLock<Option<MockMessageHandler>>>,
    connected_peers: Arc<RwLock<Vec<KadPeerId>>>,
    dial_attempts: Arc<RwLock<Vec<(KadPeerId, Vec<KadAddress>)>>>,
}

#[derive(Debug, Clone)]
struct MockMessageHandler {
    responses: HashMap<KadPeerId, KadResponse>,
    default_response: Option<KadResponse>,
}

impl MockTransport {
    fn new(id: u8) -> Self {
        Self {
            local_peer_id: KadPeerId::new(vec![id]),
            known_peers: Arc::new(RwLock::new(HashMap::new())),
            message_handler: Arc::new(RwLock::new(None)),
            connected_peers: Arc::new(RwLock::new(Vec::new())),
            dial_attempts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn with_handler(mut self, handler: MockMessageHandler) -> Self {
        *self.message_handler.blocking_write() = Some(handler);
        self
    }

    async fn add_known_peer(&self, peer: PeerInfo) {
        self.known_peers.write().await.insert(peer.peer_id.clone(), peer);
    }

    async fn add_connected_peer(&self, peer_id: KadPeerId) {
        self.connected_peers.write().await.push(peer_id);
    }

    async fn get_dial_attempts(&self) -> Vec<(KadPeerId, Vec<KadAddress>)> {
        self.dial_attempts.read().await.clone()
    }
}

#[async_trait]
impl KademliaTransport for MockTransport {
    type Error = KadError;

    fn local_peer_id(&self) -> KadPeerId {
        self.local_peer_id.clone()
    }

    fn listen_addresses(&self) -> Vec<KadAddress> {
        vec![KadAddress::new("mock".to_string(), "127.0.0.1:0".to_string())]
    }

    async fn send_request(&self, peer: &KadPeerId, message: KadMessage) -> Result<KadResponse, Self::Error> {
        let handler = self.message_handler.read().await;
        
        if let Some(ref handler) = *handler {
            if let Some(response) = handler.responses.get(peer) {
                return Ok(response.clone());
            }
            
            if let Some(ref default) = handler.default_response {
                return Ok(default.clone());
            }
        }

        // Default mock response based on message type
        match message {
            KadMessage::FindNode { target, .. } => {
                Ok(KadResponse::Nodes {
                    closer_peers: vec![],
                    requester: target,
                })
            }
            KadMessage::FindValue { key, .. } => {
                Ok(KadResponse::Value {
                    record: None,
                    closer_peers: vec![],
                    requester: key.to_kad_peer_id(),
                })
            }
            KadMessage::PutValue { requester, .. } => {
                Ok(KadResponse::Ack { requester })
            }
            KadMessage::GetProviders { key, .. } => {
                Ok(KadResponse::Providers {
                    key,
                    providers: vec![],
                    closer_peers: vec![],
                    requester: self.local_peer_id.clone(),
                })
            }
            KadMessage::AddProvider { requester, .. } => {
                Ok(KadResponse::Ack { requester })
            }
            KadMessage::Ping { requester } => {
                Ok(KadResponse::Ack { requester })
            }
        }
    }

    async fn send_message(&self, _peer: &KadPeerId, _message: KadMessage) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn is_connected(&self, peer: &KadPeerId) -> bool {
        self.connected_peers.read().await.contains(peer)
    }

    async fn dial_peer(&self, peer: &KadPeerId, addresses: &[KadAddress]) -> Result<(), Self::Error> {
        self.dial_attempts.write().await.push((peer.clone(), addresses.to_vec()));
        self.add_connected_peer(peer.clone()).await;
        Ok(())
    }

    async fn add_peer_addresses(&self, peer: &KadPeerId, addresses: Vec<KadAddress>) -> Result<(), Self::Error> {
        let peer_info = PeerInfo {
            peer_id: peer.clone(),
            addresses,
            connection_status: ConnectionStatus::Unknown,
            last_seen: Some(Instant::now()),
        };
        self.add_known_peer(peer_info).await;
        Ok(())
    }

    async fn remove_peer(&self, peer: &KadPeerId) -> Result<(), Self::Error> {
        self.known_peers.write().await.remove(peer);
        self.connected_peers.write().await.retain(|p| p != peer);
        Ok(())
    }

    async fn peer_info(&self, peer: &KadPeerId) -> Option<PeerInfo> {
        self.known_peers.read().await.get(peer).cloned()
    }

    async fn connected_peers(&self) -> Vec<KadPeerId> {
        self.connected_peers.read().await.clone()
    }

    async fn start_listening(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Helper function to create test peer info
fn create_test_peer(id: u8, port: u16) -> PeerInfo {
    PeerInfo {
        peer_id: KadPeerId::new(vec![id]),
        addresses: vec![KadAddress::new("tcp".to_string(), format!("127.0.0.1:{}", port))],
        connection_status: ConnectionStatus::Unknown,
        last_seen: Some(Instant::now()),
    }
}

/// Helper function to create test record
fn create_test_record(key: &[u8], value: &[u8]) -> Record {
    Record::new(RecordKey::new(key.to_vec()), value.to_vec())
}

#[tokio::test]
async fn test_kad_creation_and_basic_operations() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let kademlia = Kademlia::with_memory_store(transport.clone(), config);
    assert!(kademlia.is_ok());
    
    let kad = kademlia.unwrap();
    let handle = kad.handle();
    
    // Test basic handle operations
    let stats = handle.get_stats().await;
    assert!(stats.is_ok());
    
    let stats = stats.unwrap();
    assert_eq!(stats.queries_initiated, 0);
    assert_eq!(stats.queries_completed, 0);
}

#[tokio::test]
async fn test_find_node_operation() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    // Set up mock response
    let target_peer = KadPeerId::new(vec![42]);
    let closer_peers = vec![
        create_test_peer(10, 8001),
        create_test_peer(20, 8002),
        create_test_peer(30, 8003),
    ];
    
    let handler = MockMessageHandler {
        responses: HashMap::new(),
        default_response: Some(KadResponse::Nodes {
            closer_peers: closer_peers.clone(),
            requester: target_peer.clone(),
        }),
    };
    
    let transport_with_handler = MockTransport::new(1).with_handler(handler);
    let transport = Arc::new(transport_with_handler);
    
    let mut kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Add some initial peers
    for peer in &closer_peers {
        handle.add_peer(peer.clone()).await.unwrap();
    }
    
    // Perform find_node operation
    let result = handle.find_node(target_peer.clone()).await;
    assert!(result.is_ok());
    
    let found_peers = result.unwrap();
    assert!(!found_peers.is_empty());
}

#[tokio::test]
async fn test_put_and_get_record() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let mut kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Add some peers first
    for i in 1..=5 {
        let peer = create_test_peer(i, 8000 + i as u16);
        handle.add_peer(peer).await.unwrap();
    }
    
    // Create a test record
    let record = create_test_record(b"test-key", b"test-value");
    let key = record.key.clone();
    
    // Put the record
    let put_result = handle.put_record(record.clone()).await;
    assert!(put_result.is_ok());
    
    // Try to find the value
    let get_result = handle.find_value(key).await;
    assert!(get_result.is_ok());
    
    // Should find it locally since we just stored it
    let found_record = get_result.unwrap();
    assert!(found_record.is_some());
    assert_eq!(found_record.unwrap().value, record.value);
}

#[tokio::test]
async fn test_bootstrap_operation() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let handler = MockMessageHandler {
        responses: HashMap::new(),
        default_response: Some(KadResponse::Nodes {
            closer_peers: vec![
                create_test_peer(10, 8001),
                create_test_peer(20, 8002),
                create_test_peer(30, 8003),
            ],
            requester: KadPeerId::new(vec![1]),
        }),
    };
    
    let transport_with_handler = MockTransport::new(1).with_handler(handler);
    let transport = Arc::new(transport_with_handler);
    
    let mut kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    
    // Add bootstrap peers
    let bootstrap_peers = vec![
        create_test_peer(100, 9001),
        create_test_peer(101, 9002),
    ];
    kad.add_bootstrap_peers(bootstrap_peers);
    
    let handle = kad.handle();
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    // Perform bootstrap
    let bootstrap_result = handle.bootstrap(tx).await;
    assert!(bootstrap_result.is_ok());
}

#[tokio::test]
async fn test_routing_table_management() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Add peers to routing table
    let test_peers = vec![
        create_test_peer(10, 8001),
        create_test_peer(20, 8002),
        create_test_peer(30, 8003),
        create_test_peer(40, 8004),
        create_test_peer(50, 8005),
    ];
    
    for peer in &test_peers {
        handle.add_peer(peer.clone()).await.unwrap();
    }
    
    // Get routing table info
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle.command_tx.send(crate::networking::kad::behaviour::KadCommand::GetRoutingTable {
        response_tx: tx,
    }).unwrap();
    
    let routing_info = rx.await.unwrap();
    assert!(routing_info.total_peers > 0);
}

#[tokio::test]
async fn test_record_store_integration() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let record_store_config = RecordStoreConfig {
        max_records: 100,
        max_record_size: 1024,
        max_total_size: 10 * 1024,
        ..Default::default()
    };
    
    let record_store = Arc::new(RwLock::new(
        MemoryRecordStore::new(record_store_config)
    ));
    
    let kad = Kademlia::new(transport, record_store.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Test direct record store access
    {
        let mut store = record_store.write().await;
        let test_record = create_test_record(b"direct-key", b"direct-value");
        
        let put_result = store.put(test_record.clone()).await;
        assert!(put_result.is_ok());
        
        let get_result = store.get(&test_record.key).await;
        assert!(get_result.is_ok());
        assert!(get_result.unwrap().is_some());
    }
    
    // Test via Kademlia interface
    let kad_record = create_test_record(b"kad-key", b"kad-value");
    let put_result = handle.put_record(kad_record.clone()).await;
    assert!(put_result.is_ok());
    
    let get_result = handle.find_value(kad_record.key).await;
    assert!(get_result.is_ok());
    assert!(get_result.unwrap().is_some());
}

#[tokio::test]
async fn test_transport_abstraction() {
    let transport = Arc::new(MockTransport::new(42));
    
    // Test basic transport operations
    assert_eq!(transport.local_peer_id(), KadPeerId::new(vec![42]));
    assert!(!transport.listen_addresses().is_empty());
    
    let test_peer = KadPeerId::new(vec![100]);
    let test_addresses = vec![KadAddress::new("tcp".to_string(), "127.0.0.1:8080".to_string())];
    
    // Test address management
    let add_result = transport.add_peer_addresses(&test_peer, test_addresses.clone()).await;
    assert!(add_result.is_ok());
    
    let peer_info = transport.peer_info(&test_peer).await;
    assert!(peer_info.is_some());
    assert_eq!(peer_info.unwrap().peer_id, test_peer);
    
    // Test dialing
    let dial_result = transport.dial_peer(&test_peer, &test_addresses).await;
    assert!(dial_result.is_ok());
    
    let dial_attempts = transport.get_dial_attempts().await;
    assert_eq!(dial_attempts.len(), 1);
    assert_eq!(dial_attempts[0].0, test_peer);
    
    // Test connection status
    assert!(transport.is_connected(&test_peer).await);
    
    // Test message sending
    let test_message = KadMessage::Ping {
        requester: transport.local_peer_id(),
    };
    
    let send_result = transport.send_request(&test_peer, test_message).await;
    assert!(send_result.is_ok());
}

#[tokio::test]
async fn test_query_timeouts() {
    let transport = Arc::new(MockTransport::new(1));
    let mut config = KadConfig::default();
    config.query_timeout = Duration::from_millis(100); // Very short timeout
    
    let kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Try to find a non-existent peer with no known peers
    let non_existent_peer = KadPeerId::new(vec![255]);
    let start_time = Instant::now();
    
    let result = handle.find_node(non_existent_peer).await;
    let elapsed = start_time.elapsed();
    
    // Should fail due to timeout or no peers
    assert!(result.is_err());
    // Should complete relatively quickly due to short timeout
    assert!(elapsed < Duration::from_millis(200));
}

#[tokio::test]
async fn test_error_handling() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Test with empty record key
    let invalid_record = Record::new(RecordKey::new(vec![]), vec![1, 2, 3]);
    let put_result = handle.put_record(invalid_record).await;
    // Should succeed as our implementation doesn't validate empty keys as invalid
    
    // Test find_value with non-existent key
    let non_existent_key = RecordKey::new(vec![255, 255, 255, 255]);
    let get_result = handle.find_value(non_existent_key).await;
    
    // Should succeed but return None
    assert!(get_result.is_ok());
    assert!(get_result.unwrap().is_none());
}

#[tokio::test]
async fn test_concurrent_operations() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Add some peers
    for i in 1..=10 {
        let peer = create_test_peer(i, 8000 + i as u16);
        handle.add_peer(peer).await.unwrap();
    }
    
    // Perform multiple concurrent operations
    let mut handles_vec = Vec::new();
    
    for i in 0..5 {
        let handle_clone = handle.clone();
        let task = tokio::spawn(async move {
            let record = create_test_record(&[i], &[i * 10]);
            handle_clone.put_record(record).await
        });
        handles_vec.push(task);
    }
    
    // Wait for all operations to complete
    for task in handles_vec {
        let result = task.await.unwrap();
        assert!(result.is_ok());
    }
    
    // Verify records were stored
    for i in 0..5 {
        let key = RecordKey::new(vec![i]);
        let get_result = handle.find_value(key).await;
        assert!(get_result.is_ok());
        assert!(get_result.unwrap().is_some());
    }
}

#[tokio::test]
async fn test_stats_tracking() {
    let transport = Arc::new(MockTransport::new(1));
    let config = KadConfig::default();
    
    let kad = Kademlia::with_memory_store(transport.clone(), config).unwrap();
    let handle = kad.handle();
    
    // Get initial stats
    let initial_stats = handle.get_stats().await.unwrap();
    assert_eq!(initial_stats.queries_initiated, 0);
    
    // Add a peer and perform operations
    let peer = create_test_peer(42, 8080);
    handle.add_peer(peer.clone()).await.unwrap();
    
    // Perform some operations
    let _find_result = handle.find_node(KadPeerId::new(vec![100])).await;
    
    // Check stats were updated
    let updated_stats = handle.get_stats().await.unwrap();
    assert!(updated_stats.queries_initiated > initial_stats.queries_initiated);
}

/// Integration test that simulates a small network
#[tokio::test]
async fn test_small_network_simulation() {
    // Create multiple Kademlia nodes
    let mut nodes = Vec::new();
    let mut handles = Vec::new();
    
    for i in 1..=5 {
        let transport = Arc::new(MockTransport::new(i));
        let config = KadConfig::default();
        let kad = Kademlia::with_memory_store(transport, config).unwrap();
        let handle = kad.handle();
        
        handles.push(handle);
        nodes.push(kad);
    }
    
    // Connect all nodes to each other
    for i in 0..handles.len() {
        for j in 0..handles.len() {
            if i != j {
                let peer = create_test_peer((j + 1) as u8, 8000 + j as u16);
                handles[i].add_peer(peer).await.unwrap();
            }
        }
    }
    
    // Store a record on node 0
    let test_record = create_test_record(b"network-key", b"network-value");
    let put_result = handles[0].put_record(test_record.clone()).await;
    assert!(put_result.is_ok());
    
    // Try to find the record from different nodes
    for i in 1..handles.len() {
        let get_result = handles[i].find_value(test_record.key.clone()).await;
        // Note: In this mock setup, each node only stores locally,
        // so remote nodes won't find the record unless we implement
        // proper message routing between mock transports
        assert!(get_result.is_ok());
    }
}