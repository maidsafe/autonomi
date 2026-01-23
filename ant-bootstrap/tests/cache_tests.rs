// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{
    BootstrapCacheConfig, BootstrapCacheStore, ContactsFetcher, InitialPeersConfig,
};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::Multiaddr;
use std::collections::HashSet;
use std::time::Duration;
use tempfile::TempDir;
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn test_empty_cache() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create empty cache
    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Write empty cache to disk
    cache_store.write().await?;

    // Try loading it back
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;

    assert!(
        loaded_data.peers.is_empty(),
        "Empty cache should remain empty when loaded"
    );

    Ok(())
}

#[tokio::test]
async fn test_max_peer_limit_enforcement() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create cache with small max_peers limit
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(3);

    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Store all addresses to check FIFO behavior
    let mut addresses = Vec::new();
    for i in 1..=5 {
        let addr: Multiaddr = format!("/ip4/127.0.0.1/udp/808{i}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{i}").parse()?;
        addresses.push(addr.clone());
        cache_store.add_addr(addr).await;

        // Add a delay to ensure distinct timestamps
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check we don't exceed max
        assert!(
            cache_store.peer_count().await <= 3,
            "Cache should enforce max_peers limit"
        );
    }

    // Get current peers in cache
    let current_addrs = cache_store.get_all_addrs().await;
    assert_eq!(
        current_addrs.len(),
        3,
        "Should have exactly 3 peers in the cache"
    );

    // Verify FIFO principle - the first two addresses should be gone,
    // and the last three should remain

    // Check that the first addresses (oldest) are NOT in the cache
    (0..2).for_each(|i| {
        let addr_str = addresses[i].to_string();
        assert!(
            !current_addrs.iter().any(|a| a.to_string() == addr_str),
            "Oldest address #{} should have been removed due to FIFO",
            i + 1
        );
    });

    // Check that the last addresses (newest) ARE in the cache
    (2..5).for_each(|i| {
        let addr_str = addresses[i].to_string();
        assert!(
            current_addrs.iter().any(|a| a.to_string() == addr_str),
            "Newest address #{} should be in the cache",
            i + 1
        );
    });

    // Write to disk and verify FIFO persists after reload
    cache_store.write().await?;

    // Load cache from disk
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs = loaded_data.get_all_addrs().cloned().collect::<Vec<_>>();

    // Verify the FIFO principle is maintained in the persisted data
    assert_eq!(
        loaded_addrs.len(),
        3,
        "Should have exactly 3 peers after reload"
    );

    // Check that oldest two are gone and newest three remain
    (0..2).for_each(|i| {
        let addr_str = addresses[i].to_string();
        assert!(
            !loaded_addrs.iter().any(|a| a.to_string() == addr_str),
            "After reload, oldest address #{} should be gone",
            i + 1
        );
    });

    (2..5).for_each(|i| {
        let addr_str = addresses[i].to_string();
        assert!(
            loaded_addrs.iter().any(|a| a.to_string() == addr_str),
            "After reload, newest address #{} should remain",
            i + 1
        );
    });

    Ok(())
}

#[tokio::test]
async fn test_peer_removal() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create cache
    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config)?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone()).await;

    // Get the peer ID
    let peer_id = ant_bootstrap::multiaddr_get_peer_id(&addr).unwrap();

    // Queue the peer for removal
    cache_store.queue_remove_peer(&peer_id).await;

    // Apply the queued removal
    cache_store.sync_and_flush_to_disk().await?;

    // Verify it's gone
    let addrs = cache_store.get_all_addrs().await;
    assert!(addrs.is_empty(), "Peer should be removed after sync");

    Ok(())
}

#[tokio::test]
async fn test_queued_peer_removal_applied_on_sync() -> Result<()> {
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

    // Write to disk (note: sync_and_flush_to_disk clears memory after writing)
    cache_store.sync_and_flush_to_disk().await?;

    // Verify peer is on disk
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs = loaded_data.get_all_addrs().collect::<Vec<_>>();
    assert_eq!(
        loaded_addrs.len(),
        1,
        "Peer should be on disk after initial sync"
    );

    // Get the peer ID
    let peer_id = ant_bootstrap::multiaddr_get_peer_id(&addr).unwrap();

    // Queue the peer for removal
    cache_store.queue_remove_peer(&peer_id).await;

    // Apply the queued removal - this loads from disk, applies the removal, and writes back
    cache_store.sync_and_flush_to_disk().await?;

    // Load from disk - peer should be removed
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs = loaded_data.get_all_addrs().collect::<Vec<_>>();
    assert_eq!(
        loaded_addrs.len(),
        0,
        "Peer should be removed from disk after queued removal sync"
    );

    Ok(())
}

#[tokio::test]
async fn test_cache_file_corruption() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create a valid cache first
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone()).await;
    cache_store.write().await?;

    // Now corrupt the cache file by writing invalid JSON
    let cache_path = cache_dir
        .join("version_1")
        .join(BootstrapCacheStore::cache_file_name(false));
    std::fs::write(&cache_path, "{not valid json}")?;

    // Attempt to load the corrupted cache
    let result = BootstrapCacheStore::load_cache_data(&config);
    assert!(
        result.is_err(),
        "Loading corrupted cache should return error"
    );

    // The code should now attempt to create a new cache
    let new_store = BootstrapCacheStore::new(config.clone())?;
    assert_eq!(new_store.peer_count().await, 0);
    new_store.write().await?;

    // load the cache data and check it's empty
    let cache_data = BootstrapCacheStore::load_cache_data(&config)?;
    assert_eq!(cache_data.peers.len(), 0, "Cache data should be empty");

    Ok(())
}

#[tokio::test]
async fn test_max_addrs_per_peer() -> Result<()> {
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

    // Write to disk and reload to check the limit
    cache_store.write().await?;

    // Create new store to read the final state
    let new_store = BootstrapCacheStore::new(config)?;

    // Count addresses for the peer
    let peer_addrs = new_store.get_all_addrs().await;
    assert!(
        peer_addrs.len() <= 2,
        "Should enforce max_addrs_per_peer limit"
    );

    Ok(())
}

#[tokio::test]
async fn test_first_flag_behavior() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create mock server with some peers
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
        ))
        .mount(&mock_server)
        .await;

    // Create InitialPeersConfig with first=true and other conflicting options
    let args = InitialPeersConfig {
        first: true,
        addrs: vec!["/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5".parse()?],
        network_contacts_url: vec![format!("{}/peers", mock_server.uri())],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    // Get bootstrap addresses
    let addrs = args.get_bootstrap_addr(None).await?;

    // First flag should override all other options and return empty list
    assert!(
        addrs.is_empty(),
        "First flag should cause empty address list regardless of other options"
    );

    Ok(())
}

#[tokio::test]
async fn test_network_failure_recovery() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Create a ContactsFetcher with a non-existent endpoint and a valid one
    let bad_url: Url = "http://does-not-exist.example.invalid".parse()?;

    // Start mock server with valid endpoint
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/valid"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
        ))
        .mount(&mock_server)
        .await;

    let valid_url = format!("{}/valid", mock_server.uri()).parse()?;

    // Test with just the bad URL
    let fetcher1 = ContactsFetcher::with_endpoints(vec![bad_url.clone()])?;
    let result1 = fetcher1.fetch_bootstrap_addresses().await;
    assert!(result1.is_ok(), "Should succeed but without any addresses");
    assert!(
        result1.unwrap().is_empty(),
        "Should return empty list when all endpoints fail"
    );

    // Test with bad URL first, then good URL
    let fetcher2 = ContactsFetcher::with_endpoints(vec![bad_url, valid_url])?;
    let result2 = fetcher2.fetch_bootstrap_addresses().await;
    assert!(
        result2.is_ok(),
        "Should succeed with at least one valid URL"
    );
    assert!(
        !result2.unwrap().is_empty(),
        "Should return addresses from valid URL"
    );

    Ok(())
}

#[tokio::test]
async fn test_empty_response_handling() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Start mock server with empty response
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/empty"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&mock_server)
        .await;

    // Create fetcher with empty response endpoint
    let url = format!("{}/empty", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;

    // Should handle empty response gracefully
    let result = fetcher.fetch_bootstrap_addresses().await;
    assert!(
        result.is_ok() && result.unwrap().is_empty(),
        "Should handle empty response gracefully"
    );

    Ok(())
}

#[tokio::test]
async fn test_sync_duplicates_overlapping_peers() -> Result<()> {
    use ant_bootstrap::cache_store::CacheDataLatest;
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mut cache1 = CacheDataLatest::default();
    let mut cache2 = CacheDataLatest::default();

    let addr1: Multiaddr =
        "/ip4/127.0.0.1/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER1"
            .parse()?;
    let addr2: Multiaddr =
        "/ip4/127.0.0.1/udp/8082/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER2"
            .parse()?;
    let addr3: Multiaddr =
        "/ip4/127.0.0.1/udp/8083/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER3"
            .parse()?;

    let peer1 = addr1
        .iter()
        .find_map(|p| match p {
            libp2p::multiaddr::Protocol::P2p(id) => Some(id),
            _ => None,
        })
        .unwrap();
    let peer2 = addr2
        .iter()
        .find_map(|p| match p {
            libp2p::multiaddr::Protocol::P2p(id) => Some(id),
            _ => None,
        })
        .unwrap();
    let peer3 = addr3
        .iter()
        .find_map(|p| match p {
            libp2p::multiaddr::Protocol::P2p(id) => Some(id),
            _ => None,
        })
        .unwrap();

    cache1.add_peer(peer1, [addr1.clone()].iter(), 10, 10);
    cache1.add_peer(peer2, [addr2.clone()].iter(), 10, 10);

    cache2.add_peer(peer1, [addr1.clone()].iter(), 10, 10);
    cache2.add_peer(peer3, [addr3.clone()].iter(), 10, 10);

    cache1.sync(&cache2, 10, 10);

    let unique_peers: HashSet<_> = cache1.peers.iter().map(|(peer_id, _)| peer_id).collect();

    assert_eq!(
        unique_peers.len(),
        cache1.peers.len(),
        "Duplicate peer entries found after sync"
    );

    assert_eq!(unique_peers.len(), 3, "Expected 3 unique peers");

    Ok(())
}

#[tokio::test]
async fn test_sync_at_limit_overwrites_unique_peers() -> Result<()> {
    use ant_bootstrap::cache_store::CacheDataLatest;
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mut cache1 = CacheDataLatest::default();
    let mut cache2 = CacheDataLatest::default();

    let addrs: Vec<Multiaddr> = (1..=7)
        .map(|i| {
            format!("/ip4/127.0.0.1/udp/808{i}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{i}")
                .parse()
                .unwrap()
        })
        .collect();

    let peers: Vec<_> = addrs
        .iter()
        .map(|addr| {
            addr.iter()
                .find_map(|p| match p {
                    libp2p::multiaddr::Protocol::P2p(id) => Some(id),
                    _ => None,
                })
                .unwrap()
        })
        .collect();

    // cache1: peers 1,2,3,4,5 (at limit)
    for i in 0..5 {
        cache1.add_peer(peers[i], [addrs[i].clone()].iter(), 10, 5);
    }

    // cache2: peers 3,4,5,6,7 (at limit, overlaps with 3,4,5)
    for i in 2..7 {
        cache2.add_peer(peers[i], [addrs[i].clone()].iter(), 10, 5);
    }

    cache1.sync(&cache2, 10, 5);

    println!("Final cache1 length: {}", cache1.peers.len());
    let cache1_peers_after: HashSet<_> = cache1.peers.iter().map(|(peer_id, _)| *peer_id).collect();
    println!(
        "Contains peer1: {}, peer2: {}",
        cache1_peers_after.contains(&peers[0]),
        cache1_peers_after.contains(&peers[1])
    );

    // With newer peer preservation, self peers (1,2) should be preserved
    // Final result should have peers 1,2,3,4,5 (self peers + some from other)
    assert_eq!(cache1.peers.len(), 5, "Should maintain max_peers limit");
    assert!(
        cache1_peers_after.contains(&peers[0]),
        "Should preserve peer 1 from self"
    );
    assert!(
        cache1_peers_after.contains(&peers[1]),
        "Should preserve peer 2 from self"
    );

    Ok(())
}

#[tokio::test]
async fn test_sync_other_at_limit_self_below_limit() -> Result<()> {
    use ant_bootstrap::cache_store::CacheDataLatest;
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mut cache1 = CacheDataLatest::default();
    let mut cache2 = CacheDataLatest::default();

    let addrs: Vec<Multiaddr> = (1..=7)
        .map(|i| {
            format!("/ip4/127.0.0.1/udp/808{i}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{i}")
                .parse()
                .unwrap()
        })
        .collect();

    let peers: Vec<_> = addrs
        .iter()
        .map(|addr| {
            addr.iter()
                .find_map(|p| match p {
                    libp2p::multiaddr::Protocol::P2p(id) => Some(id),
                    _ => None,
                })
                .unwrap()
        })
        .collect();

    // cache1: peers 1,2 (below limit of 5)
    for i in 0..2 {
        cache1.add_peer(peers[i], [addrs[i].clone()].iter(), 10, 5);
    }

    // cache2: peers 3,4,5,6,7 (at limit of 5)
    for i in 2..7 {
        cache2.add_peer(peers[i], [addrs[i].clone()].iter(), 10, 5);
    }

    assert_eq!(cache1.peers.len(), 2);
    assert_eq!(cache2.peers.len(), 5);

    cache1.sync(&cache2, 10, 5);

    println!("Final cache1 length: {}", cache1.peers.len());
    let cache1_peers_after: HashSet<_> = cache1.peers.iter().map(|(peer_id, _)| *peer_id).collect();

    // With newer peer preservation: cache1 keeps its 2 peers, adds some from cache2
    // Since we preserve self peers, final result should keep peers 1,2 from cache1
    assert_eq!(cache1.peers.len(), 5, "Should maintain max_peers limit");

    // Should preserve original cache1 peers (newer)
    assert!(
        cache1_peers_after.contains(&peers[0]),
        "Should preserve peer 1 from self"
    );
    assert!(
        cache1_peers_after.contains(&peers[1]),
        "Should preserve peer 2 from self"
    );

    Ok(())
}

#[tokio::test]
async fn test_address_ordering_preserved_after_sync() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create cache and add addresses addr1, addr2, addr3 (addr3 is newest, at front)
    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config.clone())?;

    let addr1: Multiaddr =
        "/ip4/127.0.0.1/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER1"
            .parse()?;
    let addr2: Multiaddr =
        "/ip4/127.0.0.1/udp/8082/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER2"
            .parse()?;
    let addr3: Multiaddr =
        "/ip4/127.0.0.1/udp/8083/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER3"
            .parse()?;

    // Add addresses in order: 1, 2, 3 - so order in cache is [3, 2, 1] (newest first)
    cache_store.add_addr(addr1.clone()).await;
    cache_store.add_addr(addr2.clone()).await;
    cache_store.add_addr(addr3.clone()).await;

    // Write to disk
    cache_store.write().await?;

    // Create new cache and add addr4, addr5 (newer than disk)
    let addr4: Multiaddr =
        "/ip4/127.0.0.1/udp/8084/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER4"
            .parse()?;
    let addr5: Multiaddr =
        "/ip4/127.0.0.1/udp/8085/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER5"
            .parse()?;

    let cache_store2 = BootstrapCacheStore::new(config.clone())?;
    cache_store2.add_addr(addr4.clone()).await;
    cache_store2.add_addr(addr5.clone()).await;

    // Sync with disk (this loads from disk and syncs memory with it)
    cache_store2.sync_and_flush_to_disk().await?;

    // Reload from disk to check final order
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();

    // Memory addresses (addr4, addr5) should be at front, disk addresses (addr1, addr2, addr3) at back
    // Expected exact order: [addr5, addr4, addr3, addr2, addr1] (newest first)
    let expected_order = vec![
        addr5.clone(),
        addr4.clone(),
        addr3.clone(),
        addr2.clone(),
        addr1.clone(),
    ];

    assert_eq!(
        loaded_addrs.len(),
        expected_order.len(),
        "Should have exactly {} addresses after sync",
        expected_order.len()
    );

    // Strict enforcement: compare entire vectors for exact order
    assert_eq!(
        loaded_addrs, expected_order,
        "Addresses must be in exact order [addr5, addr4, addr3, addr2, addr1]\nGot: {loaded_addrs:?}\nExpected: {expected_order:?}",
    );

    Ok(())
}

#[tokio::test]
async fn test_newest_addresses_at_front_after_reload() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());
    let cache_store = BootstrapCacheStore::new(config.clone())?;

    // Add addresses in sequence
    let addr1: Multiaddr =
        "/ip4/127.0.0.1/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER1"
            .parse()?;
    let addr2: Multiaddr =
        "/ip4/127.0.0.1/udp/8082/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER2"
            .parse()?;
    let addr3: Multiaddr =
        "/ip4/127.0.0.1/udp/8083/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER3"
            .parse()?;

    // Add in order: 1, 2, 3 - resulting in cache order [3, 2, 1]
    cache_store.add_addr(addr1.clone()).await;
    cache_store.add_addr(addr2.clone()).await;
    cache_store.add_addr(addr3.clone()).await;

    // Write to disk
    cache_store.write().await?;

    // Reload from disk
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs: Vec<Multiaddr> = loaded_data.get_all_addrs().cloned().collect();

    assert_eq!(loaded_addrs.len(), 3, "Should have all 3 addresses");

    // Verify exact order using direct index access: [addr3, addr2, addr1] (newest first)
    assert_eq!(
        loaded_addrs[0], addr3,
        "Position 0 should be addr3 (newest)"
    );
    assert_eq!(loaded_addrs[1], addr2, "Position 1 should be addr2");
    assert_eq!(
        loaded_addrs[2], addr1,
        "Position 2 should be addr1 (oldest)"
    );

    Ok(())
}

#[tokio::test]
async fn test_sync_preserves_memory_addresses_at_front() -> Result<()> {
    use ant_bootstrap::cache_store::CacheDataLatest;
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mut cache1 = CacheDataLatest::default();
    let mut cache2 = CacheDataLatest::default();

    // Create addresses for different peers
    let addr_a: Multiaddr =
        "/ip4/127.0.0.1/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER1"
            .parse()?;
    let addr_b: Multiaddr =
        "/ip4/127.0.0.1/udp/8082/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER2"
            .parse()?;
    let addr_c: Multiaddr =
        "/ip4/127.0.0.1/udp/8083/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER3"
            .parse()?;
    let addr_d: Multiaddr =
        "/ip4/127.0.0.1/udp/8084/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER4"
            .parse()?;

    let peer_a = ant_bootstrap::multiaddr_get_peer_id(&addr_a).unwrap();
    let peer_b = ant_bootstrap::multiaddr_get_peer_id(&addr_b).unwrap();
    let peer_c = ant_bootstrap::multiaddr_get_peer_id(&addr_c).unwrap();
    let peer_d = ant_bootstrap::multiaddr_get_peer_id(&addr_d).unwrap();

    // cache1 (memory) has peers A, B - added in order A, B so order is [B, A]
    cache1.add_peer(peer_a, [addr_a.clone()].iter(), 10, 10);
    cache1.add_peer(peer_b, [addr_b.clone()].iter(), 10, 10);

    // cache2 (disk) has peers C, D - added in order C, D so order is [D, C]
    cache2.add_peer(peer_c, [addr_c.clone()].iter(), 10, 10);
    cache2.add_peer(peer_d, [addr_d.clone()].iter(), 10, 10);

    // Sync cache1 with cache2 - cache1's peers should remain at front
    cache1.sync(&cache2, 10, 10);

    // Get all addresses to check order
    let all_addrs: Vec<Multiaddr> = cache1.get_all_addrs().cloned().collect();

    // Expected exact order: [B, A, D, C]
    // - cache1's peers were added A then B, so cache1 order is [B, A]
    // - cache2's peers were added C then D, so cache2 order is [D, C]
    // - After sync, cache1's peers stay at front, cache2's peers are appended in their original order
    let expected_order = vec![
        addr_b.clone(),
        addr_a.clone(),
        addr_d.clone(),
        addr_c.clone(),
    ];

    assert_eq!(all_addrs.len(), 4, "Should have all 4 addresses");

    // Strict enforcement: compare entire vectors for exact order
    assert_eq!(
        all_addrs, expected_order,
        "Addresses must be in exact order [B, A, D, C]\nGot: {all_addrs:?}\nExpected: {expected_order:?}",
    );

    Ok(())
}
