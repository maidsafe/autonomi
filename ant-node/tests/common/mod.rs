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
use ant_node::spawn::network_spawner::RunningNetwork;
use ant_protocol::antnode_proto::ant_node_client::AntNodeClient;
use ant_service_management::{
    antctl_proto::ant_ctl_client::AntCtlClient, get_local_node_registry_path, NodeRegistry,
};
use eyre::{bail, eyre, OptionExt, Result};
use itertools::Either;
use libp2p::PeerId;
use std::{net::SocketAddr, time::Duration};
use test_utils::testnet::DeploymentInventory;
use tracing::{error, warn};

// type ResultRandomContent = Result<(FilesApi, Bytes, ChunkAddress, Vec<(XorName, PathBuf)>)>;

// pub fn random_content(
//     client: &Client,
//     wallet_dir: PathBuf,
//     chunk_dir: &Path,
// ) -> ResultRandomContent {
//     let mut rng = rand::thread_rng();

//     let random_len = rng.gen_range(MIN_ENCRYPTABLE_BYTES..1024 * MIN_ENCRYPTABLE_BYTES);
//     let random_length_content: Vec<u8> =
//         <Standard as Distribution<u8>>::sample_iter(Standard, &mut rng)
//             .take(random_len)
//             .collect();

//     let file_path = chunk_dir.join("random_content");
//     let mut output_file = File::create(file_path.clone())?;
//     output_file.write_all(&random_length_content)?;

//     let files_api = FilesApi::new(client.clone(), wallet_dir);
//     let (head_chunk_address, _data_map, _file_size, chunks) =
//         FilesApi::chunk_file(&file_path, chunk_dir, true)?;

//     Ok((
//         files_api,
//         random_length_content.into(),
//         head_chunk_address,
//         chunks,
//     ))
// }

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
pub fn get_all_peer_ids(running_network: &RunningNetwork) -> Result<Vec<PeerId>> {
    let mut all_peers = Vec::new();

    for node in running_network.running_nodes().iter() {
        all_peers.push(node.peer_id());
    }
    println!(
        "Obtained the PeerId list for the running network with a node count of {}",
        all_peers.len()
    );
    Ok(all_peers)
}

/// A struct to facilitate restart of droplet/local nodes
pub struct NodeRestart {
    // Deployment inventory is used incase of Droplet nodes and NodeRegistry incase of NonDroplet nodes.
    inventory_file: Either<DeploymentInventory, NodeRegistry>,
    next_to_restart_idx: usize,
    skip_genesis_for_droplet: bool,
    retain_peer_id: bool,
}

impl NodeRestart {
    /// The genesis address is skipped for droplets as we don't want to restart the Genesis node there.
    /// The restarted node relies on the genesis multiaddr to bootstrap after restart.
    ///
    /// Setting retain_peer_id will soft restart the node by keeping the old PeerId, ports, records etc.
    pub fn new(skip_genesis_for_droplet: bool, retain_peer_id: bool) -> Result<Self> {
        let inventory_file = match DeploymentInventory::load() {
            Ok(inv) => Either::Left(inv),
            Err(_) => {
                let reg = NodeRegistry::load(&get_local_node_registry_path()?)?;
                Either::Right(reg)
            }
        };

        Ok(Self {
            inventory_file,
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
        let antnode_rpc_endpoint = match self.inventory_file.clone() {
            Either::Left(inv) => {
                // check if we've reached the end
                if loop_over && self.next_to_restart_idx > inv.antctld_endpoints.len() {
                    self.next_to_restart_idx = 0;
                }

                if let Some((peer_id, daemon_endpoint)) =
                    inv.antctld_endpoints.iter().nth(self.next_to_restart_idx)
                {
                    self.restart(*peer_id, *daemon_endpoint, progress_on_error)
                        .await?;

                    let antnode_rpc_endpoint = inv
                        .rpc_endpoints
                        .get(peer_id)
                        .ok_or_eyre("Failed to obtain antnode rpc endpoint from inventory file")?;
                    Some(*antnode_rpc_endpoint)
                } else {
                    warn!("We have restarted all the nodes in the list. Since loop_over is false, we are not restarting any nodes now.");
                    None
                }
            }
            Either::Right(reg) => {
                // check if we've reached the end
                if loop_over && self.next_to_restart_idx > reg.nodes.len() {
                    self.next_to_restart_idx = 0;
                }

                if let Some((peer_id, antnode_rpc_endpoint)) = reg
                    .nodes
                    .get(self.next_to_restart_idx)
                    .map(|node| (node.peer_id, node.rpc_socket_addr))
                {
                    let peer_id =
                        peer_id.ok_or_eyre("PeerId should be present for a local node")?;
                    self.restart(peer_id, antnode_rpc_endpoint, progress_on_error)
                        .await?;
                    Some(antnode_rpc_endpoint)
                } else {
                    warn!("We have restarted all the nodes in the list. Since loop_over is false, we are not restarting any nodes now.");
                    None
                }
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
        match &self.inventory_file {
            Either::Left(_inv) =>  {
                todo!("Not implemented yet for WanNetwork");
                // match WanNetwork::restart_node(&peer_id, endpoint, self.retain_peer_id)
                //         .await
                //         .map_err(|err| eyre!("Failed to restart peer {peer_id:} on daemon endpoint: {endpoint:?} with err {err:?}")) {
                //             Ok(_) => {
                //                 self.next_to_restart_idx += 1;
                //             },
                //             Err(err) => {
                //                 if progress_on_error {
                //                     self.next_to_restart_idx += 1;
                //                 }
                //                 return Err(err);
                //             },
                //         }
            },
            Either::Right(_reg) => {
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
            }
        }
        Ok(())
    }

    pub fn reset_index(&mut self) {
        self.next_to_restart_idx = 0;
    }
}
