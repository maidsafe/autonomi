// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    config::cache_file_name,
    craft_valid_multiaddr, craft_valid_multiaddr_from_str,
    error::{Error, Result},
    BootstrapAddr, BootstrapCacheConfig, BootstrapCacheStore, ContactsFetcher,
};
use ant_protocol::version::{get_network_id, MAINNET_ID};
use clap::Args;
use libp2p::Multiaddr;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;
use url::Url;

/// The name of the environment variable that can be used to pass peers to the node.
pub const ANT_PEERS_ENV: &str = "ANT_PEERS";

/// Previous version of InitialPeersConfig with the disable_mainnet_contacts field
#[derive(Args, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InitialPeersConfigV0 {
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
    /// Set to indicate this is a testnet.
    ///
    /// This disables fetching peers from the mainnet network contacts.
    #[clap(name = "testnet", long)]
    pub disable_mainnet_contacts: bool,
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
    #[clap(long)]
    pub bootstrap_cache_dir: Option<PathBuf>,
}

/// Latest version of InitialPeersConfig without the disable_mainnet_contacts field
#[derive(Args, Debug, Clone, Default, PartialEq, Serialize)]
pub struct InitialPeersConfigV1 {
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
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\bootstrap_cache/bootstrap_cache_<network_id>.json
    #[clap(long)]
    pub bootstrap_cache_dir: Option<PathBuf>,
}

impl From<InitialPeersConfigV0> for InitialPeersConfigV1 {
    fn from(v0: InitialPeersConfigV0) -> Self {
        Self {
            first: v0.first,
            addrs: v0.addrs,
            network_contacts_url: v0.network_contacts_url,
            local: v0.local,
            ignore_cache: v0.ignore_cache,
            bootstrap_cache_dir: v0.bootstrap_cache_dir,
        }
    }
}

pub type InitialPeersConfig = InitialPeersConfigV1;

impl InitialPeersConfig {
    /// Get bootstrap peers sorted by the failure rate.
    ///
    /// The peer with the lowest failure rate will be the first in the list.
    pub async fn get_addrs(
        &self,
        config: Option<BootstrapCacheConfig>,
        count: Option<usize>,
    ) -> Result<Vec<Multiaddr>> {
        Ok(self
            .get_bootstrap_addr(config, count)
            .await?
            .into_iter()
            .map(|addr| addr.addr)
            .collect())
    }

    /// Get bootstrap peers sorted by the failure rate.
    ///
    /// The peer with the lowest failure rate will be the first in the list.
    pub async fn get_bootstrap_addr(
        &self,
        config: Option<BootstrapCacheConfig>,
        count: Option<usize>,
    ) -> Result<Vec<BootstrapAddr>> {
        // If this is the first node, return an empty list
        if self.first {
            info!("First node in network, no initial bootstrap peers");
            return Ok(vec![]);
        }

        let mut bootstrap_addresses = vec![];

        // Read from ANT_PEERS environment variable if present
        bootstrap_addresses.extend(Self::read_bootstrap_addr_from_env());

        if !bootstrap_addresses.is_empty() {
            return Ok(bootstrap_addresses);
        }

        // Add addrs from arguments if present
        for addr in &self.addrs {
            if let Some(addr) = craft_valid_multiaddr(addr, false) {
                info!("Adding addr from arguments: {addr}");
                bootstrap_addresses.push(BootstrapAddr::new(addr));
            } else {
                warn!("Invalid multiaddress format from arguments: {addr}");
            }
        }

        if let Some(count) = count {
            if bootstrap_addresses.len() >= count {
                bootstrap_addresses.sort_by_key(|addr| addr.failure_rate() as u64);
                bootstrap_addresses.truncate(count);
                info!("Returning early as enough bootstrap addresses are found");
                return Ok(bootstrap_addresses);
            }
        }

        // load from cache if present
        if !self.ignore_cache {
            let cfg = if let Some(config) = config {
                Some(config)
            } else {
                BootstrapCacheConfig::default_config(self.local).ok()
            };
            if let Some(mut cfg) = cfg {
                if let Some(file_path) = self.get_bootstrap_cache_path()? {
                    cfg.cache_file_path = file_path;
                }
                info!("Loading bootstrap addresses from cache");
                if let Ok(data) = BootstrapCacheStore::load_cache_data(&cfg) {
                    let from_cache = data.peers.into_iter().filter_map(|(_, addrs)| {
                        addrs
                            .0
                            .into_iter()
                            .min_by_key(|addr| addr.failure_rate() as u64)
                    });
                    bootstrap_addresses.extend(from_cache);

                    if let Some(count) = count {
                        if bootstrap_addresses.len() >= count {
                            bootstrap_addresses.sort_by_key(|addr| addr.failure_rate() as u64);
                            bootstrap_addresses.truncate(count);
                            info!("Returning early as enough bootstrap addresses are found");
                            return Ok(bootstrap_addresses);
                        }
                    }
                }
            }
        } else {
            info!("Ignoring cache, not loading bootstrap addresses from cache");
        }

        if !self.local && !self.network_contacts_url.is_empty() {
            info!(
                "Fetching bootstrap address from network contacts URLs: {:?}",
                self.network_contacts_url
            );
            let addrs = self
                .network_contacts_url
                .iter()
                .map(|url| url.parse::<Url>().map_err(|_| Error::FailedToParseUrl))
                .collect::<Result<Vec<Url>>>()?;
            let mut contacts_fetcher = ContactsFetcher::with_endpoints(addrs)?;
            if let Some(count) = count {
                contacts_fetcher.set_max_addrs(count);
            }
            let addrs = contacts_fetcher.fetch_bootstrap_addresses().await?;
            bootstrap_addresses.extend(addrs);

            if let Some(count) = count {
                if bootstrap_addresses.len() >= count {
                    bootstrap_addresses.sort_by_key(|addr| addr.failure_rate() as u64);
                    bootstrap_addresses.truncate(count);
                    info!("Returning early as enough bootstrap addresses are found");
                    return Ok(bootstrap_addresses);
                }
            }
        }

        if !self.local && get_network_id() == MAINNET_ID {
            let mut contacts_fetcher = ContactsFetcher::with_mainnet_endpoints()?;
            if let Some(count) = count {
                contacts_fetcher.set_max_addrs(count);
            }
            let addrs = contacts_fetcher.fetch_bootstrap_addresses().await?;
            bootstrap_addresses.extend(addrs);
        }

        if !bootstrap_addresses.is_empty() {
            bootstrap_addresses.sort_by_key(|addr| addr.failure_rate() as u64);
            if let Some(count) = count {
                bootstrap_addresses.truncate(count);
            }
            Ok(bootstrap_addresses)
        } else {
            error!("No initial bootstrap peers found through any means");
            Err(Error::NoBootstrapPeersFound)
        }
    }

    pub fn read_addr_from_env() -> Vec<Multiaddr> {
        Self::read_bootstrap_addr_from_env()
            .into_iter()
            .map(|addr| addr.addr)
            .collect()
    }

    pub fn read_bootstrap_addr_from_env() -> Vec<BootstrapAddr> {
        let mut bootstrap_addresses = Vec::new();
        // Read from ANT_PEERS environment variable if present
        if let Ok(addrs) = std::env::var(ANT_PEERS_ENV) {
            for addr_str in addrs.split(',') {
                if let Some(addr) = craft_valid_multiaddr_from_str(addr_str, false) {
                    info!("Adding addr from environment variable: {addr}");
                    bootstrap_addresses.push(BootstrapAddr::new(addr));
                } else {
                    warn!("Invalid multiaddress format from environment variable: {addr_str}");
                }
            }
        }
        bootstrap_addresses
    }

    pub fn get_bootstrap_cache_path(&self) -> Result<Option<PathBuf>> {
        if let Some(dir) = &self.bootstrap_cache_dir {
            if dir.is_file() {
                return Err(Error::InvalidBootstrapCacheDir);
            }

            if !dir.exists() {
                std::fs::create_dir_all(dir)?;
            }

            let path = dir.join(cache_file_name());
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }
}

// Implementation of custom deserialization for InitialPeersConfig to automatically convert from V0 to V1
impl<'de> Deserialize<'de> for InitialPeersConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct InitialPeersConfigHelper {
            first: bool,
            addrs: Vec<Multiaddr>,
            network_contacts_url: Vec<String>,
            local: bool,
            ignore_cache: bool,
            bootstrap_cache_dir: Option<PathBuf>,
        }

        let helper = InitialPeersConfigHelper::deserialize(deserializer)?;

        Ok(InitialPeersConfigV1 {
            first: helper.first,
            addrs: helper.addrs,
            network_contacts_url: helper.network_contacts_url,
            local: helper.local,
            ignore_cache: helper.ignore_cache,
            bootstrap_cache_dir: helper.bootstrap_cache_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_from_v0_to_v1() {
        let v0 = InitialPeersConfigV0 {
            first: false,
            addrs: vec![],
            network_contacts_url: vec!["https://example.com".to_string()],
            local: true,
            disable_mainnet_contacts: true,
            ignore_cache: false,
            bootstrap_cache_dir: Some(PathBuf::from("/tmp/cache")),
        };

        let v1: InitialPeersConfigV1 = v0.clone().into();

        assert_eq!(v1.first, v0.first);
        assert_eq!(v1.addrs, v0.addrs);
        assert_eq!(v1.network_contacts_url, v0.network_contacts_url);
        assert_eq!(v1.local, v0.local);
        assert_eq!(v1.ignore_cache, v0.ignore_cache);
        assert_eq!(v1.bootstrap_cache_dir, v0.bootstrap_cache_dir);

        // Verify the disable_mainnet_contacts field is not present in V1
        // by checking the serialized JSON output
        let v1_json = serde_json::to_value(v1).unwrap();
        let obj = v1_json.as_object().unwrap();
        assert!(!obj.contains_key("disable_mainnet_contacts"));
    }

    #[test]
    fn test_serialize_deserialize_v0() {
        let v0 = InitialPeersConfigV0 {
            first: false,
            addrs: vec![],
            network_contacts_url: vec!["https://example.com".to_string()],
            local: true,
            disable_mainnet_contacts: true,
            ignore_cache: false,
            bootstrap_cache_dir: Some(PathBuf::from("/tmp/cache")),
        };

        let json_str = serde_json::to_string(&v0).unwrap();
        let v0_deserialized: InitialPeersConfigV0 = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v0_deserialized.first, v0.first);
        assert_eq!(v0_deserialized.addrs, v0.addrs);
        assert_eq!(
            v0_deserialized.network_contacts_url,
            v0.network_contacts_url
        );
        assert_eq!(v0_deserialized.local, v0.local);
        assert_eq!(
            v0_deserialized.disable_mainnet_contacts,
            v0.disable_mainnet_contacts
        );
        assert_eq!(v0_deserialized.ignore_cache, v0.ignore_cache);
        assert_eq!(v0_deserialized.bootstrap_cache_dir, v0.bootstrap_cache_dir);
    }

    #[test]
    fn test_serialize_deserialize_v1() {
        let v1 = InitialPeersConfigV1 {
            first: true,
            addrs: vec![],
            network_contacts_url: vec!["https://example.org".to_string()],
            local: false,
            ignore_cache: true,
            bootstrap_cache_dir: None,
        };

        let json_str = serde_json::to_string(&v1).unwrap();
        let v1_deserialized: InitialPeersConfigV1 = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v1_deserialized.first, v1.first);
        assert_eq!(v1_deserialized.addrs, v1.addrs);
        assert_eq!(
            v1_deserialized.network_contacts_url,
            v1.network_contacts_url
        );
        assert_eq!(v1_deserialized.local, v1.local);
        assert_eq!(v1_deserialized.ignore_cache, v1.ignore_cache);
        assert_eq!(v1_deserialized.bootstrap_cache_dir, v1.bootstrap_cache_dir);
    }

    #[test]
    fn test_deserialize_v0_as_initial_peers_config() {
        let json = json!({
            "first": false,
            "addrs": [],
            "network_contacts_url": ["https://example.com"],
            "local": true,
            "disable_mainnet_contacts": true,
            "ignore_cache": false,
            "bootstrap_cache_dir": "/tmp/cache"
        });

        let config: InitialPeersConfig = serde_json::from_value(json).unwrap();

        assert!(!config.first);
        assert_eq!(config.addrs.len(), 0);
        assert_eq!(config.network_contacts_url, vec!["https://example.com"]);
        assert!(config.local);
        assert!(!config.ignore_cache);
        assert_eq!(
            config.bootstrap_cache_dir,
            Some(PathBuf::from("/tmp/cache"))
        );
    }

    #[test]
    fn test_deserialize_v1_as_initial_peers_config() {
        let json = json!({
            "first": true,
            "addrs": [],
            "network_contacts_url": ["https://example.org"],
            "local": false,
            "ignore_cache": true,
            "bootstrap_cache_dir": null
        });

        let config: InitialPeersConfig = serde_json::from_value(json).unwrap();

        assert!(config.first);
        assert_eq!(config.addrs.len(), 0);
        assert_eq!(config.network_contacts_url, vec!["https://example.org"]);
        assert!(!config.local);
        assert!(config.ignore_cache);
        assert_eq!(config.bootstrap_cache_dir, None);
    }

    #[test]
    fn test_deserialize_with_multiaddr() {
        let addr_str = "/ip4/127.0.0.1/tcp/8080";
        let addr = addr_str.parse::<Multiaddr>().unwrap();

        let json = json!({
            "first": false,
            "addrs": [addr_str],
            "network_contacts_url": [],
            "local": false,
            "disable_mainnet_contacts": false,
            "ignore_cache": false,
            "bootstrap_cache_dir": null
        });

        let config: InitialPeersConfig = serde_json::from_value(json).unwrap();

        assert_eq!(config.addrs.len(), 1);
        assert_eq!(config.addrs[0], addr);
    }
}
