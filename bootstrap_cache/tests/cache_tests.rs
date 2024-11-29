use bootstrap_cache::{BootstrapConfig, CacheStore};
use libp2p::Multiaddr;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

#[tokio::test]
async fn test_cache_store_operations() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create cache store with config
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };
    let cache_store = CacheStore::new(config).await?;

    // Test adding and retrieving peers
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_peer(addr.clone()).await?;
    cache_store
        .update_peer_status(&addr.to_string(), true)
        .await?;

    let peers = cache_store.get_reliable_peers().await;
    assert!(!peers.is_empty(), "Cache should contain the added peer");
    assert!(
        peers.iter().any(|p| p.addr == addr),
        "Cache should contain our specific peer"
    );

    Ok(())
}

#[tokio::test]
async fn test_cache_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create first cache store
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };
    let cache_store1 = CacheStore::new(config.clone()).await?;

    // Add a peer and mark it as reliable
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store1.add_peer(addr.clone()).await?;
    cache_store1
        .update_peer_status(&addr.to_string(), true)
        .await?;

    // Create a new cache store with the same path
    let cache_store2 = CacheStore::new(config).await?;
    let peers = cache_store2.get_reliable_peers().await;

    assert!(!peers.is_empty(), "Cache should persist across instances");
    assert!(
        peers.iter().any(|p| p.addr == addr),
        "Specific peer should persist"
    );

    Ok(())
}

#[tokio::test]
async fn test_cache_reliability_tracking() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };
    let cache_store = CacheStore::new(config).await?;

    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_peer(addr.clone()).await?;

    // Test successful connections
    for _ in 0..3 {
        cache_store
            .update_peer_status(&addr.to_string(), true)
            .await?;
    }

    let peers = cache_store.get_reliable_peers().await;
    assert!(
        peers.iter().any(|p| p.addr == addr),
        "Peer should be reliable after successful connections"
    );

    // Test failed connections
    for _ in 0..5 {
        cache_store
            .update_peer_status(&addr.to_string(), false)
            .await?;
    }

    let peers = cache_store.get_reliable_peers().await;
    assert!(
        !peers.iter().any(|p| p.addr == addr),
        "Peer should not be reliable after failed connections"
    );

    Ok(())
}

#[tokio::test]
async fn test_cache_max_peers() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("bootstrap_cache=debug")
        .try_init();

    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create cache with small max_peers limit
    let config = BootstrapConfig {
        cache_file_path: cache_path,
        max_peers: 2,
        ..Default::default()
    };
    let cache_store = CacheStore::new(config).await?;

    // Add three peers with distinct timestamps
    let mut addresses = Vec::new();
    for i in 1..=3 {
        let addr: Multiaddr = format!("/ip4/127.0.0.1/udp/808{}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{}", i, i).parse()?;
        addresses.push(addr.clone());
        cache_store.add_peer(addr).await?;
        // Add a delay to ensure distinct timestamps
        sleep(Duration::from_millis(100)).await;
    }

    let peers = cache_store.get_peers().await;
    assert_eq!(peers.len(), 2, "Cache should respect max_peers limit");

    // Get the addresses of the peers we have
    let peer_addrs: Vec<_> = peers.iter().map(|p| p.addr.to_string()).collect();
    tracing::debug!("Final peers: {:?}", peer_addrs);

    // We should have the two most recently added peers (addresses[1] and addresses[2])
    for peer in peers {
        let addr_str = peer.addr.to_string();
        assert!(
            addresses[1..].iter().any(|a| a.to_string() == addr_str),
            "Should have one of the two most recent peers, got {}",
            addr_str
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_cache_concurrent_access() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    let config = BootstrapConfig {
        cache_file_path: cache_path,
        ..Default::default()
    };
    let cache_store = CacheStore::new(config).await?;
    let cache_store_clone = cache_store.clone();

    // Create multiple addresses
    let addrs: Vec<Multiaddr> = (1..=5)
        .map(|i| format!("/ip4/127.0.0.1/udp/808{}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{}", i, i).parse().unwrap())
        .collect();

    // Spawn a task that adds peers
    let add_task = tokio::spawn(async move {
        for addr in addrs {
            if let Err(e) = cache_store.add_peer(addr).await {
                eprintln!("Error adding peer: {}", e);
            }
            sleep(Duration::from_millis(10)).await;
        }
    });

    // Spawn another task that reads peers
    let read_task = tokio::spawn(async move {
        for _ in 0..10 {
            let _ = cache_store_clone.get_peers().await;
            sleep(Duration::from_millis(5)).await;
        }
    });

    // Wait for both tasks to complete
    tokio::try_join!(add_task, read_task)?;

    Ok(())
}

#[tokio::test]
async fn test_cache_file_corruption() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let cache_path = temp_dir.path().join("cache.json");

    // Create cache with some peers
    let config = BootstrapConfig {
        cache_file_path: cache_path.clone(),
        ..Default::default()
    };
    let cache_store = CacheStore::new(config.clone()).await?;

    // Add a peer
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER1"
            .parse()?;
    cache_store.add_peer(addr.clone()).await?;

    // Corrupt the cache file
    tokio::fs::write(&cache_path, "invalid json content").await?;

    // Create a new cache store - it should handle the corruption gracefully
    let new_cache_store = CacheStore::new(config).await?;
    let peers = new_cache_store.get_peers().await;
    assert!(peers.is_empty(), "Cache should be empty after corruption");

    // Should be able to add peers again
    new_cache_store.add_peer(addr).await?;
    let peers = new_cache_store.get_peers().await;
    assert_eq!(
        peers.len(),
        1,
        "Should be able to add peers after corruption"
    );

    Ok(())
}
