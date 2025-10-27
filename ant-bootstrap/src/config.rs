// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::{Error, Result};
use clap::Args;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

/// Maximum peers to store
const MAX_CACHED_PEERS: usize = 1500;

/// Maximum number of addresses to store for a Peer
const MAX_ADDRS_PER_CACHED_PEER: usize = 3;

// Min time until we save the bootstrap cache to disk. 30 secs
const MIN_BOOTSTRAP_CACHE_SAVE_INTERVAL: Duration = Duration::from_secs(30);

// Max time until we save the bootstrap cache to disk. 3 hours
const MAX_BOOTSTRAP_CACHE_SAVE_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60);

/// The max number of concurrent dials to be made during the initial bootstrap process.
const CONCURRENT_DIALS: usize = 10;

/// The max number of peers to be added before stopping the initial bootstrap process.
const MAX_PEERS_BEFORE_TERMINATION: usize = 5;

/// Configurations to fetch the initial peers which is used to bootstrap the network.
/// This could optionally also be used as a command line argument struct.
#[derive(Args, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InitialPeersConfig {
    /// Set to indicate this is the first node in a new network
    ///
    /// If this argument is used, any others will be ignored because they do not apply to the first
    /// node.
    #[clap(long, default_value = "false")]
    pub first: bool,
    /// Addr(s) to use for bootstrap, in a 'multiaddr' format containing the peer ID.
    ///
    /// A multiaddr looks like
    /// '/ip4/1.2.3.4/tcp/1200/tcp/p2p/12D3KooWRi6wF7yxWLuPSNskXc6kQ5cJ6eaymeMbCRdTnMesPgFx' where
    /// `1.2.3.4` is the IP, `1200` is the port and the (optional) last part is the peer ID.
    ///
    /// This argument can be provided multiple times to connect to multiple peers.
    ///
    /// Alternatively, the `ANT_PEERS` environment variable can provide a comma-separated peer
    /// list.
    #[clap(
        long = "peer",
        value_name = "multiaddr",
        value_delimiter = ',',
        conflicts_with = "first"
    )]
    pub addrs: Vec<Multiaddr>,
    /// Specify the URL to fetch the network contacts from.
    ///
    /// The URL can point to a text file containing Multiaddresses separated by newline character, or
    /// a bootstrap cache JSON file.
    #[clap(long, conflicts_with = "first", value_delimiter = ',')]
    pub network_contacts_url: Vec<String>,
    /// Set to indicate this is a local network.
    #[clap(long, conflicts_with = "network_contacts_url", default_value = "false")]
    pub local: bool,
    /// Set to not load the bootstrap addresses from the local cache.
    #[clap(long, default_value = "false")]
    pub ignore_cache: bool,
    /// The directory to load and store the bootstrap cache. If not provided, the default path will be used.
    ///
    /// The JSON filename will be derived automatically from the network ID
    ///
    /// The default location is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/bootstrap_cache/bootstrap_cache_<network_id>.json
    ///  - macOS: $HOME/Library/Application Support/autonomi/bootstrap_cache/bootstrap_cache_<network_id>.json
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\bootstrap_cache\bootstrap_cache_<network_id>.json
    ///
    /// We fallback to $HOME dir and then to current working directory if the platform specific directory cannot be
    /// determined.
    #[clap(long)]
    pub bootstrap_cache_dir: Option<PathBuf>,
}

/// Configuration for Bootstrapping
///
/// If you have `InitialPeersConfig`, you can convert it to `BootstrapConfig` using `TryFrom` trait.
#[derive(Clone, Debug)]
pub struct BootstrapConfig {
    /// Enable backwards compatibility while writing the cache file.
    /// This will write the cache file in all versions of the cache file format.
    pub backwards_compatible_writes: bool,
    /// The directory to load and store the bootstrap cache. If not provided, the default path will be used.
    ///
    /// The JSON filename will be derived automatically from the network ID
    ///
    /// The default location is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/bootstrap_cache/bootstrap_cache_<network_id>.json
    ///  - macOS: $HOME/Library/Application Support/autonomi/bootstrap_cache/bootstrap_cache_<network_id>.json
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\bootstrap_cache\bootstrap_cache_<network_id>.json
    pub cache_dir: PathBuf,
    /// The cache save scaling factor. We start with the min_cache_save_duration and scale it up to the max_cache_save_duration.
    pub cache_save_scaling_factor: u32,
    /// Flag to disable writing to the cache file
    pub disable_cache_writing: bool,
    /// Flag to disable reading from the cache file
    pub disable_cache_reading: bool,
    /// Flag to disable reading peers from the ANT_PEERS environment variable
    pub disable_env_peers: bool,
    /// Indicate that this is the first node in a new network.
    pub first: bool,
    /// The initial peers that are used to bootstrap/connect the network.
    pub initial_peers: Vec<Multiaddr>,
    /// If set to true, the cache filename will be suffixed with "_local"
    pub local: bool,
    /// The max time duration until we save the bootstrap cache to disk.
    pub max_cache_save_duration: Duration,
    /// The max number of concurrent dials to be made during the initial bootstrap process.
    ///
    /// This is the number of peers we will try to dial in parallel.
    /// Default is 5.
    pub max_concurrent_dials: usize,
    /// The max number of peers to be added to RT / connected before stopping the initial bootstrap process.
    /// Default is 5.
    pub max_contacted_peers_before_termination: usize,
    /// Maximum number of peers to store inside the bootstrap cache
    ///
    /// When the number of cached peers exceeds this value, the least recently seen peers will be removed.
    /// Default is 1500.
    pub max_cached_peers: usize,
    /// Maximum number of addresses stored per peer inside the bootstrap cache.
    /// Default is 3.
    pub max_addrs_per_cached_peer: usize,
    /// The min time duration until we save the bootstrap cache to disk.
    pub min_cache_save_duration: Duration,
    /// Specify the URL to fetch the network contacts from.
    ///
    /// The URL can point to a text file containing Multiaddresses separated by newline character, or
    /// a bootstrap cache JSON file.
    pub network_contacts_url: Vec<String>,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            backwards_compatible_writes: false,
            cache_dir: default_cache_dir(),
            cache_save_scaling_factor: 2,
            disable_cache_writing: false,
            disable_cache_reading: false,
            disable_env_peers: false,
            first: false,
            initial_peers: vec![],
            local: false,
            max_concurrent_dials: CONCURRENT_DIALS,
            max_contacted_peers_before_termination: MAX_PEERS_BEFORE_TERMINATION,
            max_cached_peers: MAX_CACHED_PEERS,
            max_addrs_per_cached_peer: MAX_ADDRS_PER_CACHED_PEER,
            min_cache_save_duration: MIN_BOOTSTRAP_CACHE_SAVE_INTERVAL,
            max_cache_save_duration: MAX_BOOTSTRAP_CACHE_SAVE_INTERVAL,
            network_contacts_url: vec![],
        }
    }
}

impl TryFrom<&InitialPeersConfig> for BootstrapConfig {
    type Error = Error;
    fn try_from(config: &InitialPeersConfig) -> Result<Self> {
        let cache_dir = if let Some(cache_dir) = &config.bootstrap_cache_dir {
            cache_dir.clone()
        } else {
            default_cache_dir()
        };

        let bootstrap_config = Self {
            cache_dir,
            disable_cache_reading: config.ignore_cache,
            first: config.first,
            initial_peers: config.addrs.clone(),
            local: config.local,
            network_contacts_url: config.network_contacts_url.clone(),
            ..Self::default()
        };
        Ok(bootstrap_config)
    }
}

impl BootstrapConfig {
    /// Creates a new BootstrapConfig with default settings
    pub fn new(local: bool) -> Self {
        Self {
            local,
            cache_dir: default_cache_dir(),
            ..Self::default()
        }
    }

    /// Set backwards compatible writes
    pub fn with_backwards_compatible_writes(mut self, enable: bool) -> Self {
        self.backwards_compatible_writes = enable;
        self
    }

    /// Set the local flag
    pub fn with_local(mut self, enable: bool) -> Self {
        self.local = enable;
        self
    }

    /// Update the config with a custom cache file path
    pub fn with_cache_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.cache_dir = path.as_ref().to_path_buf();
        self
    }

    /// Sets the maximum number of concurrent dials to be made during the initial bootstrap process
    /// Default is 5.
    pub fn with_max_concurrent_dials(mut self, max_dials: usize) -> Self {
        self.max_concurrent_dials = max_dials;
        self
    }

    /// Sets the maximum number of peers to be added / contacted before stopping the initial bootstrap process
    /// Default is 5.
    pub fn with_max_contacted_peers_before_termination(mut self, max_peers: usize) -> Self {
        self.max_contacted_peers_before_termination = max_peers;
        self
    }

    /// Sets the maximum number of cached peers
    pub fn with_max_cached_peers(mut self, max_peers: usize) -> Self {
        self.max_cached_peers = max_peers;
        self
    }

    /// Sets the maximum number of addresses for a single peer in the bootstrap cache
    pub fn with_max_addrs_per_cached_peer(mut self, max_addrs: usize) -> Self {
        self.max_addrs_per_cached_peer = max_addrs;
        self
    }

    /// Sets the flag to disable writing to the cache file
    pub fn with_disable_cache_writing(mut self, disable: bool) -> Self {
        self.disable_cache_writing = disable;
        self
    }

    /// Sets the flag to disable reading from the cache file
    pub fn with_disable_cache_reading(mut self, disable: bool) -> Self {
        self.disable_cache_reading = disable;
        self
    }

    /// Sets the flag to disable reading peers from the ANT_PEERS environment variable
    pub fn with_disable_env_peers(mut self, disable: bool) -> Self {
        self.disable_env_peers = disable;
        self
    }

    /// Sets whether this config represents the first node in the network
    pub fn with_first(mut self, first: bool) -> Self {
        self.first = first;
        self
    }

    /// Sets the initial peers that should be used for bootstrapping
    pub fn with_initial_peers(mut self, peers: Vec<Multiaddr>) -> Self {
        self.initial_peers = peers;
        self
    }

    /// Sets the cache save scaling factor
    pub fn with_cache_save_scaling_factor(mut self, factor: u32) -> Self {
        self.cache_save_scaling_factor = factor;
        self
    }

    /// Sets the maximum duration between cache saves
    pub fn with_max_cache_save_duration(mut self, duration: Duration) -> Self {
        self.max_cache_save_duration = duration;
        self
    }

    /// Sets the minimum duration between cache saves
    pub fn with_min_cache_save_duration(mut self, duration: Duration) -> Self {
        self.min_cache_save_duration = duration;
        self
    }

    /// Sets the list of network contact URLs
    pub fn with_network_contacts_url(mut self, urls: Vec<String>) -> Self {
        self.network_contacts_url = urls;
        self
    }
}

/// Returns the default dir that should contain the bootstrap cache file
///
/// The default location is platform specific:
///  - Linux: $HOME/.local/share/autonomi/bootstrap_cache/bootstrap_cache_<network_id>.json
///  - macOS: $HOME/Library/Application Support/autonomi/bootstrap_cache/bootstrap_cache_<network_id>.json
///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\bootstrap_cache\bootstrap_cache_<network_id>.json
///
/// We fallback to $HOME dir and then to current working directory if the platform specific directory cannot be
/// determined.
fn default_cache_dir() -> PathBuf {
    let base_dir = if let Some(dir) = dirs_next::data_dir() {
        dir
    } else if let Some(home) = dirs_next::home_dir() {
        warn!("Failed to obtain platform data directory, falling back to home directory");
        home
    } else {
        let cwd = env::current_dir().unwrap_or_else(|err| {
            error!("Failed to obtain current working directory: {err}. Using current process directory '.'");
            PathBuf::from(".")
        });
        warn!("Falling back to current working directory for bootstrap cache");
        cwd
    };

    base_dir.join("autonomi").join("bootstrap_cache")
}
