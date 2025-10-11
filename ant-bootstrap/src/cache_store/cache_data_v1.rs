// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Error;

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheData {
    pub peers: VecDeque<(PeerId, VecDeque<Multiaddr>)>,
    pub last_updated: SystemTime,
    pub network_version: String,
    pub cache_version: String,
}

impl CacheData {
    /// The version of the cache data format
    /// This has to be bumped whenever the cache data format changes to ensure compatibility.
    pub const CACHE_DATA_VERSION: u32 = 1;

    /// Sync the self cache with another cache. Self peers (newer) are preserved by pushing
    /// other peers to the back.
    pub fn sync(&mut self, other: &CacheData, max_addrs_per_peer: usize, max_peers: usize) {
        let old_len = self.peers.len();
        let mut other_peers = other.peers.iter().cloned().collect::<HashMap<_, _>>();

        for (peer, addrs) in self.peers.iter_mut() {
            if let Some(other_addrs) = other_peers.get(peer) {
                for other_addr in other_addrs.iter() {
                    // push the other addr to the back if we don't already have it
                    if !addrs.contains(other_addr) {
                        addrs.push_back(other_addr.clone());
                    }
                }
                // remove excess addrs from the back (oldest) if we exceed max_addrs_per_peer
                while addrs.len() > max_addrs_per_peer {
                    addrs.pop_back();
                }
            }
            // remove from other_peers to avoid re-processing
            other_peers.remove(peer);
        }

        // Apply max_peers limit by removing from back (oldest from other)
        while self.peers.len() > max_peers {
            self.peers.pop_back();
        }

        // Now add any remaining peers from other_peers
        let required_len = max_peers.saturating_sub(self.peers.len());
        let other_len = other_peers.len();
        let other_peers = other_peers.into_iter().take(required_len);
        self.peers.extend(other_peers);

        let new_len = self.peers.len();

        info!(
            "Synced {other_len} peers to our current {old_len:?} peers to have a final count of {new_len:?} peers"
        );

        self.last_updated = SystemTime::now();
    }

    /// Add a peer to front of the cache as the newest, pruning old from tail
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
                    present_addrs.push_front(addr.clone());
                }
            }
            while present_addrs.len() > max_addrs_per_peer {
                present_addrs.pop_back();
            }
        } else {
            self.peers.push_front((
                peer_id,
                addrs
                    .into_iter()
                    .take(max_addrs_per_peer)
                    .cloned()
                    .collect(),
            ));
        }

        while self.peers.len() > max_peers {
            self.peers.pop_back();
        }
    }

    /// Remove a peer from the cache. This does not update the cache on disk.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.retain(|(id, _)| id != peer_id);
    }

    pub fn get_all_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.peers
            .iter()
            .flat_map(|(_, bootstrap_addresses)| bootstrap_addresses.iter().next())
    }

    fn lock_with_retry<F, L>(mut operation: F, mut log_failure: L) -> std::io::Result<()>
    where
        F: FnMut() -> std::io::Result<()>,
        L: FnMut(&std::io::Error, usize, usize),
    {
        const MAX_ATTEMPTS: usize = 3;
        const RETRY_DELAY_MS: u64 = 50;

        for attempt in 1..=MAX_ATTEMPTS {
            match operation() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    log_failure(&err, attempt, MAX_ATTEMPTS);
                    if attempt == MAX_ATTEMPTS {
                        return Err(err);
                    }

                    thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                }
            }
        }

        Ok(())
    }

    pub fn read_from_file(cache_dir: &Path, file_name: &str) -> Result<Self, Error> {
        let file_path = Self::cache_file_path(cache_dir, file_name);

        let contents = {
            let mut file = OpenOptions::new()
                .read(true)
                .open(&file_path)
                .inspect_err(|err| warn!("Failed to open cache file at {file_path:?} : {err}",))?;

            debug!("Attempting to lock cache file for reading: {file_path:?}");
            Self::lock_with_retry(
                || file.lock_shared(),
                |err, attempt, max_attempts| {
                    warn!(
                        "Failed to acquire shared lock on cache file {file_path:?} (attempt {attempt}/{max_attempts}): {err}"
                    );
                },
            )?;

            let mut contents = String::new();
            file.read_to_string(&mut contents).inspect_err(|err| {
                warn!("Failed to read cache file: {err}");
            })?;

            contents
        };

        let data = serde_json::from_str::<Self>(&contents).map_err(|err| {
            warn!("Failed to parse cache data: {err}");
            Error::FailedToParseCacheData
        })?;

        Ok(data)
    }

    pub fn write_to_file(&self, cache_dir: &Path, file_name: &str) -> Result<(), Error> {
        let file_path = Self::cache_file_path(cache_dir, file_name);

        // Create parent directory if it doesn't exist
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Manual truncation after lock acquisition is required for thread-safety.
        // Using .truncate(true) would truncate during open() before the lock is acquired,
        // creating a race window where readers can see empty/partial file content.
        #[allow(clippy::suspicious_open_options)]
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&file_path)
            .inspect_err(|err| {
                error!("Failed to open cache file at {file_path:?}: {err}");
            })?;

        debug!("Attempting to lock cache file for writing: {file_path:?}");
        Self::lock_with_retry(
            || file.lock(),
            |err, attempt, max_attempts| {
                error!(
                    "Failed to acquire exclusive lock on cache file {file_path:?} (attempt {attempt}/{max_attempts}): {err}"
                );
            },
        )?;

        let data = serde_json::to_string_pretty(&self).inspect_err(|err| {
            error!("Failed to serialize cache data: {err}");
        })?;

        file.set_len(0).inspect_err(|err| {
            error!("Failed to truncate cache file {file_path:?} before writing: {err}");
        })?;

        file.seek(SeekFrom::Start(0)).inspect_err(|err| {
            error!("Failed to seek cache file {file_path:?} before writing: {err}");
        })?;

        file.write_all(data.as_bytes()).inspect_err(|err| {
            error!("Failed to write cache file {file_path:?}: {err}");
        })?;

        file.write_all(b"\n").inspect_err(|err| {
            error!("Failed to write newline to cache file {file_path:?}: {err}");
        })?;

        file.flush().inspect_err(|err| {
            error!("Failed to flush cache file {file_path:?}: {err}");
        })?;

        file.sync_all().inspect_err(|err| {
            error!("Failed to sync cache file {file_path:?}: {err}");
        })?;

        info!(
            "Cache with {} peers written to disk: {file_path:?}",
            self.peers.len()
        );

        Ok(())
    }

    pub fn cache_file_path(cache_dir: &Path, file_name: &str) -> PathBuf {
        cache_dir
            .join(format!("version_{}", Self::CACHE_DATA_VERSION))
            .join(file_name)
    }
}

impl Default for CacheData {
    fn default() -> Self {
        Self {
            peers: Default::default(),
            last_updated: SystemTime::now(),
            network_version: crate::get_network_version(),
            cache_version: Self::CACHE_DATA_VERSION.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng, rngs::SmallRng};
    use serde_json::Value;
    use std::{
        fs,
        str::FromStr,
        sync::{Arc, Barrier},
        thread,
    };

    const THREAD_COUNT: usize = 100;
    const ITERATIONS_PER_THREAD: usize = 25;
    const READ_PROBABILITY: f64 = 0.4;

    fn create_test_peer_data(rng: &mut SmallRng) -> CacheData {
        let mut data = CacheData::default();
        let peer = PeerId::random();
        let port = rng.gen_range(1000..2000);
        let addr = Multiaddr::from_str(&format!("/ip4/192.168.1.3/udp/{port}/quic-v1/p2p/{peer}"))
            .expect("valid multiaddr");

        data.add_peer(peer, [addr].iter(), 5, 10);
        data
    }

    fn perform_random_cache_operation(cache_dir: &Path, file_name: &str, rng: &mut SmallRng) {
        if rng.gen_bool(READ_PROBABILITY) {
            CacheData::read_from_file(cache_dir, file_name)
                .expect("concurrent read should succeed");
        } else {
            let data = create_test_peer_data(rng);
            data.write_to_file(cache_dir, file_name)
                .expect("concurrent write should succeed");
        }
    }

    /// Validates that concurrent reads and writes maintain file integrity.
    ///
    /// This test ensures that the lock-then-truncate approach prevents readers
    /// from seeing partial or corrupted JSON data during concurrent write operations.
    /// It spawns 100 threads performing 25 random read/write operations each.
    ///
    /// The test verifies:
    /// - No parse errors occur during concurrent access
    /// - Final cache file contains valid JSON
    /// - Cache version remains consistent
    #[test]
    fn cache_file_remains_valid_under_concurrent_access() {
        let _ = tracing_subscriber::fmt::try_init();

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let cache_dir = Arc::new(temp_dir.path().to_path_buf());
        let file_name = "cache.json";

        CacheData::default()
            .write_to_file(cache_dir.as_path(), file_name)
            .expect("initial cache write");

        let start_barrier = Arc::new(Barrier::new(THREAD_COUNT + 1));
        let mut handles = Vec::with_capacity(THREAD_COUNT);

        for thread_seed in 0..THREAD_COUNT {
            let cache_dir = Arc::clone(&cache_dir);
            let barrier = Arc::clone(&start_barrier);

            handles.push(thread::spawn(move || {
                let mut rng = SmallRng::seed_from_u64(thread_seed as u64 + 1);

                barrier.wait();

                for _ in 0..ITERATIONS_PER_THREAD {
                    perform_random_cache_operation(cache_dir.as_path(), file_name, &mut rng);
                }
            }));
        }

        start_barrier.wait();

        for handle in handles {
            handle
                .join()
                .expect("all threads should complete successfully");
        }

        let final_data = CacheData::read_from_file(cache_dir.as_path(), file_name)
            .expect("should read final cache state");

        assert_eq!(
            final_data.cache_version,
            CacheData::CACHE_DATA_VERSION.to_string(),
            "cache version should remain consistent after concurrent access"
        );

        let cache_file = CacheData::cache_file_path(cache_dir.as_path(), file_name);
        let contents =
            fs::read_to_string(&cache_file).expect("should read final cache file contents");

        serde_json::from_str::<Value>(&contents)
            .expect("final cache file should contain valid, parseable JSON");
    }
}
