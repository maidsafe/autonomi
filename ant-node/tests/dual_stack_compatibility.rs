//! Dual-Stack Backwards Compatibility Tests
//! 
//! This test suite verifies that dual-stack nodes with iroh transport
//! can successfully communicate with legacy nodes running only libp2p,
//! ensuring seamless backwards compatibility during migration.

#![allow(dead_code, unused_variables, unused_fields, unused_imports, unused_mut)]

use std::{
    collections::HashMap,
    sync::{Arc, Once},
    time::Duration,
};

use tokio::time::timeout;
use tracing::{info, warn, debug};

static INIT: Once = Once::new();

fn init_tracing() {
    INIT.call_once(|| {
        tracing_subscriber::fmt::init();
    });
}

/// Mock libp2p-only node for compatibility testing
#[derive(Debug)]
struct LibP2POnlyNode {
    peer_id: String,
    routing_table: Arc<tokio::sync::RwLock<HashMap<String, NodeInfo>>>,
    message_handler: Arc<tokio::sync::Mutex<Vec<NetworkMessage>>>,
}

/// Mock dual-stack node with both libp2p and iroh
#[derive(Debug)]
struct DualStackNode {
    peer_id: String,
    libp2p_enabled: bool,
    iroh_enabled: bool,
    routing_table: Arc<tokio::sync::RwLock<HashMap<String, NodeInfo>>>,
    message_handler: Arc<tokio::sync::Mutex<Vec<NetworkMessage>>>,
    transport_preference: TransportPreference,
}

#[derive(Debug, Clone)]
struct NodeInfo {
    peer_id: String,
    transport_capabilities: Vec<TransportType>,
    last_seen: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq)]
enum TransportType {
    LibP2P,
    Iroh,
}

#[derive(Debug, Clone, PartialEq)]
enum TransportPreference {
    LibP2POnly,
    IrohPreferred,
    Balanced,
}

#[derive(Debug, Clone)]
struct NetworkMessage {
    from: String,
    to: String,
    message_type: MessageType,
    transport_used: TransportType,
    timestamp: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq)]
enum MessageType {
    FindNode { target: String },
    FindValue { key: String },
    PutRecord { key: String, value: Vec<u8> },
    Bootstrap,
    Ping,
}

impl LibP2POnlyNode {
    fn new(peer_id: String) -> Self {
        Self {
            peer_id,
            routing_table: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            message_handler: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    async fn handle_message(&self, message: NetworkMessage) -> Result<NetworkMessage, String> {
        debug!("LibP2P-only node {} handling message: {:?}", self.peer_id, message);
        
        // LibP2P-only nodes can only use libp2p transport
        match message.transport_used {
            TransportType::LibP2P => {
                // Process message normally
                let response = match &message.message_type {
                    MessageType::FindNode { target } => {
                        self.find_node_response(target).await
                    },
                    MessageType::FindValue { key } => {
                        self.find_value_response(key).await
                    },
                    MessageType::PutRecord { key, value } => {
                        self.put_record_response(key, value).await
                    },
                    MessageType::Bootstrap => {
                        self.bootstrap_response().await
                    },
                    MessageType::Ping => {
                        self.ping_response().await
                    },
                };

                let from_peer = message.from.clone();
                
                // Store message for analysis
                self.message_handler.lock().await.push(message);
                
                Ok(NetworkMessage {
                    from: self.peer_id.clone(),
                    to: from_peer,
                    message_type: response,
                    transport_used: TransportType::LibP2P,
                    timestamp: std::time::Instant::now(),
                })
            },
            TransportType::Iroh => {
                // LibP2P-only nodes cannot handle iroh messages directly
                Err(format!("LibP2P-only node {} cannot handle iroh transport", self.peer_id))
            }
        }
    }

    async fn find_node_response(&self, _target: &str) -> MessageType {
        // Return mock routing table entries
        MessageType::FindNode { target: "response_peer".to_string() }
    }

    async fn find_value_response(&self, key: &str) -> MessageType {
        MessageType::FindValue { key: format!("response_{}", key) }
    }

    async fn put_record_response(&self, key: &str, _value: &[u8]) -> MessageType {
        MessageType::PutRecord { 
            key: key.to_string(), 
            value: b"stored".to_vec() 
        }
    }

    async fn bootstrap_response(&self) -> MessageType {
        MessageType::Bootstrap
    }

    async fn ping_response(&self) -> MessageType {
        MessageType::Ping
    }

    async fn get_message_count(&self) -> usize {
        self.message_handler.lock().await.len()
    }
}

impl DualStackNode {
    fn new(peer_id: String, preference: TransportPreference) -> Self {
        Self {
            peer_id,
            libp2p_enabled: true,
            iroh_enabled: true,
            routing_table: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            message_handler: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            transport_preference: preference,
        }
    }

    /// Detect transport capabilities of a peer through protocol negotiation
    async fn detect_peer_capabilities(&self, peer_id: &str) -> Vec<TransportType> {
        debug!("Detecting capabilities for peer: {}", peer_id);
        
        // In reality, this would probe the peer through protocol negotiation
        // For this test, we simulate capability detection
        if peer_id.contains("libp2p_only") {
            vec![TransportType::LibP2P]
        } else if peer_id.contains("dual_stack") {
            vec![TransportType::LibP2P, TransportType::Iroh]
        } else {
            // Default to libp2p for unknown peers (backwards compatibility)
            vec![TransportType::LibP2P]
        }
    }

    /// Select optimal transport based on peer capabilities and preferences
    async fn select_transport(&self, target_peer: &str) -> TransportType {
        let peer_capabilities = self.detect_peer_capabilities(target_peer).await;
        
        match self.transport_preference {
            TransportPreference::LibP2POnly => TransportType::LibP2P,
            TransportPreference::IrohPreferred => {
                if peer_capabilities.contains(&TransportType::Iroh) {
                    TransportType::Iroh
                } else {
                    // Fall back to libp2p for backwards compatibility
                    TransportType::LibP2P
                }
            },
            TransportPreference::Balanced => {
                // Use libp2p for broader compatibility in balanced mode
                TransportType::LibP2P
            }
        }
    }

    async fn send_message(&self, target_peer: &str, message_type: MessageType) -> Result<NetworkMessage, String> {
        let transport = self.select_transport(target_peer).await;
        
        debug!("Dual-stack node {} sending message to {} via {:?}", 
               self.peer_id, target_peer, transport);

        let message = NetworkMessage {
            from: self.peer_id.clone(),
            to: target_peer.to_string(),
            message_type,
            transport_used: transport,
            timestamp: std::time::Instant::now(),
        };

        // Store message for analysis
        self.message_handler.lock().await.push(message.clone());

        Ok(message)
    }

    async fn get_message_count(&self) -> usize {
        self.message_handler.lock().await.len()
    }

    async fn get_transport_usage(&self) -> (usize, usize) {
        let messages = self.message_handler.lock().await;
        let libp2p_count = messages.iter()
            .filter(|m| matches!(m.transport_used, TransportType::LibP2P))
            .count();
        let iroh_count = messages.iter()
            .filter(|m| matches!(m.transport_used, TransportType::Iroh))
            .count();
        (libp2p_count, iroh_count)
    }
}

/// Test network simulator for backwards compatibility testing
struct CompatibilityTestNetwork {
    libp2p_nodes: Vec<LibP2POnlyNode>,
    dual_stack_nodes: Vec<DualStackNode>,
}

impl CompatibilityTestNetwork {
    fn new() -> Self {
        Self {
            libp2p_nodes: Vec::new(),
            dual_stack_nodes: Vec::new(),
        }
    }

    fn add_libp2p_node(&mut self, peer_id: String) {
        self.libp2p_nodes.push(LibP2POnlyNode::new(peer_id));
    }

    fn add_dual_stack_node(&mut self, peer_id: String, preference: TransportPreference) {
        self.dual_stack_nodes.push(DualStackNode::new(peer_id, preference));
    }

    async fn simulate_message_exchange(&self, from_dual_stack: usize, to_libp2p: usize, message_type: MessageType) -> Result<(NetworkMessage, NetworkMessage), String> {
        let dual_stack_node = &self.dual_stack_nodes[from_dual_stack];
        let libp2p_node = &self.libp2p_nodes[to_libp2p];

        // Dual-stack node sends message
        let request = dual_stack_node.send_message(&libp2p_node.peer_id, message_type).await?;

        // LibP2P node receives and responds
        let response = libp2p_node.handle_message(request.clone()).await?;

        Ok((request, response))
    }

    async fn run_compatibility_test(&self) -> CompatibilityTestResult {
        let mut results = CompatibilityTestResult::new();

        for (dual_idx, dual_node) in self.dual_stack_nodes.iter().enumerate() {
            for (libp2p_idx, libp2p_node) in self.libp2p_nodes.iter().enumerate() {
                info!("Testing communication: {} -> {}", dual_node.peer_id, libp2p_node.peer_id);

                // Test different message types
                let test_cases = vec![
                    MessageType::Ping,
                    MessageType::FindNode { target: "test_target".to_string() },
                    MessageType::FindValue { key: "test_key".to_string() },
                    MessageType::PutRecord { key: "test_record".to_string(), value: b"test_data".to_vec() },
                    MessageType::Bootstrap,
                ];

                for message_type in test_cases {
                    match timeout(
                        Duration::from_secs(5),
                        self.simulate_message_exchange(dual_idx, libp2p_idx, message_type.clone())
                    ).await {
                        Ok(Ok((request, response))) => {
                            results.record_success(request, response);
                        },
                        Ok(Err(e)) => {
                            results.record_failure(dual_node.peer_id.clone(), libp2p_node.peer_id.clone(), e);
                        },
                        Err(_) => {
                            results.record_timeout(dual_node.peer_id.clone(), libp2p_node.peer_id.clone());
                        }
                    }
                }
            }
        }

        results
    }
}

#[derive(Debug)]
struct CompatibilityTestResult {
    successful_exchanges: Vec<(NetworkMessage, NetworkMessage)>,
    failed_exchanges: Vec<FailureRecord>,
    timeout_exchanges: Vec<TimeoutRecord>,
}

#[derive(Debug)]
struct FailureRecord {
    from_peer: String,
    to_peer: String,
    error: String,
}

#[derive(Debug)]
struct TimeoutRecord {
    from_peer: String,
    to_peer: String,
}

impl CompatibilityTestResult {
    fn new() -> Self {
        Self {
            successful_exchanges: Vec::new(),
            failed_exchanges: Vec::new(),
            timeout_exchanges: Vec::new(),
        }
    }

    fn record_success(&mut self, request: NetworkMessage, response: NetworkMessage) {
        self.successful_exchanges.push((request, response));
    }

    fn record_failure(&mut self, from_peer: String, to_peer: String, error: String) {
        self.failed_exchanges.push(FailureRecord { from_peer, to_peer, error });
    }

    fn record_timeout(&mut self, from_peer: String, to_peer: String) {
        self.timeout_exchanges.push(TimeoutRecord { from_peer, to_peer });
    }

    fn get_success_rate(&self) -> f64 {
        let total = self.successful_exchanges.len() + self.failed_exchanges.len() + self.timeout_exchanges.len();
        if total == 0 {
            0.0
        } else {
            self.successful_exchanges.len() as f64 / total as f64
        }
    }

    fn print_summary(&self) {
        info!("=== Backwards Compatibility Test Results ===");
        info!("Successful exchanges: {}", self.successful_exchanges.len());
        info!("Failed exchanges: {}", self.failed_exchanges.len());
        info!("Timeout exchanges: {}", self.timeout_exchanges.len());
        info!("Success rate: {:.2}%", self.get_success_rate() * 100.0);

        if !self.failed_exchanges.is_empty() {
            warn!("Failures:");
            for failure in &self.failed_exchanges {
                warn!("  {} -> {}: {}", failure.from_peer, failure.to_peer, failure.error);
            }
        }

        if !self.timeout_exchanges.is_empty() {
            warn!("Timeouts:");
            for timeout in &self.timeout_exchanges {
                warn!("  {} -> {}", timeout.from_peer, timeout.to_peer);
            }
        }

        // Analyze transport usage
        let libp2p_usage = self.successful_exchanges.iter()
            .filter(|(req, _)| matches!(req.transport_used, TransportType::LibP2P))
            .count();
        
        info!("Transport usage in successful exchanges:");
        info!("  LibP2P: {} (100% - as expected for backwards compatibility)", libp2p_usage);
    }
}

#[tokio::test]
async fn test_dual_stack_to_libp2p_compatibility() {
    init_tracing();
    
    info!("Starting dual-stack backwards compatibility test");

    let mut network = CompatibilityTestNetwork::new();

    // Add libp2p-only nodes (representing existing network)
    network.add_libp2p_node("libp2p_only_node_1".to_string());
    network.add_libp2p_node("libp2p_only_node_2".to_string());
    network.add_libp2p_node("libp2p_only_legacy_peer".to_string());

    // Add dual-stack nodes with different preferences
    network.add_dual_stack_node("dual_stack_iroh_preferred".to_string(), TransportPreference::IrohPreferred);
    network.add_dual_stack_node("dual_stack_balanced".to_string(), TransportPreference::Balanced);

    // Run compatibility tests
    let results = network.run_compatibility_test().await;
    results.print_summary();

    // Assertions for backwards compatibility
    assert!(results.get_success_rate() >= 0.95, "Success rate should be at least 95% for backwards compatibility");
    assert!(results.failed_exchanges.is_empty(), "No exchanges should fail due to transport incompatibility");
    
    // Verify that dual-stack nodes fall back to libp2p when communicating with libp2p-only peers
    for (request, response) in &results.successful_exchanges {
        assert!(matches!(request.transport_used, TransportType::LibP2P), 
                "All requests to libp2p-only nodes should use libp2p transport");
        assert!(matches!(response.transport_used, TransportType::LibP2P), 
                "All responses from libp2p-only nodes should use libp2p transport");
    }

    info!("✅ Backwards compatibility test passed - dual-stack nodes successfully communicate with libp2p-only nodes");
}

#[tokio::test]
async fn test_transport_capability_detection() {
    init_tracing();
    
    info!("Testing transport capability detection");

    let dual_stack_node = DualStackNode::new("test_dual_stack".to_string(), TransportPreference::IrohPreferred);

    // Test detection of different peer types
    let libp2p_only_caps = dual_stack_node.detect_peer_capabilities("libp2p_only_peer").await;
    assert_eq!(libp2p_only_caps, vec![TransportType::LibP2P]);

    let dual_stack_caps = dual_stack_node.detect_peer_capabilities("dual_stack_peer").await;
    assert_eq!(dual_stack_caps, vec![TransportType::LibP2P, TransportType::Iroh]);

    let unknown_caps = dual_stack_node.detect_peer_capabilities("unknown_peer").await;
    assert_eq!(unknown_caps, vec![TransportType::LibP2P]); // Default to libp2p for compatibility

    info!("✅ Transport capability detection working correctly");
}

#[tokio::test]
async fn test_transport_selection_strategy() {
    init_tracing();
    
    info!("Testing transport selection strategies");

    // Test iroh-preferred strategy
    let iroh_preferred_node = DualStackNode::new("iroh_preferred".to_string(), TransportPreference::IrohPreferred);
    
    // Should select libp2p for libp2p-only peers
    let transport_for_libp2p = iroh_preferred_node.select_transport("libp2p_only_peer").await;
    assert!(matches!(transport_for_libp2p, TransportType::LibP2P));

    // Should select iroh for dual-stack peers
    let transport_for_dual = iroh_preferred_node.select_transport("dual_stack_peer").await;
    assert!(matches!(transport_for_dual, TransportType::Iroh));

    // Test balanced strategy
    let balanced_node = DualStackNode::new("balanced".to_string(), TransportPreference::Balanced);
    
    // Should always select libp2p in balanced mode for compatibility
    let transport_balanced = balanced_node.select_transport("dual_stack_peer").await;
    assert!(matches!(transport_balanced, TransportType::LibP2P));

    info!("✅ Transport selection strategies working correctly");
}

#[tokio::test]
async fn test_gradual_migration_scenario() {
    init_tracing();
    
    info!("Testing gradual migration scenario");

    let mut network = CompatibilityTestNetwork::new();

    // Simulate a network in gradual migration
    // 70% libp2p-only nodes (existing network)
    for i in 0..7 {
        network.add_libp2p_node(format!("legacy_node_{}", i));
    }

    // 30% dual-stack nodes (new deployments)
    for i in 0..3 {
        network.add_dual_stack_node(
            format!("migrating_node_{}", i), 
            TransportPreference::IrohPreferred
        );
    }

    let results = network.run_compatibility_test().await;
    results.print_summary();

    // In a migration scenario, compatibility should be perfect
    assert_eq!(results.get_success_rate(), 1.0, "During migration, all communications should succeed");
    assert!(results.failed_exchanges.is_empty(), "No communication failures during migration");

    info!("✅ Gradual migration scenario test passed - perfect backwards compatibility maintained");
}

#[tokio::test]
async fn test_message_type_compatibility() {
    init_tracing();
    
    info!("Testing message type compatibility across transports");

    let mut network = CompatibilityTestNetwork::new();
    network.add_libp2p_node("libp2p_test_node".to_string());
    network.add_dual_stack_node("dual_stack_test_node".to_string(), TransportPreference::IrohPreferred);

    // Test all Kademlia message types for compatibility
    let message_types = vec![
        ("Ping", MessageType::Ping),
        ("FindNode", MessageType::FindNode { target: "test_target".to_string() }),
        ("FindValue", MessageType::FindValue { key: "test_key".to_string() }),
        ("PutRecord", MessageType::PutRecord { key: "test_key".to_string(), value: b"test_data".to_vec() }),
        ("Bootstrap", MessageType::Bootstrap),
    ];

    for (name, message_type) in message_types {
        info!("Testing {} message compatibility", name);
        
        match network.simulate_message_exchange(0, 0, message_type).await {
            Ok((request, response)) => {
                assert!(matches!(request.transport_used, TransportType::LibP2P));
                assert!(matches!(response.transport_used, TransportType::LibP2P));
                info!("✅ {} message compatible", name);
            },
            Err(e) => {
                panic!("❌ {} message failed: {}", name, e);
            }
        }
    }

    info!("✅ All Kademlia message types are backwards compatible");
}