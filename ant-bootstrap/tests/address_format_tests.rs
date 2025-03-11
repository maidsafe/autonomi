// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{BootstrapCacheConfig, InitialPeersConfig};
use ant_logging::LogBuilder;
use libp2p::Multiaddr;
use tempfile::TempDir;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// Setup function to create a new temp directory and config for each test
async fn setup() -> (TempDir, BootstrapCacheConfig) {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("cache.json");

    let config = BootstrapCacheConfig::empty()
        .with_cache_path(&cache_path)
        .with_max_peers(50);

    (temp_dir, config)
}

#[tokio::test]
async fn test_multiaddr_format_parsing() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("address_format_tests", false);

    // Test various multiaddr formats
    let addrs = vec![
        // quic
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
        // ws
        "/ip4/127.0.0.1/tcp/8080/ws/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
    ];

    for addr_str in addrs {
        let (_temp_dir, _config) = setup().await; // Fresh config for each test case
        let addr = addr_str.parse::<Multiaddr>()?;
        let args = InitialPeersConfig {
            first: false,
            addrs: vec![addr.clone()],
            network_contacts_url: vec![],
            local: false,
            disable_mainnet_contacts: true,
            ignore_cache: true,
            bootstrap_cache_dir: None,
        };

        let bootstrap_addresses = args.get_bootstrap_addr(None, None).await?;
        assert_eq!(bootstrap_addresses.len(), 1, "Should have one peer");
        assert_eq!(
            bootstrap_addresses[0].addr, addr,
            "Address format should match"
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_network_contacts_format() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("address_format_tests", false);

    let (_temp_dir, _config) = setup().await;

    // Create a mock server with network contacts format
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE\n\
             /ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERF"
        ))
        .mount(&mock_server)
        .await;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![format!("{}/peers", mock_server.uri()).parse()?],
        local: false,
        disable_mainnet_contacts: true,
        ignore_cache: true,
        bootstrap_cache_dir: None,
    };

    let addrs = args.get_bootstrap_addr(None, None).await?;
    assert_eq!(
        addrs.len(),
        2,
        "Should have two peers from network contacts"
    );

    // Verify address formats
    for addr in addrs {
        let addr_str = addr.addr.to_string();
        assert!(addr_str.contains("/ip4/"), "Should have IPv4 address");
        assert!(addr_str.contains("/udp/"), "Should have UDP port");
        assert!(addr_str.contains("/quic-v1/"), "Should have QUIC protocol");
        assert!(addr_str.contains("/p2p/"), "Should have peer ID");
    }

    Ok(())
}
