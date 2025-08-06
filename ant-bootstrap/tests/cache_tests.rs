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

    // Remove the peer
    cache_store.remove_peer(&peer_id).await;

    // Verify it's gone
    let addrs = cache_store.get_all_addrs().await;
    assert!(addrs.is_empty(), "Peer should be removed");

    Ok(())
}

#[tokio::test]
async fn peer_removal_should_not_affect_fs_cache() -> Result<()> {
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

    // write to disk
    cache_store.sync_and_flush_to_disk().await?;

    // Get the peer ID
    let peer_id = ant_bootstrap::multiaddr_get_peer_id(&addr).unwrap();

    // Remove the peer
    cache_store.remove_peer(&peer_id).await;

    // Verify it's gone
    let addrs = cache_store.get_all_addrs().await;
    assert!(addrs.is_empty(), "Peer should be removed");

    // but syncing to disk should not remove the peer
    cache_store.sync_and_flush_to_disk().await?;
    let loaded_data = BootstrapCacheStore::load_cache_data(&config)?;
    let loaded_addrs = loaded_data.get_all_addrs().collect::<Vec<_>>();
    assert_eq!(loaded_addrs.len(), 1, "Peer should remain in the cache");

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
