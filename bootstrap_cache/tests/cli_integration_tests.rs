// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use bootstrap_cache::{BootstrapConfig, CacheStore, PeersArgs};
use libp2p::Multiaddr;
use std::env;
use std::fs;
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

async fn setup() -> (TempDir, BootstrapConfig) {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().join("cache.json");
    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };
    (temp_dir, config)
}

#[tokio::test]
async fn test_first_flag() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let (_temp_dir, config) = setup().await;

    let args = PeersArgs {
        first: true,
        peers: vec![],
        network_contacts_url: None,
        local: false,
        test_network: false,
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "First node should have no peers");

    Ok(())
}

#[tokio::test]
async fn test_peer_argument() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let (_temp_dir, config) = setup().await;

    let peer_addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    
    let args = PeersArgs {
        first: false,
        peers: vec![peer_addr.clone()],
        network_contacts_url: None,
        local: false,
        test_network: false,
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert_eq!(peers.len(), 1, "Should have one peer");
    assert_eq!(peers[0].addr, peer_addr, "Should have the correct peer address");

    Ok(())
}

#[tokio::test]
async fn test_safe_peers_env() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Set SAFE_PEERS environment variable
    let peer_addr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
    env::set_var("SAFE_PEERS", peer_addr);

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: false,
        test_network: false,
    };

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    
    // We should have multiple peers (env var + cache/endpoints)
    assert!(peers.len() > 0, "Should have peers");
    
    // Verify that our env var peer is included in the set
    let has_env_peer = peers.iter().any(|p| p.addr.to_string() == peer_addr);
    assert!(has_env_peer, "Should include the peer from env var");

    // Clean up
    env::remove_var("SAFE_PEERS");

    Ok(())
}

#[tokio::test]
async fn test_network_contacts_fallback() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
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

    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: Some(format!("{}/peers", mock_server.uri()).parse()?),
        local: false,
        test_network: true,
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert_eq!(peers.len(), 2, "Should have two peers from network contacts");

    Ok(())
}

#[tokio::test]
async fn test_local_mode() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create a config with some peers in the cache
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };

    // Create args with local mode enabled
    let args = PeersArgs {
        first: false,
        peers: vec![],
        network_contacts_url: None,
        local: true,
        test_network: false,
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert!(peers.is_empty(), "Local mode should have no peers");

    // Verify cache was not touched
    assert!(!cache_path.exists(), "Cache file should not exist in local mode");

    Ok(())
}

#[tokio::test]
async fn test_test_network_peers() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let peer_addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };

    let args = PeersArgs {
        first: false,
        peers: vec![peer_addr.clone()],
        network_contacts_url: None,
        local: false,
        test_network: true,
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert_eq!(peers.len(), 1, "Should have exactly one test network peer");
    assert_eq!(peers[0].addr, peer_addr, "Should have the correct test network peer");

    // Verify cache was not updated
    assert!(!cache_path.exists(), "Cache file should not exist for test network");

    Ok(())
}

#[tokio::test]
async fn test_peers_update_cache() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create a peer address for testing
    let peer_addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };

    // Create args with peers but no test network mode
    let args = PeersArgs {
        first: false,
        peers: vec![peer_addr.clone()],
        network_contacts_url: None,
        local: false,
        test_network: false,
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert_eq!(peers.len(), 1, "Should have one peer");
    assert_eq!(peers[0].addr, peer_addr, "Should have the correct peer");

    // Verify cache was updated
    assert!(cache_path.exists(), "Cache file should exist");
    let cache_contents = fs::read_to_string(&cache_path)?;
    assert!(cache_contents.contains(&peer_addr.to_string()), "Cache should contain the peer address");

    Ok(())
}

#[tokio::test]
async fn test_test_network_mode() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create a peer address for testing
    let peer_addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };

    // Create args with test network mode enabled
    let args = PeersArgs {
        first: false,
        peers: vec![peer_addr.clone()],
        network_contacts_url: None,
        local: false,
        test_network: true,
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    assert_eq!(peers.len(), 1, "Should have one test network peer");
    assert_eq!(peers[0].addr, peer_addr, "Should have the correct test network peer");

    // Verify cache was not touched
    assert!(!cache_path.exists(), "Cache file should not exist for test network");

    Ok(())
}

#[tokio::test]
async fn test_default_mode() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create a store with some initial peers in the cache
    let initial_config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };
    let initial_store = CacheStore::new(initial_config).await?;
    let cache_peer: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE".parse()?;
    initial_store.add_peer(cache_peer.clone()).await?;
    initial_store.save_cache().await?;

    // Create store in default mode (no special flags)
    let args = PeersArgs::default();
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };

    let store = CacheStore::from_args(args, config).await?;
    let peers = store.get_peers().await;
    
    assert!(!peers.is_empty(), "Should have peers from cache");
    assert!(peers.iter().any(|p| p.addr == cache_peer), "Should have the cache peer");

    Ok(())
} 