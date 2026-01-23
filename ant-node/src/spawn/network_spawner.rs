// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::RunningNode;
use crate::spawn::node_spawner::NodeSpawner;
use ant_evm::{EvmNetwork, RewardsAddress};
use libp2p::Multiaddr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct NetworkSpawner {
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
    /// Whether this is a local network.
    local: bool,
}

impl NetworkSpawner {
    /// Creates a new `NetworkSpawner` with default configurations.
    ///
    /// Default values:
    /// - `evm_network`: `EvmNetwork::default()`
    /// - `rewards_address`: `RewardsAddress::default()`
    /// - `no_upnp`: `false`
    /// - `root_dir`: `None`
    /// - `size`: `5`
    /// - `local`: `true`
    pub fn new() -> Self {
        Self {
            evm_network: Default::default(),
            rewards_address: Default::default(),
            no_upnp: false,
            root_dir: None,
            size: 5,
            local: true,
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

    /// Sets whether this is a local network.
    ///
    /// # Arguments
    ///
    /// * `local` - Whether this is a local network.
    pub fn with_local(mut self, local: bool) -> Self {
        self.local = local;
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

    /// Sets the number of nodes to spawn in the network.
    ///
    /// # Arguments
    ///
    /// * `size` - The number of nodes to create. Must be at least 1.
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }

    /// Spawns the network by creating `size` nodes.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the `RunningNetwork` if successful, or an error otherwise.
    ///
    /// # Errors
    ///
    /// This function will return an error if any node fails to spawn.
    pub async fn spawn(self) -> eyre::Result<RunningNetwork> {
        spawn_network(
            self.evm_network,
            self.rewards_address,
            self.no_upnp,
            self.root_dir,
            self.size,
            self.local,
        )
        .await
    }
}

impl Default for NetworkSpawner {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a running network consisting of multiple nodes.
#[derive(Debug)]
pub struct RunningNetwork {
    running_nodes: Vec<RunningNode>,
}

impl RunningNetwork {
    /// Returns a reference to the running nodes.
    pub fn running_nodes(&self) -> &[RunningNode] {
        &self.running_nodes
    }

    /// Returns all listen addresses from all running nodes.
    pub async fn get_all_listen_multiaddr(&self) -> eyre::Result<Vec<Multiaddr>> {
        let mut all_listen_addrs: Vec<Multiaddr> = vec![];
        for node in &self.running_nodes {
            if let Ok(listen_addrs) = node.get_listen_addrs_with_peer_id().await {
                all_listen_addrs.extend(listen_addrs);
            }
        }
        Ok(all_listen_addrs)
    }

    /// Shuts down all running nodes.
    pub fn shutdown(self) {
        for node in self.running_nodes {
            node.shutdown();
        }
    }
}

/// Spawns a local network with the given configuration.
async fn spawn_network(
    evm_network: EvmNetwork,
    rewards_address: RewardsAddress,
    no_upnp: bool,
    root_dir: Option<PathBuf>,
    size: usize,
    local: bool,
) -> eyre::Result<RunningNetwork> {
    let mut running_nodes: Vec<RunningNode> = vec![];

    for i in 0..size {
        // Determine the socket address for the node
        let socket_addr = SocketAddr::new(IpAddr::from(Ipv4Addr::LOCALHOST), 0);

        // Get the initial peers from the previously spawned nodes
        let mut initial_peers: Vec<Multiaddr> = vec![];

        for peer in running_nodes.iter() {
            if let Ok(listen_addrs_with_peer_id) = peer.get_listen_addrs_with_peer_id().await {
                initial_peers.extend(listen_addrs_with_peer_id);
            }
        }

        let node = NodeSpawner::new()
            .with_socket_addr(socket_addr)
            .with_evm_network(evm_network.clone())
            .with_rewards_address(rewards_address)
            .with_initial_peers(initial_peers)
            .with_local(local)
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

        let running_network = NetworkSpawner::new()
            .with_evm_network(Default::default())
            .with_local(true)
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
            let kbuckets = node.get_kbuckets().await.unwrap();
            let kbucket_peer_count = kbuckets.1;
            // Each node should know at least half of the other nodes
            assert!(
                kbucket_peer_count >= network_size / 2,
                "Node does not know enough peers: {kbucket_peer_count} < {}",
                network_size / 2
            );
        }

        running_network.shutdown();
    }
}
