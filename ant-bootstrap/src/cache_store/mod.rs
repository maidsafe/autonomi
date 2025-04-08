// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod cache_data_v0;
pub mod cache_data_v1;

use crate::{BootstrapCacheConfig, Error, Result, craft_valid_multiaddr};
use libp2p::{Multiaddr, PeerId, multiaddr::Protocol};
use rand::Rng;
use std::{fs, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::Instrument;

pub type CacheDataLatest = cache_data_v1::CacheData;
pub const CACHE_DATA_VERSION_LATEST: u32 = cache_data_v1::CacheData::CACHE_DATA_VERSION;

#[derive(Clone, Debug)]
pub struct BootstrapCacheStore {
    pub(crate) config: Arc<BootstrapCacheConfig>,
    pub(crate) data: Arc<RwLock<CacheDataLatest>>,
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
        };

        Ok(store)
    }

    pub async fn peer_count(&self) -> usize {
        self.data.read().await.peers.len()
    }

    pub async fn get_all_addrs(&self) -> Vec<Multiaddr> {
        self.data.read().await.get_all_addrs().cloned().collect()
    }

    /// Remove a peer from the cache. This does not update the cache on disk.
    pub async fn remove_peer(&self, peer_id: &PeerId) {
        self.data
            .write()
            .await
            .peers
            .retain(|(id, _)| id != peer_id);
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
                    data.peers.pop_front();
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
                    data.peers.pop_front();
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
    pub async fn sync_and_flush_to_disk(&self) -> Result<()> {
        if self.config.disable_cache_writing {
            info!("Cache writing is disabled, skipping sync to disk");
            return Ok(());
        }

        if self.data.peers.is_empty() {
            info!("No peers to write to disk, skipping sync to disk");
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
    if random_adjustment.as_secs() % 2 == 0 {
        duration - random_adjustment
    } else {
        duration + random_adjustment
    }
}

#[cfg(test)]
mod tests {
    use super::duration_with_variance;
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
}
