//! Node.js bindings for ant-node.
//!
//! This library provides Node.js bindings for the ant-node library, which
//! provides network spawning capabilities and convergent encryption on file-based data.

use ant_node::spawn::node_spawner::Multiaddr;
use napi::bindgen_prelude::*;
use napi::tokio::sync::Mutex;
use napi::{Result, Status};
use napi_derive::napi;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr as _;

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
    pub fn peers_in_routing_table(&self) -> usize {
        self.0.peers_in_routing_table
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
    pub fn new(args: Option<NetworkSpawnerFields>) -> Self {
        let mut spawner = ant_node::spawn::network_spawner::NetworkSpawner::new();
        if let Some(args) = args {
            if let Some(local) = args.local {
                spawner = spawner.with_local(local);
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

        Self(spawner)
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
pub struct NetworkSpawnerFields {
    // pub evm_network: Option<ant_node::spawn::node_spawner::EvmNetwork>,
    // pub rewards_address: Option<RewardsAddress>,
    pub local: Option<bool>,
    pub no_upnp: Option<bool>,
    pub root_dir: Option<Option<String>>,
    pub size: Option<u32>,
}

#[napi(object)]
pub struct NodeSpawnerFields {
    pub evm_network: Option<String>,
    pub socket_addr: Option<String>,
    pub rewards_address: Option<String>,
    pub initial_peers: Option<Vec<String>>,
    pub local: Option<bool>,
    pub no_upnp: Option<bool>,
    pub root_dir: Option<Option<String>>,
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
                    spawner.with_socket_addr(SocketAddr::from_str(&socket_addr).map_err(|_| {
                        napi::Error::new(Status::InvalidArg, "Invalid socket address format")
                    })?);
            }
            if let Some(rewards_address) = args.rewards_address {
                spawner = spawner.with_rewards_address(
                    ant_node::spawn::node_spawner::RewardsAddress::from_str(&rewards_address)
                        .map_err(|_| {
                            napi::Error::new(Status::InvalidArg, "Invalid rewards address format")
                        })?,
                );
            }
            if let Some(initial_peers) = args.initial_peers {
                spawner = spawner.with_initial_peers(
                    initial_peers
                        .iter()
                        .map(|peer| {
                            peer.parse::<Multiaddr>().map_err(|_| {
                                napi::Error::new(
                                    Status::InvalidArg,
                                    "Invalid initial peer address format",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            if let Some(local) = args.local {
                spawner = spawner.with_local(local);
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
