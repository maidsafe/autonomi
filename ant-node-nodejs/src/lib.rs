//! Node.js bindings for ant-node.
//!
//! This library provides Node.js bindings for the ant-node library, which
//! provides network spawning capabilities and convergent encryption on file-based data.

use ant_node::BootstrapConfig;
use ant_node::spawn::node_spawner::Multiaddr;
use napi::bindgen_prelude::*;
use napi::tokio::sync::Mutex;
use napi::{Result, Status};
use napi_derive::napi;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr as _;
use std::time::Duration;

// Convert Rust errors to JavaScript errors
fn map_error<E>(err: E) -> napi::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    let mut err_str = String::new();
    err_str.push_str(&format!("{err:?}: {err}\n"));
    let mut source = err.source();
    while let Some(err) = source {
        err_str.push_str(&format!(" Caused by: {err:?}: {err}\n"));
        source = err.source();
    }

    napi::Error::new(Status::GenericFailure, err_str)
}

fn _try_from_big_int<T: TryFrom<u64>>(value: BigInt, arg: &str) -> Result<T> {
    let (_signed, value, losless) = value.get_u64();
    if losless && let Ok(value) = T::try_from(value) {
        return Ok(value);
    }

    Err(napi::Error::new(
        Status::InvalidArg,
        format!(
            "expected `{arg}` to fit in a {}",
            std::any::type_name::<T>()
        ),
    ))
}

// Convert BootstrapConfigFields to BootstrapConfig
fn convert_bootstrap_config(config: BootstrapConfigFields) -> Result<BootstrapConfig> {
    let initial_peers = if let Some(peers) = config.initial_peers {
        peers
            .iter()
            .map(|peer| {
                peer.parse::<Multiaddr>().map_err(|e| {
                    napi::Error::new(
                        Status::InvalidArg,
                        format!("Invalid initial peer address format: {e}"),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![]
    };

    let mut bootstrap_config = BootstrapConfig::new(config.local.unwrap_or(false));

    if let Some(backwards_compatible_writes) = config.backwards_compatible_writes {
        bootstrap_config =
            bootstrap_config.with_backwards_compatible_writes(backwards_compatible_writes);
    }
    if let Some(cache_dir) = config.cache_dir {
        bootstrap_config = bootstrap_config.with_cache_dir(PathBuf::from(cache_dir));
    }
    if let Some(cache_save_scaling_factor) = config.cache_save_scaling_factor {
        bootstrap_config.cache_save_scaling_factor = cache_save_scaling_factor;
    }
    if let Some(disable_cache_writing) = config.disable_cache_writing {
        bootstrap_config.disable_cache_writing = disable_cache_writing;
    }
    if let Some(disable_cache_reading) = config.disable_cache_reading {
        bootstrap_config.disable_cache_reading = disable_cache_reading;
    }
    if let Some(disable_env_peers) = config.disable_env_peers {
        bootstrap_config.disable_env_peers = disable_env_peers;
    }
    if let Some(first) = config.first {
        bootstrap_config = bootstrap_config.with_first(first);
    }
    if !initial_peers.is_empty() {
        bootstrap_config = bootstrap_config.with_initial_peers(initial_peers);
    }
    if let Some(max_cached_peers) = config.max_cached_peers {
        bootstrap_config = bootstrap_config.with_max_cached_peers(max_cached_peers as usize);
    }
    if let Some(max_addrs_per_cached_peer) = config.max_addrs_per_cached_peer {
        bootstrap_config =
            bootstrap_config.with_max_addrs_per_cached_peer(max_addrs_per_cached_peer as usize);
    }
    if let Some(min_duration_ms) = config.min_cache_save_duration_ms {
        bootstrap_config.min_cache_save_duration = Duration::from_millis(min_duration_ms as u64);
    }
    if let Some(max_duration_ms) = config.max_cache_save_duration_ms {
        bootstrap_config.max_cache_save_duration = Duration::from_millis(max_duration_ms as u64);
    }
    if let Some(network_contacts_url) = config.network_contacts_url {
        bootstrap_config.network_contacts_url = network_contacts_url;
    }

    Ok(bootstrap_config)
}

#[napi]
pub struct SwarmLocalState(ant_node::SwarmLocalState);

#[napi]
impl SwarmLocalState {
    #[napi(getter)]
    pub fn connected_peers(&self) -> Vec<String> {
        self.0
            .connected_peers
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[napi(getter)]
    pub fn peers_in_routing_table(&self) -> u32 {
        self.0.peers_in_routing_table as u32
    }

    #[napi(getter)]
    pub fn listeners(&self) -> Vec<String> {
        self.0.listeners.iter().map(ToString::to_string).collect()
    }
}

/// Once a node is started and running, the user obtains a NodeRunning object which can be used to interact with it.
#[napi]
pub struct RunningNode(ant_node::RunningNode);

#[napi]
impl RunningNode {
    /// Returns this node's `PeerId`
    #[napi]
    pub fn peer_id(&self) -> String {
        self.0.peer_id().to_string()
    }

    /// Returns the root directory path for the node.
    ///
    /// This will either be a value defined by the user, or a default location, plus the peer ID
    /// appended. The default location is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/node/<peer-id>
    ///  - macOS: $HOME/Library/Application Support/autonomi/node/<peer-id>
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\node\<peer-id>
    #[allow(rustdoc::invalid_html_tags)]
    #[napi]
    pub fn root_dir_path(&self) -> String {
        self.0.root_dir_path().to_string_lossy().to_string()
    }

    /// Returns a `SwarmLocalState` with some information obtained from swarm's local state.
    #[napi]
    pub async fn get_swarm_local_state(&self) -> Result<SwarmLocalState> {
        self.0
            .get_swarm_local_state()
            .await
            .map(SwarmLocalState)
            .map_err(map_error)
    }

    /// Return the node's listening addresses.
    #[napi]
    pub async fn get_listen_addrs(&self) -> Result<Vec<String>> {
        self.0
            .get_listen_addrs()
            .await
            .map(|addrs| addrs.iter().map(|addr| addr.to_string()).collect())
            .map_err(map_error)
    }

    /// Return the node's listening addresses with the peer id appended.
    #[napi]
    pub async fn get_listen_addrs_with_peer_id(&self) -> Result<Vec<String>> {
        self.0
            .get_listen_addrs_with_peer_id()
            .await
            .map(|addrs| addrs.iter().map(|addr| addr.to_string()).collect())
            .map_err(map_error)
    }

    /// Return the node's listening port
    #[napi]
    pub async fn get_node_listening_port(&self) -> Result<u16> {
        self.0.get_node_listening_port().await.map_err(map_error)
    }

    // /// Returns the node events channel where to subscribe to receive `NodeEvent`s
    // #[napi]
    // pub fn node_events_channel(&self) -> &NodeEventsChannel {
    //     self.0.node_events_channel()
    // }

    /// Returns the list of all the RecordKeys held by the node
    #[napi]
    pub async fn get_all_record_addresses(&self) -> Result<Vec<Vec<u8>>> {
        self.0
            .get_all_record_addresses()
            .await
            .map(|addrs| addrs.iter().map(|addr| addr.as_bytes()).collect())
            .map_err(map_error)
    }

    // /// Returns a map where each key is the ilog2 distance of that Kbucket and each value is a vector of peers in that
    // /// bucket.
    // #[napi]
    // pub async fn get_kbuckets(&self) -> Result<BTreeMap<u32, Vec<PeerId>>> {
    //     self.0.get_kbuckets()
    // }

    /// Returns the node's reward address
    #[napi]
    pub fn reward_address(&self) -> Vec<u8> {
        self.0.reward_address().to_vec()
    }

    /// Shutdown the SwarmDriver loop and the node (NetworkEvents) loop.
    #[napi]
    pub fn shutdown(&self) {
        self.0.clone().shutdown()
    }
}

/// Represents a running test network.
#[napi]
pub struct RunningNetwork(Mutex<Option<ant_node::spawn::network_spawner::RunningNetwork>>);

#[napi]
impl RunningNetwork {
    /// Returns a bootstrap peer from this network.
    #[napi]
    pub async fn bootstrap_peer(&self) -> Result<String> {
        let running_network = self.0.lock().await;
        let running_network = running_network.as_ref().ok_or_else(|| {
            napi::Error::new(Status::GenericFailure, "Network has already been shutdown")
        })?;

        let peer = running_network.bootstrap_peer().await;
        Ok(peer.to_string())
    }

    /// Returns a bootstrap peer from this network.
    #[napi]
    pub async fn running_nodes(&self) -> Result<Vec<RunningNode>> {
        let running_network = self.0.lock().await;
        let running_network = running_network.as_ref().ok_or_else(|| {
            napi::Error::new(Status::GenericFailure, "Network has already been shutdown")
        })?;

        let nodes = running_network
            .running_nodes()
            .iter()
            .map(|node| RunningNode(node.clone()))
            .collect::<Vec<_>>();
        Ok(nodes)
    }

    /// Shutdown all running nodes.
    #[napi]
    pub async fn shutdown(&self) -> Result<()> {
        let mut running_network = self.0.lock().await;
        let running_network = running_network.take().ok_or_else(|| {
            napi::Error::new(Status::GenericFailure, "Network has already been shutdown")
        })?;

        running_network.shutdown();
        Ok(())
    }
}

/// A spawner for creating local SAFE networks for testing and development.
#[napi]
pub struct NetworkSpawner(ant_node::spawn::network_spawner::NetworkSpawner);

#[napi]
impl NetworkSpawner {
    #[napi(constructor)]
    pub fn new(args: Option<NetworkSpawnerFields>, network: Option<&Network>) -> Result<Self> {
        let mut spawner = ant_node::spawn::network_spawner::NetworkSpawner::new();

        if let Some(evm_network) = network {
            spawner = spawner.with_evm_network(evm_network.0.clone());
        }

        if let Some(args) = args {
            if let Some(rewards_address) = args.rewards_address {
                spawner = spawner.with_rewards_address(
                    ant_node::spawn::node_spawner::RewardsAddress::from_str(&rewards_address)
                        .map_err(|e| {
                            napi::Error::new(
                                Status::InvalidArg,
                                format!("Invalid rewards address format: {e:?}"),
                            )
                        })?,
                );
            }
            if let Some(bootstrap_config) = args.bootstrap_config {
                let config = convert_bootstrap_config(bootstrap_config)?;
                spawner = spawner.with_bootstrap_config(config);
            }
            if let Some(no_upnp) = args.no_upnp {
                spawner = spawner.with_no_upnp(no_upnp);
            }
            if let Some(root_dir) = args.root_dir {
                spawner = spawner.with_root_dir(root_dir.map(PathBuf::from));
            }
            if let Some(size) = args.size {
                spawner = spawner.with_size(size as usize);
            }
        }

        Ok(Self(spawner))
    }

    /// Spawns the network with the configured parameters.
    #[napi]
    pub async fn spawn(&self) -> Result<RunningNetwork> {
        let running_network = self.0.clone().spawn().await.map_err(|e| {
            napi::Error::new(
                Status::GenericFailure,
                format!("Failed to spawn network: {e}"),
            )
        })?;

        Ok(RunningNetwork(Mutex::new(Some(running_network))))
    }
}

#[napi(object)]
pub struct BootstrapConfigFields {
    pub backwards_compatible_writes: Option<bool>,
    pub cache_dir: Option<String>,
    pub cache_save_scaling_factor: Option<u32>,
    pub disable_cache_writing: Option<bool>,
    pub disable_cache_reading: Option<bool>,
    pub disable_env_peers: Option<bool>,
    pub first: Option<bool>,
    pub initial_peers: Option<Vec<String>>,
    pub local: Option<bool>,
    pub max_cached_peers: Option<u32>,
    pub max_addrs_per_cached_peer: Option<u32>,
    pub min_cache_save_duration_ms: Option<u32>,
    pub max_cache_save_duration_ms: Option<u32>,
    pub network_contacts_url: Option<Vec<String>>,
}

#[napi(object)]
pub struct NetworkSpawnerFields {
    /// NOTE: Currently dead code - not read by constructor.
    /// The EVM network is passed via a separate `network` parameter.
    /// TODO: Future cleanup should read this field instead for consistency.
    pub evm_network: Option<String>,
    pub rewards_address: Option<String>,
    pub no_upnp: Option<bool>,
    pub root_dir: Option<Option<String>>,
    pub size: Option<u32>,
    pub bootstrap_config: Option<BootstrapConfigFields>,
}

#[napi(object)]
pub struct NodeSpawnerFields {
    /// NOTE: Currently dead code - not read by constructor.
    /// The EVM network is passed via a separate `network` parameter.
    /// TODO: Future cleanup should read this field instead for consistency.
    pub evm_network: Option<String>,
    pub socket_addr: Option<String>,
    pub rewards_address: Option<String>,
    pub no_upnp: Option<bool>,
    pub root_dir: Option<Option<String>>,
    pub bootstrap_config: Option<BootstrapConfigFields>,
}

/// A spawner for creating local SAFE networks for testing and development.
#[napi]
pub struct NodeSpawner(ant_node::spawn::node_spawner::NodeSpawner);

#[napi]
impl NodeSpawner {
    #[napi(constructor)]
    pub fn new(args: Option<NodeSpawnerFields>, network: Option<&Network>) -> Result<Self> {
        let mut spawner = ant_node::spawn::node_spawner::NodeSpawner::new();
        if let Some(args) = args {
            if let Some(evm_network) = network {
                spawner = spawner.with_evm_network(evm_network.0.clone());
            }
            if let Some(socket_addr) = args.socket_addr {
                spawner =
                    spawner.with_socket_addr(SocketAddr::from_str(&socket_addr).map_err(|e| {
                        napi::Error::new(
                            Status::InvalidArg,
                            format!("Invalid socket address format: {e}"),
                        )
                    })?);
            }
            if let Some(rewards_address) = args.rewards_address {
                spawner = spawner.with_rewards_address(
                    ant_node::spawn::node_spawner::RewardsAddress::from_str(&rewards_address)
                        .map_err(|e| {
                            napi::Error::new(
                                Status::InvalidArg,
                                format!("Invalid rewards address format: {e:?}"),
                            )
                        })?,
                );
            }
            if let Some(bootstrap_config) = args.bootstrap_config {
                let config = convert_bootstrap_config(bootstrap_config)?;
                spawner = spawner.with_bootstrap_config(config);
            }
            if let Some(no_upnp) = args.no_upnp {
                spawner = spawner.with_no_upnp(no_upnp);
            }
            if let Some(root_dir) = args.root_dir {
                spawner = spawner.with_root_dir(root_dir.map(PathBuf::from));
            }
        }

        Ok(Self(spawner))
    }

    /// Spawns the network with the configured parameters.
    #[napi]
    pub async fn spawn(&self) -> Result<RunningNode> {
        let running_node = self.0.clone().spawn().await.map_err(|e| {
            napi::Error::new(Status::GenericFailure, format!("Failed to spawn node: {e}"))
        })?;

        Ok(RunningNode(running_node))
    }
}

#[napi]
pub struct Network(ant_node::spawn::node_spawner::EvmNetwork);

#[napi]
impl Network {
    #[napi(constructor)]
    pub fn new(local: bool) -> Result<Self> {
        let network = ant_node::spawn::node_spawner::EvmNetwork::new(local).map_err(map_error)?;
        Ok(Self(network))
    }

    #[napi]
    pub fn from_string(name: String) -> Result<Self> {
        let network = ant_node::spawn::node_spawner::EvmNetwork::from_str(&name).map_err(|()| {
            napi::Error::new(Status::InvalidArg, format!("Invalid network name '{name}'"))
        })?;
        Ok(Self(network))
    }
}

/// A local EVM testnet for development and testing.
///
/// Starts an Anvil node and deploys the required smart contracts.
/// Requires Anvil to be installed (via Foundry).
#[napi]
pub struct Testnet(evmlib::testnet::Testnet);

#[napi]
impl Testnet {
    /// Creates and starts a new local EVM testnet.
    ///
    /// This starts an Anvil node and deploys:
    /// - Network token contract
    /// - Data payments contract
    /// - Merkle payments contract
    #[napi(factory, js_name = "new")]
    pub async fn new_testnet() -> Result<Self> {
        let testnet = evmlib::testnet::Testnet::new().await;
        Ok(Self(testnet))
    }

    /// Returns the Network configuration for this testnet.
    /// Use this with NetworkSpawner to spawn nodes connected to this EVM.
    #[napi]
    pub fn to_network(&self) -> Network {
        Network(self.0.to_network())
    }

    /// Returns the default wallet private key (from Anvil's first account).
    /// This wallet has pre-funded ETH for testing.
    #[napi]
    pub fn default_wallet_private_key(&self) -> String {
        self.0.default_wallet_private_key()
    }

    /// Returns the RPC URL of the local Anvil node.
    #[napi]
    pub fn rpc_url(&self) -> String {
        self.0.to_network().rpc_url().to_string()
    }
}
