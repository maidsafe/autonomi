// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{
    BootstrapCacheConfig, BootstrapCacheStore,
    cache_store::{cache_data_v0, cache_data_v1},
};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::{Multiaddr, PeerId};
use std::time::SystemTime;
use tempfile::TempDir;

// =============================================================================
// Version migration tests (5 tests)
// =============================================================================

#[tokio::test]
async fn test_load_v1_cache() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create a v1 cache data directly
    let mut v1_data = cache_data_v1::CacheData::default();

    // Add a peer
    let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse()?;
    v1_data.add_peer(peer_id, [addr.clone()].iter(), 10, 10);

    // Write v1 data to file
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let filename = BootstrapCacheStore::cache_file_name(false);
    v1_data.write_to_file(cache_dir, &filename)?;

    // Load cache - should load v1 directly
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Verify the peers were preserved
    assert!(
        !loaded_data.peers.is_empty(),
        "Peers should be loaded from v1 cache"
    );
    assert_eq!(
        loaded_data.cache_version,
        cache_data_v1::CacheData::CACHE_DATA_VERSION.to_string()
    );

    // Verify the address is correct
    let (loaded_peer_id, loaded_addrs) = loaded_data.peers.front().unwrap();
    assert_eq!(*loaded_peer_id, peer_id);
    assert_eq!(loaded_addrs.front().unwrap(), &addr);

    Ok(())
}

#[tokio::test]
async fn test_load_v0_cache_migrates_to_v1() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create a v0 cache data
    let mut v0_data = cache_data_v0::CacheData {
        peers: Default::default(),
        last_updated: SystemTime::now(),
        network_version: ant_bootstrap::get_network_version(),
    };

    // Add a peer
    let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse()?;
    let boot_addr = cache_data_v0::BootstrapAddr {
        addr: addr.clone(),
        success_count: 5,
        failure_count: 1,
        last_seen: SystemTime::now(),
    };
    let addrs = cache_data_v0::BootstrapAddresses(vec![boot_addr]);
    v0_data.peers.insert(peer_id, addrs);

    // Write v0 data to file
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let filename = BootstrapCacheStore::cache_file_name(false);
    v0_data.write_to_file(cache_dir, &filename)?;

    // Load cache with v0 data - should be upgraded to v1
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Verify the peers were preserved
    assert!(
        !loaded_data.peers.is_empty(),
        "Peers should be preserved after version upgrade"
    );

    // Verify each peer has a multiaddr in the final cache
    let has_addrs = loaded_data.get_all_addrs().next().is_some();
    assert!(
        has_addrs,
        "Addresses should be preserved after version upgrade"
    );

    // Verify it's now v1 format
    assert_eq!(
        loaded_data.cache_version,
        cache_data_v1::CacheData::CACHE_DATA_VERSION.to_string()
    );

    Ok(())
}

#[tokio::test]
async fn test_v1_preferred_when_both_exist() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    let filename = BootstrapCacheStore::cache_file_name(false);

    // Create v0 cache data with one peer
    let mut v0_data = cache_data_v0::CacheData {
        peers: Default::default(),
        last_updated: SystemTime::now(),
        network_version: ant_bootstrap::get_network_version(),
    };
    let v0_peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    let v0_addr: Multiaddr = "/ip4/10.0.0.1/udp/8080/quic-v1".parse()?;
    let boot_addr = cache_data_v0::BootstrapAddr {
        addr: v0_addr.clone(),
        success_count: 1,
        failure_count: 0,
        last_seen: SystemTime::now(),
    };
    v0_data.peers.insert(
        v0_peer_id,
        cache_data_v0::BootstrapAddresses(vec![boot_addr]),
    );
    v0_data.write_to_file(cache_dir, &filename)?;

    // Create v1 cache data with a different peer
    let mut v1_data = cache_data_v1::CacheData::default();
    let v1_peer_id: PeerId = "12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc".parse()?;
    let v1_addr: Multiaddr = "/ip4/192.168.1.1/udp/9090/quic-v1".parse()?;
    v1_data.add_peer(v1_peer_id, [v1_addr.clone()].iter(), 10, 10);
    v1_data.write_to_file(cache_dir, &filename)?;

    // Load cache - should load v1 (preferred)
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Should have v1 peer, not v0 peer
    let peer_ids: Vec<PeerId> = loaded_data.peers.iter().map(|(id, _)| *id).collect();
    assert!(peer_ids.contains(&v1_peer_id), "V1 peer should be present");
    assert!(
        !peer_ids.contains(&v0_peer_id),
        "V0 peer should not be present when v1 is loaded"
    );

    Ok(())
}

#[tokio::test]
async fn test_v1_corrupted_falls_back_to_v0() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    let filename = BootstrapCacheStore::cache_file_name(false);

    // Create valid v0 cache data
    let mut v0_data = cache_data_v0::CacheData {
        peers: Default::default(),
        last_updated: SystemTime::now(),
        network_version: ant_bootstrap::get_network_version(),
    };
    let v0_peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    let v0_addr: Multiaddr = "/ip4/10.0.0.1/udp/8080/quic-v1".parse()?;
    let boot_addr = cache_data_v0::BootstrapAddr {
        addr: v0_addr.clone(),
        success_count: 1,
        failure_count: 0,
        last_seen: SystemTime::now(),
    };
    v0_data.peers.insert(
        v0_peer_id,
        cache_data_v0::BootstrapAddresses(vec![boot_addr]),
    );
    v0_data.write_to_file(cache_dir, &filename)?;

    // Write corrupted v1 cache file
    let v1_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &filename);
    std::fs::create_dir_all(v1_path.parent().unwrap())?;
    std::fs::write(&v1_path, "{ corrupted json data }")?;

    // Load cache - should fall back to v0
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Should have v0 peer (fallback)
    let peer_ids: Vec<PeerId> = loaded_data.peers.iter().map(|(id, _)| *id).collect();
    assert!(
        peer_ids.contains(&v0_peer_id),
        "V0 peer should be present as fallback"
    );

    Ok(())
}

#[tokio::test]
async fn test_both_corrupted_returns_error() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    let filename = BootstrapCacheStore::cache_file_name(false);

    // Write corrupted v0 cache file
    let v0_path = cache_data_v0::CacheData::cache_file_path(cache_dir, &filename);
    std::fs::create_dir_all(v0_path.parent().unwrap())?;
    std::fs::write(&v0_path, "{ invalid v0 json }")?;

    // Write corrupted v1 cache file
    let v1_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &filename);
    std::fs::create_dir_all(v1_path.parent().unwrap())?;
    std::fs::write(&v1_path, "{ invalid v1 json }")?;

    // Load cache - should return error since both are corrupted
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let result = BootstrapCacheStore::load_cache_data(&config);

    assert!(
        result.is_err(),
        "Should return error when both caches are corrupted"
    );

    Ok(())
}

// =============================================================================
// Backwards compatibility tests (2 tests)
// =============================================================================

#[tokio::test]
async fn test_backwards_compat_writes_both_formats() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create config with backwards compatibility enabled
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(cache_dir)
        .with_backwards_compatible_writes(true);

    // Create and populate cache store
    let cache_store = BootstrapCacheStore::new(config)?;
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone()).await;

    // Write cache to disk
    cache_store.write().await?;

    // Check that v0 format file exists and can be read
    let filename = BootstrapCacheStore::cache_file_name(false);
    let v0_path = cache_data_v0::CacheData::cache_file_path(cache_dir, &filename);
    assert!(v0_path.exists(), "V0 cache file should exist");
    let v0_data = cache_data_v0::CacheData::read_from_file(cache_dir, &filename)?;

    // Check that v1 format file exists and can be read
    let v1_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &filename);
    assert!(v1_path.exists(), "V1 cache file should exist");
    let v1_data = cache_data_v1::CacheData::read_from_file(cache_dir, &filename)?;

    // Verify data was written in v0 format
    assert!(
        !v0_data.peers.is_empty(),
        "Peers should be written in v0 format"
    );

    // Verify data was written in v1 format
    assert!(
        !v1_data.peers.is_empty(),
        "Peers should be written in v1 format"
    );

    Ok(())
}

#[tokio::test]
async fn test_v0_and_v1_have_same_peer_data() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create config with backwards compatibility enabled
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(cache_dir)
        .with_backwards_compatible_writes(true);

    // Create and populate cache store with multiple peers
    let cache_store = BootstrapCacheStore::new(config)?;
    let addr1: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    let addr2: Multiaddr =
        "/ip4/192.168.1.1/udp/9090/quic-v1/p2p/12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc"
            .parse()?;
    cache_store.add_addr(addr1.clone()).await;
    cache_store.add_addr(addr2.clone()).await;

    // Write cache to disk
    cache_store.write().await?;

    // Read both formats
    let filename = BootstrapCacheStore::cache_file_name(false);
    let v0_data = cache_data_v0::CacheData::read_from_file(cache_dir, &filename)?;
    let v1_data = cache_data_v1::CacheData::read_from_file(cache_dir, &filename)?;

    // Verify both have the same number of peers
    assert_eq!(
        v0_data.peers.len(),
        v1_data.peers.len(),
        "V0 and V1 should have the same number of peers"
    );

    // Verify peer IDs match
    let v0_peer_ids: std::collections::HashSet<PeerId> = v0_data.peers.keys().copied().collect();
    let v1_peer_ids: std::collections::HashSet<PeerId> =
        v1_data.peers.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        v0_peer_ids, v1_peer_ids,
        "V0 and V1 should have the same peer IDs"
    );

    Ok(())
}

// =============================================================================
// File paths tests (2 tests)
// =============================================================================

#[tokio::test]
async fn test_v1_path_includes_version() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Get path for v1
    let filename = BootstrapCacheStore::cache_file_name(false);
    let v1_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &filename);

    // V1 should include version in path
    let path_str = v1_path.to_string_lossy();
    assert!(
        path_str.contains(&format!(
            "version_{}",
            cache_data_v1::CacheData::CACHE_DATA_VERSION
        )),
        "V1 path should include version number, got: {path_str}"
    );

    Ok(())
}

#[tokio::test]
async fn test_v0_path_no_version_prefix() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Get path for v0
    let filename = BootstrapCacheStore::cache_file_name(false);
    let v0_path = cache_data_v0::CacheData::cache_file_path(cache_dir, &filename);

    // V0 shouldn't have version in path
    let path_str = v0_path.to_string_lossy();
    assert!(
        !path_str.contains("version_"),
        "V0 path should not include version number, got: {path_str}"
    );

    Ok(())
}
