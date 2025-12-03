// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_releases::AutonomiReleaseInfo;
use serde::{Deserialize, Serialize};

use super::Result;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

const CACHE_TTL_SECS: u64 = 3600; // 1 hour

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedReleaseInfo {
    pub release_info: AutonomiReleaseInfo,
    pub cached_at: SystemTime,
}

impl CachedReleaseInfo {
    /// Check if the cache is still valid based on TTL
    pub fn is_valid(&self) -> bool {
        match self.cached_at.elapsed() {
            Ok(elapsed) => elapsed < Duration::from_secs(CACHE_TTL_SECS),
            Err(_) => {
                // SystemTime went backwards (clock adjustment)
                warn!("Clock skew detected - invalidating cache");
                false
            }
        }
    }
}

/// Get the path to the release info cache file
///
/// The optional directory is for testing purposes.
pub fn get_release_cache_path(cache_path: Option<&Path>) -> Result<PathBuf> {
    let upgrade_path = match cache_path {
        Some(dir) => dir.to_path_buf(),
        None => super::get_upgrade_dir_path()?,
    };
    Ok(upgrade_path.join("release_info_cache.json"))
}

/// Get the path to the release cache lock file
///
/// The optional directory is for testing purposes.
fn get_release_cache_lock_path(cache_path: Option<&Path>) -> Result<PathBuf> {
    let upgrade_path = match cache_path {
        Some(dir) => dir.to_path_buf(),
        None => super::get_upgrade_dir_path()?,
    };
    Ok(upgrade_path.join(".release_info.lock"))
}

/// Acquire exclusive lock on the release cache
fn acquire_release_cache_lock(lock_path: &Path) -> Result<File> {
    super::acquire_exclusive_lock(lock_path, "Release cache lock")
}

/// Read cached release info from disk
///
/// The optional directory is for testing purposes.
pub fn read_cached_release_info(cache_path: Option<&Path>) -> Result<Option<CachedReleaseInfo>> {
    let cache_path = get_release_cache_path(cache_path)?;

    if !cache_path.exists() {
        debug!("Release cache file does not exist");
        return Ok(None);
    }

    let mut file = File::open(&cache_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    match serde_json::from_str::<CachedReleaseInfo>(&contents) {
        Ok(cached) => {
            if cached.is_valid() {
                info!(
                    "Valid release cache found (age: {:?})",
                    cached.cached_at.elapsed().unwrap_or_default()
                );
                Ok(Some(cached))
            } else {
                info!("Release cache expired");
                Ok(None)
            }
        }
        Err(e) => {
            warn!("Failed to parse release cache: {}", e);
            // Delete corrupted cache
            let _ = fs::remove_file(&cache_path);
            Ok(None)
        }
    }
}

/// Write release info to cache with proper locking
///
/// The optional directory is for testing purposes.
pub fn write_cached_release_info(
    release_info: &AutonomiReleaseInfo,
    cache_dir_path: Option<&Path>,
) -> Result<()> {
    let cache_path = get_release_cache_path(cache_dir_path)?;
    let lock_path = get_release_cache_lock_path(cache_dir_path)?;

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let _lock = acquire_release_cache_lock(&lock_path)?;

    #[allow(clippy::suspicious_open_options)]
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&cache_path)?;

    let cached = CachedReleaseInfo {
        release_info: release_info.clone(),
        cached_at: SystemTime::now(),
    };

    let json = serde_json::to_string_pretty(&cached)?;

    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(json.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_all()?;

    info!("Release cache written to disk: {}", cache_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_releases::{BinaryInfo, Platform, PlatformBinaries};
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn create_test_release_info() -> AutonomiReleaseInfo {
        AutonomiReleaseInfo {
            commit_hash: "abc123def456".to_string(),
            name: "v0.1.0".to_string(),
            platform_binaries: vec![PlatformBinaries {
                platform: Platform::LinuxMusl,
                binaries: vec![BinaryInfo {
                    name: "antnode".to_string(),
                    version: "0.1.0".to_string(),
                    sha256: "deadbeef".to_string(),
                }],
            }],
        }
    }

    #[test]
    fn test_cache_write_and_read() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let cache_dir = temp_dir.path();

        let release_info = create_test_release_info();

        write_cached_release_info(&release_info, Some(cache_dir))?;

        let cached = read_cached_release_info(Some(cache_dir))?.expect("Cache should exist");

        assert_eq!(cached.release_info.commit_hash, "abc123def456");
        assert_eq!(cached.release_info.name, "v0.1.0");
        assert!(cached.is_valid());

        Ok(())
    }

    #[test]
    fn test_cache_expiration() {
        let cached = CachedReleaseInfo {
            release_info: create_test_release_info(),
            cached_at: SystemTime::now() - Duration::from_secs(CACHE_TTL_SECS + 1),
        };

        assert!(!cached.is_valid(), "Cache should be expired");
    }

    #[test]
    fn test_concurrent_cache_access() -> Result<()> {
        const THREAD_COUNT: usize = 10;
        const ITERATIONS_PER_THREAD: usize = 5;

        let temp_dir = tempfile::tempdir()?;
        let cache_dir = temp_dir.path().to_path_buf();

        let barrier = Arc::new(Barrier::new(THREAD_COUNT + 1));
        let mut handles = Vec::with_capacity(THREAD_COUNT);

        let release_info = create_test_release_info();
        write_cached_release_info(&release_info, Some(&cache_dir))?;

        for thread_id in 0..THREAD_COUNT {
            let barrier_clone = Arc::clone(&barrier);
            let cache_dir_clone = cache_dir.clone();

            handles.push(thread::spawn(move || {
                barrier_clone.wait();

                for i in 0..ITERATIONS_PER_THREAD {
                    if thread_id % 2 == 0 {
                        let _ = read_cached_release_info(Some(&cache_dir_clone));
                    } else {
                        let mut info = create_test_release_info();
                        info.commit_hash = format!("commit_{thread_id}_{i}");
                        let _ = write_cached_release_info(&info, Some(&cache_dir_clone));
                    }
                }
            }));
        }

        barrier.wait();

        for handle in handles {
            handle.join().expect("Thread should complete successfully");
        }

        let final_cached = read_cached_release_info(Some(&cache_dir))?;
        assert!(
            final_cached.is_some(),
            "Cache should exist after concurrent access"
        );

        Ok(())
    }

    #[test]
    fn test_corrupted_cache_recovery() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let cache_dir = temp_dir.path();
        let cache_path = get_release_cache_path(Some(cache_dir))?;

        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&cache_path, b"{ invalid json }")?;

        let result = read_cached_release_info(Some(cache_dir))?;
        assert!(result.is_none(), "Corrupted cache should return None");
        assert!(!cache_path.exists(), "Corrupted cache should be deleted");

        Ok(())
    }
}
