// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Tests for periodic sync (`sync_and_flush_periodically`) and `disable_cache_writing` flag.
//!
//! These tests verify the background cache sync behavior and the ability to disable writes.

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::Multiaddr;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

// ============================================================================
// sync_and_flush_periodically tests (3 tests)
// ============================================================================

/// Verifies that the periodic sync task actually writes cache data to disk.
///
/// This test uses tokio's time mocking to avoid waiting for real durations.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_periodic_sync_writes_to_disk() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Use short but valid intervals (need at least 10 seconds for variance to work)
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_min_cache_save_duration(Duration::from_secs(10))
        .with_max_cache_save_duration(Duration::from_secs(20))
        .with_cache_save_scaling_factor(2);

    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer to the cache
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone()).await;

    // Verify peer is in memory
    assert_eq!(
        cache_store.peer_count().await,
        1,
        "Should have 1 peer in memory before periodic sync"
    );

    // Start the periodic sync task
    let handle = cache_store.sync_and_flush_periodically();

    // Wait for the first sync cycle to complete
    // With 10s min interval and 10% variance, the actual interval is 9-11s
    // Wait a bit longer to ensure the sync happens
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Stop the task
    handle.abort();

    // Verify data was persisted to disk
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    assert!(
        !loaded_data.peers.is_empty(),
        "Periodic sync should have written peer to disk"
    );

    // Verify the correct peer was written
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();
    assert_eq!(
        loaded_addrs[0].to_string(),
        addr.to_string(),
        "Written address should match original"
    );

    Ok(())
}

/// Verifies that the periodic sync task exits immediately when disable_cache_writing is true.
#[tokio::test]
async fn test_periodic_sync_respects_disable_flag() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create config with cache writing disabled
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_disable_cache_writing(true)
        .with_min_cache_save_duration(Duration::from_millis(50));

    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer (should still work in memory)
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr).await;

    // Start periodic sync - should exit immediately since writing is disabled
    let handle = cache_store.sync_and_flush_periodically();

    // Use timeout to verify task exits quickly
    let result = tokio::time::timeout(Duration::from_millis(100), handle).await;

    assert!(
        result.is_ok(),
        "Periodic sync task should exit immediately when cache writing is disabled"
    );

    // Verify no cache file was created
    let cache_path = temp_dir
        .path()
        .join("version_1")
        .join(BootstrapCacheStore::cache_file_name(false));
    assert!(
        !cache_path.exists(),
        "No cache file should be created when writing is disabled"
    );

    Ok(())
}

/// Verifies that sync continues running even when errors occur.
///
/// This test is Unix-only because it uses file permission manipulation.
/// Uses longer intervals to ensure variance calculations don't panic:
/// - min_cache_save_duration: 10s (10% variance = 1s, works)
/// - max_cache_save_duration: 100s (1% variance = 1s, works)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg(unix)]
async fn test_periodic_sync_continues_on_error() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Use intervals that work with variance calculations:
    // - min needs 10% of it to be >= 1 second (so min >= 10s)
    // - max needs 1% of it to be >= 1 second (so max >= 100s)
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_min_cache_save_duration(Duration::from_secs(10))
        .with_max_cache_save_duration(Duration::from_secs(100));

    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr).await;

    // Start the periodic sync task
    let handle = cache_store.sync_and_flush_periodically();

    // Wait for first sync cycle to complete
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Verify the file was created
    let version_dir = temp_dir.path().join("version_1");
    assert!(
        version_dir.exists(),
        "version_1 directory should exist after first sync"
    );

    // Make the cache directory read-only to cause write errors
    let mut perms = std::fs::metadata(&version_dir)?.permissions();
    perms.set_mode(0o444);
    std::fs::set_permissions(&version_dir, perms)?;

    // Add another peer to trigger a write on next cycle
    let addr2: Multiaddr =
        "/ip4/127.0.0.2/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .parse()?;
    cache_store.add_addr(addr2).await;

    // Wait for another sync cycle that should fail but not crash
    // After first cycle with scaling_factor=2, next interval is ~20s
    tokio::time::sleep(Duration::from_secs(25)).await;

    // Verify the task is still running (not panicked)
    assert!(
        !handle.is_finished(),
        "Periodic sync task should continue running despite errors"
    );

    // Restore permissions
    let mut perms = std::fs::metadata(&version_dir)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&version_dir, perms)?;

    // Stop the task
    handle.abort();

    Ok(())
}

// ============================================================================
// disable_cache_writing flag tests (2 tests)
// ============================================================================

/// Verifies that sync_and_flush_to_disk returns early without writing when disabled.
#[tokio::test]
async fn test_disable_cache_writing_sync_and_flush() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create config with cache writing disabled
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_disable_cache_writing(true);

    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr).await;

    // Verify peer is in memory
    assert_eq!(
        cache_store.peer_count().await,
        1,
        "Peer should be in memory"
    );

    // Call sync_and_flush_to_disk - should return early without writing
    let result = cache_store.sync_and_flush_to_disk().await;
    assert!(result.is_ok(), "sync_and_flush_to_disk should return Ok");

    // Verify no cache file was created
    let cache_path = temp_dir
        .path()
        .join("version_1")
        .join(BootstrapCacheStore::cache_file_name(false));
    assert!(
        !cache_path.exists(),
        "No cache file should be created when sync_and_flush_to_disk is called with writing disabled"
    );

    Ok(())
}

/// Verifies that write() returns early without writing when disabled.
#[tokio::test]
async fn test_disable_cache_writing_write() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create config with cache writing disabled
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_disable_cache_writing(true);

    let cache_store = BootstrapCacheStore::new(config)?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr).await;

    // Verify peer is in memory
    assert_eq!(
        cache_store.peer_count().await,
        1,
        "Peer should be in memory"
    );

    // Call write() - should return early without writing
    let result = cache_store.write().await;
    assert!(result.is_ok(), "write() should return Ok");

    // Verify no cache file was created
    let cache_path = temp_dir
        .path()
        .join("version_1")
        .join(BootstrapCacheStore::cache_file_name(false));
    assert!(
        !cache_path.exists(),
        "No cache file should be created when write() is called with writing disabled"
    );

    Ok(())
}

// ============================================================================
// Helper function
// ============================================================================

#[allow(dead_code)]
fn file_exists(path: &Path) -> bool {
    path.exists()
}
