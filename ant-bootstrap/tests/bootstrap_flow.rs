// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Test-specific allows: env var manipulation requires unsafe
#![allow(unsafe_code)]
#![allow(clippy::needless_range_loop)]

//! Integration tests for the bootstrap flow.
//!
//! Tests that manipulate environment variables use the `#[serial]` attribute from
//! `serial_test` crate to ensure they run sequentially and don't race.

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore, InitialPeersConfig};
use ant_logging::LogBuilder;
use ant_protocol::version::set_network_id;
use color_eyre::Result;
use libp2p::Multiaddr;
use serial_test::serial;
use tempfile::TempDir;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

// Use a unique network ID for tests to avoid polluting real caches
const TEST_NETWORK_ID: u8 = 99;

// Valid peer IDs for testing (base58-encoded Ed25519 public keys)
const PEER_IDS: [&str; 5] = [
    "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
    "12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
    "12D3KooWCKCeqLPSgMnDjyFsJuWqREDtKNHx1JEBiwxME7Zdw68n",
    "12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc",
    "12D3KooWSBTB1jzXPyBGpWLMqXfN7MPMNwSsVWCbfkeLXPZr9Dm3",
];

// Helper to set up test environment
fn setup_test_env() {
    set_network_id(TEST_NETWORK_ID);
}

// Helper to clean up env var after test
struct EnvVarGuard {
    var_name: &'static str,
    original_value: Option<String>,
}

impl EnvVarGuard {
    fn new(var_name: &'static str) -> Self {
        let original_value = std::env::var(var_name).ok();
        Self {
            var_name,
            original_value,
        }
    }

    fn set(&self, value: &str) {
        // SAFETY: We're in a test environment and this is the standard way to set env vars for tests.
        // The guard ensures we restore the original value when dropped.
        unsafe {
            std::env::set_var(self.var_name, value);
        }
    }

    fn remove(&self) {
        // SAFETY: We're in a test environment and this is the standard way to remove env vars for tests.
        // The guard ensures we restore the original value when dropped.
        unsafe {
            std::env::remove_var(self.var_name);
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: We're restoring the original environment state at the end of the test.
        unsafe {
            if let Some(ref val) = self.original_value {
                std::env::set_var(self.var_name, val);
            } else {
                std::env::remove_var(self.var_name);
            }
        }
    }
}

// ============================================================================
// First node scenarios (2 tests)
// ============================================================================

#[tokio::test]
#[serial]
async fn test_first_flag_returns_empty() -> Result<()> {
    // When first=true, get_bootstrap_addr should return empty Vec
    // regardless of other sources (cache, CLI args, network contacts)
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Create mock server with some peers
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
        ))
        .mount(&mock_server)
        .await;

    // Pre-populate cache with an address
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10);
    let cache = BootstrapCacheStore::new(config)?;
    let cached_addr: Multiaddr = "/ip4/192.168.1.1/udp/8080/quic-v1/p2p/12D3KooWEHbMXSPvGCQAHjSTYWRKz1PcizQYdq5vMDqV2wLiXyJ9".parse()?;
    cache.add_addr(cached_addr).await;
    cache.write().await?;

    // Create InitialPeersConfig with first=true and all other sources configured
    let args = InitialPeersConfig {
        first: true,
        addrs: vec![
            "/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
                .parse()?,
        ],
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

    drop(env_guard);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_first_node_no_cache_no_contacts() -> Result<()> {
    // Empty cache + no network contacts = should return empty for first node
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    let args = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let addrs = args.get_bootstrap_addr(None).await?;

    assert!(
        addrs.is_empty(),
        "First node with no sources should return empty"
    );

    drop(env_guard);
    Ok(())
}

// ============================================================================
// Multi-source resolution (6 tests)
// ============================================================================

#[tokio::test]
#[serial]
async fn test_env_var_takes_precedence() -> Result<()> {
    // Set ANT_PEERS env var, verify those addrs are used
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Set up env var guard to clean up after test
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    let env_addr =
        "/ip4/10.0.0.1/udp/9000/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
    env_guard.set(env_addr);

    // Pre-populate cache with different addresses
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10);
    let cache = BootstrapCacheStore::new(config)?;
    let cached_addr: Multiaddr = "/ip4/192.168.1.1/udp/8080/quic-v1/p2p/12D3KooWEHbMXSPvGCQAHjSTYWRKz1PcizQYdq5vMDqV2wLiXyJ9".parse()?;
    cache.add_addr(cached_addr).await;
    cache.write().await?;

    // Create mock server with different peers
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
        ))
        .mount(&mock_server)
        .await;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![
            "/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWHehYgXKLxsXjzFzDqMLKhcAVc4LaktnT7Zei1G2zcpJB"
                .parse()?,
        ],
        network_contacts_url: vec![format!("{}/peers", mock_server.uri())],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let addrs = args.get_bootstrap_addr(None).await?;

    // When env var is set, only env var addresses should be returned (early return in the code)
    assert_eq!(addrs.len(), 1, "Should return only env var address");
    assert!(
        addrs[0].to_string().contains("10.0.0.1"),
        "Should contain the env var address"
    );

    drop(env_guard); // Restore original env state
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_cli_args_used_when_no_env_var() -> Result<()> {
    // No env var, CLI args provided, verify CLI args used
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    let cli_addr: Multiaddr =
        "/ip4/192.168.1.100/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![cli_addr.clone()],
        network_contacts_url: vec![],
        local: true,        // local=true to avoid network fetching
        ignore_cache: true, // Ignore cache to test CLI args specifically
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let addrs = args.get_bootstrap_addr(None).await?;

    assert!(!addrs.is_empty(), "Should return CLI arg addresses");
    assert!(
        addrs
            .iter()
            .any(|a| a.to_string().contains("192.168.1.100")),
        "Should contain the CLI arg address"
    );

    drop(env_guard);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_cache_used_when_no_cli_args() -> Result<()> {
    // No env var, no CLI args, cache has peers, verify cache used
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Pre-populate cache - use local=true to match the InitialPeersConfig below
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10)
        .with_local(true); // Must match the local flag in InitialPeersConfig
    let cache = BootstrapCacheStore::new(config)?;
    let cached_addr: Multiaddr = "/ip4/10.20.30.40/udp/8080/quic-v1/p2p/12D3KooWEHbMXSPvGCQAHjSTYWRKz1PcizQYdq5vMDqV2wLiXyJ9".parse()?;
    cache.add_addr(cached_addr).await;
    cache.write().await?;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![], // No CLI args
        network_contacts_url: vec![],
        local: true, // local=true to avoid fetching from mainnet
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let addrs = args.get_bootstrap_addr(None).await?;

    assert!(!addrs.is_empty(), "Should return cached addresses");
    assert!(
        addrs.iter().any(|a| a.to_string().contains("10.20.30.40")),
        "Should contain the cached address"
    );

    drop(env_guard);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_network_contacts_fetched_when_cache_empty() -> Result<()> {
    // Empty cache, network contacts available, verify fetched
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Start mock server
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/172.16.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
        ))
        .mount(&mock_server)
        .await;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: true, // Force fetching from network contacts
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let addrs = args.get_bootstrap_addr(None).await?;

    assert!(!addrs.is_empty(), "Should return network contact addresses");
    assert!(
        addrs.iter().any(|a| a.to_string().contains("172.16.0.1")),
        "Should contain the network contact address"
    );

    drop(env_guard);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_multiple_sources_combined() -> Result<()> {
    // CLI args + cache, verify both sources contribute addresses
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Pre-populate cache with one address
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10);
    let cache = BootstrapCacheStore::new(config)?;
    let cached_addr: Multiaddr =
        "/ip4/10.0.0.1/udp/8080/quic-v1/p2p/12D3KooWEHbMXSPvGCQAHjSTYWRKz1PcizQYdq5vMDqV2wLiXyJ9"
            .parse()?;
    cache.add_addr(cached_addr).await;
    cache.write().await?;

    // Start mock server with different address
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/10.0.0.3/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
        ))
        .mount(&mock_server)
        .await;

    // CLI arg with different address
    let cli_addr: Multiaddr =
        "/ip4/10.0.0.2/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![cli_addr],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let addrs = args.get_bootstrap_addr(None).await?;

    // Should have addresses from CLI args, cache, and network contacts
    assert!(
        addrs.len() >= 2,
        "Should combine addresses from multiple sources, got {}",
        addrs.len()
    );

    drop(env_guard);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_duplicate_addrs_deduplicated() -> Result<()> {
    // Same addr in multiple sources, verify no duplicates in result
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Use the same address in cache
    let shared_addr_str = "/ip4/192.168.1.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
    let shared_addr: Multiaddr = shared_addr_str.parse()?;

    // Pre-populate cache with the address
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10);
    let cache = BootstrapCacheStore::new(config)?;
    cache.add_addr(shared_addr.clone()).await;
    cache.write().await?;

    // Start mock server returning the same address
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(shared_addr_str))
        .mount(&mock_server)
        .await;

    // CLI args also with the same address
    let args = InitialPeersConfig {
        first: false,
        addrs: vec![shared_addr],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let addrs = args.get_bootstrap_addr(None).await?;

    // Should deduplicate - HashSet is used internally
    assert_eq!(addrs.len(), 1, "Duplicate addresses should be deduplicated");

    drop(env_guard);
    Ok(())
}

// ============================================================================
// Count limiting (2 tests)
// ============================================================================

#[tokio::test]
#[serial]
async fn test_count_limits_returned_addresses() -> Result<()> {
    // Request count=Some(n), verify at most n addresses returned
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Pre-populate cache with multiple addresses - use local=true to match InitialPeersConfig
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10)
        .with_local(true); // Must match the local flag in InitialPeersConfig
    let cache = BootstrapCacheStore::new(config)?;

    for i in 0..5 {
        let addr: Multiaddr = format!(
            "/ip4/192.168.1.{}/udp/8080/quic-v1/p2p/{}",
            i + 1,
            PEER_IDS[i]
        )
        .parse()?;
        cache.add_addr(addr).await;
    }
    cache.write().await?;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: true, // local mode - don't fetch from network
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    // Request only 2 addresses
    let addrs = args.get_bootstrap_addr(Some(2)).await?;

    assert!(
        addrs.len() <= 2,
        "Should return at most 2 addresses when count=2, got {}",
        addrs.len()
    );

    drop(env_guard);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_stops_early_when_count_reached() -> Result<()> {
    // If first source provides enough, subsequent sources not queried
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Start mock server - should NOT be called if early return works
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/172.16.0.1/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
        ))
        .expect(0) // Expect no calls
        .mount(&mock_server)
        .await;

    // CLI args with enough addresses
    let args = InitialPeersConfig {
        first: false,
        addrs: vec![
            "/ip4/10.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                .parse()?,
            "/ip4/10.0.0.2/udp/8080/quic-v1/p2p/12D3KooWEHbMXSPvGCQAHjSTYWRKz1PcizQYdq5vMDqV2wLiXyJ9"
                .parse()?,
        ],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: true,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    // Request only 2 addresses - CLI args provide exactly 2
    let addrs = args.get_bootstrap_addr(Some(2)).await?;

    assert_eq!(
        addrs.len(),
        2,
        "Should return exactly 2 addresses from CLI args"
    );

    // Verify mock was not called (wiremock will fail if expect(0) is violated)
    drop(env_guard);
    Ok(())
}

// ============================================================================
// Full integration (1 test)
// ============================================================================

#[tokio::test]
#[serial]
async fn test_full_fallback_chain_accumulates_all_sources() -> Result<()> {
    // Tests the FULL integration flow:
    // 1. CLI args provide 1 address (not enough if count=3)
    // 2. Cache provides 1 more address
    // 3. Network contacts provide 1 more address
    // Result: All 3 addresses accumulated from different sources

    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set (so we don't early-return)
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // 1. CLI arg with one address
    let cli_addr: Multiaddr =
        format!("/ip4/10.0.0.1/udp/8080/quic-v1/p2p/{}", PEER_IDS[0]).parse()?;

    // 2. Pre-populate cache with a DIFFERENT address
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(temp_dir.path())
        .with_max_peers(10);
    let cache = BootstrapCacheStore::new(config)?;
    let cached_addr: Multiaddr =
        format!("/ip4/10.0.0.2/udp/8080/quic-v1/p2p/{}", PEER_IDS[1]).parse()?;
    cache.add_addr(cached_addr.clone()).await;
    cache.write().await?;

    // 3. Mock server with a THIRD different address
    let mock_server = MockServer::start().await;
    let network_addr = format!("/ip4/10.0.0.3/udp/8080/quic-v1/p2p/{}", PEER_IDS[2]);
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&network_addr))
        .expect(1) // Should be called exactly once
        .mount(&mock_server)
        .await;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![cli_addr.clone()],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    // Request with count=None to get ALL addresses
    let addrs = args.get_bootstrap_addr(None).await?;

    // Verify we got addresses from ALL THREE sources
    assert_eq!(
        addrs.len(),
        3,
        "Should accumulate addresses from CLI, cache, AND network contacts"
    );

    // Verify each source contributed
    let addr_strings: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
    assert!(
        addr_strings.iter().any(|a| a.contains("10.0.0.1")),
        "Should contain CLI arg address (10.0.0.1)"
    );
    assert!(
        addr_strings.iter().any(|a| a.contains("10.0.0.2")),
        "Should contain cached address (10.0.0.2)"
    );
    assert!(
        addr_strings.iter().any(|a| a.contains("10.0.0.3")),
        "Should contain network contact address (10.0.0.3)"
    );

    drop(env_guard);
    Ok(())
}

// ============================================================================
// Error cases (2 tests)
// ============================================================================

#[tokio::test]
#[serial]
async fn test_all_sources_empty_returns_error() -> Result<()> {
    // No sources have addresses, verify appropriate error
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    // Start mock server returning empty response
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&mock_server)
        .await;

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![format!("{}/contacts", mock_server.uri())],
        local: false,
        ignore_cache: true,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let result = args.get_bootstrap_addr(None).await;

    assert!(
        result.is_err(),
        "Should return error when no sources have addresses"
    );

    drop(env_guard);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_local_mode_returns_empty() -> Result<()> {
    // local=true should return empty (for local network testing)
    // when there's no cache and no CLI args
    setup_test_env();
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Ensure env var is not set
    let env_guard = EnvVarGuard::new("ANT_PEERS");
    env_guard.remove();

    let args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![], // local mode ignores network contacts anyway
        local: true,
        ignore_cache: true,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
    };

    let result = args.get_bootstrap_addr(None).await;

    // In local mode with no sources, it should return an error (no bootstrap peers found)
    // because local mode doesn't fetch from network but still needs peers
    assert!(
        result.is_err(),
        "Local mode with no sources should return error"
    );

    drop(env_guard);
    Ok(())
}
