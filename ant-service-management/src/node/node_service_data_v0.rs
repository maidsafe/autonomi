// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::NodeServiceData;
use super::node_service_data_v1::{NODE_SERVICE_DATA_SCHEMA_V1, NodeServiceDataV1};
use crate::ServiceStatus;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
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

#[cfg(test)]
mod tests {
    use super::super::node_service_data::NodeServiceData;
    use super::super::node_service_data_v1::NodeServiceDataV1;
    use super::*;
    use crate::{ServiceStatus, node::NODE_SERVICE_DATA_SCHEMA_LATEST};
    use ant_bootstrap::InitialPeersConfig;
    use ant_evm::EvmNetwork;
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::PathBuf,
    };

    #[test]
    fn test_v0_conversion_to_latest() {
        let v0_data = NodeServiceDataV0 {
            antnode_path: PathBuf::from("/usr/bin/antnode"),
            data_dir_path: PathBuf::from("/data"),
            log_dir_path: PathBuf::from("/logs"),
            number: 1,
            rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000),
            service_name: "test".to_string(),
            status: ServiceStatus::Running,
            user_mode: true,
            version: "0.1.0".to_string(),
            upnp: true,
            home_network: false,
            // Add other required fields
            auto_restart: false,
            connected_peers: None,
            evm_network: EvmNetwork::ArbitrumSepoliaTest,
            listen_addr: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            peer_id: None,
            peers_args: InitialPeersConfig {
                first: false,
                local: false,
                addrs: vec![],
                network_contacts_url: vec![],
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            pid: None,
            rewards_address: Default::default(),
            reward_balance: None,
            user: None,
        };

        let v0_json = serde_json::to_value(&v0_data).unwrap();
        let latest: NodeServiceData = serde_json::from_value(v0_json).unwrap();

        // Verify it's the latest version
        assert_eq!(latest.schema_version, NODE_SERVICE_DATA_SCHEMA_LATEST);
    }

    #[test]
    fn test_v0_to_v1_conversion() {
        let v0_data = NodeServiceDataV0 {
            upnp: false,        // Should become !no_upnp in V1
            home_network: true, // Should become relay in V1
            peers_args: InitialPeersConfig {
                first: true,
                local: false,
                addrs: vec![],
                network_contacts_url: vec![],
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            // Add minimal required fields
            antnode_path: PathBuf::from("/usr/bin/antnode"),
            data_dir_path: PathBuf::from("/data"),
            log_dir_path: PathBuf::from("/logs"),
            number: 1,
            rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000),
            service_name: "test".to_string(),
            status: ServiceStatus::Running,
            user_mode: true,
            version: "0.1.0".to_string(),
            auto_restart: false,
            connected_peers: None,
            evm_network: EvmNetwork::ArbitrumSepoliaTest,
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
        };

        let v1: NodeServiceDataV1 = v0_data.into();

        // Check field transformations
        assert!(v1.no_upnp); // V0 upnp: false → V1 no_upnp: true
        assert!(v1.relay); // V0 home_network: true → V1 relay: true
        assert!(v1.initial_peers_config.first); // peers_args became initial_peers_config
    }
}
