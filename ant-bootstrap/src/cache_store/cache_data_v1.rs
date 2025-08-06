// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Error;

use atomic_write_file::AtomicWriteFile;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
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

    /// Sync the self cache with another cache. This would just add the 'other' state to self.
    pub fn sync(&mut self, other: &CacheData, max_addrs_per_peer: usize, max_peers: usize) {
        let old_len = self.peers.len();
        let other_len = other.peers.len();

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

        let new_len = self.peers.len();

        info!(
            "Synced {other_len} peers to our current {old_len:?} peers to have a final count of {new_len:?} peers"
        );

        self.last_updated = SystemTime::now();
    }

    /// Add a peer to the cachse data
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

    pub fn read_from_file(cache_dir: &Path, file_name: &str) -> Result<Self, Error> {
        let file_path = Self::cache_file_path(cache_dir, file_name);
        // Try to open the file with read permissions
        let mut file = OpenOptions::new()
            .read(true)
            .open(&file_path)
            .inspect_err(|err| warn!("Failed to open cache file at {file_path:?} : {err}",))?;

        // Read the file contents
        let mut contents = String::new();
        file.read_to_string(&mut contents).inspect_err(|err| {
            warn!("Failed to read cache file: {err}");
        })?;

        // Parse the cache data
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

        let mut file = AtomicWriteFile::options()
            .open(&file_path)
            .inspect_err(|err| {
                error!("Failed to open cache file at {file_path:?} using AtomicWriteFile: {err}");
            })?;

        let data = serde_json::to_string_pretty(&self).inspect_err(|err| {
            error!("Failed to serialize cache data: {err}");
        })?;
        writeln!(file, "{data}")?;
        file.commit().inspect_err(|err| {
            error!("Failed to commit atomic write: {err}");
        })?;

        info!("Cache written to disk: {:?}", file_path);

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
