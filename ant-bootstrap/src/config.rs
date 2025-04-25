// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::{Error, Result};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// The duration since last)seen before removing the address of a Peer.
const ADDR_EXPIRY_DURATION: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// Maximum peers to store
const MAX_PEERS: usize = 1500;

/// Maximum number of addresses to store for a Peer
const MAX_ADDRS_PER_PEER: usize = 6;

// Min time until we save the bootstrap cache to disk. 5 mins
const MIN_BOOTSTRAP_CACHE_SAVE_INTERVAL: Duration = Duration::from_secs(5 * 60);

// Max time until we save the bootstrap cache to disk. 3 hours
const MAX_BOOTSTRAP_CACHE_SAVE_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60);

/// Configuration for the bootstrap cache
#[derive(Clone, Debug)]
pub struct BootstrapCacheConfig {
    /// The duration since last)seen before removing the address of a Peer.
    pub addr_expiry_duration: Duration,
    /// Maximum number of peers to keep in the cache
    pub max_peers: usize,
    /// Maximum number of addresses stored per peer.
    pub max_addrs_per_peer: usize,
    /// Path to the bootstrap cache file
    pub cache_file_path: PathBuf,
    /// Flag to disable writing to the cache file
    pub disable_cache_writing: bool,
    /// The min time duration until we save the bootstrap cache to disk.
    pub min_cache_save_duration: Duration,
    /// The max time duration until we save the bootstrap cache to disk.
    pub max_cache_save_duration: Duration,
    /// The cache save scaling factor. We start with the min_cache_save_duration and scale it up to the max_cache_save_duration.
    pub cache_save_scaling_factor: u64,
}

impl BootstrapCacheConfig {
    /// Creates a new BootstrapConfig with default settings
    ///
    /// When `local` is set to true, a different cache file name is used.
    /// I.e. the file name will include `_local_` in the name.
    pub fn default_config(local: bool) -> Result<Self> {
        let cache_file_path = if local {
            default_cache_path_local()?
        } else {
            default_cache_path()?
        };
        Ok(Self {
            cache_file_path,
            ..Self::empty()
        })
    }

    /// Creates a new BootstrapConfig with empty settings
    pub fn empty() -> Self {
        Self {
            addr_expiry_duration: ADDR_EXPIRY_DURATION,
            max_peers: MAX_PEERS,
            max_addrs_per_peer: MAX_ADDRS_PER_PEER,
            cache_file_path: PathBuf::new(),
            disable_cache_writing: false,
            min_cache_save_duration: MIN_BOOTSTRAP_CACHE_SAVE_INTERVAL,
            max_cache_save_duration: MAX_BOOTSTRAP_CACHE_SAVE_INTERVAL,
            cache_save_scaling_factor: 2,
        }
    }

    /// Set a new addr expiry duration
    pub fn with_addr_expiry_duration(mut self, duration: Duration) -> Self {
        self.addr_expiry_duration = duration;
        self
    }

    /// Update the config with a custom cache file path
    pub fn with_cache_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.cache_file_path = path.as_ref().to_path_buf();
        self
    }

    /// Sets the maximum number of peers
    pub fn with_max_peers(mut self, max_peers: usize) -> Self {
        self.max_peers = max_peers;
        self
    }

    /// Sets the maximum number of addresses for a single peer.
    pub fn with_addrs_per_peer(mut self, max_addrs: usize) -> Self {
        self.max_addrs_per_peer = max_addrs;
        self
    }

    /// Sets the flag to disable writing to the cache file
    pub fn with_disable_cache_writing(mut self, disable: bool) -> Self {
        self.disable_cache_writing = disable;
        self
    }
}

/// Returns the default path for the bootstrap cache file
fn default_cache_path() -> Result<PathBuf> {
    Ok(default_cache_dir()?.join(cache_file_name()))
}
/// Returns the default path for the bootstrap cache file that is used for
/// local networks
fn default_cache_path_local() -> Result<PathBuf> {
    Ok(default_cache_dir()?.join(cache_file_name_local()))
}

/// Returns the default dir that should contain the bootstrap cache file
fn default_cache_dir() -> Result<PathBuf> {
    let dir = dirs_next::data_dir()
        .ok_or_else(|| Error::CouldNotObtainDataDir)?
        .join("autonomi")
        .join("bootstrap_cache");

    std::fs::create_dir_all(&dir)?;

    Ok(dir)
}

/// Returns the name of the cache file
pub fn cache_file_name() -> String {
    format!("bootstrap_cache_{}.json", crate::get_network_version())
}

/// Returns the name of the cache file for local networks
pub fn cache_file_name_local() -> String {
    format!(
        "bootstrap_cache_local_{}.json",
        crate::get_network_version()
    )
}
