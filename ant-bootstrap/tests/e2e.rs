// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore, InitialPeersConfig};
use ant_logging::LogBuilder;
use ant_protocol::version::set_network_id;
use color_eyre::Result;
use libp2p::Multiaddr;
use tempfile::TempDir;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn test_full_bootstrap_flow() -> Result<()> {
    // to disable fetching mainnet contacts
    // This would make any tests under the same file also to use the same id. So it is better to not have any other tests
    // in this file to avoid race conditions.
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
    let addrs = args.get_bootstrap_addr(None).await?;
    assert!(
        addrs.is_empty(),
        "First node should have no bootstrap addresses"
    );

    // 2. Add some known peers
    let cache = BootstrapCacheStore::new(config.clone())?;
    let addr1: Multiaddr = "/ip4/192.168.1.1/udp/8080/quic-v1/p2p/12D3KooWEHbMXSPvGCQAHjSTYWRKz1PcizQYdq5vMDqV2wLiXyJ9".parse()?;
    let addr2: Multiaddr =
        "/ip4/192.168.1.2/tcp/8080/ws/p2p/12D3KooWQF3NMWHRmMQBY8GVdpQh1V6TFYuQqZkKKvYE7yCS6fYK"
            .parse()?;

    cache.add_addr(addr1.clone()).await;
    cache.add_addr(addr2.clone()).await;
    cache.write().await?;

    // 3. Try to get bootstrap addresses from cache
    let cache_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let cache_addrs = cache_args.get_bootstrap_addr(None).await?;
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

    let network_addrs = network_args.get_bootstrap_addr(None).await?;
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

    let combined_addrs = combined_args.get_bootstrap_addr(None).await?;
    assert!(
        combined_addrs.len() >= 3,
        "Should combine addresses from multiple sources"
    );

    Ok(())
}

#[tokio::test]
async fn test_multiple_network_contacts() -> Result<()> {
    set_network_id(100); // to disable fetching mainnet contacts
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    // Start mock servers
    let mock_server1 = MockServer::start().await;
    let mock_server2 = MockServer::start().await;

    // Set up endpoints with different peers
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
        ))
        .mount(&mock_server1)
        .await;

    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
        ))
        .mount(&mock_server2)
        .await;

    // Set up CLI args with both endpoints
    let args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![
            format!("{}/contacts", mock_server1.uri()),
            format!("{}/contacts", mock_server2.uri()),
        ],
        local: false,
        ignore_cache: true,
        bootstrap_cache_dir: None,
    };

    // Should fetch from both endpoints
    let addrs = args.get_bootstrap_addr(None).await?;

    assert_eq!(addrs.len(), 2, "Should fetch from both endpoints");

    Ok(())
}
