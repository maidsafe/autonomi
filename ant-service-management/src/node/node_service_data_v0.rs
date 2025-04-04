// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::node_service_data_v1::{NodeServiceDataV1, NODE_SERVICE_DATA_SCHEMA_V1};
use super::NodeServiceData;
use crate::ServiceStatus;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use libp2p::{Multiaddr, PeerId};
use serde::Deserialize;
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};

#[derive(Deserialize)]
pub(super) struct NodeServiceDataV0 {
    pub antnode_path: PathBuf,
    #[serde(default)]
    pub auto_restart: bool,
    #[serde(deserialize_with = "NodeServiceData::deserialize_connected_peers")]
    pub connected_peers: Option<Vec<PeerId>>,
    pub data_dir_path: PathBuf,
    #[serde(default)]
    pub evm_network: EvmNetwork,
    #[serde(default)]
    pub home_network: bool,
    pub listen_addr: Option<Vec<Multiaddr>>,
    pub log_dir_path: PathBuf,
    pub log_format: Option<LogFormat>,
    pub max_archived_log_files: Option<usize>,
    pub max_log_files: Option<usize>,
    #[serde(default)]
    pub metrics_port: Option<u16>,
    pub network_id: Option<u8>,
    #[serde(default)]
    pub node_ip: Option<Ipv4Addr>,
    #[serde(default)]
    pub node_port: Option<u16>,
    pub number: u16,
    #[serde(deserialize_with = "NodeServiceData::deserialize_peer_id")]
    pub peer_id: Option<PeerId>,
    pub peers_args: InitialPeersConfig,
    pub pid: Option<u32>,
    #[serde(default)]
    pub rewards_address: RewardsAddress,
    pub reward_balance: Option<AttoTokens>,
    pub rpc_socket_addr: SocketAddr,
    pub service_name: String,
    pub status: ServiceStatus,
    #[serde(default = "default_upnp")]
    pub upnp: bool,
    pub user: Option<String>,
    pub user_mode: bool,
    pub version: String,
}

fn default_upnp() -> bool {
    true
}

impl From<NodeServiceDataV0> for NodeServiceDataV1 {
    fn from(v0: NodeServiceDataV0) -> Self {
        NodeServiceDataV1 {
            schema_version: NODE_SERVICE_DATA_SCHEMA_V1,
            antnode_path: v0.antnode_path,
            auto_restart: v0.auto_restart,
            connected_peers: v0.connected_peers,
            data_dir_path: v0.data_dir_path,
            evm_network: v0.evm_network,
            // Renamed field
            initial_peers_config: v0.peers_args,
            listen_addr: v0.listen_addr,
            log_dir_path: v0.log_dir_path,
            log_format: v0.log_format,
            max_archived_log_files: v0.max_archived_log_files,
            max_log_files: v0.max_log_files,
            metrics_port: v0.metrics_port,
            network_id: v0.network_id,
            node_ip: v0.node_ip,
            node_port: v0.node_port,
            // Inverted boolean value
            no_upnp: !v0.upnp,
            number: v0.number,
            peer_id: v0.peer_id,
            pid: v0.pid,
            // Renamed field
            relay: v0.home_network,
            rewards_address: v0.rewards_address,
            reward_balance: v0.reward_balance,
            rpc_socket_addr: v0.rpc_socket_addr,
            service_name: v0.service_name,
            status: v0.status,
            user: v0.user,
            user_mode: v0.user_mode,
            version: v0.version,
        }
    }
}
