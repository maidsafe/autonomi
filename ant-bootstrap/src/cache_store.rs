// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{craft_valid_multiaddr, BootstrapCacheConfig, Error, InitialPeersConfig, Result};
use atomic_write_file::AtomicWriteFile;
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    time::SystemTime,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheData {
    pub peers: VecDeque<(PeerId, VecDeque<Multiaddr>)>,
    pub last_updated: SystemTime,
    pub network_version: String,
}

impl CacheData {
    /// Sync the self cache with another cache. This would just add the 'other' state to self.
    pub fn sync(&mut self, other: &CacheData, max_addrs_per_peer: usize, max_peers: usize) {
        for (other_peer, other_addrs) in other.peers.iter() {
            if other_addrs.is_empty() {
                continue;
            }
            for (peer, addrs) in self.peers.iter_mut() {
                if peer == other_peer {
                    for addr in other_addrs.iter() {
                        if !addrs.contains(addr) {
                            addrs.push_back(addr.clone());
                        }
                    }
                    while addrs.len() > max_addrs_per_peer {
                        addrs.pop_front();
                    }
                    break;
                }
            }

            self.peers.push_back((*other_peer, other_addrs.clone()));

            while self.peers.len() > max_peers {
                self.peers.pop_front();
            }
        }

        self.last_updated = SystemTime::now();
    }

    /// Add a peer to the cache data
    pub fn add_peer<'a>(
        &mut self,
        peer_id: PeerId,
        addrs: impl Iterator<Item = &'a Multiaddr>,
        max_addrs_per_peer: usize,
        max_peers: usize,
    ) {
        if let Some((_, present_addrs)) = self.peers.iter_mut().find(|(id, _)| id == &peer_id) {
            for addr in addrs {
                if !present_addrs.contains(addr) {
                    present_addrs.push_back(addr.clone());
                }
            }
            while present_addrs.len() > max_addrs_per_peer {
                present_addrs.pop_front();
            }
        } else {
            self.peers.push_back((
                peer_id,
                addrs
                    .into_iter()
                    .take(max_addrs_per_peer)
                    .cloned()
                    .collect(),
            ));
        }

        while self.peers.len() > max_peers {
            self.peers.pop_front();
        }
    }

    pub fn get_all_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.peers
            .iter()
            .flat_map(|(_, bootstrap_addresses)| bootstrap_addresses.iter().next())
    }
}

impl Default for CacheData {
    fn default() -> Self {
        Self {
            peers: Default::default(),
            last_updated: SystemTime::now(),
            network_version: crate::get_network_version(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BootstrapCacheStore {
    pub(crate) cache_path: PathBuf,
    pub(crate) config: BootstrapCacheConfig,
    pub(crate) data: CacheData,
}

impl BootstrapCacheStore {
    pub fn config(&self) -> &BootstrapCacheConfig {
        &self.config
    }

    /// Create an empty CacheStore with the given configuration
    pub fn new(config: BootstrapCacheConfig) -> Result<Self> {
        info!("Creating new CacheStore with config: {:?}", config);
        let cache_path = config.cache_file_path.clone();

        // Create cache directory if it doesn't exist
        if let Some(parent) = cache_path.parent() {
            if !parent.exists() {
                info!("Attempting to create cache directory at {parent:?}");
                fs::create_dir_all(parent).inspect_err(|err| {
                    warn!("Failed to create cache directory at {parent:?}: {err}");
                })?;
            }
        }

        let store = Self {
            cache_path,
            config,
            data: CacheData::default(),
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
            BootstrapCacheConfig::default_config(init_peers_config.local)?
        };
        if let Some(bootstrap_cache_path) = init_peers_config.get_bootstrap_cache_path()? {
            config.cache_file_path = bootstrap_cache_path;
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

    /// Load cache data from disk
    /// Make sure to have clean addrs inside the cache as we don't call craft_valid_multiaddr
    pub fn load_cache_data(cfg: &BootstrapCacheConfig) -> Result<CacheData> {
        // Try to open the file with read permissions
        let mut file = OpenOptions::new()
            .read(true)
            .open(&cfg.cache_file_path)
            .inspect_err(|err| warn!("Failed to open cache file: {err}",))?;

        // Read the file contents
        let mut contents = String::new();
        file.read_to_string(&mut contents).inspect_err(|err| {
            warn!("Failed to read cache file: {err}");
        })?;

        // Parse the cache data
        let mut data = serde_json::from_str::<CacheData>(&contents).map_err(|err| {
            warn!("Failed to parse cache data: {err}");
            Error::FailedToParseCacheData
        })?;

        while data.peers.len() > cfg.max_peers {
            data.peers.pop_front();
        }

        Ok(data)
    }

    pub fn peer_count(&self) -> usize {
        self.data.peers.len()
    }

    pub fn get_all_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.data.get_all_addrs()
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

    /// Remove a peer from the cache. This does not update the cache on disk.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.data.peers.retain(|(id, _)| id != peer_id);
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
        debug!("Writing cache to disk: {:?}", self.cache_path);
        // Create parent directory if it doesn't exist
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = AtomicWriteFile::options()
            .open(&self.cache_path)
            .inspect_err(|err| {
                error!("Failed to open cache file using AtomicWriteFile: {err}");
            })?;

        let data = serde_json::to_string_pretty(&self.data).inspect_err(|err| {
            error!("Failed to serialize cache data: {err}");
        })?;
        writeln!(file, "{data}")?;
        file.commit().inspect_err(|err| {
            error!("Failed to commit atomic write: {err}");
        })?;

        info!("Cache written to disk: {:?}", self.cache_path);

        Ok(())
    }
}
