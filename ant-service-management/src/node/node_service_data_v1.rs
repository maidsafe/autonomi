// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::NodeServiceData;
use crate::{
    node::node_service_data_v2::{NodeServiceDataV2, NODE_SERVICE_DATA_SCHEMA_V2},
    ServiceStatus,
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};

pub const NODE_SERVICE_DATA_SCHEMA_V1: u32 = 1;

fn schema_v1_value() -> u32 {
    NODE_SERVICE_DATA_SCHEMA_V1
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NodeServiceDataV1 {
    #[serde(default = "schema_v1_value")]
    /// Added schema version to the struct to handle future changes.
    pub schema_version: u32,
    pub antnode_path: PathBuf,
    #[serde(default)]
    pub auto_restart: bool,
    #[serde(
        serialize_with = "NodeServiceData::serialize_connected_peers",
        deserialize_with = "NodeServiceData::deserialize_connected_peers"
    )]
    pub connected_peers: Option<Vec<PeerId>>,
    pub data_dir_path: PathBuf,
    #[serde(default)]
    pub evm_network: EvmNetwork,
    /// Renamed `peers_args` to `initial_peers_config` for clarity.
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
    /// Renamed `upnp` to `no_upnp`.
    pub no_upnp: bool,
    pub number: u16,
    #[serde(
        serialize_with = "NodeServiceData::serialize_peer_id",
        deserialize_with = "NodeServiceData::deserialize_peer_id"
    )]
    pub peer_id: Option<PeerId>,
    pub pid: Option<u32>,
    /// Renamed `home_network` to `relay`.
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

impl From<NodeServiceDataV1> for NodeServiceDataV2 {
    fn from(v1: NodeServiceDataV1) -> Self {
        NodeServiceDataV2 {
            alpha: false, //  Default value for upgraded instances
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
            schema_version: NODE_SERVICE_DATA_SCHEMA_V2,
            service_name: v1.service_name,
            status: v1.status,
            user: v1.user,
            user_mode: v1.user_mode,
            version: v1.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::node_service_data::NodeServiceData;
    use super::super::node_service_data_v2::NodeServiceDataV2;
    use super::*;
    use crate::node::NODE_SERVICE_DATA_SCHEMA_LATEST;
    use crate::ServiceStatus;
    use ant_bootstrap::InitialPeersConfig;
    use ant_evm::EvmNetwork;
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::PathBuf,
    };

    #[test]
    fn test_v1_conversion_to_latest() {
        let v1_data = NodeServiceDataV1 {
            schema_version: NODE_SERVICE_DATA_SCHEMA_V1,
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
            // Add other required fields
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
        };

        let v1_json = serde_json::to_value(&v1_data).unwrap();
        let latest: NodeServiceData = serde_json::from_value(v1_json).unwrap();

        // Verify it's the latest version
        assert_eq!(latest.schema_version, NODE_SERVICE_DATA_SCHEMA_LATEST);
    }

    #[test]
    fn test_v1_to_v2_conversion() {
        let v1_data = NodeServiceDataV1 {
            schema_version: NODE_SERVICE_DATA_SCHEMA_V1,
            relay: false,
            no_upnp: true,
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
        };

        let v2: NodeServiceDataV2 = v1_data.into();

        // Check field transformations
        assert!(!v2.alpha); // V2 adds alpha field and sets it to false
        assert!(!v2.relay); // V1 field preserved
        assert!(v2.no_upnp); // V1 field preserved
    }
}
