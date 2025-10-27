// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::RunningNode;
use crate::spawn::node_spawner::NodeSpawner;
use ant_bootstrap::BootstrapConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use libp2p::Multiaddr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct NetworkSpawner {
    /// Bootstrap configuration for all nodes in the network.
    bootstrap_config: Option<BootstrapConfig>,
    /// The EVM network to which the spawned nodes will connect.
    evm_network: EvmNetwork,
    /// The address that will receive rewards from the spawned nodes.
    rewards_address: RewardsAddress,
    /// Disables UPnP on the node (automatic port forwarding).
    no_upnp: bool,
    /// Optional root directory to store node data and configurations.
    root_dir: Option<PathBuf>,
    /// Number of nodes to spawn in the network.
    size: usize,
}

impl NetworkSpawner {
    /// Creates a new `NetworkSpawner` with default configurations.
    ///
    /// Default values:
    /// - `bootstrap_config`: `None`
    /// - `evm_network`: `EvmNetwork::default()`
    /// - `rewards_address`: `RewardsAddress::default()`
    /// - `no_upnp`: `false`
    /// - `root_dir`: `None`
    /// - `size`: `5`
    pub fn new() -> Self {
        Self {
            evm_network: Default::default(),
            rewards_address: Default::default(),
            no_upnp: false,
            root_dir: None,
            size: 5,
            bootstrap_config: None,
        }
    }

    /// Sets the EVM network to be used by the nodes.
    ///
    /// # Arguments
    ///
    /// * `evm_network` - The target `EvmNetwork` for the nodes.
    pub fn with_evm_network(mut self, evm_network: EvmNetwork) -> Self {
        self.evm_network = evm_network;
        self
    }

    /// Sets the rewards address for the nodes.
    ///
    /// # Arguments
    ///
    /// * `rewards_address` - A valid `RewardsAddress` to collect rewards.
    pub fn with_rewards_address(mut self, rewards_address: RewardsAddress) -> Self {
        self.rewards_address = rewards_address;
        self
    }

    /// Sets the bootstrap configuration for all nodes in the network.
    ///
    /// # Arguments
    ///
    /// * `bootstrap_config` - Bootstrap configuration including peer addresses, cache settings, etc.
    pub fn with_bootstrap_config(mut self, bootstrap_config: BootstrapConfig) -> Self {
        self.bootstrap_config = Some(bootstrap_config);
        self
    }

    /// Disabled UPnP for the nodes.
    ///
    /// # Arguments
    ///
    /// * `value` - If `false`, nodes will attempt automatic port forwarding using UPnP.
    pub fn with_no_upnp(mut self, value: bool) -> Self {
        self.no_upnp = value;
        self
    }

    /// Sets the root directory for the nodes.
    ///
    /// # Arguments
    ///
    /// * `root_dir` - An optional file path where nodes will store their data.
    pub fn with_root_dir(mut self, root_dir: Option<PathBuf>) -> Self {
        self.root_dir = root_dir;
        self
    }

    /// Specifies the number of nodes to spawn in the network.
    ///
    /// # Arguments
    ///
    /// * `size` - The number of nodes to create. Default is 5.
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }

    /// Spawns the network with the configured parameters.
    ///
    /// # Returns
    ///
    /// A future resolving to a `SpawnedNetwork` containing the running nodes,
    /// or an error if the spawning process fails.
    pub async fn spawn(self) -> eyre::Result<RunningNetwork> {
        spawn_network(
            self.evm_network,
            self.rewards_address,
            self.no_upnp,
            self.root_dir,
            self.size,
            self.bootstrap_config,
        )
        .await
    }
}

impl Default for NetworkSpawner {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RunningNetwork {
    running_nodes: Vec<RunningNode>,
}

impl RunningNetwork {
    /// Returns a bootstrap peer from this network.
    pub async fn bootstrap_peer(&self) -> Multiaddr {
        self.running_nodes()
            .first()
            .expect("No nodes running, cannot get bootstrap peer")
            .get_listen_addrs_with_peer_id()
            .await
            .expect("Could not get listen addresses for bootstrap peer")
            .last()
            .expect("Bootstrap peer has no listen addresses")
            .clone()
    }

    /// Return all running nodes.
    pub fn running_nodes(&self) -> &Vec<RunningNode> {
        &self.running_nodes
    }

    /// Shutdown all running nodes.
    pub fn shutdown(self) {
        for node in self.running_nodes.into_iter() {
            node.shutdown();
        }
    }
}

async fn spawn_network(
    evm_network: EvmNetwork,
    rewards_address: RewardsAddress,
    no_upnp: bool,
    root_dir: Option<PathBuf>,
    size: usize,
    bootstrap_config: Option<BootstrapConfig>,
) -> eyre::Result<RunningNetwork> {
    let mut running_nodes: Vec<RunningNode> = vec![];

    // Extract local flag from bootstrap_config, default to false
    let local = bootstrap_config.as_ref().map(|c| c.local).unwrap_or(false);

    for i in 0..size {
        let ip = match local {
            true => IpAddr::V4(Ipv4Addr::LOCALHOST),
            false => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        };

        let socket_addr = SocketAddr::new(ip, 0);

        // Get the initial peers from the previously spawned nodes
        let mut initial_peers: Vec<Multiaddr> = vec![];

        for peer in running_nodes.iter() {
            if let Ok(listen_addrs_with_peer_id) = peer.get_listen_addrs_with_peer_id().await {
                initial_peers.extend(listen_addrs_with_peer_id);
            }
        }

        // Merge bootstrap_config with node-specific config
        let mut node_bootstrap_config = bootstrap_config.clone().unwrap_or_default();
        node_bootstrap_config.initial_peers.extend(initial_peers);
        node_bootstrap_config.first = running_nodes.is_empty();
        node_bootstrap_config.local = local;

        let node = NodeSpawner::new()
            .with_socket_addr(socket_addr)
            .with_evm_network(evm_network.clone())
            .with_rewards_address(rewards_address)
            .with_bootstrap_config(node_bootstrap_config)
            .with_no_upnp(no_upnp)
            .with_root_dir(root_dir.clone())
            .spawn()
            .await?;

        let listen_addrs = node.get_listen_addrs().await;

        info!(
            "Spawned node #{} with listen addresses: {:?}",
            i + 1,
            listen_addrs
        );

        running_nodes.push(node);
    }

    Ok(RunningNetwork { running_nodes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_spawn_network() {
        let network_size = 20;

        let bootstrap_config = BootstrapConfig::new(true)
            .with_disable_cache_reading(true)
            .with_disable_env_peers(true);

        let running_network = NetworkSpawner::new()
            .with_evm_network(Default::default())
            .with_bootstrap_config(bootstrap_config)
            .with_no_upnp(true)
            .with_size(network_size)
            .spawn()
            .await
            .unwrap();

        assert_eq!(running_network.running_nodes().len(), network_size);

        // Wait for nodes to fill up their RT
        sleep(Duration::from_secs(15)).await;

        // Validate that all nodes know each other
        for node in running_network.running_nodes() {
            let peers_in_routing_table = node
                .get_swarm_local_state()
                .await
                .unwrap()
                .peers_in_routing_table;

            assert!(
                peers_in_routing_table >= network_size - 2 && peers_in_routing_table < network_size,
                "Node with PeerId {} has {} peers in its routing table, expected between {} and {}",
                node.peer_id(),
                peers_in_routing_table,
                network_size - 2,
                network_size - 1
            );
        }

        running_network.shutdown();
    }
}
