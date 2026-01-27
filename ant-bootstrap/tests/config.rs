// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{BootstrapCacheConfig, InitialPeersConfig};
use ant_logging::LogBuilder;
use color_eyre::Result;
use tempfile::TempDir;

// =============================================================================
// Cache directory resolution tests (2 tests)
// =============================================================================

#[tokio::test]
async fn test_default_cache_dir_uses_platform_data_dir() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Default config uses platform-specific data dir
    let config = BootstrapCacheConfig::new(false);

    // Check cache_dir is set to something reasonable (not empty)
    assert!(
        !config.cache_dir.as_os_str().is_empty(),
        "Default cache dir should not be empty"
    );

    // Should contain "autonomi" and "bootstrap_cache" in the path
    let path_str = config.cache_dir.to_string_lossy();
    assert!(
        path_str.contains("autonomi"),
        "Cache dir should contain 'autonomi', got: {path_str}"
    );
    assert!(
        path_str.contains("bootstrap_cache"),
        "Cache dir should contain 'bootstrap_cache', got: {path_str}"
    );

    Ok(())
}

#[tokio::test]
async fn test_cache_dir_override() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let custom_path = temp_dir.path();

    // Override with custom path
    let config = BootstrapCacheConfig::empty().with_cache_dir(custom_path);

    // Should use the custom path
    assert_eq!(
        config.cache_dir,
        custom_path.to_path_buf(),
        "Cache dir should be overridden to custom path"
    );

    // Should not contain default path components
    let default_config = BootstrapCacheConfig::new(false);
    if !default_config.cache_dir.as_os_str().is_empty() {
        assert_ne!(
            config.cache_dir, default_config.cache_dir,
            "Custom cache dir should differ from default"
        );
    }

    Ok(())
}

// =============================================================================
// Config building tests (2 tests)
// =============================================================================

#[tokio::test]
async fn test_config_from_initial_peers() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;

    // Create InitialPeersConfig with custom cache dir
    let initial_peers_config = InitialPeersConfig {
        local: true,
        bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    };

    // Convert to BootstrapCacheConfig
    let bootstrap_config = BootstrapCacheConfig::try_from(&initial_peers_config)?;

    // Verify local flag is transferred
    assert!(
        bootstrap_config.local,
        "Local flag should be transferred from InitialPeersConfig"
    );

    // Verify cache dir is transferred
    assert_eq!(
        bootstrap_config.cache_dir,
        temp_dir.path().to_path_buf(),
        "Cache dir should be transferred from InitialPeersConfig"
    );

    Ok(())
}

#[tokio::test]
async fn test_config_builder_chaining() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let custom_path = temp_dir.path();

    // Test builder pattern with multiple chained calls
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(custom_path)
        .with_local(true)
        .with_backwards_compatible_writes(true)
        .with_max_peers(500)
        .with_addrs_per_peer(5)
        .with_disable_cache_writing(true);

    // Verify all settings
    assert_eq!(config.cache_dir, custom_path.to_path_buf());
    assert!(config.local);
    assert!(config.backwards_compatible_writes);
    assert_eq!(config.max_peers, 500);
    assert_eq!(config.max_addrs_per_peer, 5);
    assert!(config.disable_cache_writing);

    Ok(())
}
