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

async fn setup() -> (TempDir, BootstrapCacheConfig) {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("cache.json");
    let config = BootstrapCacheConfig::empty().with_cache_path(&cache_path);

    (temp_dir, config)
}

#[tokio::test]
async fn test_first_flag() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("cli_integration_tests", false);
    let (_temp_dir, config) = setup().await;

    let args = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let addrs = args.get_addrs(Some(config), None).await?;

    assert!(addrs.is_empty(), "First node should have no addrs");

    Ok(())
}

#[tokio::test]
async fn test_peer_argument() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("cli_integration_tests", false);
    let (_temp_dir, _config) = setup().await;

    let peer_addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![peer_addr.clone()],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: true,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let addrs = args.get_addrs(None, None).await?;

    assert_eq!(addrs.len(), 1, "Should have one addr");
    assert_eq!(addrs[0], peer_addr, "Should have the correct address");

    Ok(())
}

#[tokio::test]
async fn test_network_contacts_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("cli_integration_tests", false);

    let (_temp_dir, config) = setup().await;

    // Start mock server
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

    let addrs = args.get_addrs(Some(config), None).await?;
    assert_eq!(
        addrs.len(),
        2,
        "Should have two peers from network contacts"
    );

    Ok(())
}

#[tokio::test]
async fn test_test_network_peers() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("cli_integration_tests", false);

    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let peer_addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;

    let config = BootstrapCacheConfig::empty().with_cache_path(&cache_path);

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![peer_addr.clone()],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: true,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let addrs = args.get_addrs(Some(config), None).await?;

    assert_eq!(addrs.len(), 1, "Should have exactly one test network peer");
    assert_eq!(
        addrs[0], peer_addr,
        "Should have the correct test network peer"
    );

    Ok(())
}
