// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore};
use ant_logging::LogBuilder;
use libp2p::Multiaddr;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

#[tokio::test]
async fn test_cache_store_operations() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("cache_tests", false);

    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create cache store with config
    let config = BootstrapCacheConfig::empty().with_cache_path(&cache_path);

    let mut cache_store = BootstrapCacheStore::new(config)?;

    // Test adding and retrieving peers
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone());
    cache_store.update_addr_status(&addr, true);

    let addrs = cache_store.get_sorted_addrs().collect::<Vec<_>>();
    assert!(!addrs.is_empty(), "Cache should contain the added peer");
    assert!(
        addrs.contains(&(&addr)),
        "Cache should contain our specific peer"
    );

    Ok(())
}

#[tokio::test]
async fn test_cache_max_peers() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("cache_tests", false);

    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create cache with small max_peers limit
    let mut config = BootstrapCacheConfig::empty().with_cache_path(&cache_path);
    config.max_peers = 2;

    let mut cache_store = BootstrapCacheStore::new(config)?;

    // Add three peers with distinct timestamps
    let mut addresses = Vec::new();
    for i in 1..=3 {
        let addr: Multiaddr = format!("/ip4/127.0.0.1/udp/808{i}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{i}").parse()?;
        addresses.push(addr.clone());
        cache_store.add_addr(addr);
        // Add a delay to ensure distinct timestamps
        sleep(Duration::from_millis(100)).await;
    }

    let addrs = cache_store.get_all_addrs().collect::<Vec<_>>();
    assert_eq!(addrs.len(), 2, "Cache should respect max_peers limit");

    // Get the addresses of the peers we have
    let peer_addrs: Vec<_> = addrs.iter().map(|p| p.addr.to_string()).collect();
    tracing::debug!("Final peers: {:?}", peer_addrs);

    // We should have the two most recently added peers (addresses[1] and addresses[2])
    for addr in addrs {
        let addr_str = addr.addr.to_string();
        assert!(
            addresses[1..].iter().any(|a| a.to_string() == addr_str),
            "Should have one of the two most recent peers, got {addr_str}"
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_cache_file_corruption() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("cache_tests", false);
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create cache with some peers
    let config = BootstrapCacheConfig::empty().with_cache_path(&cache_path);

    let mut cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER1"
            .parse()?;
    cache_store.add_addr(addr.clone());

    assert_eq!(cache_store.peer_count(), 1);

    // Corrupt the cache file
    tokio::fs::write(&cache_path, "invalid json content").await?;

    // Create a new cache store - it should handle the corruption gracefully
    let mut new_cache_store = BootstrapCacheStore::new(config)?;
    let addrs = new_cache_store.get_all_addrs().collect::<Vec<_>>();
    assert!(addrs.is_empty(), "Cache should be empty after corruption");

    // Should be able to add peers again
    new_cache_store.add_addr(addr);
    let addrs = new_cache_store.get_all_addrs().collect::<Vec<_>>();
    assert_eq!(
        addrs.len(),
        1,
        "Should be able to add peers after corruption"
    );

    Ok(())
}
