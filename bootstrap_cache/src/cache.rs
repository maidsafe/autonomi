// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{BootstrapCache, Error};
use fs2::FileExt;
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::PathBuf,
};
use tracing::{debug, error, info, warn};

/// Manages reading and writing of the bootstrap cache file
pub struct CacheManager {
    cache_path: PathBuf,
}

impl CacheManager {
    /// Creates a new CacheManager instance
    pub fn new() -> Result<Self, Error> {
        let cache_path = Self::get_cache_path()?;
        Ok(Self { cache_path })
    }

    /// Returns the platform-specific cache file path
    fn get_cache_path() -> io::Result<PathBuf> {
        let path = if cfg!(target_os = "macos") {
            PathBuf::from("/Library/Application Support/Safe/bootstrap_cache.json")
        } else if cfg!(target_os = "linux") {
            PathBuf::from("/var/safe/bootstrap_cache.json")
        } else if cfg!(target_os = "windows") {
            PathBuf::from(r"C:\ProgramData\Safe\bootstrap_cache.json")
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Unsupported operating system",
            ));
        };

        // Try to create the directory structure
        if let Some(parent) = path.parent() {
            info!("Ensuring cache directory exists at: {:?}", parent);
            match fs::create_dir_all(parent) {
                Ok(_) => {
                    debug!("Successfully created/verified cache directory");
                    // Try to set directory permissions to be user-writable
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Err(e) = fs::set_permissions(parent, fs::Permissions::from_mode(0o755)) {
                            warn!("Failed to set cache directory permissions: {}", e);
                        }
                    }
                }
                Err(e) => {
                    // If we can't create in system directory, fall back to user's home directory
                    warn!("Failed to create system cache directory: {}", e);
                    if let Some(home) = dirs::home_dir() {
                        let user_path = home.join(".safe").join("bootstrap_cache.json");
                        info!("Falling back to user directory: {:?}", user_path);
                        if let Some(user_parent) = user_path.parent() {
                            fs::create_dir_all(user_parent)?;
                        }
                        return Ok(user_path);
                    }
                }
            }
        }
        Ok(path)
    }

    /// Reads the cache file with file locking, handling potential corruption
    pub fn read_cache(&self) -> Result<BootstrapCache, Error> {
        debug!("Reading bootstrap cache from {:?}", self.cache_path);
        
        let mut file = match File::open(&self.cache_path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                info!("Cache file not found, creating new empty cache");
                return Ok(BootstrapCache::new());
            }
            Err(e) => {
                error!("Failed to open cache file: {}", e);
                return Err(e.into());
            }
        };

        // Acquire shared lock for reading
        file.lock_shared().map_err(|e| {
            error!("Failed to acquire shared lock: {}", e);
            Error::LockError
        })?;

        let mut contents = String::new();
        if let Err(e) = file.read_to_string(&mut contents) {
            error!("Failed to read cache file: {}", e);
            // Release lock before returning
            let _ = file.unlock();
            return Err(Error::Io(e));
        }

        // Release lock
        file.unlock().map_err(|e| {
            error!("Failed to release lock: {}", e);
            Error::LockError
        })?;

        // Try to parse the cache, if it fails it might be corrupted
        match serde_json::from_str(&contents) {
            Ok(cache) => Ok(cache),
            Err(e) => {
                error!("Cache file appears to be corrupted: {}", e);
                Err(Error::CacheCorrupted(e))
            }
        }
    }

    /// Rebuilds the cache using provided peers or fetches new ones if none provided
    pub async fn rebuild_cache(&self, peers: Option<Vec<BootstrapPeer>>) -> Result<BootstrapCache, Error> {
        info!("Rebuilding bootstrap cache");
        
        let cache = if let Some(peers) = peers {
            info!("Rebuilding cache with {} in-memory peers", peers.len());
            BootstrapCache {
                last_updated: chrono::Utc::now(),
                peers,
            }
        } else {
            info!("No in-memory peers available, fetching from endpoints");
            let discovery = InitialPeerDiscovery::new();
            let peers = discovery.fetch_peers().await?;
            BootstrapCache {
                last_updated: chrono::Utc::now(),
                peers,
            }
        };

        // Write the rebuilt cache
        self.write_cache(&cache)?;
        Ok(cache)
    }

    /// Writes the cache file with file locking and atomic replacement
    pub fn write_cache(&self, cache: &BootstrapCache) -> Result<(), Error> {
        debug!("Writing bootstrap cache to {:?}", self.cache_path);
        
        let temp_path = self.cache_path.with_extension("tmp");
        let mut file = File::create(&temp_path).map_err(|e| {
            error!("Failed to create temporary cache file: {}", e);
            Error::Io(e)
        })?;

        // Acquire exclusive lock for writing
        file.lock_exclusive().map_err(|e| {
            error!("Failed to acquire exclusive lock: {}", e);
            Error::LockError
        })?;

        let contents = serde_json::to_string_pretty(cache).map_err(|e| {
            error!("Failed to serialize cache: {}", e);
            Error::Json(e)
        })?;

        file.write_all(contents.as_bytes()).map_err(|e| {
            error!("Failed to write cache file: {}", e);
            Error::Io(e)
        })?;

        file.sync_all().map_err(|e| {
            error!("Failed to sync cache file: {}", e);
            Error::Io(e)
        })?;

        // Release lock
        file.unlock().map_err(|e| {
            error!("Failed to release lock: {}", e);
            Error::LockError
        })?;

        // Atomic rename
        fs::rename(&temp_path, &self.cache_path).map_err(|e| {
            error!("Failed to rename temporary cache file: {}", e);
            Error::Io(e)
        })?;

        info!("Successfully wrote cache file");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::fs::OpenOptions;
    use tempfile::tempdir;
    use tokio;

    #[test]
    fn test_cache_read_write() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("test_cache.json");

        let cache = BootstrapCache {
            last_updated: Utc::now(),
            peers: vec![],
        };

        let manager = CacheManager { cache_path };
        manager.write_cache(&cache).unwrap();

        let read_cache = manager.read_cache().unwrap();
        assert_eq!(cache.peers.len(), read_cache.peers.len());
    }

    #[test]
    fn test_missing_cache_file() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("nonexistent.json");

        let manager = CacheManager { cache_path };
        let cache = manager.read_cache().unwrap();
        assert!(cache.peers.is_empty());
    }

    #[test]
    fn test_corrupted_cache_file() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("corrupted.json");

        // Write corrupted JSON
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&cache_path)
            .unwrap();
        file.write_all(b"{invalid json}").unwrap();

        let manager = CacheManager { cache_path };
        match manager.read_cache() {
            Err(Error::CacheCorrupted(_)) => (),
            other => panic!("Expected CacheCorrupted error, got {:?}", other),
        }
    }

    #[test]
    fn test_partially_corrupted_cache() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("partial_corrupt.json");

        // Write partially valid JSON
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&cache_path)
            .unwrap();
        file.write_all(b"{\"last_updated\":\"2024-01-01T00:00:00Z\",\"peers\":[{}]}").unwrap();

        let manager = CacheManager { cache_path };
        match manager.read_cache() {
            Err(Error::CacheCorrupted(_)) => (),
            other => panic!("Expected CacheCorrupted error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_rebuild_cache_with_memory_peers() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("rebuild.json");
        let manager = CacheManager { cache_path };

        // Create some test peers
        let test_peers = vec![
            BootstrapPeer {
                addr: "/ip4/127.0.0.1/tcp/8080".parse().unwrap(),
                success_count: 1,
                failure_count: 0,
                last_success: Some(Utc::now()),
                last_failure: None,
            }
        ];

        // Rebuild cache with in-memory peers
        let rebuilt = manager.rebuild_cache(Some(test_peers.clone())).await.unwrap();
        assert_eq!(rebuilt.peers.len(), 1);
        assert_eq!(rebuilt.peers[0].addr, test_peers[0].addr);

        // Verify the cache was written to disk
        let read_cache = manager.read_cache().unwrap();
        assert_eq!(read_cache.peers.len(), 1);
        assert_eq!(read_cache.peers[0].addr, test_peers[0].addr);
    }

    #[tokio::test]
    async fn test_rebuild_cache_from_endpoints() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("rebuild_endpoints.json");
        let manager = CacheManager { cache_path };

        // Write corrupted cache first
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&cache_path)
            .unwrap();
        file.write_all(b"{corrupted}").unwrap();

        // Verify corrupted cache is detected
        match manager.read_cache() {
            Err(Error::CacheCorrupted(_)) => (),
            other => panic!("Expected CacheCorrupted error, got {:?}", other),
        }

        // Mock the InitialPeerDiscovery for testing
        // Note: In a real implementation, you might want to use a trait for InitialPeerDiscovery
        // and mock it properly. This test will actually try to fetch from real endpoints.
        match manager.rebuild_cache(None).await {
            Ok(cache) => {
                // Verify the cache was rebuilt and written
                let read_cache = manager.read_cache().unwrap();
                assert_eq!(read_cache.peers.len(), cache.peers.len());
            }
            Err(Error::NoPeersFound(_)) => {
                // This is also acceptable if no endpoints are reachable during test
                ()
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_concurrent_cache_access() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("concurrent.json");
        let manager = CacheManager { cache_path.clone() };

        // Initial cache
        let cache = BootstrapCache {
            last_updated: Utc::now(),
            peers: vec![],
        };
        manager.write_cache(&cache).unwrap();

        // Try to read while holding write lock
        let file = OpenOptions::new()
            .write(true)
            .open(&cache_path)
            .unwrap();
        file.lock_exclusive().unwrap();

        // This should fail with a lock error
        match manager.read_cache() {
            Err(Error::LockError) => (),
            other => panic!("Expected LockError, got {:?}", other),
        }

        // Release lock
        file.unlock().unwrap();
    }

    #[test]
    fn test_cache_file_permissions() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("permissions.json");
        let manager = CacheManager { cache_path: cache_path.clone() };

        // Write initial cache
        let cache = BootstrapCache {
            last_updated: Utc::now(),
            peers: vec![],
        };
        manager.write_cache(&cache).unwrap();

        // Make file read-only
        let mut perms = fs::metadata(&cache_path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&cache_path, perms).unwrap();

        // Try to write to read-only file
        match manager.write_cache(&cache) {
            Err(Error::Io(_)) => (),
            other => panic!("Expected Io error, got {:?}", other),
        }
    }
}
