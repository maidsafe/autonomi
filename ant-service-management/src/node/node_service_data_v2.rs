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

pub const NODE_SERVICE_DATA_SCHEMA_V2: u32 = 2;

fn schema_v2_value() -> u32 {
    NODE_SERVICE_DATA_SCHEMA_V2
}

#[derive(Clone, Debug, Serialize, PartialEq)]
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
    /// New field in V2: indicates if older cache files should be written
    /// Serde::default is used here for backward compatibility
    #[serde(default)]
    pub write_older_cache_files: bool,
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
            #[serde(default)]
            write_older_cache_files: bool,
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
            write_older_cache_files: helper.write_older_cache_files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::node_service_data::NodeServiceData;
    use crate::{
        node::{
            node_service_data_v2::{NodeServiceDataV2, NODE_SERVICE_DATA_SCHEMA_V2},
            NODE_SERVICE_DATA_SCHEMA_LATEST,
        },
        ServiceStatus,
    };
    use ant_bootstrap::InitialPeersConfig;
    use ant_evm::EvmNetwork;
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::PathBuf,
    };

    #[test]
    fn test_v2_conversion_to_latest() {
        let v2_data = NodeServiceDataV2 {
            alpha: true,
            schema_version: NODE_SERVICE_DATA_SCHEMA_V2,
            antnode_path: PathBuf::from("/usr/bin/antnode"),
            data_dir_path: PathBuf::from("/data"),
            log_dir_path: PathBuf::from("/logs"),
            number: 1,
            rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000),
            service_name: "test".to_string(),
            status: ServiceStatus::Running,
            user_mode: true,
            version: "0.1.0".to_string(),
            no_upnp: false,
            relay: true,
            auto_restart: false,
            connected_peers: None,
            evm_network: EvmNetwork::ArbitrumSepoliaTest,
            initial_peers_config: InitialPeersConfig {
                first: false,
                local: false,
                addrs: vec![],
                network_contacts_url: vec![],
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            listen_addr: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            peer_id: None,
            pid: None,
            rewards_address: Default::default(),
            reward_balance: None,
            user: None,
            write_older_cache_files: false,
        };

        let v2_json = serde_json::to_value(&v2_data).unwrap();
        let latest: NodeServiceData = serde_json::from_value(v2_json).unwrap();

        // Verify it's the latest version
        assert_eq!(latest.schema_version, NODE_SERVICE_DATA_SCHEMA_LATEST);
    }

    // V2 is the latest version, so no direct conversion test needed
}
