// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{BootstrapPeer, Error, InitialPeerDiscovery, Result};
use fs2::FileExt;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tempfile::NamedTempFile;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

const PEER_EXPIRY_DURATION: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheData {
    peers: std::collections::HashMap<String, BootstrapPeer>,
    #[serde(default = "SystemTime::now")]
    last_updated: SystemTime,
    #[serde(default = "default_version")]
    version: u32,
}

fn default_version() -> u32 {
    1
}

impl Default for CacheData {
    fn default() -> Self {
        Self {
            peers: std::collections::HashMap::new(),
            last_updated: SystemTime::now(),
            version: default_version(),
        }
    }
}

#[derive(Clone)]
pub struct CacheStore {
    cache_path: PathBuf,
    config: Arc<crate::BootstrapConfig>,
    data: Arc<RwLock<CacheData>>,
}

impl CacheStore {
    pub async fn new(config: crate::BootstrapConfig) -> Result<Self> {
        tracing::info!("Creating new CacheStore with config: {:?}", config);
        let cache_path = config.cache_file_path.clone();
        let config = Arc::new(config);

        // Create cache directory if it doesn't exist
        if let Some(parent) = cache_path.parent() {
            tracing::info!("Attempting to create cache directory at {:?}", parent);
            // Try to create the directory
            match fs::create_dir_all(parent) {
                Ok(_) => {
                    tracing::info!("Successfully created cache directory");
                }
                Err(e) => {
                    tracing::warn!("Failed to create cache directory at {:?}: {}", parent, e);
                    // Try user's home directory as fallback
                    if let Some(home) = dirs::home_dir() {
                        let user_path = home.join(".safe").join("bootstrap_cache.json");
                        tracing::info!("Falling back to user directory: {:?}", user_path);
                        if let Some(user_parent) = user_path.parent() {
                            if let Err(e) = fs::create_dir_all(user_parent) {
                                tracing::error!("Failed to create user cache directory: {}", e);
                                return Err(Error::Io(e));
                            }
                            tracing::info!("Successfully created user cache directory");
                        }
                        let future = Self::new(crate::BootstrapConfig::with_cache_path(user_path));
                        return Box::pin(future).await;
                    }
                }
            }
        }

        let data = if cache_path.exists() {
            tracing::info!("Cache file exists at {:?}, attempting to load", cache_path);
            match Self::load_cache_data(&cache_path).await {
                Ok(data) => {
                    tracing::info!("Successfully loaded cache data with {} peers", data.peers.len());
                    // If cache data exists but has no peers and file is not read-only,
                    // fallback to default
                    let is_readonly = cache_path
                        .metadata()
                        .map(|m| m.permissions().readonly())
                        .unwrap_or(false);

                    if data.peers.is_empty() && !is_readonly {
                        tracing::info!("Cache is empty and not read-only, falling back to default");
                        Self::fallback_to_default(&config).await?
                    } else {
                        // Ensure we don't exceed max_peers
                        let mut filtered_data = data;
                        if filtered_data.peers.len() > config.max_peers {
                            tracing::info!(
                                "Trimming cache from {} to {} peers",
                                filtered_data.peers.len(),
                                config.max_peers
                            );
                            let peers: Vec<_> = filtered_data.peers.into_iter().collect();
                            filtered_data.peers = peers
                                .into_iter()
                                .take(config.max_peers)
                                .collect();
                        }
                        filtered_data
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load cache data: {}", e);
                    // If we can't read or parse the cache file, return empty cache
                    CacheData::default()
                }
            }
        } else {
            tracing::info!("Cache file does not exist at {:?}, falling back to default", cache_path);
            // If cache file doesn't exist, fallback to default
            Self::fallback_to_default(&config).await?
        };

        let store = Self {
            cache_path,
            config,
            data: Arc::new(RwLock::new(data)),
        };

        // Only clean up stale peers if the file is not read-only
        let is_readonly = store
            .cache_path
            .metadata()
            .map(|m| m.permissions().readonly())
            .unwrap_or(false);

        if !is_readonly {
            if let Err(e) = store.cleanup_stale_peers().await {
                tracing::warn!("Failed to clean up stale peers: {}", e);
            }
        }

        tracing::info!("Successfully created CacheStore");
        Ok(store)
    }

    pub async fn new_without_init(config: crate::BootstrapConfig) -> Result<Self> {
        tracing::info!("Creating new CacheStore with config: {:?}", config);
        let cache_path = config.cache_file_path.clone();
        let config = Arc::new(config);

        // Create cache directory if it doesn't exist
        if let Some(parent) = cache_path.parent() {
            tracing::info!("Attempting to create cache directory at {:?}", parent);
            // Try to create the directory
            match fs::create_dir_all(parent) {
                Ok(_) => {
                    tracing::info!("Successfully created cache directory");
                }
                Err(e) => {
                    tracing::warn!("Failed to create cache directory at {:?}: {}", parent, e);
                    // Try user's home directory as fallback
                    if let Some(home) = dirs::home_dir() {
                        let user_path = home.join(".safe").join("bootstrap_cache.json");
                        tracing::info!("Falling back to user directory: {:?}", user_path);
                        if let Some(user_parent) = user_path.parent() {
                            if let Err(e) = fs::create_dir_all(user_parent) {
                                tracing::error!("Failed to create user cache directory: {}", e);
                                return Err(Error::Io(e));
                            }
                            tracing::info!("Successfully created user cache directory");
                        }
                        let future = Self::new_without_init(crate::BootstrapConfig::with_cache_path(user_path));
                        return Box::pin(future).await;
                    }
                }
            }
        }

        let store = Self {
            cache_path,
            config,
            data: Arc::new(RwLock::new(CacheData::default())),
        };

        tracing::info!("Successfully created CacheStore");
        Ok(store)
    }

    pub async fn init(&self) -> Result<()> {
        let mut data = if self.cache_path.exists() {
            tracing::info!("Cache file exists at {:?}, attempting to load", self.cache_path);
            match Self::load_cache_data(&self.cache_path).await {
                Ok(data) => {
                    tracing::info!("Successfully loaded cache data with {} peers", data.peers.len());
                    // If cache data exists but has no peers and file is not read-only,
                    // fallback to default
                    let is_readonly = self.cache_path
                        .metadata()
                        .map(|m| m.permissions().readonly())
                        .unwrap_or(false);

                    if data.peers.is_empty() && !is_readonly {
                        tracing::info!("Cache is empty and not read-only, falling back to default");
                        Self::fallback_to_default(&self.config).await?
                    } else {
                        // Ensure we don't exceed max_peers
                        let mut filtered_data = data;
                        if filtered_data.peers.len() > self.config.max_peers {
                            tracing::info!(
                                "Trimming cache from {} to {} peers",
                                filtered_data.peers.len(),
                                self.config.max_peers
                            );
                            let peers: Vec<_> = filtered_data.peers.into_iter().collect();
                            filtered_data.peers = peers
                                .into_iter()
                                .take(self.config.max_peers)
                                .collect();
                        }
                        filtered_data
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load cache data: {}", e);
                    // If we can't read or parse the cache file, fallback to default
                    Self::fallback_to_default(&self.config).await?
                }
            }
        } else {
            tracing::info!("Cache file does not exist at {:?}, falling back to default", self.cache_path);
            // If cache file doesn't exist, fallback to default
            Self::fallback_to_default(&self.config).await?
        };

        // Only clean up stale peers if the file is not read-only
        let is_readonly = self.cache_path
            .metadata()
            .map(|m| m.permissions().readonly())
            .unwrap_or(false);

        if !is_readonly {
            // Clean up stale peers
            let now = SystemTime::now();
            data.peers.retain(|_, peer| {
                if let Ok(duration) = now.duration_since(peer.last_seen) {
                    duration < PEER_EXPIRY_DURATION
                } else {
                    false
                }
            });
        }

        // Update the store's data
        *self.data.write().await = data;

        Ok(())
    }

    async fn fallback_to_default(config: &crate::BootstrapConfig) -> Result<CacheData> {
        tracing::info!("Falling back to default peers from endpoints");
        let mut data = CacheData {
            peers: std::collections::HashMap::new(),
            last_updated: SystemTime::now(),
            version: default_version(),
        };

        // If no endpoints are configured, just return empty cache
        if config.endpoints.is_empty() {
            tracing::warn!("No endpoints configured, returning empty cache");
            return Ok(data);
        }

        // Try to discover peers from configured endpoints
        let discovery = InitialPeerDiscovery::with_endpoints(config.endpoints.clone());
        match discovery.fetch_peers().await {
            Ok(peers) => {
                tracing::info!("Successfully fetched {} peers from endpoints", peers.len());
                // Only add up to max_peers from the discovered peers
                for peer in peers.into_iter().take(config.max_peers) {
                    data.peers.insert(peer.addr.to_string(), peer);
                }

                // Create parent directory if it doesn't exist
                if let Some(parent) = config.cache_file_path.parent() {
                    tracing::info!("Creating cache directory at {:?}", parent);
                    if let Err(e) = fs::create_dir_all(parent) {
                        tracing::warn!("Failed to create cache directory: {}", e);
                    }
                }

                // Try to write the cache file immediately
                match serde_json::to_string_pretty(&data) {
                    Ok(json) => {
                        tracing::info!("Writing {} peers to cache file", data.peers.len());
                        if let Err(e) = fs::write(&config.cache_file_path, json) {
                            tracing::warn!("Failed to write cache file: {}", e);
                        } else {
                            tracing::info!("Successfully wrote cache file at {:?}", config.cache_file_path);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to serialize cache data: {}", e);
                    }
                }

                Ok(data)
            }
            Err(e) => {
                tracing::warn!("Failed to fetch peers from endpoints: {}", e);
                Ok(data) // Return empty cache on error
            }
        }
    }

    async fn load_cache_data(cache_path: &PathBuf) -> Result<CacheData> {
        // Try to open the file with read permissions
        let mut file = match OpenOptions::new().read(true).open(cache_path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Failed to open cache file: {}", e);
                return Err(Error::from(e));
            }
        };

        // Acquire shared lock for reading
        if let Err(e) = Self::acquire_shared_lock(&file).await {
            tracing::warn!("Failed to acquire shared lock: {}", e);
            return Err(e);
        }

        // Read the file contents
        let mut contents = String::new();
        if let Err(e) = file.read_to_string(&mut contents) {
            tracing::warn!("Failed to read cache file: {}", e);
            return Err(Error::from(e));
        }

        // Parse the cache data
        match serde_json::from_str::<CacheData>(&contents) {
            Ok(data) => Ok(data),
            Err(e) => {
                tracing::warn!("Failed to parse cache data: {}", e);
                Err(Error::Io(io::Error::new(io::ErrorKind::InvalidData, e)))
            }
        }
    }

    pub async fn get_peers(&self) -> Vec<BootstrapPeer> {
        let data = self.data.read().await;
        data.peers.values().cloned().collect()
    }

    pub async fn get_reliable_peers(&self) -> Vec<BootstrapPeer> {
        let data = self.data.read().await;
        let reliable_peers: Vec<_> = data
            .peers
            .values()
            .filter(|peer| peer.success_count > peer.failure_count)
            .cloned()
            .collect();

        // If we have no reliable peers and the cache file is not read-only,
        // try to refresh from default endpoints
        if reliable_peers.is_empty()
            && !self
                .cache_path
                .metadata()
                .map(|m| m.permissions().readonly())
                .unwrap_or(false)
        {
            drop(data);
            if let Ok(new_data) = Self::fallback_to_default(&self.config).await {
                let mut data = self.data.write().await;
                *data = new_data;
                return data
                    .peers
                    .values()
                    .filter(|peer| peer.success_count > peer.failure_count)
                    .cloned()
                    .collect();
            }
        }

        reliable_peers
    }

    pub async fn update_peer_status(&self, addr: &str, success: bool) -> Result<()> {
        // Check if the file is read-only before attempting to modify
        let is_readonly = self
            .cache_path
            .metadata()
            .map(|m| m.permissions().readonly())
            .unwrap_or(false);

        if is_readonly {
            tracing::warn!("Cannot update peer status: cache file is read-only");
            return Ok(());
        }

        let mut data = self.data.write().await;

        match addr.parse::<Multiaddr>() {
            Ok(addr) => {
                let peer = data
                    .peers
                    .entry(addr.to_string())
                    .or_insert_with(|| BootstrapPeer::new(addr));
                peer.update_status(success);
                self.save_to_disk(&data).await?;
                Ok(())
            }
            Err(e) => Err(Error::from(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid multiaddr: {}", e),
            ))),
        }
    }

    pub async fn add_peer(&self, addr: Multiaddr) -> Result<()> {
        let mut data = self.data.write().await;
        let addr_str = addr.to_string();

        // Check if we already have this peer
        if data.peers.contains_key(&addr_str) {
            debug!("Updating existing peer {}", addr_str);
            if let Some(peer) = data.peers.get_mut(&addr_str) {
                peer.last_seen = SystemTime::now();
            }
            return Ok(());
        }

        // If we're at max peers, remove the oldest peer
        if data.peers.len() >= self.config.max_peers {
            debug!("At max peers limit ({}), removing oldest peer", self.config.max_peers);
            if let Some((oldest_addr, _)) = data.peers
                .iter()
                .min_by_key(|(_, peer)| peer.last_seen)
            {
                let oldest_addr = oldest_addr.clone();
                data.peers.remove(&oldest_addr);
            }
        }

        // Add the new peer
        debug!("Adding new peer {} (under max_peers limit)", addr_str);
        data.peers.insert(addr_str, BootstrapPeer::new(addr));

        // Only save to disk if we have a valid cache path
        if !self.cache_path.as_os_str().is_empty() {
            self.save_to_disk(&data).await?;
        }

        Ok(())
    }

    pub async fn remove_peer(&self, addr: &str) -> Result<()> {
        // Check if the file is read-only before attempting to modify
        let is_readonly = self
            .cache_path
            .metadata()
            .map(|m| m.permissions().readonly())
            .unwrap_or(false);

        if is_readonly {
            tracing::warn!("Cannot remove peer: cache file is read-only");
            return Ok(());
        }

        let mut data = self.data.write().await;
        data.peers.remove(addr);
        self.save_to_disk(&data).await?;
        Ok(())
    }

    pub async fn cleanup_unreliable_peers(&self) -> Result<()> {
        // Check if the file is read-only before attempting to modify
        let is_readonly = self
            .cache_path
            .metadata()
            .map(|m| m.permissions().readonly())
            .unwrap_or(false);

        if is_readonly {
            tracing::warn!("Cannot cleanup unreliable peers: cache file is read-only");
            return Ok(());
        }

        let mut data = self.data.write().await;
        let unreliable_peers: Vec<String> = data
            .peers
            .iter()
            .filter(|(_, peer)| !peer.is_reliable())
            .map(|(addr, _)| addr.clone())
            .collect();

        for addr in unreliable_peers {
            data.peers.remove(&addr);
        }

        self.save_to_disk(&data).await?;
        Ok(())
    }

    pub async fn cleanup_stale_peers(&self) -> Result<()> {
        // Check if the file is read-only before attempting to modify
        let is_readonly = self
            .cache_path
            .metadata()
            .map(|m| m.permissions().readonly())
            .unwrap_or(false);

        if is_readonly {
            tracing::warn!("Cannot cleanup stale peers: cache file is read-only");
            return Ok(());
        }

        let mut data = self.data.write().await;
        let stale_peers: Vec<String> = data
            .peers
            .iter()
            .filter(|(_, peer)| {
                // Only remove peers that have failed more times than succeeded
                peer.failure_count > peer.success_count && peer.failure_count >= self.config.max_retries
            })
            .map(|(addr, _)| addr.clone())
            .collect();

        for addr in stale_peers {
            data.peers.remove(&addr);
        }

        // Only save to disk if we have a valid cache path
        if !self.cache_path.as_os_str().is_empty() {
            self.save_to_disk(&data).await?;
        }

        Ok(())
    }

    pub async fn save_to_disk(&self, data: &CacheData) -> Result<()> {
        // Check if the file is read-only before attempting to write
        let is_readonly = self
            .cache_path
            .metadata()
            .map(|m| m.permissions().readonly())
            .unwrap_or(false);

        if is_readonly {
            tracing::warn!("Cannot save to disk: cache file is read-only");
            return Ok(());
        }

        match self.atomic_write(data).await {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!("Failed to save cache to disk: {}", e);
                Err(e)
            }
        }
    }

    async fn acquire_shared_lock(file: &File) -> Result<()> {
        let file = file.try_clone().map_err(Error::from)?;

        tokio::task::spawn_blocking(move || file.try_lock_shared().map_err(Error::from))
            .await
            .map_err(|e| {
                Error::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to spawn blocking task: {}", e),
                ))
            })?
    }

    async fn acquire_exclusive_lock(file: &File) -> Result<()> {
        let mut backoff = Duration::from_millis(10);
        let max_attempts = 5;
        let mut attempts = 0;

        loop {
            match file.try_lock_exclusive() {
                Ok(_) => return Ok(()),
                Err(_) if attempts >= max_attempts => {
                    return Err(Error::LockError);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
                Err(_) => return Err(Error::LockError),
            }
        }
    }

    async fn atomic_write(&self, data: &CacheData) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent).map_err(Error::from)?;
        }

        // Create a temporary file in the same directory as the cache file
        let temp_file = NamedTempFile::new().map_err(Error::from)?;

        // Write data to temporary file
        serde_json::to_writer_pretty(&temp_file, &data).map_err(Error::from)?;

        // Open the target file with proper permissions
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.cache_path)
            .map_err(Error::from)?;

        // Acquire exclusive lock
        Self::acquire_exclusive_lock(&file).await?;

        // Perform atomic rename
        temp_file.persist(&self.cache_path).map_err(|e| {
            Error::from(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to persist cache file: {}", e),
            ))
        })?;

        // Lock will be automatically released when file is dropped
        Ok(())
    }

    /// Clear all peers from the cache
    pub async fn clear_peers(&self) -> Result<()> {
        let mut data = self.data.write().await;
        data.peers.clear();
        Ok(())
    }

    /// Save the current cache to disk
    pub async fn save_cache(&self) -> Result<()> {
        let data = self.data.read().await;
        let temp_file = NamedTempFile::new()?;
        let file = File::create(&temp_file)?;
        file.lock_exclusive()?;

        serde_json::to_writer_pretty(&file, &*data)?;
        file.sync_all()?;
        file.unlock()?;

        // Atomically replace the cache file
        temp_file.persist(&self.cache_path)?;
        info!("Successfully wrote cache file at {:?}", self.cache_path);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_store() -> (CacheStore, PathBuf) {
        let temp_dir = tempdir().unwrap();
        let cache_file = temp_dir.path().join("cache.json");

        let config = crate::BootstrapConfig::new(
            vec![], // Empty endpoints to prevent fallback
            1500,
            cache_file.clone(),
            Duration::from_secs(60),
            Duration::from_secs(10),
            3,
        );

        let store = CacheStore::new(config).await.unwrap();
        (store.clone(), store.cache_path.clone())
    }

    #[tokio::test]
    async fn test_peer_update_and_save() {
        let (store, _) = create_test_store().await;
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();

        // Manually add a peer without using fallback
        {
            let mut data = store.data.write().await;
            data.peers
                .insert(addr.to_string(), BootstrapPeer::new(addr.clone()));
            store.save_to_disk(&data).await.unwrap();
        }

        store
            .update_peer_status(&addr.to_string(), true)
            .await
            .unwrap();

        let peers = store.get_peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].addr, addr);
        assert_eq!(peers[0].success_count, 1);
        assert_eq!(peers[0].failure_count, 0);
    }

    #[tokio::test]
    async fn test_peer_cleanup() {
        let (store, _) = create_test_store().await;
        let good_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        let bad_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8081".parse().unwrap();

        // Add peers
        store.add_peer(good_addr.clone()).await.unwrap();
        store.add_peer(bad_addr.clone()).await.unwrap();

        // Make one peer reliable and one unreliable
        store
            .update_peer_status(&good_addr.to_string(), true)
            .await
            .unwrap();
        
        // Fail the bad peer more times than max_retries
        for _ in 0..5 {
            store
                .update_peer_status(&bad_addr.to_string(), false)
                .await
                .unwrap();
        }

        // Clean up unreliable peers
        store.cleanup_unreliable_peers().await.unwrap();

        // Get all peers (not just reliable ones)
        let peers = store.get_peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].addr, good_addr);
    }

    #[tokio::test]
    async fn test_peer_not_removed_if_successful() {
        let (store, _) = create_test_store().await;
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();

        // Add a peer and make it successful
        store.add_peer(addr.clone()).await.unwrap();
        store.update_peer_status(&addr.to_string(), true).await.unwrap();

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Run cleanup
        store.cleanup_stale_peers().await.unwrap();

        // Verify peer is still there
        let peers = store.get_peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].addr, addr);
    }

    #[tokio::test]
    async fn test_peer_removed_only_when_unresponsive() {
        let (store, _) = create_test_store().await;
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();

        // Add a peer
        store.add_peer(addr.clone()).await.unwrap();

        // Make it fail max_retries times
        for _ in 0..store.config.max_retries {
            store.update_peer_status(&addr.to_string(), false).await.unwrap();
        }

        // Run cleanup
        store.cleanup_stale_peers().await.unwrap();

        // Verify peer is removed
        let peers = store.get_peers().await;
        assert_eq!(peers.len(), 0, "Peer should be removed after max_retries failures");

        // Test with some successes but more failures
        store.add_peer(addr.clone()).await.unwrap();
        store.update_peer_status(&addr.to_string(), true).await.unwrap();
        store.update_peer_status(&addr.to_string(), true).await.unwrap();
        
        for _ in 0..5 {
            store.update_peer_status(&addr.to_string(), false).await.unwrap();
        }

        // Run cleanup
        store.cleanup_stale_peers().await.unwrap();

        // Verify peer is removed due to more failures than successes
        let peers = store.get_peers().await;
        assert_eq!(peers.len(), 0, "Peer should be removed when failures exceed successes");
    }
}
