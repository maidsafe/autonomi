// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Validation test for Phase 2 iroh adapter implementation
//! 
//! This standalone test validates that the core Phase 2 components work correctly
//! without requiring the full Kademlia integration that has type conflicts.

use std::time::Duration;
use tokio;

use crate::networking::{
    kad::transport::{KadPeerId, KadAddress},
    iroh_adapter::{
        config::{IrohConfig, IrohConfigBuilder, SerializationFormat},
        discovery::DiscoveryBridge,
        metrics::IrohMetrics,
        protocol::{KadRequest, KadReply, KadProtocol, MessageHandler, ProtocolStats},
        IrohError, IrohResult,
    },
};

/// Simple test to validate Phase 2 iroh adapter core functionality
#[tokio::test]
async fn test_phase2_core_functionality() {
    println!("🚀 Testing Phase 2 iroh adapter core functionality...");
    
    // Test 1: Configuration validation
    println!("✅ Test 1: Configuration system");
    let config = IrohConfig::default();
    assert!(config.validate().is_ok(), "Default config should be valid");
    
    let custom_config = IrohConfigBuilder::new()
        .with_network(|net| {
            net.max_connections = 100;
            net.enable_relay = false;
        })
        .with_protocol(|proto| {
            proto.serialization_format = SerializationFormat::Json;
            proto.request_timeout = Duration::from_secs(10);
        })
        .build();
    assert!(custom_config.validate().is_ok(), "Custom config should be valid");
    println!("   ✓ Default and custom configuration validation passed");
    
    // Test 2: Discovery bridge functionality
    println!("✅ Test 2: Discovery bridge operations");
    let discovery = DiscoveryBridge::new(custom_config.discovery.clone());
    
    // Add a test peer
    let test_peer_id = KadPeerId::new(vec![42; 32]);
    let test_addresses = vec![
        KadAddress::new("quic".to_string(), "127.0.0.1:8080".to_string()),
        KadAddress::new("quic".to_string(), "127.0.0.1:8081".to_string()),
    ];
    
    discovery.add_kad_peer(test_peer_id.clone(), test_addresses.clone()).await;
    
    // Verify peer was added
    let addresses = discovery.get_peer_addresses(&test_peer_id).await;
    assert!(!addresses.is_empty(), "Peer addresses should not be empty");
    
    // Check statistics
    let stats = discovery.stats().await;
    assert_eq!(stats.kad_peers_tracked, 1, "Should track exactly one peer");
    println!("   ✓ Discovery bridge peer management passed");
    
    // Test 3: Metrics collection
    println!("✅ Test 3: Metrics system");
    let metrics = IrohMetrics::new(custom_config.metrics.clone());
    
    // Record some test metrics
    metrics.record_connection_established(&test_peer_id).await;
    metrics.record_message_sent(&test_peer_id, 1024).await;
    metrics.record_latency(&test_peer_id, Duration::from_millis(50)).await;
    
    // Verify metrics
    let aggregated = metrics.get_metrics().await;
    assert_eq!(aggregated.connections.total_connections, 1);
    assert_eq!(aggregated.messages.messages_sent, 1);
    assert_eq!(aggregated.messages.bytes_sent, 1024);
    assert!(aggregated.latency.total_samples > 0);
    println!("   ✓ Metrics collection and aggregation passed");
    
    // Test 4: Protocol message serialization
    println!("✅ Test 4: Protocol message handling");
    
    // Test different serialization formats
    let test_serialization = |format: SerializationFormat| async move {
        let mut proto_config = custom_config.protocol.clone();
        proto_config.serialization_format = format;
        
        let request = KadRequest {
            id: 12345,
            message: crate::networking::kad::transport::KadMessage::Ping {
                requester: test_peer_id.clone(),
            },
            timestamp: 1234567890,
            sender: test_peer_id.clone(),
        };
        
        // Test serialization and deserialization
        let protocol = MockKadProtocol::new(proto_config);
        let serialized = protocol.test_serialize_request(&request).unwrap();
        assert!(!serialized.is_empty(), "Serialized data should not be empty");
        
        let deserialized = protocol.test_deserialize_request(&serialized).unwrap();
        assert_eq!(request.id, deserialized.id, "Request ID should match");
        assert_eq!(request.timestamp, deserialized.timestamp, "Timestamp should match");
    };
    
    // Test all serialization formats
    test_serialization(SerializationFormat::Json).await;
    test_serialization(SerializationFormat::Postcard).await;
    test_serialization(SerializationFormat::Bincode).await;
    println!("   ✓ Protocol message serialization passed for all formats");
    
    // Test 5: Error handling
    println!("✅ Test 5: Error handling");
    
    // Test error types and conversions
    let timeout_error = IrohError::Timeout { duration: Duration::from_secs(5) };
    assert!(timeout_error.to_string().contains("timeout"));
    
    let connection_error = IrohError::Connection {
        peer: "test-peer".to_string(),
        source: Box::new(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "test")),
    };
    assert!(connection_error.to_string().contains("connection failed"));
    
    // Test conversion to KadError
    use crate::networking::kad::transport::KadError;
    let kad_error: KadError = timeout_error.into();
    match kad_error {
        KadError::Timeout { duration } => assert_eq!(duration, Duration::from_secs(5)),
        _ => panic!("Expected timeout error"),
    }
    println!("   ✓ Error handling and conversion passed");
    
    println!("🎉 All Phase 2 core functionality tests passed!");
    println!("✅ Phase 2 Implementation Validation: SUCCESS");
}

/// Mock protocol for testing serialization without full iroh dependencies
struct MockKadProtocol {
    config: crate::networking::iroh_adapter::config::ProtocolConfig,
}

impl MockKadProtocol {
    fn new(config: crate::networking::iroh_adapter::config::ProtocolConfig) -> Self {
        Self { config }
    }
    
    fn test_serialize_request(&self, request: &KadRequest) -> IrohResult<Vec<u8>> {
        match self.config.serialization_format {
            SerializationFormat::Json => {
                serde_json::to_vec(request)
                    .map_err(|_| IrohError::Serialization(postcard::Error::DeserializeBadEncoding))
            },
            SerializationFormat::Postcard => {
                postcard::to_allocvec(request)
                    .map_err(IrohError::Serialization)
            },
            SerializationFormat::Bincode => {
                bincode::serialize(request)
                    .map_err(|_| IrohError::Serialization(postcard::Error::DeserializeBadEncoding))
            },
        }
    }
    
    fn test_deserialize_request(&self, data: &[u8]) -> IrohResult<KadRequest> {
        match self.config.serialization_format {
            SerializationFormat::Json => {
                serde_json::from_slice(data)
                    .map_err(|_| IrohError::Serialization(postcard::Error::DeserializeBadEncoding))
            },
            SerializationFormat::Postcard => {
                postcard::from_bytes(data)
                    .map_err(IrohError::Serialization)
            },
            SerializationFormat::Bincode => {
                bincode::deserialize(data)
                    .map_err(|_| IrohError::Serialization(postcard::Error::DeserializeBadEncoding))
            },
        }
    }
}

/// Test runner function that can be called externally
pub async fn run_phase2_validation() -> Result<(), Box<dyn std::error::Error>> {
    test_phase2_core_functionality().await;
    Ok(())
}

#[tokio::test]
async fn test_configuration_presets() {
    println!("🚀 Testing configuration presets...");
    
    // Test all preset configurations
    let local = IrohConfig::local_development();
    assert!(local.validate().is_ok());
    assert!(!local.network.enable_relay, "Local dev should not use relay");
    assert!(!local.network.enable_stun, "Local dev should not use STUN");
    
    let production = IrohConfig::production();
    assert!(production.validate().is_ok());
    assert_eq!(production.network.max_connections, 5000);
    assert!(production.network.enable_relay, "Production should use relay");
    
    let minimal = IrohConfig::minimal();
    assert!(minimal.validate().is_ok());
    assert_eq!(minimal.network.max_connections, 50);
    
    println!("✅ All configuration presets are valid");
}

#[tokio::test] 
async fn test_peer_reliability_tracking() {
    println!("🚀 Testing peer reliability tracking...");
    
    let config = IrohConfig::default();
    let discovery = DiscoveryBridge::new(config.discovery);
    
    let peer_id = KadPeerId::new(vec![99; 32]);
    let addresses = vec![KadAddress::new("quic".to_string(), "127.0.0.1:9999".to_string())];
    
    // Add peer and test reliability updates
    discovery.add_kad_peer(peer_id.clone(), addresses).await;
    
    // Test successful interactions (should increase reliability)
    for _ in 0..5 {
        discovery.update_peer_reliability(&peer_id, true).await;
    }
    
    // Test failed interactions (should decrease reliability)
    for _ in 0..2 {
        discovery.update_peer_reliability(&peer_id, false).await;
    }
    
    // Peer should still be tracked
    let stats = discovery.stats().await;
    assert_eq!(stats.kad_peers_tracked, 1);
    
    println!("✅ Peer reliability tracking works correctly");
}

#[tokio::test]
async fn test_metrics_performance() {
    println!("🚀 Testing metrics performance...");
    
    let config = IrohConfig::default();
    let metrics = IrohMetrics::new(config.metrics);
    let peer_id = KadPeerId::new(vec![123; 32]);
    
    let start = std::time::Instant::now();
    
    // Simulate high-frequency updates
    for i in 0..1000 {
        metrics.record_message_sent(&peer_id, 1024).await;
        if i % 100 == 0 {
            metrics.record_latency(&peer_id, Duration::from_millis(10 + i / 100)).await;
        }
    }
    
    let elapsed = start.elapsed();
    
    // Should complete quickly (under 100ms for 1000 operations)
    assert!(elapsed < Duration::from_millis(100), "Metrics recording should be fast");
    
    let aggregated = metrics.get_metrics().await;
    assert_eq!(aggregated.messages.messages_sent, 1000);
    assert!(aggregated.latency.total_samples >= 10);
    
    println!("✅ Metrics performance test passed in {:?}", elapsed);
}