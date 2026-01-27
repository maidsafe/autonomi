// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod cache_data_v0;
pub mod cache_data_v1;

use crate::{BootstrapCacheConfig, Error, Result, craft_valid_multiaddr, multiaddr_get_peer_id};
use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};
use rand::Rng;
use std::{collections::HashSet, fs, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::Instrument;

pub type CacheDataLatest = cache_data_v1::CacheData;
pub const CACHE_DATA_VERSION_LATEST: u32 = cache_data_v1::CacheData::CACHE_DATA_VERSION;

#[derive(Clone, Debug)]
pub struct BootstrapCacheStore {
    pub(crate) config: Arc<BootstrapCacheConfig>,
    pub(crate) data: Arc<RwLock<CacheDataLatest>>,
    /// List of peers to remove from the fs cache, would be done during the next sync_and_flush_to_disk call
    pub(crate) to_remove: Arc<RwLock<HashSet<PeerId>>>,
}

impl BootstrapCacheStore {
    pub fn config(&self) -> &BootstrapCacheConfig {
        &self.config
    }

    /// Create an empty CacheStore with the given configuration
    pub fn new(config: BootstrapCacheConfig) -> Result<Self> {
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
        let Some(addr) = craft_valid_multiaddr(&addr) else {
            return;
        };
        let Some(peer_id) = multiaddr_get_peer_id(&addr) else {
            return;
        };

        debug!("Adding addr to bootstrap cache: {addr}");

        self.data.write().await.add_peer(
            peer_id,
            [addr].iter(),
            self.config.max_addrs_per_peer,
            self.config.max_peers,
        );
    }

    /// Load cache data from disk
    /// Make sure to have clean addrs inside the cache as we don't call craft_valid_multiaddr
    pub fn load_cache_data(cfg: &BootstrapCacheConfig) -> Result<CacheDataLatest> {
        // try loading latest first
        match cache_data_v1::CacheData::read_from_file(
            &cfg.cache_dir,
            &Self::cache_file_name(cfg.local),
        ) {
            Ok(mut data) => {
                while data.peers.len() > cfg.max_peers {
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
                while data.peers.len() > cfg.max_peers {
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

        if self.data.read().await.peers.is_empty() && self.to_remove.read().await.is_empty() {
            info!("No peers to write to disk and no removals queued, skipping sync to disk");
            return Ok(());
        }

        info!(
            "Flushing cache to disk, with data containing: {} peers",
            self.data.read().await.peers.len(),
        );

        if let Ok(data_from_file) = Self::load_cache_data(&self.config) {
            self.data.write().await.sync(
                &data_from_file,
                self.config.max_addrs_per_peer,
                self.config.max_peers,
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
    pub fn sync_and_flush_periodically(&self) -> tokio::task::JoinHandle<()> {
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
    use super::{CacheDataLatest, duration_with_variance};
    use libp2p::{Multiaddr, PeerId};
    use std::time::Duration;

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

    // Duration variance additional tests (3 more)
    #[test]
    #[should_panic(expected = "cannot sample empty range")]
    fn test_duration_variance_zero_duration() {
        // Zero duration results in zero variance which causes gen_range(0..0) to panic
        let _ = duration_with_variance(Duration::ZERO, 50);
    }

    #[test]
    #[should_panic(expected = "cannot sample empty range")]
    fn test_duration_variance_zero_percent() {
        // Zero percent variance causes gen_range(0..0) to panic
        let base = Duration::from_secs(100);
        let _ = duration_with_variance(base, 0);
    }

    #[test]
    fn test_duration_variance_bounds_check() {
        let base = Duration::from_secs(100);
        let variance_pct = 20;
        // Run 100 times to verify bounds
        for _ in 0..100 {
            let result = duration_with_variance(base, variance_pct);
            let min = Duration::from_secs(80);
            let max = Duration::from_secs(120);
            assert!(
                result >= min && result <= max,
                "Result {result:?} outside bounds [{min:?}, {max:?}]"
            );
        }
    }

    // CacheData sync() tests (9 tests)
    #[test]
    fn test_sync_empty_self_nonempty_other() {
        let mut self_cache = CacheDataLatest::default();
        let mut other_cache = CacheDataLatest::default();

        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        other_cache.add_peer(peer_id, [addr.clone()].iter(), 10, 10);

        self_cache.sync(&other_cache, 10, 10);

        assert_eq!(self_cache.peers.len(), 1);
    }

    #[test]
    fn test_sync_nonempty_self_empty_other() {
        let mut self_cache = CacheDataLatest::default();
        let other_cache = CacheDataLatest::default();

        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        self_cache.add_peer(peer_id, [addr.clone()].iter(), 10, 10);

        self_cache.sync(&other_cache, 10, 10);

        assert_eq!(self_cache.peers.len(), 1);
    }

    #[test]
    fn test_sync_overlapping_peers_merges_addresses() {
        let mut self_cache = CacheDataLatest::default();
        let mut other_cache = CacheDataLatest::default();

        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();

        self_cache.add_peer(peer_id, [addr1.clone()].iter(), 10, 10);
        other_cache.add_peer(peer_id, [addr2.clone()].iter(), 10, 10);

        self_cache.sync(&other_cache, 10, 10);

        assert_eq!(self_cache.peers.len(), 1);
        let (_, addrs) = self_cache.peers.front().unwrap();
        assert_eq!(addrs.len(), 2); // Both addresses merged
    }

    #[test]
    fn test_sync_address_deduplication() {
        let mut self_cache = CacheDataLatest::default();
        let mut other_cache = CacheDataLatest::default();

        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();

        self_cache.add_peer(peer_id, [addr.clone()].iter(), 10, 10);
        other_cache.add_peer(peer_id, [addr.clone()].iter(), 10, 10);

        self_cache.sync(&other_cache, 10, 10);

        let (_, addrs) = self_cache.peers.front().unwrap();
        assert_eq!(addrs.len(), 1); // No duplicate
    }

    #[test]
    fn test_sync_truncates_addrs_to_max_per_peer() {
        let mut self_cache = CacheDataLatest::default();
        let mut other_cache = CacheDataLatest::default();

        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();
        let addr3: Multiaddr = "/ip4/127.0.0.3/udp/8080/quic-v1".parse().unwrap();

        self_cache.add_peer(peer_id, [addr1.clone()].iter(), 10, 10);
        other_cache.add_peer(peer_id, [addr2.clone(), addr3.clone()].iter(), 10, 10);

        self_cache.sync(&other_cache, 2, 10); // max 2 addrs per peer

        let (_, addrs) = self_cache.peers.front().unwrap();
        assert_eq!(addrs.len(), 2);
    }

    #[test]
    fn test_sync_truncates_peers_to_max_peers() {
        let mut self_cache = CacheDataLatest::default();
        let other_cache = CacheDataLatest::default();

        // Add 3 peers to self
        let peer1: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let peer2: PeerId = "12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc"
            .parse()
            .unwrap();
        let peer3: PeerId = "12D3KooWSBTB1jzXPyBGpWLMqXfN7MPMNwSsVWCbfkeLXPZr9Dm3"
            .parse()
            .unwrap();

        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();
        let addr3: Multiaddr = "/ip4/127.0.0.3/udp/8080/quic-v1".parse().unwrap();

        self_cache.add_peer(peer1, [addr1].iter(), 10, 10);
        self_cache.add_peer(peer2, [addr2].iter(), 10, 10);
        self_cache.add_peer(peer3, [addr3].iter(), 10, 10);

        self_cache.sync(&other_cache, 10, 2); // max 2 peers

        assert_eq!(self_cache.peers.len(), 2);
    }

    #[test]
    fn test_sync_self_peers_at_front_other_at_back() {
        let mut self_cache = CacheDataLatest::default();
        let mut other_cache = CacheDataLatest::default();

        let self_peer: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let other_peer: PeerId = "12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc"
            .parse()
            .unwrap();

        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();

        self_cache.add_peer(self_peer, [addr1].iter(), 10, 10);
        other_cache.add_peer(other_peer, [addr2].iter(), 10, 10);

        self_cache.sync(&other_cache, 10, 10);

        // Self peer should be at front
        let (front_peer, _) = self_cache.peers.front().unwrap();
        assert_eq!(*front_peer, self_peer);

        // Other peer should be at back
        let (back_peer, _) = self_cache.peers.back().unwrap();
        assert_eq!(*back_peer, other_peer);
    }

    #[test]
    fn test_sync_preserves_other_peer_order() {
        let mut self_cache = CacheDataLatest::default();
        let mut other_cache = CacheDataLatest::default();

        // Add peers to other in specific order
        let peer1: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let peer2: PeerId = "12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc"
            .parse()
            .unwrap();

        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();

        other_cache.add_peer(peer1, [addr1].iter(), 10, 10);
        other_cache.add_peer(peer2, [addr2].iter(), 10, 10);
        // Order in other_cache: [peer2, peer1] (peer2 added last, at front)

        self_cache.sync(&other_cache, 10, 10);

        // Should preserve other's order
        let peers: Vec<_> = self_cache.peers.iter().map(|(id, _)| *id).collect();
        assert_eq!(peers, vec![peer2, peer1]);
    }

    #[test]
    fn test_sync_max_peers_zero_truncates_all() {
        let mut self_cache = CacheDataLatest::default();
        let other_cache = CacheDataLatest::default();

        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        self_cache.add_peer(peer_id, [addr].iter(), 10, 10);

        self_cache.sync(&other_cache, 10, 0); // max 0 peers

        assert!(self_cache.peers.is_empty());
    }

    // CacheData add_peer() tests (5 tests)
    #[test]
    fn test_add_peer_to_empty_cache() {
        let mut cache = CacheDataLatest::default();
        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();

        cache.add_peer(peer_id, [addr.clone()].iter(), 10, 10);

        assert_eq!(cache.peers.len(), 1);
        let (stored_peer, stored_addrs) = cache.peers.front().unwrap();
        assert_eq!(*stored_peer, peer_id);
        assert_eq!(stored_addrs.front().unwrap(), &addr);
    }

    #[test]
    fn test_add_peer_existing_adds_addrs_to_front() {
        let mut cache = CacheDataLatest::default();
        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();

        cache.add_peer(peer_id, [addr1.clone()].iter(), 10, 10);
        cache.add_peer(peer_id, [addr2.clone()].iter(), 10, 10);

        assert_eq!(cache.peers.len(), 1);
        let (_, addrs) = cache.peers.front().unwrap();
        // Newer address should be at front
        assert_eq!(addrs.front().unwrap(), &addr2);
    }

    #[test]
    fn test_add_peer_no_duplicate_addresses() {
        let mut cache = CacheDataLatest::default();
        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();

        cache.add_peer(peer_id, [addr.clone()].iter(), 10, 10);
        cache.add_peer(peer_id, [addr.clone()].iter(), 10, 10);

        let (_, addrs) = cache.peers.front().unwrap();
        assert_eq!(addrs.len(), 1);
    }

    #[test]
    fn test_add_peer_truncates_to_max_addrs() {
        let mut cache = CacheDataLatest::default();
        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();

        let addrs: Vec<Multiaddr> = (1..=5)
            .map(|i| {
                format!("/ip4/127.0.0.{i}/udp/8080/quic-v1")
                    .parse()
                    .unwrap()
            })
            .collect();

        cache.add_peer(peer_id, addrs.iter(), 2, 10); // max 2 addrs

        let (_, stored_addrs) = cache.peers.front().unwrap();
        assert_eq!(stored_addrs.len(), 2);
    }

    #[test]
    fn test_add_peer_evicts_oldest_when_at_max_peers() {
        let mut cache = CacheDataLatest::default();

        // Add 3 peers with max_peers=2
        let peer1: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let peer2: PeerId = "12D3KooWD8QYTnpFcBfACHYmrAFHCRdeH5N6eFg6mes3FKJR8Xbc"
            .parse()
            .unwrap();
        let peer3: PeerId = "12D3KooWSBTB1jzXPyBGpWLMqXfN7MPMNwSsVWCbfkeLXPZr9Dm3"
            .parse()
            .unwrap();

        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();
        let addr3: Multiaddr = "/ip4/127.0.0.3/udp/8080/quic-v1".parse().unwrap();

        cache.add_peer(peer1, [addr1].iter(), 10, 2);
        cache.add_peer(peer2, [addr2].iter(), 10, 2);
        cache.add_peer(peer3, [addr3].iter(), 10, 2);

        assert_eq!(cache.peers.len(), 2);
        // First peer (oldest) should be evicted
        assert!(!cache.peers.iter().any(|(id, _)| *id == peer1));
    }

    // CacheData remove_peer() tests (2 tests)
    #[test]
    fn test_remove_peer_exists() {
        let mut cache = CacheDataLatest::default();
        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();

        cache.add_peer(peer_id, [addr].iter(), 10, 10);
        cache.remove_peer(&peer_id);

        assert!(cache.peers.is_empty());
    }

    #[test]
    fn test_remove_peer_not_exists_no_op() {
        let mut cache = CacheDataLatest::default();
        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();

        // Should not panic
        cache.remove_peer(&peer_id);
        assert!(cache.peers.is_empty());
    }

    // CacheData get_all_addrs() tests (2 tests)
    #[test]
    fn test_get_all_addrs_returns_first_addr_per_peer() {
        let mut cache = CacheDataLatest::default();
        let peer_id: PeerId = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()
            .unwrap();
        let addr1: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/udp/8080/quic-v1".parse().unwrap();

        cache.add_peer(peer_id, [addr1.clone(), addr2.clone()].iter(), 10, 10);

        let all_addrs: Vec<_> = cache.get_all_addrs().collect();
        assert_eq!(all_addrs.len(), 1); // Only first addr per peer
    }

    #[test]
    fn test_get_all_addrs_empty_cache() {
        let cache = CacheDataLatest::default();
        let all_addrs: Vec<_> = cache.get_all_addrs().collect();
        assert!(all_addrs.is_empty());
    }
}
