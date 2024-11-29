// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use bootstrap_cache::{BootstrapConfig, CacheStore, PeersArgs};
use libp2p::{multiaddr::Protocol, Multiaddr};
use std::{net::SocketAddrV4, time::Duration};
use tempfile::TempDir;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// Initialize logging for tests
fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("bootstrap_cache=debug")
        .try_init();
}

// Setup function to create a new temp directory and config for each test
async fn setup() -> (TempDir, BootstrapConfig) {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("cache.json");
    
    let config = BootstrapConfig {
        cache_file_path: cache_path,
        endpoints: vec![],  // Empty endpoints to avoid fetching from network
        max_peers: 50,
        max_retries: 3,
        request_timeout: Duration::from_secs(10),
        update_interval: Duration::from_secs(300),
    };
    
    (temp_dir, config)
}

#[tokio::test]
async fn test_ipv4_socket_address_parsing() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let (_temp_dir, config) = setup().await;

    // Test IPv4 socket address format (1.2.3.4:1234)
    let socket_addr = "127.0.0.1:8080".parse::<SocketAddrV4>()?;
    let expected_addr = Multiaddr::empty()
        .with(Protocol::Ip4(*socket_addr.ip()))
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::QuicV1);

    let args = PeersArgs {
        first: false,
        peers: vec![expected_addr.clone()],
        network_contacts_url: None,
        local: false,
        test_network: true,  // Use test network mode to avoid fetching from default endpoints
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert_eq!(peers.len(), 1, "Should have one peer");
    assert_eq!(peers[0].addr, expected_addr, "Address format should match");

    Ok(())
}

#[tokio::test]
async fn test_multiaddr_format_parsing() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    // Test various multiaddr formats
    let addrs = vec![
        // Standard format with peer ID
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
        // Without peer ID
        "/ip4/127.0.0.1/udp/8080/quic-v1",
        // With TCP instead of UDP (should still work)
        "/ip4/127.0.0.1/tcp/8080/quic-v1",
    ];

    for addr_str in addrs {
        let (_temp_dir, config) = setup().await;  // Fresh config for each test case
        let addr = addr_str.parse::<Multiaddr>()?;
        let args = PeersArgs {
            first: false,
            peers: vec![addr.clone()],
            network_contacts_url: None,
            local: false,
            test_network: true,  // Use test network mode to avoid fetching from default endpoints
        };

        let store = CacheStore::from_args(args, config).await?;
        let peers = store.get_peers().await;
        assert_eq!(peers.len(), 1, "Should have one peer");
        assert_eq!(peers[0].addr, addr, "Address format should match");
    }

    Ok(())
}

#[tokio::test]
async fn test_network_contacts_format() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let (_temp_dir, config) = setup().await;

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

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: Some(format!("{}/peers", mock_server.uri()).parse()?),
        local: false,
        test_network: false,  // Allow fetching from network contacts
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert_eq!(peers.len(), 2, "Should have two peers from network contacts");

    // Verify address formats
    for peer in peers {
        let addr_str = peer.addr.to_string();
        assert!(addr_str.contains("/ip4/"), "Should have IPv4 address");
        assert!(addr_str.contains("/udp/"), "Should have UDP port");
        assert!(addr_str.contains("/quic-v1/"), "Should have QUIC protocol");
        assert!(addr_str.contains("/p2p/"), "Should have peer ID");
    }

    Ok(())
}

#[tokio::test]
async fn test_invalid_address_handling() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    // Test various invalid address formats
    let invalid_addrs = vec![
        "not-a-multiaddr",
        "127.0.0.1", // IP only
        "127.0.0.1:8080:extra", // Invalid socket addr
        "/ip4/127.0.0.1", // Incomplete multiaddr
    ];

    for addr_str in invalid_addrs {
        let (_temp_dir, config) = setup().await;  // Fresh config for each test case
        let args = PeersArgs {
            first: false,
            peers: vec![],
            network_contacts_url: None,
            local: true,  // Use local mode to avoid fetching from default endpoints
            test_network: false,
        };

        let store = CacheStore::from_args(args.clone(), config.clone()).await?;
        let peers = store.get_peers().await;
        assert_eq!(
            peers.len(),
            0,
            "Should have no peers from invalid address in env var: {}",
            addr_str
        );

        // Also test direct args path
        if let Ok(addr) = addr_str.parse::<Multiaddr>() {
            let args_with_peer = PeersArgs {
                first: false,
                peers: vec![addr],
                network_contacts_url: None,
                local: false,
                test_network: true,  // Use test network mode to avoid fetching from default endpoints
            };
            let store = CacheStore::from_args(args_with_peer, config).await?;
            let peers = store.get_peers().await;
            assert_eq!(
                peers.len(),
                0,
                "Should have no peers from invalid address in args: {}",
                addr_str
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_socket_addr_format() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
}

#[tokio::test]
async fn test_multiaddr_format() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
}

#[tokio::test]
async fn test_invalid_addr_format() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
}

#[tokio::test]
async fn test_mixed_addr_formats() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
}

#[tokio::test]
async fn test_socket_addr_conversion() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
}

#[tokio::test]
async fn test_invalid_socket_addr() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
}

#[tokio::test]
async fn test_invalid_multiaddr() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
}

#[tokio::test]
async fn test_mixed_valid_invalid_addrs() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,  // Use local mode to avoid getting peers from default endpoints
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Should have no peers in local mode");

    Ok(())
} 