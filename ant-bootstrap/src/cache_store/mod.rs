// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod cache_data_v0;
pub mod cache_data_v1;

use crate::{craft_valid_multiaddr, BootstrapCacheConfig, Error, InitialPeersConfig, Result};
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use std::fs;

pub type CacheDataLatest = cache_data_v1::CacheData;
pub const CACHE_DATA_VERSION_LATEST: u32 = cache_data_v1::CacheData::CACHE_DATA_VERSION;

#[derive(Clone, Debug)]
pub struct BootstrapCacheStore {
    pub(crate) config: BootstrapCacheConfig,
    pub(crate) data: CacheDataLatest,
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
            config,
            data: CacheDataLatest::default(),
        };

        Ok(store)
    }

    /// Create an empty CacheStore from the given Initial Peers Configuration.
    /// This also modifies the `BootstrapCacheConfig` if provided based on the `InitialPeersConfig`.
    /// And also performs some actions based on the `InitialPeersConfig`.
    ///
    /// `InitialPeersConfig::bootstrap_cache_dir` will take precedence over the path provided inside `config`.
    pub fn new_from_initial_peers_config(
        init_peers_config: &InitialPeersConfig,
        config: Option<BootstrapCacheConfig>,
    ) -> Result<Self> {
        let mut config = if let Some(cfg) = config {
            cfg
        } else {
            BootstrapCacheConfig::new(init_peers_config.local)?
        };

        if let Some(cache_dir) = &init_peers_config.bootstrap_cache_dir {
            config.cache_dir = cache_dir.clone();
        }

        let mut store = Self::new(config)?;

        // If it is the first node, clear the cache.
        if init_peers_config.first {
            info!("First node in network, writing empty cache to disk");
            store.write()?;
        } else {
            info!("Flushing cache to disk on init.");
            store.sync_and_flush_to_disk()?;
        }

        Ok(store)
    }

    pub fn peer_count(&self) -> usize {
        self.data.peers.len()
    }

    pub fn get_all_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.data.get_all_addrs()
    }

    /// Remove a peer from the cache. This does not update the cache on disk.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.data.peers.retain(|(id, _)| id != peer_id);
    }

    /// Add an address to the cache
    pub fn add_addr(&mut self, addr: Multiaddr) {
        let Some(addr) = craft_valid_multiaddr(&addr, false) else {
            return;
        };
        let peer_id = match addr.iter().find(|p| matches!(p, Protocol::P2p(_))) {
            Some(Protocol::P2p(id)) => id,
            _ => return,
        };
        if addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
            return;
        }

        debug!("Adding addr to bootstrap cache: {addr}");

        self.data.add_peer(
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
    /// Do not perform cleanup when `data` is fetched from the network. The SystemTime might not be accurate.
    pub fn sync_and_flush_to_disk(&mut self) -> Result<()> {
        if self.config.disable_cache_writing {
            info!("Cache writing is disabled, skipping sync to disk");
            return Ok(());
        }

        info!(
            "Flushing cache to disk, with data containing: {} peers",
            self.data.peers.len(),
        );

        if let Ok(data_from_file) = Self::load_cache_data(&self.config) {
            self.data.sync(
                &data_from_file,
                self.config.max_addrs_per_peer,
                self.config.max_peers,
            );
        } else {
            warn!("Failed to load cache data from file, overwriting with new data");
        }

        self.write().inspect_err(|e| {
            error!("Failed to save cache to disk: {e}");
        })?;

        // Flush after writing
        self.data.peers.clear();

        Ok(())
    }

    /// Write the cache to disk atomically. This will overwrite the existing cache file, use sync_and_flush_to_disk to
    /// sync with the file first.
    pub fn write(&self) -> Result<()> {
        if self.config.disable_cache_writing {
            info!("Cache writing is disabled, skipping sync to disk");
            return Ok(());
        }

        let filename = Self::cache_file_name(self.config.local);

        self.data.write_to_file(&self.config.cache_dir, &filename)?;

        if self.config.backwards_compatible_writes {
            cache_data_v0::CacheData::from(&self.data)
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
}
