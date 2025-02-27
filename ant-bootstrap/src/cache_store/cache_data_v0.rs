// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use atomic_write_file::AtomicWriteFile;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};

use crate::Error;

use super::cache_data_v1;

/// A addr that can be used for bootstrapping into the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapAddr {
    /// The multiaddress of the peer
    pub addr: Multiaddr,
    /// The number of successful connections to this address
    pub success_count: u32,
    /// The number of failed connection attempts to this address
    pub failure_count: u32,
    /// The last time this address was successfully contacted
    pub last_seen: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Set of addresses for a particular PeerId
pub struct BootstrapAddresses(pub Vec<BootstrapAddr>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheData {
    pub peers: std::collections::HashMap<PeerId, BootstrapAddresses>,
    pub last_updated: SystemTime,
    pub network_version: String,
}

impl From<&cache_data_v1::CacheData> for CacheData {
    fn from(data: &cache_data_v1::CacheData) -> Self {
        let mut peers = std::collections::HashMap::new();
        for (peer_id, addrs) in &data.peers {
            let addrs = addrs
                .iter()
                .map(|addr| BootstrapAddr {
                    addr: addr.clone(),
                    success_count: 0,
                    failure_count: 0,
                    last_seen: SystemTime::now(),
                })
                .collect();
            peers.insert(*peer_id, BootstrapAddresses(addrs));
        }

        Self {
            peers,
            last_updated: data.last_updated,
            network_version: data.network_version.clone(),
        }
    }
}

impl From<CacheData> for cache_data_v1::CacheData {
    fn from(val: CacheData) -> Self {
        let peers = val
            .peers
            .into_iter()
            .map(|(peer_id, addrs)| {
                let addrs = addrs.0.into_iter().map(|addr| addr.addr).collect();
                (peer_id, addrs)
            })
            .collect();

        cache_data_v1::CacheData {
            peers,
            last_updated: val.last_updated,
            network_version: val.network_version,
            cache_version: cache_data_v1::CacheData::CACHE_DATA_VERSION.to_string(),
        }
    }
}

impl CacheData {
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
        cache_dir.join(file_name)
    }
}
