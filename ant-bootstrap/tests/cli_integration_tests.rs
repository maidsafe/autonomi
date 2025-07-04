// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{
    cache_store::cache_data_v1, BootstrapCacheConfig, BootstrapCacheStore, InitialPeersConfig,
};
use ant_logging::LogBuilder;
use ant_protocol::version::set_network_id;
use color_eyre::Result;
use libp2p::Multiaddr;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;
use tracing::info;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn test_full_bootstrap_flow() -> Result<()> {
    // to disable fetching mainnet contacts
    set_network_id(100);
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create a mock server for network contacts
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE\n\
             /ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
        ))
        .mount(&mock_server)
        .await;

    // 1. Initialize with first=true to create an empty cache
    let args = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10);

    // Get bootstrap addresses (should be empty)
    let addrs = args.get_bootstrap_addr(Some(config.clone()), None).await?;
    assert!(
        addrs.is_empty(),
        "First node should have no bootstrap addresses"
    );

    // 2. Add some known peers
    let mut cache = BootstrapCacheStore::new(config.clone())?;
    let addr1: Multiaddr = "/ip4/192.168.1.1/udp/8080/quic-v1/p2p/12D3KooWEHbMXSPvGCQAHjSTYWRKz1PcizQYdq5vMDqV2wLiXyJ9".parse()?;
    let addr2: Multiaddr =
        "/ip4/192.168.1.2/tcp/8080/ws/p2p/12D3KooWQF3NMWHRmMQBY8GVdpQh1V6TFYuQqZkKKvYE7yCS6fYK"
            .parse()?;

    cache.add_addr(addr1.clone());
    cache.add_addr(addr2.clone());
    cache.write()?;

    // 3. Try to get bootstrap addresses from cache
    let cache_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let cache_addrs = cache_args
        .get_bootstrap_addr(Some(config.clone()), None)
        .await?;
    assert_eq!(cache_addrs.len(), 2, "Should get addresses from cache");

    // 4. Try to get addresses from network contacts
    let network_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: true, // Ignore cache to force fetching from network
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let network_addrs = network_args
        .get_bootstrap_addr(Some(config.clone()), None)
        .await?;
    assert_eq!(
        network_addrs.len(),
        2,
        "Should get addresses from network contacts"
    );

    // 5. Combine CLI arguments with cache
    let combined_args = InitialPeersConfig {
        first: false,
        addrs: vec!["/ip4/192.168.1.3/udp/8080/quic-v1/p2p/12D3KooWHehYgXKLxsXjzFzDqMLKhcAVc4LaktnT7Zei1G2zcpJB".parse()?],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let combined_addrs = combined_args.get_bootstrap_addr(Some(config), None).await?;
    assert!(
        combined_addrs.len() >= 3,
        "Should combine addresses from multiple sources"
    );

    Ok(())
}

#[tokio::test]
async fn test_concurrent_cache_access() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path().to_path_buf();

    // Create a shared config
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir.clone());

    // Simulate multiple concurrent writers using multiple tasks
    let mut handles = Vec::new();
    for i in 0..5 {
        let config_clone = config.clone();

        let handle = tokio::spawn(async move {
            // Create a new cache store (simulating a different process)
            let mut cache_store = BootstrapCacheStore::new(config_clone)?;

            // Add a unique addr for this "process"
            let addr: Multiaddr = format!(
                "/ip4/127.0.0.{}/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{}",
                i + 1, i + 1
            ).parse().unwrap();
            cache_store.add_addr(addr);

            // Sleep a bit to increase chances of concurrent access
            sleep(Duration::from_millis(10)).await;

            // Write to the cache and flush
            cache_store.sync_and_flush_to_disk()
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await??;
    }

    // Create a new cache store to read the final state
    let final_store = BootstrapCacheStore::new(config)?;

    // Load the cache and check the results - should have at least one peer
    let cache_data = BootstrapCacheStore::load_cache_data(final_store.config())?;
    assert_eq!(cache_data.peers.len(), 5, "Should have 5 unique peers");

    Ok(())
}

#[tokio::test]
async fn test_cli_arguments_precedence() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create config
    let config = BootstrapCacheConfig::empty().with_cache_dir(temp_dir.path());

    let env_addr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
    // Set environment variable
    std::env::set_var(ant_bootstrap::ANT_PEERS_ENV, env_addr);

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![ "/ip4/192.168.1.3/udp/8080/quic-v1/p2p/12D3KooWHehYgXKLxsXjzFzDqMLKhcAVc4LaktnT7Zei1G2zcpJB".parse()?],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: true,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    // Get bootstrap addresses
    let addrs = args.get_bootstrap_addr(Some(config), None).await?;

    // Environment variables should take precedence
    assert_eq!(addrs.len(), 1, "Should only use one address source");
    assert_eq!(
        addrs[0].to_string(),
        env_addr,
        "CLI argument should be used"
    );

    // Cleanup
    std::env::remove_var(ant_bootstrap::ANT_PEERS_ENV);

    Ok(())
}

#[tokio::test]
async fn test_cache_sync_functionality() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create initial cache data
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);

    // Create and populate first cache
    let mut first_store = BootstrapCacheStore::new(config.clone())?;
    let addr1: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    first_store.add_addr(addr1.clone());
    first_store.write()?;

    // debug by printing the cache file content
    let cache_file = BootstrapCacheStore::cache_file_name(false);
    let cache_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &cache_file);
    let cache_content = std::fs::read_to_string(&cache_path)?;
    info!("Cache file content after first write:\n{cache_content}");

    // Create second cache with different peer
    let mut second_store = BootstrapCacheStore::new(config.clone())?;
    let addr2: Multiaddr =
        "/ip4/127.0.0.2/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .parse()?;
    second_store.add_addr(addr2.clone());

    // Sync and flush - should merge with existing cache
    second_store.sync_and_flush_to_disk()?;

    let cache_file = BootstrapCacheStore::cache_file_name(false);
    let cache_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &cache_file);
    let cache_content = std::fs::read_to_string(&cache_path)?;
    info!("Cache file content after second write:\n{cache_content}");

    // Create new cache store to verify
    let new_store = BootstrapCacheStore::new(config)?;

    // Load new cache data and verify it has both peers
    let cache_data = BootstrapCacheStore::load_cache_data(new_store.config())?;
    let addrs = cache_data.get_all_addrs().collect::<Vec<_>>();

    info!("Read addresses from cache: {addrs:?}");

    // Both addresses should be present after sync
    let has_addr1 = addrs
        .iter()
        .any(|&addr| addr.to_string() == addr1.to_string());
    let has_addr2 = addrs
        .iter()
        .any(|&addr| addr.to_string() == addr2.to_string());
    info!("Has addr1: {has_addr1}, Has addr2: {has_addr2}");

    assert!(
        has_addr1 && has_addr2,
        "Sync should merge peers from both caches"
    );

    Ok(())
}
