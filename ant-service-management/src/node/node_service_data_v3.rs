// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::NodeServiceData;
use crate::{ServiceStatus, error::Result};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};

pub const NODE_SERVICE_DATA_SCHEMA_V3: u32 = 3;

fn schema_v3_value() -> u32 {
    NODE_SERVICE_DATA_SCHEMA_V3
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NodeServiceDataV3 {
    #[serde(default)]
    pub alpha: bool,
    pub antnode_path: PathBuf,
    #[serde(default)]
    pub auto_restart: bool,
    /// Updated the connected_peers field to be a count instead of a list.
    pub connected_peers: u32,
    pub data_dir_path: PathBuf,
    #[serde(default)]
    pub evm_network: EvmNetwork,
    pub initial_peers_config: InitialPeersConfig,
    pub listen_addr: Option<Vec<Multiaddr>>,
    pub log_dir_path: PathBuf,
    pub log_format: Option<LogFormat>,
    pub max_archived_log_files: Option<usize>,
    pub max_log_files: Option<usize>,
    /// Updated the metrics_port field to be a required field.
    pub metrics_port: u16,
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
    /// Removed reward_balance field in V3
    /// Updated rpc_socket_addr to be optional
    pub rpc_socket_addr: Option<SocketAddr>,
    #[serde(default = "schema_v3_value")]
    pub schema_version: u32,
    pub service_name: String,
    /// Added `skip_reachability_check` to indicate if the node should skip performing the reachability check.
    pub skip_reachability_check: bool,
    pub status: ServiceStatus,
    pub user: Option<String>,
    pub user_mode: bool,
    pub version: String,
    pub write_older_cache_files: bool,
}

// Helper method for direct V3 deserialization
impl NodeServiceDataV3 {
    pub fn deserialize_v3<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Define a helper struct that matches V3 exactly
        #[derive(Deserialize)]
        struct NodeServiceDataV3Helper {
            #[serde(default)]
            alpha: bool,
            antnode_path: PathBuf,
            #[serde(default)]
            auto_restart: bool,
            connected_peers: u32,
            data_dir_path: PathBuf,
            #[serde(default)]
            evm_network: EvmNetwork,
            initial_peers_config: InitialPeersConfig,
            listen_addr: Option<Vec<Multiaddr>>,
            log_dir_path: PathBuf,
            log_format: Option<LogFormat>,
            max_archived_log_files: Option<usize>,
            max_log_files: Option<usize>,
            metrics_port: u16,
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
            skip_reachability_check: bool,
            relay: bool,
            #[serde(default)]
            rewards_address: RewardsAddress,
            rpc_socket_addr: Option<SocketAddr>,
            #[serde(default = "schema_v3_value")]
            schema_version: u32,
            service_name: String,
            status: ServiceStatus,
            user: Option<String>,
            user_mode: bool,
            version: String,
            write_older_cache_files: bool,
        }

        let helper = NodeServiceDataV3Helper::deserialize(deserializer)?;

        Ok(Self {
            alpha: helper.alpha,
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
            rpc_socket_addr: helper.rpc_socket_addr,
            service_name: helper.service_name,
            schema_version: helper.schema_version,
            skip_reachability_check: helper.skip_reachability_check,
            status: helper.status,
            user: helper.user,
            user_mode: helper.user_mode,
            version: helper.version,
            write_older_cache_files: helper.write_older_cache_files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::node_service_data::NodeServiceData;
    use crate::{
        ServiceStatus,
        node::{
            NODE_SERVICE_DATA_SCHEMA_LATEST,
            node_service_data_v3::{NODE_SERVICE_DATA_SCHEMA_V3, NodeServiceDataV3},
        },
    };
    use ant_bootstrap::InitialPeersConfig;
    use ant_evm::EvmNetwork;
    use std::path::PathBuf;

    #[test]
    fn test_v3_conversion_to_latest() {
        let v3_data = NodeServiceDataV3 {
            alpha: true,
            schema_version: NODE_SERVICE_DATA_SCHEMA_V3,
            antnode_path: PathBuf::from("/usr/bin/antnode"),
            data_dir_path: PathBuf::from("/data"),
            log_dir_path: PathBuf::from("/logs"),
            number: 1,
            rpc_socket_addr: None,
            service_name: "test".to_string(),
            status: ServiceStatus::Running,
            user_mode: true,
            version: "0.1.0".to_string(),
            no_upnp: false,
            relay: true,
            auto_restart: false,
            connected_peers: 10,
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
            metrics_port: 0,
            network_id: None,
            node_ip: None,
            node_port: None,
            peer_id: None,
            pid: None,
            rewards_address: Default::default(),
            skip_reachability_check: true,
            user: None,
            write_older_cache_files: false,
        };

        let v3_json = serde_json::to_value(&v3_data).unwrap();
        let latest: NodeServiceData = serde_json::from_value(v3_json).unwrap();

        // Verify it's the latest version
        assert_eq!(latest.schema_version, NODE_SERVICE_DATA_SCHEMA_LATEST);
    }

    // V3 is the latest version, so no direct conversion test needed
}
