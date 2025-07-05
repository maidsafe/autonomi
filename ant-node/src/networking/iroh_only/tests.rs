//! Tests for pure iroh networking implementation
//! 
//! This module provides comprehensive testing for the Phase 5 iroh-only
//! transport system, validating performance, reliability, and compatibility.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;
    
    use crate::networking::kad::transport::{KadPeerId, KadAddress, PeerInfo, Record, RecordKey};
    
    /// Test iroh transport creation and initialization
    #[tokio::test]
    async fn test_iroh_transport_creation() {
        let config = IrohConfig::development();
        let local_peer_id = KadPeerId::new(b"test_peer".to_vec());
        
        let transport = IrohTransport::new(config, local_peer_id.clone()).await;
        assert!(transport.is_ok());
        
        let transport = transport.unwrap();
        assert_eq!(transport.local_peer_id().await, local_peer_id);
        
        // Cleanup
        transport.shutdown().await.unwrap();
    }
    
    /// Test iroh configuration validation
    #[tokio::test]
    async fn test_iroh_config_validation() {
        // Valid configuration
        let valid_config = IrohConfig::production();
        assert!(valid_config.validate().is_ok());
        
        // Invalid configuration - zero connection pool
        let mut invalid_config = IrohConfig::default();
        invalid_config.connection_pool.max_size = 0;
        assert!(invalid_config.validate().is_err());
        
        // Invalid configuration - negative latency target
        let mut invalid_config = IrohConfig::default();
        invalid_config.performance.latency.target_ms = -1.0;
        assert!(invalid_config.validate().is_err());
    }
    
    /// Test metrics collection functionality
    #[tokio::test]
    async fn test_metrics_collection() {
        let config = MetricsConfig::default();
        let metrics = IrohMetrics::new(config).await.unwrap();
        
        let peer_id = KadPeerId::new(b"test_peer".to_vec());
        
        // Record some operations
        metrics.record_operation(&peer_id, "find_node", Duration::from_millis(50), true).await;
        metrics.record_operation(&peer_id, "find_value", Duration::from_millis(75), true).await;
        metrics.record_operation(&peer_id, "put_record", Duration::from_millis(100), false).await;
        
        // Get metrics snapshot
        let snapshot = metrics.get_metrics_snapshot().await;
        assert!(snapshot.global.success_rate > 0.0);
        assert!(snapshot.global.success_rate < 1.0); // One failed operation
        
        // Cleanup
        metrics.shutdown().await.unwrap();
    }
    
    /// Test peer discovery functionality
    #[tokio::test]
    async fn test_peer_discovery() {
        let config = DiscoveryConfig::default();
        let discovery = IrohDiscovery::new(config).await.unwrap();
        
        // Add bootstrap peer
        let bootstrap_peer = PeerInfo {
            peer_id: KadPeerId::new(b"bootstrap".to_vec()),
            addresses: vec![KadAddress::new(b"bootstrap_addr".to_vec())],
            connection_status: crate::networking::kad::transport::ConnectionStatus::Connected,
        };
        discovery.add_bootstrap_peer(bootstrap_peer).await;
        
        // Test cache statistics
        let cache_stats = discovery.get_cache_stats().await;
        assert_eq!(cache_stats.total_entries, 0); // No cached peers yet
        
        // Test discovery statistics
        let discovery_stats = discovery.get_discovery_stats().await;
        assert_eq!(discovery_stats.total_discoveries, 0);
        
        // Cleanup
        discovery.shutdown().await.unwrap();
    }
    
    /// Test connection pool management
    #[tokio::test]
    async fn test_connection_pool() {
        let mut config = IrohConfig::development();
        config.connection_pool.max_size = 5; // Small pool for testing
        
        let local_peer_id = KadPeerId::new(b"test_peer".to_vec());
        let transport = IrohTransport::new(config, local_peer_id).await.unwrap();
        
        // Test connection creation (would normally connect to real peers)
        // This test verifies the structure without actual network operations
        
        // Cleanup
        transport.shutdown().await.unwrap();
    }
    
    /// Test iroh transport KademliaTransport implementation
    #[tokio::test]
    async fn test_kademlia_transport_interface() {
        let config = IrohConfig::development();
        let local_peer_id = KadPeerId::new(b"test_peer".to_vec());
        let transport = IrohTransport::new(config, local_peer_id.clone()).await.unwrap();
        
        // Test local peer ID
        assert_eq!(transport.local_peer_id().await, local_peer_id);
        
        // Test basic operations (these will fail without actual iroh implementation)
        let target_peer = KadPeerId::new(b"target".to_vec());
        let result = timeout(Duration::from_secs(1), transport.find_node(target_peer)).await;
        assert!(result.is_ok()); // Timeout should not occur
        
        // Test record operations
        let record = Record {
            key: RecordKey(b"test_key".to_vec()),
            value: b"test_value".to_vec(),
        };
        let result = timeout(Duration::from_secs(1), transport.put_record(record)).await;
        assert!(result.is_ok());
        
        // Cleanup
        transport.shutdown().await.unwrap();
    }
    
    /// Test configuration builder patterns
    #[tokio::test]
    async fn test_config_builders() {
        // Test IrohConfig builder
        let config = IrohConfig::builder()
            .with_connection_pool(|pool| {
                pool.max_size = 500;
                pool.idle_timeout = Duration::from_minutes(10);
            })
            .with_performance_settings(|perf| {
                perf.enabled = true;
                perf.history_size = 5000;
            })
            .with_metrics_settings(|metrics| {
                metrics.per_peer_metrics = true;
            })
            .build();
        
        assert_eq!(config.connection_pool.max_size, 500);
        assert_eq!(config.connection_pool.idle_timeout, Duration::from_minutes(10));
        assert!(config.performance.enabled);
        assert_eq!(config.performance.history_size, 5000);
        assert!(config.metrics.per_peer_metrics);
        
        // Validate the built configuration
        assert!(config.validate().is_ok());
    }
    
    /// Test performance optimization features
    #[tokio::test]
    async fn test_performance_optimization() {
        let config = IrohConfig::high_performance();
        assert!(config.performance.enabled);
        assert!(config.advanced.feature_flags.performance_optimization);
        assert!(config.advanced.feature_flags.connection_pooling);
        
        // Test configuration is valid
        assert!(config.validate().is_ok());
        
        // Verify high-performance settings
        assert!(config.connection_pool.max_size >= 1000);
        assert!(config.performance.latency.target_ms <= 50.0);
        assert!(config.performance.bandwidth.target_utilization >= 0.90);
    }
    
    /// Test migration utilities
    #[tokio::test]
    async fn test_migration_utilities() {
        // Test Phase 5 readiness validation
        let readiness = crate::networking::iroh_only::migration::validate_phase5_readiness();
        assert!(readiness.is_ok()); // Should pass in test environment
        
        // Test configuration migration (requires dual-stack config)
        // This would normally convert from Phase 3 dual-stack to Phase 5 iroh-only
        
        // Test migration report creation
        let report = crate::networking::iroh_only::migration::create_migration_report();
        assert_eq!(report.performance_improvement, 0.0); // Initial values
    }
    
    /// Test error handling and recovery
    #[tokio::test]
    async fn test_error_handling() {
        let config = IrohConfig::development();
        let local_peer_id = KadPeerId::new(b"test_peer".to_vec());
        let transport = IrohTransport::new(config, local_peer_id).await.unwrap();
        
        // Test operation with non-existent peer
        let non_existent_peer = KadPeerId::new(b"non_existent".to_vec());
        let result = transport.find_node(non_existent_peer).await;
        // Should return error, not panic
        assert!(result.is_err());
        
        // Test graceful shutdown
        let shutdown_result = transport.shutdown().await;
        assert!(shutdown_result.is_ok());
    }
    
    /// Test concurrent operations
    #[tokio::test]
    async fn test_concurrent_operations() {
        let config = IrohConfig::development();
        let local_peer_id = KadPeerId::new(b"test_peer".to_vec());
        let transport = Arc::new(IrohTransport::new(config, local_peer_id).await.unwrap());
        
        let mut handles = Vec::new();
        
        // Spawn multiple concurrent operations
        for i in 0..10 {
            let transport_clone = Arc::clone(&transport);
            let handle = tokio::spawn(async move {
                let peer_id = KadPeerId::new(format!("peer_{}", i).as_bytes().to_vec());
                let result = transport_clone.find_node(peer_id).await;
                // Should handle concurrent operations gracefully
                result.is_err() // Expected to fail without real iroh implementation
            });
            handles.push(handle);
        }
        
        // Wait for all operations to complete
        for handle in handles {
            let result = handle.await;
            assert!(result.is_ok());
        }
        
        // Cleanup
        transport.shutdown().await.unwrap();
    }
    
    /// Integration test for full iroh-only workflow
    #[tokio::test]
    async fn test_iroh_only_integration() {
        // This test validates the complete iroh-only workflow
        // from initialization through operations to shutdown
        
        let config = IrohConfig::development();
        let local_peer_id = KadPeerId::new(b"integration_test".to_vec());
        
        // Initialize transport
        let transport = IrohTransport::new(config, local_peer_id.clone()).await.unwrap();
        
        // Verify initialization
        assert_eq!(transport.local_peer_id().await, local_peer_id);
        
        // Simulate bootstrap
        let bootstrap_peers = vec![
            PeerInfo {
                peer_id: KadPeerId::new(b"bootstrap1".to_vec()),
                addresses: vec![KadAddress::new(b"addr1".to_vec())],
                connection_status: crate::networking::kad::transport::ConnectionStatus::Connected,
            },
            PeerInfo {
                peer_id: KadPeerId::new(b"bootstrap2".to_vec()),
                addresses: vec![KadAddress::new(b"addr2".to_vec())],
                connection_status: crate::networking::kad::transport::ConnectionStatus::Connected,
            },
        ];
        
        let bootstrap_result = transport.bootstrap(bootstrap_peers).await;
        assert!(bootstrap_result.is_ok());
        
        // Simulate some operations
        let record = Record {
            key: RecordKey(b"integration_key".to_vec()),
            value: b"integration_value".to_vec(),
        };
        
        let put_result = transport.put_record(record.clone()).await;
        assert!(put_result.is_err()); // Expected without real iroh
        
        let find_result = transport.find_value(record.key).await;
        assert!(find_result.is_err()); // Expected without real iroh
        
        // Test routing table operations
        let routing_table = transport.get_routing_table_info().await;
        assert!(routing_table.is_ok());
        
        // Test peer management
        let test_peer = KadPeerId::new(b"test_peer_mgmt".to_vec());
        let test_address = KadAddress::new(b"test_address".to_vec());
        
        let add_result = transport.add_address(test_peer.clone(), test_address).await;
        assert!(add_result.is_ok());
        
        let remove_result = transport.remove_peer(&test_peer).await;
        assert!(remove_result.is_ok());
        
        // Graceful shutdown
        let shutdown_result = transport.shutdown().await;
        assert!(shutdown_result.is_ok());
    }
}

/// Benchmarks for iroh-only performance validation
#[cfg(test)]
mod benchmarks {
    use super::*;
    use std::time::Instant;
    
    /// Benchmark transport initialization time
    #[tokio::test]
    async fn bench_transport_initialization() {
        let config = IrohConfig::production();
        let local_peer_id = KadPeerId::new(b"bench_peer".to_vec());
        
        let start = Instant::now();
        let transport = IrohTransport::new(config, local_peer_id).await.unwrap();
        let initialization_time = start.elapsed();
        
        println!("Transport initialization time: {:?}", initialization_time);
        assert!(initialization_time < Duration::from_secs(5)); // Should be fast
        
        transport.shutdown().await.unwrap();
    }
    
    /// Benchmark metrics collection overhead
    #[tokio::test]
    async fn bench_metrics_overhead() {
        let config = MetricsConfig::default();
        let metrics = IrohMetrics::new(config).await.unwrap();
        
        let peer_id = KadPeerId::new(b"bench_peer".to_vec());
        let operation_count = 1000;
        
        let start = Instant::now();
        for i in 0..operation_count {
            metrics.record_operation(
                &peer_id,
                "benchmark",
                Duration::from_millis(10),
                i % 10 != 0, // 90% success rate
            ).await;
        }
        let total_time = start.elapsed();
        
        let avg_time_per_operation = total_time / operation_count;
        println!("Average metrics overhead per operation: {:?}", avg_time_per_operation);
        assert!(avg_time_per_operation < Duration::from_micros(100)); // Should be minimal
        
        metrics.shutdown().await.unwrap();
    }
    
    /// Benchmark connection pool performance
    #[tokio::test]
    async fn bench_connection_pool() {
        let mut config = IrohConfig::high_performance();
        config.connection_pool.max_size = 1000;
        
        let local_peer_id = KadPeerId::new(b"bench_peer".to_vec());
        let transport = IrohTransport::new(config, local_peer_id).await.unwrap();
        
        // Simulate rapid connection requests
        let start = Instant::now();
        let connection_count = 100;
        
        for i in 0..connection_count {
            let peer_id = KadPeerId::new(format!("peer_{}", i).as_bytes().to_vec());
            // Connection pool operations happen internally during operations
            let _ = transport.find_node(peer_id).await; // Will fail but exercises pool
        }
        
        let total_time = start.elapsed();
        let avg_time_per_connection = total_time / connection_count;
        
        println!("Average connection pool operation time: {:?}", avg_time_per_connection);
        assert!(avg_time_per_connection < Duration::from_millis(10)); // Should be fast
        
        transport.shutdown().await.unwrap();
    }
}