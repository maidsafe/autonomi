// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

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

pub const NODE_SERVICE_DATA_SCHEMA_V1: u32 = 1;

fn schema_v1_value() -> u32 {
    NODE_SERVICE_DATA_SCHEMA_V1
}

#[derive(Clone, Debug, Serialize)]
pub struct NodeServiceDataV1 {
    #[serde(default = "schema_v1_value")]
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

// Helper method for direct V1 deserialization
impl NodeServiceDataV1 {
    pub fn deserialize_v1<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Define a helper struct that matches V1 exactly
        #[derive(Deserialize)]
        struct NodeServiceDataV1Helper {
            #[serde(default = "schema_v1_value")]
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
        }

        let helper = NodeServiceDataV1Helper::deserialize(deserializer)?;

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
        })
    }
}
