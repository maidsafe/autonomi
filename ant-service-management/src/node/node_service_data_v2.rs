// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::node_service_data_v3::NodeServiceDataV3;
use super::node_service_data_v3::NODE_SERVICE_DATA_SCHEMA_V3;
use super::NodeServiceData;
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

pub const NODE_SERVICE_DATA_SCHEMA_V2: u32 = 2;

fn schema_v2_value() -> u32 {
    NODE_SERVICE_DATA_SCHEMA_V2
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

impl From<NodeServiceDataV2> for NodeServiceDataV3 {
    fn from(v2: NodeServiceDataV2) -> Self {
        NodeServiceDataV3 {
            alpha: v2.alpha,
            antnode_path: v2.antnode_path,
            auto_restart: v2.auto_restart,
            connected_peers: v2.connected_peers,
            data_dir_path: v2.data_dir_path,
            evm_network: v2.evm_network,
            initial_peers_config: v2.initial_peers_config,
            listen_addr: v2.listen_addr,
            log_dir_path: v2.log_dir_path,
            log_format: v2.log_format,
            max_archived_log_files: v2.max_archived_log_files,
            max_log_files: v2.max_log_files,
            metrics_port: v2.metrics_port,
            network_id: v2.network_id,
            node_ip: v2.node_ip,
            node_port: v2.node_port,
            no_upnp: v2.no_upnp,
            number: v2.number,
            peer_id: v2.peer_id,
            pid: v2.pid,
            reachability_check: false, // Default value for upgraded instances
            relay: v2.relay,
            rewards_address: v2.rewards_address,
            reward_balance: v2.reward_balance,
            rpc_socket_addr: v2.rpc_socket_addr,
            schema_version: NODE_SERVICE_DATA_SCHEMA_V3,
            service_name: v2.service_name,
            status: v2.status,
            user: v2.user,
            user_mode: v2.user_mode,
            version: v2.version,
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

        let v2_json = serde_json::to_value(&v2_data).unwrap();
        let latest: NodeServiceData = serde_json::from_value(v2_json).unwrap();

        // Verify it's the latest version
        assert_eq!(latest.schema_version, NODE_SERVICE_DATA_SCHEMA_LATEST);
    }

    #[test]
    fn test_v2_to_v3_conversion() {
        let v2_data = NodeServiceDataV2 {
            alpha: true,
            schema_version: NODE_SERVICE_DATA_SCHEMA_V2,
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

        let v3: NodeServiceDataV3 = v2_data.into();

        // Check field transformations
        assert!(!v3.reachability_check); // V3 adds reachability_check field and sets it to false
        assert!(!v3.relay); // V2 field preserved
        assert!(v3.alpha); // V2 field preserved
        assert!(v3.no_upnp); // V2 field preserved
    }
}
