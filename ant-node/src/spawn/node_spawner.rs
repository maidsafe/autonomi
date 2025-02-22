// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::utils::get_root_dir_and_keypair;
use crate::{NodeBuilder, RunningNode};
use ant_evm::{EvmNetwork, RewardsAddress};
use libp2p::Multiaddr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

pub struct NodeSpawner {
    /// The socket address where the node will listen.
    socket_addr: SocketAddr,
    /// The EVM network the node will connect to.
    evm_network: EvmNetwork,
    /// The rewards address used for receiving rewards.
    rewards_address: RewardsAddress,
    /// A vector of `Multiaddr` representing the initial peers.
    initial_peers: Vec<Multiaddr>,
    /// A boolean indicating whether the node should run in local mode.
    local: bool,
    /// A boolean indicating whether UPnP should be enabled.
    upnp: bool,
    /// An optional `PathBuf` representing the root directory for the node.
    root_dir: Option<PathBuf>,
}

impl NodeSpawner {
    /// Create a new instance of `NodeSpawner` with default values.
    pub fn new() -> Self {
        Self {
            socket_addr: SocketAddr::new(IpAddr::from(Ipv4Addr::UNSPECIFIED), 0),
            evm_network: Default::default(),
            rewards_address: Default::default(),
            initial_peers: vec![],
            local: false,
            upnp: false,
            root_dir: None,
        }
    }

    /// Set the socket address for the node.
    ///
    /// # Arguments
    ///
    /// * `socket_addr` - The `SocketAddr` where the node will listen.
    pub fn with_socket_addr(mut self, socket_addr: SocketAddr) -> Self {
        self.socket_addr = socket_addr;
        self
    }

    /// Set the EVM network for the node.
    ///
    /// # Arguments
    ///
    /// * `evm_network` - The `EvmNetwork` the node will connect to.
    pub fn with_evm_network(mut self, evm_network: EvmNetwork) -> Self {
        self.evm_network = evm_network;
        self
    }

    /// Set the rewards address for the node.
    ///
    /// # Arguments
    ///
    /// * `rewards_address` - The `RewardsAddress` used for distributing rewards.
    pub fn with_rewards_address(mut self, rewards_address: RewardsAddress) -> Self {
        self.rewards_address = rewards_address;
        self
    }

    /// Set the initial peers for the node.
    ///
    /// # Arguments
    ///
    /// * `initial_peers` - A vector of `Multiaddr` representing the initial peers.
    pub fn with_initial_peers(mut self, initial_peers: Vec<Multiaddr>) -> Self {
        self.initial_peers = initial_peers;
        self
    }

    /// Set the local mode flag for the node.
    ///
    /// # Arguments
    ///
    /// * `local` - A boolean indicating whether the node should run in local mode.
    pub fn with_local(mut self, local: bool) -> Self {
        self.local = local;
        self
    }

    /// Set the UPnP flag for the node.
    ///
    /// # Arguments
    ///
    /// * `upnp` - A boolean indicating whether UPnP should be enabled.
    pub fn with_upnp(mut self, upnp: bool) -> Self {
        self.upnp = upnp;
        self
    }

    /// Set the root directory for the node.
    ///
    /// # Arguments
    ///
    /// * `root_dir` - An optional `PathBuf` representing the root directory for the node.
    pub fn with_root_dir(mut self, root_dir: Option<PathBuf>) -> Self {
        self.root_dir = root_dir;
        self
    }

    /// Spawn the node using the configured parameters.
    ///
    /// # Returns
    ///
    /// An `eyre::Result` containing a `RunningNode` if successful, or an error.
    pub async fn spawn(self) -> eyre::Result<RunningNode> {
        spawn_node(
            self.socket_addr,
            self.evm_network,
            self.rewards_address,
            self.initial_peers,
            self.local,
            self.upnp,
            &self.root_dir,
        )
        .await
    }
}

impl Default for NodeSpawner {
    fn default() -> Self {
        Self::new()
    }
}

async fn spawn_node(
    socket_addr: SocketAddr,
    evm_network: EvmNetwork,
    rewards_address: RewardsAddress,
    initial_peers: Vec<Multiaddr>,
    local: bool,
    upnp: bool,
    root_dir: &Option<PathBuf>,
) -> eyre::Result<RunningNode> {
    let (root_dir, keypair) = get_root_dir_and_keypair(root_dir)?;

    let mut node_builder = NodeBuilder::new(
        keypair,
        initial_peers,
        rewards_address,
        evm_network,
        socket_addr,
        root_dir,
    );
    node_builder.local(local);
    node_builder.upnp(upnp);

    let running_node = node_builder.build_and_run()?;

    // Verify that node is running
    let mut retries: u8 = 0;

    let listen_addrs: Vec<Multiaddr> = loop {
        // Wait till we have at least 1 listen addrs
        if let Ok(listen_addrs) = running_node.get_listen_addrs().await {
            if !listen_addrs.is_empty() {
                break Ok(listen_addrs);
            }
        }

        if retries >= 3 {
            break Err(eyre::eyre!(
                "Failed to get listen addresses after {} retries",
                retries
            ));
        }

        retries += 1;

        tokio::time::sleep(tokio::time::Duration::from_secs(retries as u64)).await;
    }?;

    info!("Node listening on addresses: {:?}", listen_addrs);

    Ok(running_node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_evm::EvmNetwork;
    use futures::StreamExt;
    use libp2p::swarm::dummy;

    #[tokio::test]
    async fn test_launch_node() {
        let evm_network = EvmNetwork::ArbitrumSepolia;

        let running_node = NodeSpawner::new()
            .with_evm_network(evm_network)
            .with_local(true)
            .spawn()
            .await
            .unwrap();

        let listen_addrs = running_node.get_listen_addrs().await.unwrap();

        assert!(!listen_addrs.is_empty());

        let mut swarm = libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_quic()
            .with_behaviour(|_| dummy::Behaviour)
            .unwrap()
            .build();

        let address = listen_addrs.first().unwrap().clone();

        assert!(swarm.dial(address).is_ok());
        assert!(matches!(
            swarm.next().await,
            Some(libp2p::swarm::SwarmEvent::ConnectionEstablished { .. })
        ));

        running_node.shutdown();
    }
}
