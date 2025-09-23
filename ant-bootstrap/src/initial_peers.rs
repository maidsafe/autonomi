// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    BootstrapCacheConfig, BootstrapCacheStore, ContactsFetcher, craft_valid_multiaddr,
    craft_valid_multiaddr_from_str,
    error::{Error, Result},
};
use ant_protocol::version::{ALPHANET_ID, MAINNET_ID, get_network_id};
use clap::Args;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};
use url::Url;

/// The name of the environment variable that can be used to pass peers to the node.
pub const ANT_PEERS_ENV: &str = "ANT_PEERS";

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
    #[clap(long)]
    pub bootstrap_cache_dir: Option<PathBuf>,
}

impl InitialPeersConfig {
    /// Get bootstrap peers
    pub async fn get_bootstrap_addr(&self, count: Option<usize>) -> Result<Vec<Multiaddr>> {
        // If this is the first node, return an empty list
        if self.first {
            info!("First node in network, no initial bootstrap peers");
            return Ok(vec![]);
        }

        let mut bootstrap_addresses: HashSet<Multiaddr> = HashSet::new();

        // Read from ANT_PEERS environment variable if present
        bootstrap_addresses.extend(Self::read_bootstrap_addr_from_env());

        if !bootstrap_addresses.is_empty() {
            info!(
                "Found {} bootstrap addresses from environment variable",
                bootstrap_addresses.len()
            );
            return Ok(bootstrap_addresses.into_iter().collect());
        }

        // Add addrs from arguments if present
        for addr in &self.addrs {
            if let Some(addr) = craft_valid_multiaddr(addr) {
                info!("Adding addr from arguments: {addr}");
                let _ = bootstrap_addresses.insert(addr);
            } else {
                warn!("Invalid multiaddress format from arguments: {addr}");
            }
        }

        if let Some(count) = count
            && bootstrap_addresses.len() >= count
        {
            bootstrap_addresses.truncate(count);
            info!(
                "Found {} bootstrap addresses. Returning early.",
                bootstrap_addresses.len()
            );
            return Ok(bootstrap_addresses.into_iter().collect());
        }

        // load from cache if present
        if !self.ignore_cache {
            let cfg = BootstrapCacheConfig::try_from(self)
                .inspect_err(|err| {
                    error!("Failed to create bootstrap cache config: {err}");
                })
                .ok();

            if let Some(cfg) = cfg {
                if let Ok(data) = BootstrapCacheStore::load_cache_data(&cfg) {
                    let addrs_from_cache = data.get_all_addrs().cloned().collect::<Vec<_>>();
                    info!(
                        "Found {} bootstrap addresses from local cache",
                        addrs_from_cache.len()
                    );
                    bootstrap_addresses.extend(addrs_from_cache);

                    if let Some(count) = count
                        && bootstrap_addresses.len() >= count
                    {
                        bootstrap_addresses.truncate(count);
                        info!(
                            "Found {} bootstrap addresses. Returning early.",
                            bootstrap_addresses.len()
                        );
                        return Ok(bootstrap_addresses.into_iter().collect());
                    }
                }
            } else {
                info!("Bootstrap cache config could not be created, skipping cache loading");
            }
        } else {
            info!("Ignoring cache, not loading bootstrap addresses from cache");
        }

        // If we have a network contacts URL, fetch addrs from there.
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

            if let Some(count) = count
                && bootstrap_addresses.len() >= count
            {
                bootstrap_addresses.truncate(count);
                info!(
                    "Found {} bootstrap addresses. Returning early.",
                    bootstrap_addresses.len()
                );
                return Ok(bootstrap_addresses.into_iter().collect());
            }
        }

        if !self.local && get_network_id() == MAINNET_ID {
            let mut contacts_fetcher = ContactsFetcher::with_mainnet_endpoints()?;
            if let Some(count) = count {
                contacts_fetcher.set_max_addrs(count);
            }
            info!("Fetching bootstrap address from mainnet contacts");
            let addrs = contacts_fetcher.fetch_bootstrap_addresses().await?;
            bootstrap_addresses.extend(addrs);
        } else if !self.local && get_network_id() == ALPHANET_ID {
            let mut contacts_fetcher = ContactsFetcher::with_alphanet_endpoints()?;
            if let Some(count) = count {
                contacts_fetcher.set_max_addrs(count);
            }
            info!("Fetching bootstrap address from alphanet contacts");
            let addrs = contacts_fetcher.fetch_bootstrap_addresses().await?;
            bootstrap_addresses.extend(addrs);
        }

        if !bootstrap_addresses.is_empty() {
            if let Some(count) = count {
                bootstrap_addresses.truncate(count);
            }
            info!(
                "Found {} bootstrap addresses. Returning early.",
                bootstrap_addresses.len()
            );
            Ok(bootstrap_addresses.into_iter().collect())
        } else {
            error!("No initial bootstrap peers found through any means");
            Err(Error::NoBootstrapPeersFound)
        }
    }

    pub fn read_bootstrap_addr_from_env() -> Vec<Multiaddr> {
        let mut bootstrap_addresses = Vec::new();
        // Read from ANT_PEERS environment variable if present
        if let Ok(addrs) = std::env::var(ANT_PEERS_ENV) {
            for addr_str in addrs.split(',') {
                if let Some(addr) = craft_valid_multiaddr_from_str(addr_str) {
                    info!("Adding addr from environment variable: {addr}");
                    bootstrap_addresses.push(addr);
                } else {
                    warn!("Invalid multiaddress format from environment variable: {addr_str}");
                }
            }
        }
        bootstrap_addresses
    }
}

trait HashSetTruncate<T> {
    fn truncate(&mut self, n: usize);
}

impl<T: Clone + std::hash::Hash + Eq> HashSetTruncate<T> for HashSet<T> {
    fn truncate(&mut self, n: usize) {
        while self.len() > n {
            if let Some(item) = self.iter().next().cloned() {
                self.remove(&item);
            } else {
                break;
            }
        }
    }
}
