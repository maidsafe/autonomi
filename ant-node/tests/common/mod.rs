// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
#![allow(dead_code)]

pub mod client;

use self::client::LocalNetwork;
use ant_protocol::antnode_proto::{NodeInfoRequest, ant_node_client::AntNodeClient};
use ant_service_management::{
    NodeRegistryManager, antctl_proto::ant_ctl_client::AntCtlClient, get_local_node_registry_path,
};
use eyre::{OptionExt, Result, bail, eyre};
use libp2p::PeerId;
use std::{net::SocketAddr, time::Duration};
use tonic::Request;
use tracing::{debug, error, warn};

// Connect to a RPC socket addr with retry
pub async fn get_antnode_rpc_client(
    socket_addr: SocketAddr,
) -> Result<AntNodeClient<tonic::transport::Channel>> {
    // get the new PeerId for the current NodeIndex
    let endpoint = format!("https://{socket_addr}");
    let mut attempts = 0;
    loop {
        if let Ok(rpc_client) = AntNodeClient::connect(endpoint.clone()).await {
            break Ok(rpc_client);
        }
        attempts += 1;
        println!("Could not connect to rpc {endpoint:?}. Attempts: {attempts:?}/10");
        error!("Could not connect to rpc {endpoint:?}. Attempts: {attempts:?}/10");
        tokio::time::sleep(Duration::from_secs(1)).await;
        if attempts >= 10 {
            bail!("Failed to connect to {endpoint:?} even after 10 retries");
        }
    }
}

// Connect to a RPC socket addr with retry
pub async fn get_antctl_rpc_client(
    socket_addr: SocketAddr,
) -> Result<AntCtlClient<tonic::transport::Channel>> {
    // get the new PeerId for the current NodeIndex
    let endpoint = format!("https://{socket_addr}");
    let mut attempts = 0;
    loop {
        if let Ok(rpc_client) = AntCtlClient::connect(endpoint.clone()).await {
            break Ok(rpc_client);
        }
        attempts += 1;
        println!("Could not connect to rpc {endpoint:?}. Attempts: {attempts:?}/10");
        error!("Could not connect to rpc {endpoint:?}. Attempts: {attempts:?}/10");
        tokio::time::sleep(Duration::from_secs(1)).await;
        if attempts >= 10 {
            bail!("Failed to connect to {endpoint:?} even after 10 retries");
        }
    }
}

// Returns all the PeerId for all the running nodes
pub async fn get_all_peer_ids(node_rpc_addresses: &Vec<SocketAddr>) -> Result<Vec<PeerId>> {
    let mut all_peers = Vec::new();

    for addr in node_rpc_addresses {
        let mut rpc_client = get_antnode_rpc_client(*addr).await?;

        // get the peer_id
        let response = rpc_client
            .node_info(Request::new(NodeInfoRequest {}))
            .await?;
        let peer_id = PeerId::from_bytes(&response.get_ref().peer_id)?;
        all_peers.push(peer_id);
    }
    debug!(
        "Obtained the PeerId list for the running network with a node count of {}",
        node_rpc_addresses.len()
    );
    Ok(all_peers)
}

/// A struct to facilitate restart of droplet/local nodes
pub struct NodeRestart {
    node_registry: NodeRegistryManager,
    next_to_restart_idx: usize,
    skip_genesis_for_droplet: bool,
    retain_peer_id: bool,
}

impl NodeRestart {
    /// The genesis address is skipped for droplets as we don't want to restart the Genesis node there.
    /// The restarted node relies on the genesis multiaddr to bootstrap after restart.
    ///
    /// Setting retain_peer_id will soft restart the node by keeping the old PeerId, ports, records etc.
    pub async fn new(skip_genesis_for_droplet: bool, retain_peer_id: bool) -> Result<Self> {
        let reg = NodeRegistryManager::load(&get_local_node_registry_path()?).await?;

        Ok(Self {
            node_registry: reg,
            next_to_restart_idx: 0,
            skip_genesis_for_droplet,
            retain_peer_id,
        })
    }

    /// Restart the next node in the list.
    /// Set `loop_over` to `true` if we want to start over the restart process if we have already restarted all
    /// the nodes.
    /// Set `progress_on_error` to `true` if we want to restart the next node if you call this function again.
    /// Else we'll be retrying the same node on the next call.
    ///
    /// Returns the antctl RPC service if we have restarted a node successfully.
    /// Returns `None` if `loop_over` is `false` and we have not restarted any nodes.
    pub async fn restart_next(
        &mut self,
        loop_over: bool,
        progress_on_error: bool,
    ) -> Result<Option<SocketAddr>> {
        let antnode_rpc_endpoint = {
            // check if we've reached the end
            if loop_over && self.next_to_restart_idx > self.node_registry.nodes.read().await.len() {
                self.next_to_restart_idx = 0;
            }

            let to_restart = self
                .node_registry
                .nodes
                .read()
                .await
                .get(self.next_to_restart_idx)
                .cloned();

            if let Some(node_to_restart) = to_restart {
                let node = node_to_restart.read().await;
                let peer_id = node.peer_id.ok_or_eyre(
                    "PeerId should be present for a local node in NodeRegistryManager",
                )?;
                if let Some(antnode_rpc_endpoint) = node.rpc_socket_addr {
                    self.restart(peer_id, antnode_rpc_endpoint, progress_on_error)
                        .await?;
                    Some(antnode_rpc_endpoint)
                } else {
                    warn!("Node does not have an RPC endpoint configured");
                    None
                }
            } else {
                warn!(
                    "We have restarted all the nodes in the list. Since loop_over is false, we are not restarting any nodes now."
                );
                None
            }
        };

        Ok(antnode_rpc_endpoint)
    }

    async fn restart(
        &mut self,
        peer_id: PeerId,
        endpoint: SocketAddr,
        progress_on_error: bool,
    ) -> Result<()> {
        match LocalNetwork::restart_node(endpoint, self.retain_peer_id).await
            .map_err(|err| eyre!("Failed to restart peer {peer_id:?} on antnode RPC endpoint: {endpoint:?} with err {err:?}")) {
                Ok(_) => {
                    self.next_to_restart_idx += 1;
                },
                Err(err) => {
                    if progress_on_error {
                        self.next_to_restart_idx += 1;
                    }
                    return Err(err);
                }
            }
        Ok(())
    }

    pub fn reset_index(&mut self) {
        self.next_to_restart_idx = 0;
    }
}
