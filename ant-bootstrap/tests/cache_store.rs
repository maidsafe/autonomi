// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Test-specific allows
#![allow(clippy::needless_range_loop)]

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::Multiaddr;
use std::time::Duration;
use tempfile::TempDir;

// Valid peer IDs for testing (base58-encoded Ed25519 public keys)
const PEER_IDS: [&str; 5] = [
    "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
    "12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
    "12D3KooWCKCeqLPSgMnDjyFsJuWqREDtKNHx1JEBiwxME7Zdw68n",
    "12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc",
    "12D3KooWSBTB1jzXPyBGpWLMqXfN7MPMNwSsVWCbfkeLXPZr9Dm3",
];

// ============================================================================
// Basic operations (4 tests)
// ============================================================================

#[tokio::test]
async fn test_create_empty_cache_and_reload() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create empty cache
    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Verify cache is empty
    assert_eq!(
        cache_store.peer_count().await,
        0,
        "New cache should be empty"
    );

    // Write empty cache to disk
    cache_store.write().await?;

    // Reload from disk
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Verify still empty after reload
    assert!(
        loaded_data.peers.is_empty(),
        "Empty cache should remain empty when loaded"
    );
    assert_eq!(
        loaded_data.peers.len(),
        0,
        "Loaded cache should have exactly 0 peers"
    );

    Ok(())
}

#[tokio::test]
async fn test_add_peer_and_reload() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create cache
    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone()).await;

    // Verify peer is in memory
    assert_eq!(
        cache_store.peer_count().await,
        1,
        "Cache should have 1 peer after add"
    );

    // Write to disk
    cache_store.write().await?;

    // Reload from disk
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Verify peer exists after reload
    assert_eq!(
        loaded_data.peers.len(),
        1,
        "Loaded cache should have exactly 1 peer"
    );

    // Verify the address matches
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();
    assert_eq!(loaded_addrs.len(), 1, "Should have exactly 1 address");
    assert_eq!(
        loaded_addrs[0].to_string(),
        addr.to_string(),
        "Loaded address should match original"
    );

    Ok(())
}

#[tokio::test]
async fn test_max_peers_limit_fifo_eviction() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create cache with small max_peers limit
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(3);

    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Store addresses to check FIFO behavior
    let mut addresses = Vec::new();
    for i in 0..5 {
        let addr: Multiaddr = format!(
            "/ip4/127.0.0.{}/udp/808{}/quic-v1/p2p/{}",
            i + 1,
            i,
            PEER_IDS[i]
        )
        .parse()?;
        addresses.push(addr.clone());
        cache_store.add_addr(addr).await;

        // Add delay to ensure distinct ordering
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Verify we never exceed max
        assert!(
            cache_store.peer_count().await <= 3,
            "Cache should enforce max_peers limit"
        );
    }

    // Verify exactly max_peers in cache
    assert_eq!(
        cache_store.peer_count().await,
        3,
        "Should have exactly 3 peers"
    );

    // Get current addresses
    let current_addrs = cache_store.get_all_addrs().await;

    // Verify FIFO: oldest (first 2) should be gone
    for i in 0..2 {
        let addr_str = addresses[i].to_string();
        assert!(
            !current_addrs.iter().any(|a| a.to_string() == addr_str),
            "Oldest address #{} should have been evicted (FIFO)",
            i + 1
        );
    }

    // Verify newest (last 3) remain
    for i in 2..5 {
        let addr_str = addresses[i].to_string();
        assert!(
            current_addrs.iter().any(|a| a.to_string() == addr_str),
            "Newest address #{} should be in cache",
            i + 1
        );
    }

    // Write to disk and reload
    cache_store.write().await?;
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();

    // Verify FIFO persists after reload
    assert_eq!(
        loaded_addrs.len(),
        3,
        "Should have exactly 3 peers after reload"
    );

    for i in 0..2 {
        let addr_str = addresses[i].to_string();
        assert!(
            !loaded_addrs.iter().any(|a| a.to_string() == addr_str),
            "After reload, oldest address #{} should be gone",
            i + 1
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_max_addrs_per_peer_limit() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create cache with small max_addrs_per_peer limit
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_addrs_per_peer(2);

    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Create multiple addresses for the same peer
    let peer_id = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
    for i in 1..=4 {
        let addr: Multiaddr = format!("/ip4/127.0.0.{i}/udp/8080/quic-v1/p2p/{peer_id}").parse()?;
        cache_store.add_addr(addr).await;
    }

    // Verify only 1 peer (same peer ID)
    assert_eq!(
        cache_store.peer_count().await,
        1,
        "Should have exactly 1 peer"
    );

    // Write to disk and reload
    cache_store.write().await?;
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Verify peer count
    assert_eq!(
        loaded_data.peers.len(),
        1,
        "Should have exactly 1 peer after reload"
    );

    // Verify max_addrs_per_peer is enforced
    let (_, addrs) = loaded_data.peers.front().unwrap();
    assert!(
        addrs.len() <= 2,
        "Should enforce max_addrs_per_peer limit, got {} addrs",
        addrs.len()
    );

    Ok(())
}

// ============================================================================
// Sync operations (3 tests)
// ============================================================================

#[tokio::test]
async fn test_sync_and_flush_merges_memory_with_disk() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());

    // Store 1 writes addr A to disk
    let store1 = BootstrapCacheStore::new(config.clone())?;
    let addr_a: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    store1.add_addr(addr_a.clone()).await;
    store1.write().await?;

    // Store 2 adds addr B to memory, then syncs with disk
    let store2 = BootstrapCacheStore::new(config.clone())?;
    let addr_b: Multiaddr =
        "/ip4/127.0.0.2/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .parse()?;
    store2.add_addr(addr_b.clone()).await;
    store2.sync_and_flush_to_disk().await?;

    // Reload and verify both addresses present
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();

    assert_eq!(
        loaded_addrs.len(),
        2,
        "Should have both addresses after sync"
    );

    let has_addr_a = loaded_addrs
        .iter()
        .any(|a| a.to_string() == addr_a.to_string());
    let has_addr_b = loaded_addrs
        .iter()
        .any(|a| a.to_string() == addr_b.to_string());

    assert!(has_addr_a, "Address A from disk should be present");
    assert!(has_addr_b, "Address B from memory should be present");

    Ok(())
}

#[tokio::test]
async fn test_sync_preserves_memory_peers_at_front() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());

    // Write peer A to disk first
    let store1 = BootstrapCacheStore::new(config.clone())?;
    let addr_a: Multiaddr =
        format!("/ip4/127.0.0.1/udp/8081/quic-v1/p2p/{}", PEER_IDS[0]).parse()?;
    store1.add_addr(addr_a.clone()).await;
    store1.write().await?;

    // Store 2 adds peer B (newer) to memory, syncs with disk
    let store2 = BootstrapCacheStore::new(config.clone())?;
    let addr_b: Multiaddr =
        format!("/ip4/127.0.0.1/udp/8082/quic-v1/p2p/{}", PEER_IDS[1]).parse()?;
    store2.add_addr(addr_b.clone()).await;
    store2.sync_and_flush_to_disk().await?;

    // Reload and verify order: memory peer (B) should be at front
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();

    assert_eq!(loaded_addrs.len(), 2, "Should have 2 addresses");
    assert_eq!(
        loaded_addrs[0].to_string(),
        addr_b.to_string(),
        "Memory peer (B) should be at front (newer)"
    );
    assert_eq!(
        loaded_addrs[1].to_string(),
        addr_a.to_string(),
        "Disk peer (A) should be at back (older)"
    );

    Ok(())
}

#[tokio::test]
async fn test_sync_preserves_disk_peer_order() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());

    // Write peers 1, 2, 3 to disk (order in cache: [3, 2, 1] - newest first)
    let store1 = BootstrapCacheStore::new(config.clone())?;
    let addr1: Multiaddr =
        format!("/ip4/127.0.0.1/udp/8081/quic-v1/p2p/{}", PEER_IDS[0]).parse()?;
    let addr2: Multiaddr =
        format!("/ip4/127.0.0.1/udp/8082/quic-v1/p2p/{}", PEER_IDS[1]).parse()?;
    let addr3: Multiaddr =
        format!("/ip4/127.0.0.1/udp/8083/quic-v1/p2p/{}", PEER_IDS[2]).parse()?;

    store1.add_addr(addr1.clone()).await;
    store1.add_addr(addr2.clone()).await;
    store1.add_addr(addr3.clone()).await;
    store1.write().await?;

    // Store 2 syncs with empty memory (just loads disk peers)
    let store2 = BootstrapCacheStore::new(config.clone())?;
    store2.sync_and_flush_to_disk().await?;

    // Reload and verify disk order is preserved: [3, 2, 1]
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();

    assert_eq!(loaded_addrs.len(), 3, "Should have 3 addresses");
    assert_eq!(
        loaded_addrs[0].to_string(),
        addr3.to_string(),
        "Newest disk peer (3) should be first"
    );
    assert_eq!(
        loaded_addrs[1].to_string(),
        addr2.to_string(),
        "Middle disk peer (2) should be second"
    );
    assert_eq!(
        loaded_addrs[2].to_string(),
        addr1.to_string(),
        "Oldest disk peer (1) should be last"
    );

    Ok(())
}

// ============================================================================
// Peer removal (2 tests)
// ============================================================================

#[tokio::test]
async fn test_remove_peer_immediate() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config)?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone()).await;

    // Verify peer is in cache
    assert_eq!(
        cache_store.peer_count().await,
        1,
        "Should have 1 peer before removal"
    );

    // Get the peer ID and queue for removal
    let peer_id = ant_bootstrap::multiaddr_get_peer_id(&addr).unwrap();
    cache_store.queue_remove_peer(&peer_id).await;

    // Apply removal via sync
    cache_store.sync_and_flush_to_disk().await?;

    // Verify peer is gone
    let addrs = cache_store.get_all_addrs().await;
    assert!(addrs.is_empty(), "Peer should be removed after sync");

    Ok(())
}

#[tokio::test]
async fn test_queued_removal_applied_on_sync() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());

    // Store 1: write peer to disk
    let store1 = BootstrapCacheStore::new(config.clone())?;
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    store1.add_addr(addr.clone()).await;
    store1.sync_and_flush_to_disk().await?;

    // Verify peer is on disk
    let loaded = BootstrapCacheStore::load_cache_data(&config)?;
    assert_eq!(
        loaded.peers.len(),
        1,
        "Peer should be on disk after initial sync"
    );

    // Store 2: queue removal (doesn't load peer into memory)
    let store2 = BootstrapCacheStore::new(config.clone())?;
    let peer_id = ant_bootstrap::multiaddr_get_peer_id(&addr).unwrap();
    store2.queue_remove_peer(&peer_id).await;

    // Apply queued removal - loads from disk, applies removal, writes back
    store2.sync_and_flush_to_disk().await?;

    // Verify peer is removed from disk
    let loaded = BootstrapCacheStore::load_cache_data(&config)?;
    assert_eq!(
        loaded.peers.len(),
        0,
        "Peer should be removed from disk after queued removal sync"
    );

    Ok(())
}

// ============================================================================
// Filtering (3 tests)
// ============================================================================

#[tokio::test]
async fn test_add_addr_ignores_p2p_circuit() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config)?;

    // Try to add a P2pCircuit address
    let circuit_addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE/p2p-circuit/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .parse()?;
    cache_store.add_addr(circuit_addr).await;

    // Verify it was silently ignored
    assert_eq!(
        cache_store.peer_count().await,
        0,
        "P2pCircuit addresses should be silently ignored"
    );

    Ok(())
}

#[tokio::test]
async fn test_add_addr_ignores_invalid_multiaddr() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config)?;

    // Try to add an address without quic-v1 (invalid for this network)
    // craft_valid_multiaddr should reject this
    let invalid_addr: Multiaddr =
        "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(invalid_addr).await;

    // Verify it was silently ignored (craft_valid_multiaddr returns None)
    assert_eq!(
        cache_store.peer_count().await,
        0,
        "Invalid addresses should be silently ignored"
    );

    Ok(())
}

#[tokio::test]
async fn test_add_addr_ignores_missing_peer_id() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config)?;

    // Try to add an address without peer ID
    let no_peer_id_addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse()?;
    cache_store.add_addr(no_peer_id_addr).await;

    // Verify it was silently ignored
    assert_eq!(
        cache_store.peer_count().await,
        0,
        "Addresses without peer ID should be silently ignored"
    );

    Ok(())
}

// ============================================================================
// Error handling (2 tests)
// ============================================================================

#[tokio::test]
async fn test_corrupted_cache_file_returns_error() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create a valid cache first
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer and write
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr).await;
    cache_store.write().await?;

    // Corrupt the cache file
    let cache_path = cache_dir
        .join("version_1")
        .join(BootstrapCacheStore::cache_file_name(false));
    std::fs::write(&cache_path, "{not valid json}")?;

    // Attempt to load corrupted cache
    let result = BootstrapCacheStore::load_cache_data(&config);
    assert!(
        result.is_err(),
        "Loading corrupted cache should return error"
    );

    Ok(())
}

#[tokio::test]
async fn test_recovery_after_corruption() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create and write valid cache
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let cache_store = BootstrapCacheStore::new(config.clone())?;
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr).await;
    cache_store.write().await?;

    // Corrupt the cache file
    let cache_path = cache_dir
        .join("version_1")
        .join(BootstrapCacheStore::cache_file_name(false));
    std::fs::write(&cache_path, "{not valid json}")?;

    // Verify load fails
    let result = BootstrapCacheStore::load_cache_data(&config);
    assert!(result.is_err(), "Loading corrupted cache should fail");

    // Create new store and write fresh data (recovery)
    let new_store = BootstrapCacheStore::new(config.clone())?;
    assert_eq!(new_store.peer_count().await, 0, "New store should be empty");

    // Add a different peer and write
    let new_addr: Multiaddr =
        "/ip4/192.168.1.1/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .parse()?;
    new_store.add_addr(new_addr.clone()).await;
    new_store.write().await?;

    // Verify recovery: load should now succeed
    let loaded = BootstrapCacheStore::load_cache_data(&config)?;
    assert_eq!(loaded.peers.len(), 1, "Recovery should work");

    let loaded_addrs: Vec<Multiaddr> = loaded.get_all_addrs().cloned().collect();
    assert_eq!(
        loaded_addrs[0].to_string(),
        new_addr.to_string(),
        "Recovered data should match new peer"
    );

    Ok(())
}

// ============================================================================
// Concurrent access (1 test)
// ============================================================================

#[tokio::test]
async fn test_concurrent_writes_no_data_loss() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());

    // Use a barrier to ensure true concurrency
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(5));

    let mut handles = Vec::new();
    for i in 0..5 {
        let config_clone = config.clone();
        let barrier_clone = std::sync::Arc::clone(&barrier);
        let peer_id = PEER_IDS[i].to_string();

        let handle = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier_clone.wait().await;

            // Create a new cache store
            let cache_store = BootstrapCacheStore::new(config_clone)?;

            // Add a unique addr for this task
            let addr: Multiaddr =
                format!("/ip4/127.0.0.{}/udp/8080/quic-v1/p2p/{}", i + 1, peer_id)
                    .parse()
                    .unwrap();
            cache_store.add_addr(addr).await;

            // Small random delay to increase interleaving
            tokio::time::sleep(Duration::from_millis(i as u64 * 5)).await;

            // Sync and flush
            cache_store.sync_and_flush_to_disk().await
        });

        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await??;
    }

    // Verify all peers are present (no data loss)
    let final_store = BootstrapCacheStore::new(config.clone())?;
    let cache_data = BootstrapCacheStore::load_cache_data(final_store.config())?;

    assert_eq!(
        cache_data.peers.len(),
        5,
        "Should have all 5 unique peers - no data loss"
    );

    // Verify each peer is unique
    let addrs: Vec<String> = cache_data.get_all_addrs().map(|a| a.to_string()).collect();
    for i in 1..=5 {
        let expected_ip = format!("127.0.0.{i}");
        assert!(
            addrs.iter().any(|a| a.contains(&expected_ip)),
            "Should contain peer with IP {expected_ip}"
        );
    }

    Ok(())
}
