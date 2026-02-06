// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::utils::get_root_dir_and_keypair;
use crate::{NodeBuilder, RunningNode};
use ant_bootstrap::{BootstrapConfig, bootstrap::Bootstrap};
pub use ant_evm::{EvmNetwork, RewardsAddress};
pub use libp2p::Multiaddr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct NodeSpawner {
    /// The socket address where the node will listen.
    socket_addr: SocketAddr,
    /// The EVM network the node will connect to.
    evm_network: EvmNetwork,
    /// The rewards address used for receiving rewards.
    rewards_address: RewardsAddress,
    /// The bootstrap configuration for the node.
    bootstrap_config: Option<BootstrapConfig>,
    /// A boolean indicating whether UPnP should be disabled.
    no_upnp: bool,
    /// A boolean indicating whether relay client mode should be enabled.
    /// Enable this for nodes behind NAT that cannot use UPnP.
    relay_client: bool,
    /// An optional `PathBuf` representing the root directory for the node.
    root_dir: Option<PathBuf>,
}

impl NodeSpawner {
    /// Create a new instance of `NodeSpawner` with default values.
    ///
    /// # Default Values
    ///
    /// - `socket_addr`: `0.0.0.0:0` (all interfaces, OS-assigned port)
    /// - `evm_network`: `EvmNetwork::default()` (ArbitrumOne mainnet)
    /// - `rewards_address`: `RewardsAddress::default()` (**zero address - rewards burned!**)
    /// - `bootstrap_config`: `None`
    /// - `no_upnp`: `false`
    /// - `relay_client`: `false`
    /// - `root_dir`: `None`
    pub fn new() -> Self {
        Self {
            socket_addr: SocketAddr::new(IpAddr::from(Ipv4Addr::UNSPECIFIED), 0),
            evm_network: Default::default(),
            rewards_address: Default::default(),
            bootstrap_config: None,
            no_upnp: false,
            relay_client: false,
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

    /// Set the bootstrap configuration for the node.
    ///
    /// # Arguments
    ///
    /// * `bootstrap_config` - The `BootstrapConfig` containing bootstrap configuration.
    pub fn with_bootstrap_config(mut self, bootstrap_config: BootstrapConfig) -> Self {
        self.bootstrap_config = Some(bootstrap_config);
        self
    }

    /// Set the to disable UPnP on the node.
    ///
    /// # Arguments
    ///
    /// * `no_upnp` - A boolean indicating whether UPnP should be disabled.
    pub fn with_no_upnp(mut self, no_upnp: bool) -> Self {
        self.no_upnp = no_upnp;
        self
    }

    /// Enable relay client mode for nodes behind NAT.
    ///
    /// When enabled, the node will connect to the network via relay servers.
    /// This is necessary for nodes behind NAT that cannot use UPnP.
    ///
    /// # Arguments
    ///
    /// * `relay_client` - A boolean indicating whether relay client mode should be enabled.
    pub fn with_relay_client(mut self, relay_client: bool) -> Self {
        self.relay_client = relay_client;
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
            self.bootstrap_config,
            self.no_upnp,
            self.relay_client,
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
    bootstrap_config: Option<BootstrapConfig>,
    no_upnp: bool,
    relay_client: bool,
    root_dir: &Option<PathBuf>,
) -> eyre::Result<RunningNode> {
    // Warn if using the zero address (default) - rewards would be lost
    if rewards_address == RewardsAddress::default() {
        warn!(
            "Using zero address (0x0...0) for rewards. \
             Any node rewards will be burned! \
             Use .with_rewards_address() to set your wallet address."
        );
    }

    let (root_dir, keypair) = get_root_dir_and_keypair(root_dir)?;

    let bootstrap_config = bootstrap_config.unwrap_or_default();
    let local = bootstrap_config.local;
    let bootstrap = Bootstrap::new(bootstrap_config).await?;

    let mut node_builder = NodeBuilder::new(
        keypair,
        bootstrap,
        rewards_address,
        evm_network,
        socket_addr,
        root_dir,
    );
    node_builder.local(local);
    node_builder.no_upnp(no_upnp);
    node_builder.relay_client(relay_client);

    let running_node = node_builder.build_and_run()?;

    // Verify that node is running
    let mut retries: u8 = 0;

    let listen_addrs: Vec<Multiaddr> = loop {
        // Wait till we have at least 1 listen addrs
        if let Ok(listen_addrs) = running_node.get_listen_addrs().await
            && !listen_addrs.is_empty()
        {
            break Ok(listen_addrs);
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
        let evm_network = EvmNetwork::ArbitrumSepoliaTest;

        let bootstrap_config = BootstrapConfig::new(true)
            .with_first(true)
            .with_disable_cache_reading(true)
            .with_disable_env_peers(true);

        let running_node = NodeSpawner::new()
            .with_evm_network(evm_network)
            .with_bootstrap_config(bootstrap_config)
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
