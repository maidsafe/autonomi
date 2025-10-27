// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod cache_data_v0;
pub mod cache_data_v1;

use crate::{BootstrapConfig, Error, Result, craft_valid_multiaddr};
use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};
use rand::Rng;
use std::{collections::HashSet, fs, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::Instrument;

pub type CacheDataLatest = cache_data_v1::CacheData;
pub const CACHE_DATA_VERSION_LATEST: u32 = cache_data_v1::CacheData::CACHE_DATA_VERSION;

#[derive(Clone, Debug)]
pub struct BootstrapCacheStore {
    /// Configuration for the cache store
    pub(crate) config: Arc<BootstrapConfig>,
    /// In-memory cache data
    pub(crate) data: Arc<RwLock<CacheDataLatest>>,
    /// List of peers to remove from the fs cache, would be done during the next sync_and_flush_to_disk call
    pub(crate) to_remove: Arc<RwLock<HashSet<PeerId>>>,
}

impl BootstrapCacheStore {
    pub fn config(&self) -> &BootstrapConfig {
        &self.config
    }

    /// Create an empty CacheStore with the given configuration
    pub fn new(config: BootstrapConfig) -> Result<Self> {
        info!("Creating new CacheStore with config: {:?}", config);

        // Create cache directory if it doesn't exist
        if !config.cache_dir.exists() {
            info!(
                "Attempting to create cache directory at {:?}",
                config.cache_dir
            );
            fs::create_dir_all(&config.cache_dir).inspect_err(|err| {
                warn!(
                    "Failed to create cache directory at {:?}: {err}",
                    config.cache_dir
                );
            })?;
        }

        let store = Self {
            config: Arc::new(config),
            data: Arc::new(RwLock::new(CacheDataLatest::default())),
            to_remove: Arc::new(RwLock::new(HashSet::new())),
        };

        Ok(store)
    }

    pub async fn peer_count(&self) -> usize {
        self.data.read().await.peers.len()
    }

    pub async fn get_all_addrs(&self) -> Vec<Multiaddr> {
        self.data.read().await.get_all_addrs().cloned().collect()
    }

    /// Queue a peer for removal from the cache. The actual removal will happen during the next sync_and_flush_to_disk call.
    pub async fn queue_remove_peer(&self, peer_id: &PeerId) {
        self.to_remove.write().await.insert(*peer_id);
    }

    /// Add an address to the cache. Note that the address must have a valid peer ID.
    ///
    /// We do not write P2pCircuit addresses to the cache.
    pub async fn add_addr(&self, addr: Multiaddr) {
        if addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
            return;
        }
        let Some(addr) = craft_valid_multiaddr(&addr, false) else {
            return;
        };
        let peer_id = match addr.iter().find(|p| matches!(p, Protocol::P2p(_))) {
            Some(Protocol::P2p(id)) => id,
            _ => return,
        };

        debug!("Adding addr to bootstrap cache: {addr}");

        self.data.write().await.add_peer(
            peer_id,
            [addr].iter(),
            self.config.max_addrs_per_cached_peer,
            self.config.max_cached_peers,
        );
    }

    /// Load cache data from disk
    /// Make sure to have clean addrs inside the cache as we don't call craft_valid_multiaddr
    pub fn load_cache_data(cfg: &BootstrapConfig) -> Result<CacheDataLatest> {
        // try loading latest first
        match cache_data_v1::CacheData::read_from_file(
            &cfg.cache_dir,
            &Self::cache_file_name(cfg.local),
        ) {
            Ok(mut data) => {
                while data.peers.len() > cfg.max_cached_peers {
                    data.peers.pop_back();
                }
                return Ok(data);
            }
            Err(err) => {
                warn!("Failed to load cache data from latest version: {err}");
            }
        }

        // Try loading older version
        match cache_data_v0::CacheData::read_from_file(
            &cfg.cache_dir,
            &Self::cache_file_name(cfg.local),
        ) {
            Ok(data) => {
                warn!("Loaded cache data from older version, upgrading to latest version");
                let mut data: CacheDataLatest = data.into();
                while data.peers.len() > cfg.max_cached_peers {
                    data.peers.pop_back();
                }

                Ok(data)
            }
            Err(err) => {
                warn!("Failed to load cache data from older version: {err}");
                Err(Error::FailedToParseCacheData)
            }
        }
    }

    /// Flush the cache to disk after syncing with the CacheData from the file.
    ///
    /// Note: This clears the data in memory after writing to disk.
    pub async fn sync_and_flush_to_disk(&self) -> Result<()> {
        if self.config.disable_cache_writing {
            info!("Cache writing is disabled, skipping sync to disk");
            return Ok(());
        }

        if self.data.read().await.peers.is_empty() {
            info!("Cache is empty, skipping sync and flush to disk");
            return Ok(());
        }

        info!(
            "Flushing cache to disk, with data containing: {} peers",
            self.data.read().await.peers.len(),
        );

        if let Ok(data_from_file) = Self::load_cache_data(&self.config) {
            self.data.write().await.sync(
                &data_from_file,
                self.config.max_addrs_per_cached_peer,
                self.config.max_cached_peers,
            );
        } else {
            warn!("Failed to load cache data from file, overwriting with new data");
        }

        // Remove queued peers
        let to_remove: Vec<PeerId> = self.to_remove.write().await.drain().collect();
        if !to_remove.is_empty() {
            info!("Removing {} peers from cache", to_remove.len());
            for peer_id in to_remove {
                self.data.write().await.remove_peer(&peer_id);
            }
        } else {
            debug!("No peers queued for removal from cache");
        }

        self.write().await.inspect_err(|e| {
            error!("Failed to save cache to disk: {e}");
        })?;

        // Flush after writing
        debug!("Clearing in-memory cache data after flush to disk");
        self.data.write().await.peers.clear();

        Ok(())
    }

    /// Write the cache to disk atomically. This will overwrite the existing cache file, use sync_and_flush_to_disk to
    /// sync with the file first.
    pub async fn write(&self) -> Result<()> {
        if self.config.disable_cache_writing {
            info!("Cache writing is disabled, skipping sync to disk");
            return Ok(());
        }

        let filename = Self::cache_file_name(self.config.local);

        self.data
            .write()
            .await
            .write_to_file(&self.config.cache_dir, &filename)?;

        if self.config.backwards_compatible_writes {
            let data = self.data.read().await;
            cache_data_v0::CacheData::from(&*data)
                .write_to_file(&self.config.cache_dir, &filename)?;
        }

        Ok(())
    }

    /// Returns the name of the cache filename based on the local flag
    pub fn cache_file_name(local: bool) -> String {
        if local {
            format!(
                "bootstrap_cache_local_{}.json",
                crate::get_network_version()
            )
        } else {
            format!("bootstrap_cache_{}.json", crate::get_network_version())
        }
    }

    /// Runs the sync_and_flush_to_disk method periodically
    /// This is useful for keeping the cache up-to-date without blocking the main thread.
    pub(crate) fn sync_and_flush_periodically(&self) -> tokio::task::JoinHandle<()> {
        let store = self.clone();

        let current_span = tracing::Span::current();
        tokio::spawn(async move {
            // add a variance of 10% to the interval, to avoid all nodes writing to disk at the same time.
            let mut sleep_interval =
                duration_with_variance(store.config.min_cache_save_duration, 10);
            if store.config.disable_cache_writing {
                info!("Cache writing is disabled, skipping periodic sync and flush task");
                return;
            }
            info!("Starting periodic cache sync and flush task, first sync in {sleep_interval:?}");

            loop {
                tokio::time::sleep(sleep_interval).await;
                if let Err(e) = store.sync_and_flush_to_disk().await {
                    error!("Failed to sync and flush cache to disk: {e}");
                }
                // add a variance of 1% to the max interval to avoid all nodes writing to disk at the same time.
                let max_cache_save_duration =
                    duration_with_variance(store.config.max_cache_save_duration, 1);

                let new_interval = sleep_interval
                    .checked_mul(store.config.cache_save_scaling_factor)
                    .unwrap_or(max_cache_save_duration);
                sleep_interval = new_interval.min(max_cache_save_duration);
                info!("Cache synced and flushed to disk successfully - next sync in {sleep_interval:?}");
            }
        }.instrument(current_span))
    }
}

/// Returns a new duration that is within +/- variance of the provided duration.
fn duration_with_variance(duration: Duration, variance: u32) -> Duration {
    let variance = duration.as_secs() as f64 * (variance as f64 / 100.0);

    let random_adjustment = Duration::from_secs(rand::thread_rng().gen_range(0..variance as u64));
    if random_adjustment.as_secs().is_multiple_of(2) {
        duration.saturating_sub(random_adjustment)
    } else {
        duration.saturating_add(random_adjustment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache_store::{cache_data_v0, cache_data_v1},
        multiaddr_get_peer_id,
    };
    use libp2p::{Multiaddr, PeerId};
    use std::{collections::HashSet, time::SystemTime};
    use tempfile::TempDir;
    use tokio::{
        task,
        time::{Duration, sleep},
    };

    #[tokio::test]
    async fn test_duration_variance_fn() {
        let duration = Duration::from_secs(150);
        let variance = 10;
        let expected_variance = Duration::from_secs(15); // 10% of 150
        for _ in 0..10000 {
            let new_duration = duration_with_variance(duration, variance);
            println!("new_duration: {new_duration:?}");
            if new_duration < duration - expected_variance
                || new_duration > duration + expected_variance
            {
                panic!("new_duration: {new_duration:?} is not within the expected range",);
            }
        }
    }

    fn temp_config(dir: &TempDir) -> BootstrapConfig {
        BootstrapConfig::default().with_cache_dir(dir.path())
    }

    #[tokio::test]
    async fn test_empty_cache() {
        let dir = TempDir::new().expect("temp dir");
        let config = temp_config(&dir);
        let cache = BootstrapCacheStore::new(config.clone()).expect("create cache");

        cache.write().await.expect("write empty cache");
        let loaded = BootstrapCacheStore::load_cache_data(&config).expect("load cache");
        assert!(loaded.peers.is_empty());
    }

    #[tokio::test]
    async fn test_max_peer_limit_enforcement() {
        let dir = TempDir::new().expect("temp dir");
        let config = BootstrapConfig::default()
            .with_cache_dir(dir.path())
            .with_max_cached_peers(3);
        let cache = BootstrapCacheStore::new(config.clone()).expect("create cache");

        let samples = [
            "/ip4/127.0.0.1/udp/1200/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
            "/ip4/127.0.0.2/udp/1201/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
            "/ip4/127.0.0.3/udp/1202/quic-v1/p2p/12D3KooWHehYgXKLxsXjzFzDqMLKhcAVc4LaktnT7Zei1G2zcpJB",
            "/ip4/127.0.0.4/udp/1203/quic-v1/p2p/12D3KooWQF3NMWHRmMQBY8GVdpQh1V6TFYuQqZkKKvYE7yCS6fYK",
            "/ip4/127.0.0.5/udp/1204/quic-v1/p2p/12D3KooWRi6wF7yxWLuPSNskXc6kQ5cJ6eaymeMbCRdTnMesPgFx",
        ];

        let mut recorded = Vec::new();
        for addr_str in samples {
            let addr: Multiaddr = addr_str.parse().unwrap();
            recorded.push(addr.clone());
            cache.add_addr(addr).await;
            sleep(Duration::from_millis(5)).await;
        }

        let current = cache.get_all_addrs().await;
        assert_eq!(current.len(), 3);
        assert!(current.iter().all(|addr| recorded[2..].contains(addr)));

        cache.write().await.expect("persist cache");
        let persisted = BootstrapCacheStore::load_cache_data(&config)
            .expect("load persisted")
            .get_all_addrs()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(persisted.len(), 3);
        assert!(persisted.iter().all(|addr| recorded[2..].contains(addr)));
    }

    #[tokio::test]
    async fn test_queued_peer_not_removed_until_flush() {
        let dir = TempDir::new().expect("temp dir");
        let config = temp_config(&dir);
        let cache = BootstrapCacheStore::new(config.clone()).expect("create cache");

        let addr: Multiaddr = "/ip4/127.0.0.6/udp/1205/quic-v1/p2p/12D3KooWQnE7zXkVUEGBnJtNfR88Ujz4ezgm6bVnkvxHCzhF7S5S"
            .parse()
            .unwrap();
        cache.add_addr(addr.clone()).await;
        cache.write().await.expect("persist initial cache state");
        let peer_id = multiaddr_get_peer_id(&addr).expect("peer id");
        cache.queue_remove_peer(&peer_id).await;
        let addrs_before_flush = cache.get_all_addrs().await;
        assert_eq!(
            addrs_before_flush.len(),
            1,
            "peer should remain available before flush"
        );
        assert_eq!(
            addrs_before_flush[0], addr,
            "cached address should match the inserted peer"
        );

        let persisted_before_flush =
            BootstrapCacheStore::load_cache_data(&config).expect("load persisted cache");
        let persisted_before_flush: Vec<_> =
            persisted_before_flush.get_all_addrs().cloned().collect();
        assert!(
            persisted_before_flush.iter().any(|stored| stored == &addr),
            "queued removal must not affect persisted cache before flush"
        );

        cache
            .sync_and_flush_to_disk()
            .await
            .expect("flush cache to disk");

        let persisted_after_flush =
            BootstrapCacheStore::load_cache_data(&config).expect("load persisted cache");
        let persisted_after_flush: Vec<_> =
            persisted_after_flush.get_all_addrs().cloned().collect();
        assert!(
            persisted_after_flush.iter().all(|stored| stored != &addr),
            "peer should be absent from persisted cache after flush"
        );
    }

    #[tokio::test]
    async fn test_queued_peer_removal_queue_drained_after_flush() {
        let dir = TempDir::new().expect("temp dir");
        let config = temp_config(&dir);
        let cache = BootstrapCacheStore::new(config.clone()).expect("create cache");

        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        cache.add_addr(addr.clone()).await;
        cache.write().await.expect("persist initial cache state");

        let peer_id = multiaddr_get_peer_id(&addr).expect("peer id");
        cache.queue_remove_peer(&peer_id).await;
        cache
            .sync_and_flush_to_disk()
            .await
            .expect("flush queued removals");
        let persisted_after_removal =
            BootstrapCacheStore::load_cache_data(&config).expect("load cache data");
        assert!(
            persisted_after_removal.get_all_addrs().next().is_none(),
            "peer should be removed from persisted cache after flush"
        );

        cache.add_addr(addr.clone()).await;

        cache
            .sync_and_flush_to_disk()
            .await
            .expect("flush cache after re-adding peer");
        let persisted_after_re_add =
            BootstrapCacheStore::load_cache_data(&config).expect("load cache data");
        let persisted_after_re_add: Vec<_> =
            persisted_after_re_add.get_all_addrs().cloned().collect();
        assert_eq!(
            persisted_after_re_add.len(),
            1,
            "re-added peer should persist after subsequent flush"
        );
        assert_eq!(
            persisted_after_re_add[0], addr,
            "persisted address should match the re-added peer"
        );
    }

    #[tokio::test]
    async fn test_cache_file_corruption() {
        let dir = TempDir::new().expect("temp dir");
        let cache_dir = dir.path();
        let config = BootstrapConfig::default().with_cache_dir(cache_dir);
        let cache = BootstrapCacheStore::new(config.clone()).expect("create cache");

        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        cache.add_addr(addr).await;
        cache.write().await.expect("write cache");

        let corrupted_path = cache_dir
            .join(format!(
                "version_{}",
                cache_data_v1::CacheData::CACHE_DATA_VERSION
            ))
            .join(BootstrapCacheStore::cache_file_name(false));
        std::fs::write(&corrupted_path, "{not valid json}").expect("corrupt file");

        let load_err = BootstrapCacheStore::load_cache_data(&config);
        assert!(load_err.is_err(), "loading corrupted cache should error");

        let new_store =
            BootstrapCacheStore::new(config.clone()).expect("create store after corruption");
        assert_eq!(
            new_store.peer_count().await,
            0,
            "new cache should start empty after corruption"
        );
        new_store.write().await.expect("write clean cache");

        let reloaded = BootstrapCacheStore::load_cache_data(&config).expect("reload cache");
        assert!(
            reloaded.peers.is_empty(),
            "cache data should be empty after regeneration"
        );
    }

    #[tokio::test]
    async fn test_max_addrs_per_peer() {
        let dir = TempDir::new().expect("temp dir");
        let config = BootstrapConfig::default()
            .with_cache_dir(dir.path())
            .with_max_addrs_per_cached_peer(2);
        let cache = BootstrapCacheStore::new(config.clone()).expect("create cache");

        let peer_id = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
        for octet in 1..=4 {
            let addr: Multiaddr = format!("/ip4/127.0.0.{octet}/udp/8080/quic-v1/p2p/{peer_id}")
                .parse()
                .unwrap();
            cache.add_addr(addr).await;
        }

        cache.write().await.expect("write cache");
        let reloaded = BootstrapCacheStore::load_cache_data(&config).expect("load cache");
        let collected: Vec<_> = reloaded.get_all_addrs().cloned().collect();
        assert!(
            collected.len() <= 2,
            "should honor max_addrs_per_peer limit"
        );
    }

    #[tokio::test]
    async fn test_concurrent_cache_access() {
        let dir = TempDir::new().expect("temp dir");
        let cache_dir = dir.path().to_path_buf();
        let config = BootstrapConfig::default().with_cache_dir(cache_dir.clone());

        let mut handles = Vec::new();
        for idx in 0..5 {
            let config_clone = config.clone();
            handles.push(task::spawn(async move {
                let store = BootstrapCacheStore::new(config_clone)?;
                let addr: Multiaddr = format!(
                    "/ip4/127.0.0.{}/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{}",
                    idx + 1,
                    idx + 1
                )
                .parse()
                .unwrap();
                store.add_addr(addr).await;
                sleep(Duration::from_millis(10)).await;
                store.sync_and_flush_to_disk().await
            }));
        }

        for handle in handles {
            let result = handle.await.expect("task join");
            result.expect("task result");
        }

        let final_store = BootstrapCacheStore::new(config).expect("create final store");
        let loaded = BootstrapCacheStore::load_cache_data(final_store.config()).expect("load");
        assert_eq!(loaded.peers.len(), 5, "should persist peers from all tasks");
    }

    #[tokio::test]
    async fn test_cache_sync_functionality() {
        let dir = TempDir::new().expect("temp dir");
        let cache_dir = dir.path();

        let config = BootstrapConfig::default().with_cache_dir(cache_dir);
        let first_store = BootstrapCacheStore::new(config.clone()).expect("create cache");
        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        first_store.add_addr(addr1.clone()).await;
        first_store.write().await.expect("write first cache");

        let second_store = BootstrapCacheStore::new(config.clone()).expect("create cache");
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .parse()
            .unwrap();
        second_store.add_addr(addr2.clone()).await;
        second_store
            .sync_and_flush_to_disk()
            .await
            .expect("sync cache");

        let file_name = BootstrapCacheStore::cache_file_name(false);
        let cache_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &file_name);
        let cache_content = std::fs::read_to_string(&cache_path).expect("read cache file");
        assert!(
            cache_content.contains(&addr1.to_string())
                && cache_content.contains(&addr2.to_string()),
            "cache content should include both addresses"
        );

        let check_store = BootstrapCacheStore::new(config).expect("create verifying store");
        let loaded = BootstrapCacheStore::load_cache_data(check_store.config()).expect("load");
        let addrs: Vec<_> = loaded.get_all_addrs().cloned().collect();
        assert!(
            addrs
                .iter()
                .any(|addr| addr.to_string() == addr1.to_string())
                && addrs
                    .iter()
                    .any(|addr| addr.to_string() == addr2.to_string()),
            "both addresses should be present after sync"
        );
    }

    #[tokio::test]
    async fn test_sync_duplicates_overlapping_peers() {
        let mut cache1 = CacheDataLatest::default();
        let mut cache2 = CacheDataLatest::default();

        let peers: Vec<PeerId> = (0..3).map(|_| PeerId::random()).collect();
        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .parse()
            .unwrap();
        let addr3: Multiaddr = "/ip4/127.0.0.3/udp/8082/quic-v1/p2p/12D3KooWCKCeqLPSgMnDjyFsJuWqREDtKNHx1JEBiwxME7Zdw68n"
            .parse()
            .unwrap();

        cache1.add_peer(peers[0], [addr1.clone()].iter(), 10, 10);
        cache1.add_peer(peers[1], [addr2.clone()].iter(), 10, 10);
        cache2.add_peer(peers[1], [addr2.clone()].iter(), 10, 10);
        cache2.add_peer(peers[2], [addr3.clone()].iter(), 10, 10);

        cache1.sync(&cache2, 10, 10);
        let result: HashSet<_> = cache1
            .get_all_addrs()
            .cloned()
            .map(|addr| addr.to_string())
            .collect();
        assert_eq!(result.len(), 3, "should merge and deduplicate addresses");
        assert!(result.contains(&addr1.to_string()));
        assert!(result.contains(&addr2.to_string()));
        assert!(result.contains(&addr3.to_string()));
    }

    #[tokio::test]
    async fn test_sync_at_limit_overwrites_unique_peers() {
        let mut cache1 = CacheDataLatest::default();
        let mut cache2 = CacheDataLatest::default();

        let addrs: Vec<Multiaddr> = (1..=7)
            .map(|i| {
                format!(
                    "/ip4/127.0.0.1/udp/808{i}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{i}"
                )
                .parse()
                .unwrap()
            })
            .collect();
        let peers: Vec<_> = addrs
            .iter()
            .map(|addr| match multiaddr_get_peer_id(addr) {
                Some(peer) => peer,
                None => panic!("address missing peer id"),
            })
            .collect();

        for idx in 0..5 {
            cache1.add_peer(peers[idx], [addrs[idx].clone()].iter(), 10, 5);
        }
        for idx in 2..7 {
            cache2.add_peer(peers[idx], [addrs[idx].clone()].iter(), 10, 5);
        }

        cache1.sync(&cache2, 10, 5);
        let after: HashSet<_> = cache1.peers.iter().map(|(peer_id, _)| *peer_id).collect();
        assert_eq!(cache1.peers.len(), 5, "should respect max peers");
        assert!(after.contains(&peers[0]));
        assert!(after.contains(&peers[1]));
    }

    #[tokio::test]
    async fn test_sync_other_at_limit_self_below_limit() {
        let mut cache1 = CacheDataLatest::default();
        let mut cache2 = CacheDataLatest::default();

        let addrs: Vec<Multiaddr> = (1..=7)
            .map(|i| {
                format!(
                    "/ip4/127.0.0.1/udp/908{i}/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UER{i}"
                )
                .parse()
                .unwrap()
            })
            .collect();
        let peers: Vec<_> = addrs
            .iter()
            .map(|addr| multiaddr_get_peer_id(addr).expect("peer id"))
            .collect();

        for idx in 0..2 {
            cache1.add_peer(peers[idx], [addrs[idx].clone()].iter(), 10, 5);
        }
        for idx in 2..7 {
            cache2.add_peer(peers[idx], [addrs[idx].clone()].iter(), 10, 5);
        }

        cache1.sync(&cache2, 10, 5);
        let after: HashSet<_> = cache1.peers.iter().map(|(peer_id, _)| *peer_id).collect();
        assert_eq!(cache1.peers.len(), 5);
        assert!(after.contains(&peers[0]));
        assert!(after.contains(&peers[1]));
    }

    #[tokio::test]
    async fn test_cache_version_upgrade() {
        let dir = TempDir::new().expect("temp dir");
        let cache_dir = dir.path();

        let mut v0_data = cache_data_v0::CacheData {
            peers: Default::default(),
            last_updated: SystemTime::now(),
            network_version: crate::get_network_version(),
        };
        let peer_id = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1"
            .parse()
            .expect("parse addr");
        let boot_addr = cache_data_v0::BootstrapAddr {
            addr: addr.clone(),
            success_count: 1,
            failure_count: 0,
            last_seen: SystemTime::now(),
        };
        v0_data
            .peers
            .insert(peer_id, cache_data_v0::BootstrapAddresses(vec![boot_addr]));

        let config = BootstrapConfig::default().with_cache_dir(cache_dir);
        let filename = BootstrapCacheStore::cache_file_name(false);
        v0_data
            .write_to_file(cache_dir, &filename)
            .expect("write v0 cache");

        let upgraded = BootstrapCacheStore::load_cache_data(&config).expect("load cache");
        assert!(
            !upgraded.peers.is_empty(),
            "peers should carry over after upgrade"
        );
        assert!(
            upgraded.get_all_addrs().next().is_some(),
            "addresses should be preserved after upgrade"
        );
        assert_eq!(
            upgraded.cache_version,
            cache_data_v1::CacheData::CACHE_DATA_VERSION.to_string()
        );
    }

    #[tokio::test]
    async fn test_backwards_compatible_writes() {
        let dir = TempDir::new().expect("temp dir");
        let cache_dir = dir.path();

        let config = BootstrapConfig::default()
            .with_cache_dir(cache_dir)
            .with_backwards_compatible_writes(true);
        let cache = BootstrapCacheStore::new(config.clone()).expect("create cache");
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        cache.add_addr(addr).await;
        cache.write().await.expect("write cache");

        let filename = BootstrapCacheStore::cache_file_name(false);
        let v0_data =
            cache_data_v0::CacheData::read_from_file(cache_dir, &filename).expect("read v0");
        let v1_data =
            cache_data_v1::CacheData::read_from_file(cache_dir, &filename).expect("read v1");
        assert!(!v0_data.peers.is_empty(), "v0 data should be populated");
        assert!(!v1_data.peers.is_empty(), "v1 data should be populated");
    }

    #[tokio::test]
    async fn test_version_specific_file_paths() {
        let dir = TempDir::new().expect("temp dir");
        let cache_dir = dir.path();

        let filename = BootstrapCacheStore::cache_file_name(false);
        let v0_path = cache_data_v0::CacheData::cache_file_path(cache_dir, &filename);
        let v1_path = cache_data_v1::CacheData::cache_file_path(cache_dir, &filename);

        assert!(
            v1_path.to_string_lossy().contains(&format!(
                "version_{}",
                cache_data_v1::CacheData::CACHE_DATA_VERSION
            )),
            "v1 path should include version directory"
        );
        assert!(
            !v0_path.to_string_lossy().contains("version_"),
            "v0 path should not include version segment"
        );
    }
}
