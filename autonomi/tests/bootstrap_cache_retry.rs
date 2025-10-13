// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Integration tests for bootstrap cache wipe and retry behavior

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore};
use autonomi::{Client, ClientConfig};
use tempfile::TempDir;

/// Test that the bootstrap cache delete function works correctly
#[tokio::test]
async fn test_bootstrap_cache_delete() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for the test
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path().to_path_buf();

    // Create a bootstrap cache config
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(&cache_dir)
        .with_local(true);

    // Create a bootstrap cache store and write a dummy cache
    let store = BootstrapCacheStore::new(config.clone())?;
    store.write().await?;

    // Verify the cache file exists (it's in a version subdirectory)
    let cache_file_name = BootstrapCacheStore::cache_file_name(true);
    let cache_file_path = cache_dir.join("version_1").join(&cache_file_name);
    assert!(
        cache_file_path.exists(),
        "Cache file should exist after writing at {:?}",
        cache_file_path
    );

    // Delete the cache file
    BootstrapCacheStore::delete_cache_file(&config)?;

    // Verify the cache file no longer exists
    assert!(
        !cache_file_path.exists(),
        "Cache file should not exist after deletion"
    );

    Ok(())
}

/// Test that deleting a non-existent cache file doesn't error
#[tokio::test]
async fn test_bootstrap_cache_delete_nonexistent() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for the test
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path().to_path_buf();

    // Create a bootstrap cache config pointing to a cache that doesn't exist
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(&cache_dir)
        .with_local(true);

    // Delete the cache file (which doesn't exist)
    // This should not return an error
    BootstrapCacheStore::delete_cache_file(&config)?;

    Ok(())
}

/// Scenario 1: Client has an outdated bootstrap cache
/// Expected: Client fails to connect using cache, wipes it, retries with fresh peers, and succeeds
#[tokio::test]
async fn test_outdated_cache_wipe_and_retry() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for the test
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path().to_path_buf();

    // Create a bootstrap cache config for mainnet (local=false)
    let mut config = BootstrapCacheConfig::empty()
        .with_cache_dir(&cache_dir)
        .with_local(false);

    // Create an invalid cache file with outdated/invalid peer addresses
    let store = BootstrapCacheStore::new(config.clone())?;
    let invalid_addr =
        "/ip4/127.0.0.1/udp/1/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
    store.add_addr(invalid_addr).await;
    store.write().await?;

    // Verify the cache file exists
    let cache_file_name = BootstrapCacheStore::cache_file_name(false);
    let cache_file_path = cache_dir.join("version_1").join(&cache_file_name);
    assert!(
        cache_file_path.exists(),
        "Invalid cache file should exist before client init at {:?}",
        cache_file_path
    );

    // Try to initialize the client with the invalid cache
    // The client should automatically delete the cache and retry
    config.disable_cache_writing = true; // Disable cache writing to prevent overwriting
    let client_config = ClientConfig {
        bootstrap_cache_config: Some(config),
        ..Default::default()
    };

    // This should succeed after wiping the cache and fetching fresh peers from mainnet
    let _client = Client::init_with_config(client_config).await?;

    println!("✓ Scenario 1: Successfully recovered from outdated cache");
    Ok(())
}

/// Scenario 2: Client has a good bootstrap cache
/// Expected: Client connects on first attempt using the cached peers
#[tokio::test]
async fn test_good_cache_connects_first_attempt() -> Result<(), Box<dyn std::error::Error>> {
    // First, connect to mainnet to populate the cache with valid peers
    println!("Connecting to mainnet to populate cache...");
    let _client1 = Client::init().await?;
    println!("✓ Cache populated with valid peers");

    // Now connect again - should succeed on first attempt using the good cache
    println!("Connecting again using cached peers...");
    let _client2 = Client::init().await?;

    println!("✓ Scenario 2: Successfully connected using good cache on first attempt");
    Ok(())
}

/// Scenario 3: Client has no bootstrap cache
/// Expected: Client connects on first attempt using mainnet contacts file
#[tokio::test]
async fn test_no_cache_connects_with_contacts() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for a fresh cache
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path().to_path_buf();

    // Create config with a clean cache directory (no existing cache)
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(&cache_dir)
        .with_local(false);

    // Verify no cache file exists
    let cache_file_name = BootstrapCacheStore::cache_file_name(false);
    let cache_file_path = cache_dir.join("version_1").join(&cache_file_name);
    assert!(
        !cache_file_path.exists(),
        "Cache file should not exist before client init"
    );

    let client_config = ClientConfig {
        bootstrap_cache_config: Some(config),
        ..Default::default()
    };

    // This should succeed on first attempt by fetching peers from mainnet contacts
    println!("Connecting with no cache (using mainnet contacts)...");
    let _client = Client::init_with_config(client_config).await?;

    println!("✓ Scenario 3: Successfully connected without cache using mainnet contacts");
    Ok(())
}
