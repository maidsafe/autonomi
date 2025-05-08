// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::node_service_data_v1::NodeServiceDataV1;
use super::NodeServiceData;
use crate::{error::Result, ServiceStatus};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};

pub const NODE_SERVICE_DATA_SCHEMA_V2: u32 = 2;

fn schema_v2_value() -> u32 {
    NODE_SERVICE_DATA_SCHEMA_V2
}

#[derive(Clone, Debug, Serialize)]
pub struct NodeServiceDataV2 {
    /// New field in V2: indicates if the node is running in alpha mode
    #[serde(default)]
    pub alpha: bool,
    #[serde(default = "schema_v2_value")]
    pub schema_version: u32,
    pub antnode_path: PathBuf,
    #[serde(default)]
    pub auto_restart: bool,
    #[serde(serialize_with = "NodeServiceData::serialize_connected_peers")]
    pub connected_peers: Option<Vec<PeerId>>,
    pub data_dir_path: PathBuf,
    #[serde(default)]
    pub evm_network: EvmNetwork,
    pub initial_peers_config: InitialPeersConfig,
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
    pub no_upnp: bool,
    pub number: u16,
    #[serde(serialize_with = "NodeServiceData::serialize_peer_id")]
    pub peer_id: Option<PeerId>,
    pub pid: Option<u32>,
    pub relay: bool,
    #[serde(default)]
    pub rewards_address: RewardsAddress,
    pub reward_balance: Option<AttoTokens>,
    pub rpc_socket_addr: SocketAddr,
    pub service_name: String,
    pub status: ServiceStatus,
    pub user: Option<String>,
    pub user_mode: bool,
    pub version: String,
}

// Helper method for direct V2 deserialization
impl NodeServiceDataV2 {
    pub fn deserialize_v2<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Define a helper struct that matches V2 exactly
        #[derive(Deserialize)]
        struct NodeServiceDataV2Helper {
            #[serde(default = "schema_v2_value")]
            schema_version: u32,
            antnode_path: PathBuf,
            #[serde(default)]
            auto_restart: bool,
            #[serde(deserialize_with = "NodeServiceData::deserialize_connected_peers")]
            connected_peers: Option<Vec<PeerId>>,
            data_dir_path: PathBuf,
            #[serde(default)]
            evm_network: EvmNetwork,
            initial_peers_config: InitialPeersConfig,
            listen_addr: Option<Vec<Multiaddr>>,
            log_dir_path: PathBuf,
            log_format: Option<LogFormat>,
            max_archived_log_files: Option<usize>,
            max_log_files: Option<usize>,
            #[serde(default)]
            metrics_port: Option<u16>,
            network_id: Option<u8>,
            #[serde(default)]
            node_ip: Option<Ipv4Addr>,
            #[serde(default)]
            node_port: Option<u16>,
            no_upnp: bool,
            number: u16,
            #[serde(deserialize_with = "NodeServiceData::deserialize_peer_id")]
            peer_id: Option<PeerId>,
            pid: Option<u32>,
            relay: bool,
            #[serde(default)]
            rewards_address: RewardsAddress,
            reward_balance: Option<AttoTokens>,
            rpc_socket_addr: SocketAddr,
            service_name: String,
            status: ServiceStatus,
            user: Option<String>,
            user_mode: bool,
            version: String,
            #[serde(default)]
            alpha: bool,
        }

        let helper = NodeServiceDataV2Helper::deserialize(deserializer)?;

        Ok(Self {
            schema_version: helper.schema_version,
            antnode_path: helper.antnode_path,
            auto_restart: helper.auto_restart,
            connected_peers: helper.connected_peers,
            data_dir_path: helper.data_dir_path,
            evm_network: helper.evm_network,
            initial_peers_config: helper.initial_peers_config,
            listen_addr: helper.listen_addr,
            log_dir_path: helper.log_dir_path,
            log_format: helper.log_format,
            max_archived_log_files: helper.max_archived_log_files,
            max_log_files: helper.max_log_files,
            metrics_port: helper.metrics_port,
            network_id: helper.network_id,
            node_ip: helper.node_ip,
            node_port: helper.node_port,
            no_upnp: helper.no_upnp,
            number: helper.number,
            peer_id: helper.peer_id,
            pid: helper.pid,
            relay: helper.relay,
            rewards_address: helper.rewards_address,
            reward_balance: helper.reward_balance,
            rpc_socket_addr: helper.rpc_socket_addr,
            service_name: helper.service_name,
            status: helper.status,
            user: helper.user,
            user_mode: helper.user_mode,
            version: helper.version,
            alpha: helper.alpha,
        })
    }
}

impl From<NodeServiceDataV1> for NodeServiceDataV2 {
    fn from(v1: NodeServiceDataV1) -> Self {
        NodeServiceDataV2 {
            schema_version: NODE_SERVICE_DATA_SCHEMA_V2,
            antnode_path: v1.antnode_path,
            auto_restart: v1.auto_restart,
            connected_peers: v1.connected_peers,
            data_dir_path: v1.data_dir_path,
            evm_network: v1.evm_network,
            initial_peers_config: v1.initial_peers_config,
            listen_addr: v1.listen_addr,
            log_dir_path: v1.log_dir_path,
            log_format: v1.log_format,
            max_archived_log_files: v1.max_archived_log_files,
            max_log_files: v1.max_log_files,
            metrics_port: v1.metrics_port,
            network_id: v1.network_id,
            node_ip: v1.node_ip,
            node_port: v1.node_port,
            no_upnp: v1.no_upnp,
            number: v1.number,
            peer_id: v1.peer_id,
            pid: v1.pid,
            relay: v1.relay,
            rewards_address: v1.rewards_address,
            reward_balance: v1.reward_balance,
            rpc_socket_addr: v1.rpc_socket_addr,
            service_name: v1.service_name,
            status: v1.status,
            user: v1.user,
            user_mode: v1.user_mode,
            version: v1.version,
            alpha: false, // Default value for upgraded instances
        }
    }
}
