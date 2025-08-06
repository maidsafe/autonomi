// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{
    BootstrapCacheConfig, BootstrapCacheStore,
    cache_store::{cache_data_v0, cache_data_v1},
};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::{Multiaddr, PeerId};
use std::time::SystemTime;
use tempfile::TempDir;

#[tokio::test]
async fn test_cache_version_upgrade() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create a v0 cache data
    let mut v0_data = cache_data_v0::CacheData {
        peers: Default::default(),
        last_updated: SystemTime::now(),
        network_version: ant_bootstrap::get_network_version(),
    };

    // Add a peer
    let peer_id = PeerId::random();
    let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse()?;
    let boot_addr = cache_data_v0::BootstrapAddr {
        addr: addr.clone(),
        success_count: 1,
        failure_count: 0,
        last_seen: SystemTime::now(),
    };
    let addrs = cache_data_v0::BootstrapAddresses(vec![boot_addr]);
    v0_data.peers.insert(peer_id, addrs);

    // Write v0 data to file
    let config = BootstrapCacheConfig::empty().with_cache_dir(cache_dir);
    let filename = BootstrapCacheStore::cache_file_name(false);
    v0_data.write_to_file(cache_dir, &filename)?;

    // Load cache with v0 data - should be upgraded to v1
    let cache_data = BootstrapCacheStore::load_cache_data(&config)?;

    // Verify the peers were preserved
    assert!(
        !cache_data.peers.is_empty(),
        "Peers should be preserved after version upgrade"
    );

    // Verify each peer has a multiaddr in the final cache
    let has_addrs = cache_data.get_all_addrs().next().is_some();
    assert!(
        has_addrs,
        "Addresses should be preserved after version upgrade"
    );
    assert!(cache_data.cache_version == cache_data_v1::CacheData::CACHE_DATA_VERSION.to_string());

    Ok(())
}

#[tokio::test]
async fn test_backwards_compatible_writes() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Create config with backwards compatibility enabled
    let config = BootstrapCacheConfig::empty()
        .with_cache_dir(cache_dir)
        .with_backwards_compatible_writes(true);

    // Create and populate cache store
    let cache_store = BootstrapCacheStore::new(config)?;
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    cache_store.add_addr(addr.clone()).await;

    // Write cache to disk
    cache_store.write().await?;

    // Check that v0 format file exists and can be read
    let filename = BootstrapCacheStore::cache_file_name(false);
    let v0_data = cache_data_v0::CacheData::read_from_file(cache_dir, &filename)?;

    // Check that v1 format file exists and can be read
    let filename = BootstrapCacheStore::cache_file_name(false);
    let v1_data = cache_data_v1::CacheData::read_from_file(cache_dir, &filename)?;

    // Verify data was written in v0 format
    assert!(
        !v0_data.peers.is_empty(),
        "Peers should be written in v0 format"
    );

    // Verify data was written in v1 format
    assert!(
        !v1_data.peers.is_empty(),
        "Peers should be written in v1 format"
    );

    Ok(())
}

#[tokio::test]
async fn test_version_specific_file_paths() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    let temp_dir = TempDir::new()?;
    let cache_dir = temp_dir.path();

    // Get paths for v0 and v1
    let filename = BootstrapCacheStore::cache_file_name(false);
    let v0_path = cache_data_v0::CacheData::cache_file_path(cache_dir, &filename);
    let v1_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &filename);

    // V1 should include version in path
    assert!(
        v1_path.to_string_lossy().contains(&format!(
            "version_{}",
            cache_data_v1::CacheData::CACHE_DATA_VERSION
        )),
        "V1 path should include version number"
    );

    // V0 shouldn't have version in path
    assert!(
        !v0_path.to_string_lossy().contains("version_"),
        "V0 path should not include version number"
    );

    Ok(())
}
