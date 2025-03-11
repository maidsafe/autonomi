// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    craft_valid_multiaddr, multiaddr_get_peer_id, BootstrapAddr, BootstrapAddresses,
    BootstrapCacheConfig, Error, InitialPeersConfig, Result,
};
use atomic_write_file::AtomicWriteFile;
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::Entry, HashMap},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheData {
    pub peers: std::collections::HashMap<PeerId, BootstrapAddresses>,
    pub last_updated: SystemTime,
    pub network_version: String,
}

impl CacheData {
    pub fn insert(&mut self, peer_id: PeerId, bootstrap_addr: BootstrapAddr) {
        match self.peers.entry(peer_id) {
            Entry::Occupied(mut occupied_entry) => {
                occupied_entry.get_mut().insert_addr(&bootstrap_addr);
            }
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(BootstrapAddresses(vec![bootstrap_addr]));
            }
        }
    }

    /// Sync the self cache with another cache. This would just add the 'other' state to self.
    pub fn sync(&mut self, other: &CacheData) {
        for (peer, other_addresses_state) in other.peers.iter() {
            let bootstrap_addresses = self
                .peers
                .entry(*peer)
                .or_insert(other_addresses_state.clone());

            trace!("Syncing {peer:?} from other with addrs count: {:?}. Our in memory state count: {:?}", other_addresses_state.0.len(), bootstrap_addresses.0.len());

            bootstrap_addresses.sync(other_addresses_state);
        }

        self.last_updated = SystemTime::now();
    }

    /// Perform cleanup on the Peers
    /// - Removes all the unreliable addrs for a peer
    /// - Removes all the expired addrs for a peer
    /// - Removes all peers with empty addrs set
    /// - Maintains `max_addr` per peer by removing the addr with the lowest success rate
    /// - Maintains `max_peers` in the list by removing the peer with the oldest last_seen
    pub fn perform_cleanup(&mut self, cfg: &BootstrapCacheConfig) {
        self.peers.values_mut().for_each(|bootstrap_addresses| {
            bootstrap_addresses.0.retain(|bootstrap_addr| {
                let now = SystemTime::now();
                let has_not_expired =
                    if let Ok(duration) = now.duration_since(bootstrap_addr.last_seen) {
                        duration < cfg.addr_expiry_duration
                    } else {
                        false
                    };
                bootstrap_addr.is_reliable() && has_not_expired
            })
        });

        self.peers
            .retain(|_, bootstrap_addresses| !bootstrap_addresses.0.is_empty());

        self.peers.values_mut().for_each(|bootstrap_addresses| {
            if bootstrap_addresses.0.len() > cfg.max_addrs_per_peer {
                // sort by lowest failure rate first
                bootstrap_addresses
                    .0
                    .sort_by_key(|addr| addr.failure_rate() as u64);
                bootstrap_addresses.0.truncate(cfg.max_addrs_per_peer);
            }
        });

        self.try_remove_oldest_peers(cfg);
    }

    /// Remove the oldest peers until we're under the max_peers limit
    pub fn try_remove_oldest_peers(&mut self, cfg: &BootstrapCacheConfig) {
        if self.peers.len() > cfg.max_peers {
            let mut peer_last_seen_map = HashMap::new();
            for (peer, addrs) in self.peers.iter() {
                let mut latest_seen = Duration::from_secs(u64::MAX);
                for addr in addrs.0.iter() {
                    if let Ok(elapsed) = addr.last_seen.elapsed() {
                        trace!("Time elapsed for {addr:?} is {elapsed:?}");
                        if elapsed < latest_seen {
                            trace!("Updating latest_seen to {elapsed:?}");
                            latest_seen = elapsed;
                        }
                    }
                }
                trace!("Last seen for {peer:?} is {latest_seen:?}");
                peer_last_seen_map.insert(*peer, latest_seen);
            }

            while self.peers.len() > cfg.max_peers {
                // find the peer with the largest last_seen
                if let Some((&oldest_peer, last_seen)) = peer_last_seen_map
                    .iter()
                    .max_by_key(|(_, last_seen)| **last_seen)
                {
                    debug!("Found the oldest peer to remove: {oldest_peer:?} with last_seen of {last_seen:?}");
                    self.peers.remove(&oldest_peer);
                    peer_last_seen_map.remove(&oldest_peer);
                }
            }
        }
    }
}

impl Default for CacheData {
    fn default() -> Self {
        Self {
            peers: std::collections::HashMap::new(),
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

        let store = Self::new(config)?;

        // If it is the first node, clear the cache.
        if init_peers_config.first {
            info!("First node in network, writing empty cache to disk");
            store.write()?;
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

        data.perform_cleanup(cfg);

        Ok(data)
    }

    pub fn peer_count(&self) -> usize {
        self.data.peers.len()
    }

    pub fn get_all_addrs(&self) -> impl Iterator<Item = &BootstrapAddr> {
        self.data
            .peers
            .values()
            .flat_map(|bootstrap_addresses| bootstrap_addresses.0.iter())
    }

    /// Get a list containing single addr per peer. We use the least faulty addr for each peer.
    /// This list is sorted by the failure rate of the addr.
    pub fn get_sorted_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        let mut addrs = self
            .data
            .peers
            .values()
            .flat_map(|bootstrap_addresses| bootstrap_addresses.get_least_faulty())
            .collect::<Vec<_>>();

        addrs.sort_by_key(|addr| addr.failure_rate() as u64);

        addrs.into_iter().map(|addr| &addr.addr)
    }

    /// Update the status of an addr in the cache. The peer must be added to the cache first.
    pub fn update_addr_status(&mut self, addr: &Multiaddr, success: bool) {
        if let Some(peer_id) = multiaddr_get_peer_id(addr) {
            debug!("Updating addr status: {addr} (success: {success})");
            if let Some(bootstrap_addresses) = self.data.peers.get_mut(&peer_id) {
                bootstrap_addresses.update_addr_status(addr, success);
            } else {
                debug!("Peer not found in cache to update: {addr}");
            }
        }
    }

    /// Add a set of addresses to the cache.
    pub fn add_addr(&mut self, addr: Multiaddr) {
        debug!("Trying to add new addr: {addr}");
        let Some(addr) = craft_valid_multiaddr(&addr, false) else {
            return;
        };
        let peer_id = match addr.iter().find(|p| matches!(p, Protocol::P2p(_))) {
            Some(Protocol::P2p(id)) => id,
            _ => return,
        };

        // Check if we already have this peer
        if let Some(bootstrap_addrs) = self.data.peers.get_mut(&peer_id) {
            if let Some(bootstrap_addr) = bootstrap_addrs.get_addr_mut(&addr) {
                debug!("Updating existing peer's last_seen {addr}");
                bootstrap_addr.last_seen = SystemTime::now();
                return;
            } else {
                let mut bootstrap_addr = BootstrapAddr::new(addr.clone());
                bootstrap_addr.success_count = 1;
                bootstrap_addrs.insert_addr(&bootstrap_addr);
            }
        } else {
            let mut bootstrap_addr = BootstrapAddr::new(addr.clone());
            bootstrap_addr.success_count = 1;
            self.data
                .peers
                .insert(peer_id, BootstrapAddresses(vec![bootstrap_addr]));
        }

        debug!("Added new peer {addr:?}, performing cleanup of old addrs");
        self.perform_cleanup();
    }

    /// Remove a single address for a peer.
    pub fn remove_addr(&mut self, addr: &Multiaddr) {
        if let Some(peer_id) = multiaddr_get_peer_id(addr) {
            if let Some(bootstrap_addresses) = self.data.peers.get_mut(&peer_id) {
                bootstrap_addresses.remove_addr(addr);
            } else {
                debug!("Peer {peer_id:?} not found in the cache. Not removing addr: {addr:?}")
            }
        } else {
            debug!("Could not obtain PeerId for {addr:?}, not removing addr from cache.");
        }
    }

    pub fn perform_cleanup(&mut self) {
        self.data.perform_cleanup(&self.config);
    }

    /// Flush the cache to disk after syncing with the CacheData from the file.
    /// Do not perform cleanup when `data` is fetched from the network. The SystemTime might not be accurate.
    pub fn sync_and_flush_to_disk(&mut self, with_cleanup: bool) -> Result<()> {
        if self.config.disable_cache_writing {
            info!("Cache writing is disabled, skipping sync to disk");
            return Ok(());
        }

        info!(
            "Flushing cache to disk, with data containing: {} peers",
            self.data.peers.len(),
        );

        if let Ok(data_from_file) = Self::load_cache_data(&self.config) {
            self.data.sync(&data_from_file);
        } else {
            warn!("Failed to load cache data from file, overwriting with new data");
        }

        if with_cleanup {
            self.data.perform_cleanup(&self.config);
            self.data.try_remove_oldest_peers(&self.config);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_store() -> (BootstrapCacheStore, PathBuf) {
        let temp_dir = tempdir().unwrap();
        let cache_file = temp_dir.path().join("cache.json");

        let config = crate::BootstrapCacheConfig::empty().with_cache_path(&cache_file);

        let store = BootstrapCacheStore::new(config).unwrap();
        (store.clone(), store.cache_path.clone())
    }

    #[tokio::test]
    async fn test_peer_cleanup() {
        let (mut store, _) = create_test_store().await;
        let good_addr: Multiaddr =
            "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                .parse()
                .unwrap();
        let bad_addr: Multiaddr =
            "/ip4/127.0.0.1/tcp/8081/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
                .parse()
                .unwrap();

        // Add peers
        store.add_addr(good_addr.clone());
        store.add_addr(bad_addr.clone());

        // Make one peer reliable and one unreliable
        store.update_addr_status(&good_addr, true);

        // Fail the bad peer more times than max_retries
        for _ in 0..5 {
            store.update_addr_status(&bad_addr, false);
        }

        // Clean up unreliable peers
        store.perform_cleanup();

        // Get all peers (not just reliable ones)
        let peers = store.get_all_addrs().collect::<Vec<_>>();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].addr, good_addr);
    }

    #[tokio::test]
    async fn test_peer_not_removed_if_successful() {
        let (mut store, _) = create_test_store().await;
        let addr: Multiaddr =
            "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                .parse()
                .unwrap();

        // Add a peer and make it successful
        store.add_addr(addr.clone());
        store.update_addr_status(&addr, true);

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Run cleanup
        store.perform_cleanup();

        // Verify peer is still there
        let peers = store.get_all_addrs().collect::<Vec<_>>();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].addr, addr);
    }
}
