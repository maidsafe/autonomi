// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::Result;
use ant_protocol::get_port_from_multiaddr;
use libp2p::PeerId;
use serde::{Deserialize, Deserializer, Serializer, de::Error as DeError};
use std::str::FromStr;

/// Type alias for the latest version of the node service data structure.
pub type NodeServiceData = super::node_service_data_v3::NodeServiceDataV3;
/// Type alias for the latest node service data schema version.
pub const NODE_SERVICE_DATA_SCHEMA_LATEST: u32 =
    super::node_service_data_v3::NODE_SERVICE_DATA_SCHEMA_V3;

/// Custom deserialization for NodeServiceData
/// This will perform conversion from older version to the latest one if needed.
impl<'de> Deserialize<'de> for NodeServiceData {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json_value = serde_json::Value::deserialize(deserializer)?;

        let schema_version = match &json_value {
            serde_json::Value::Object(obj) => obj.get("schema_version").and_then(|v| v.as_u64()),
            _ => None,
        };

        match schema_version {
            Some(v) if v == super::node_service_data_v3::NODE_SERVICE_DATA_SCHEMA_V3 as u64 => {
                // It's the latest version - use NodeServiceDataV3's helper method
                match super::node_service_data_v3::NodeServiceDataV3::deserialize_v3(
                    &mut serde_json::de::Deserializer::from_str(&json_value.to_string()),
                ) {
                    Ok(v3) => Ok(v3),
                    Err(e) => Err(D::Error::custom(format!(
                        "Failed to deserialize as V3: {e}"
                    ))),
                }
            }
            Some(v) if v == super::node_service_data_v2::NODE_SERVICE_DATA_SCHEMA_V2 as u64 => {
                // It's the latest version - use NodeServiceDataV2's helper method
                match serde_json::from_value::<super::node_service_data_v2::NodeServiceDataV2>(
                    json_value,
                ) {
                    Ok(v2) => {
                        // Convert V2 to V3
                        let v3: super::node_service_data_v3::NodeServiceDataV3 = v2.into();
                        Ok(v3)
                    }
                    Err(e) => Err(D::Error::custom(format!(
                        "Failed to deserialize as V2: {e}"
                    ))),
                }
            }
            Some(v) if v == super::node_service_data_v1::NODE_SERVICE_DATA_SCHEMA_V1 as u64 => {
                // It's V1 format - deserialize and convert to V2
                match serde_json::from_value::<super::node_service_data_v1::NodeServiceDataV1>(
                    json_value,
                ) {
                    Ok(v1) => {
                        // Convert V1 to latest
                        let v2: super::node_service_data_v2::NodeServiceDataV2 = v1.into();
                        let v3: super::node_service_data_v3::NodeServiceDataV3 = v2.into();
                        Ok(v3)
                    }
                    Err(e) => Err(D::Error::custom(format!(
                        "Failed to deserialize as V1: {e}"
                    ))),
                }
            }
            _ => {
                // It's V0 format - deserialize and convert
                match serde_json::from_value::<super::node_service_data_v0::NodeServiceDataV0>(
                    json_value,
                ) {
                    Ok(v0) => {
                        // Convert V0 to latest
                        let v1: super::node_service_data_v1::NodeServiceDataV1 = v0.into();
                        let v2: super::node_service_data_v2::NodeServiceDataV2 = v1.into();
                        let v3: super::node_service_data_v3::NodeServiceDataV3 = v2.into();
                        Ok(v3)
                    }
                    Err(e) => Err(D::Error::custom(format!(
                        "Failed to deserialize as V0: {e}"
                    ))),
                }
            }
        }
    }
}

impl NodeServiceData {
    /// Returns the UDP port from our node's listen address.
    pub fn get_antnode_port(&self) -> Option<u16> {
        // assuming the listening addr contains /ip4/127.0.0.1/udp/56215/quic-v1/p2p/<peer_id>
        if let Some(multi_addrs) = &self.listen_addr {
            println!("Listening addresses are defined");
            for addr in multi_addrs {
                if let Some(port) = get_port_from_multiaddr(addr) {
                    println!("Found port: {port}");
                    return Some(port);
                }
            }
        }
        None
    }

    /// Returns an optional critical failure of the node.
    pub fn get_critical_failure(&self) -> Option<(chrono::DateTime<chrono::Utc>, String)> {
        const CRITICAL_FAILURE_LOG_FILE: &str = "critical_failure.log";

        let log_path = self.log_dir_path.join(CRITICAL_FAILURE_LOG_FILE);

        if let Ok(content) = std::fs::read_to_string(log_path) {
            if let Some((timestamp, message)) = content.split_once(']') {
                let timestamp_trimmed = timestamp.trim_start_matches('[').trim();
                if let Ok(datetime) = timestamp_trimmed.parse::<chrono::DateTime<chrono::Utc>>() {
                    let message_trimmed = message
                        .trim()
                        .trim_start_matches("Node terminated due to: ");
                    return Some((datetime, message_trimmed.to_string()));
                }
            }
        }

        None
    }

    pub fn serialize_peer_id<S>(value: &Option<PeerId>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(peer_id) = value {
            return serializer.serialize_str(&peer_id.to_string());
        }
        serializer.serialize_none()
    }

    pub fn deserialize_peer_id<'de, D>(deserializer: D) -> Result<Option<PeerId>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: Option<String> = Option::deserialize(deserializer)?;
        if let Some(peer_id_str) = s {
            PeerId::from_str(&peer_id_str)
                .map(Some)
                .map_err(DeError::custom)
        } else {
            Ok(None)
        }
    }

    pub fn serialize_connected_peers<S>(
        connected_peers: &Option<Vec<PeerId>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match connected_peers {
            Some(peers) => {
                let peer_strs: Vec<String> = peers.iter().map(|p| p.to_string()).collect();
                serializer.serialize_some(&peer_strs)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize_connected_peers<'de, D>(
        deserializer: D,
    ) -> Result<Option<Vec<PeerId>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Option<Vec<String>> = Option::deserialize(deserializer)?;
        match vec {
            Some(peer_strs) => {
                let peers: Result<Vec<PeerId>, _> = peer_strs
                    .into_iter()
                    .map(|s| PeerId::from_str(&s).map_err(DeError::custom))
                    .collect();
                peers.map(Some)
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddr},
        path::PathBuf,
    };

    use ant_bootstrap::InitialPeersConfig;
    use ant_evm::{AttoTokens, EvmNetwork, RewardsAddress};
    use ant_logging::LogFormat;
    use libp2p::Multiaddr;
    use serde::Serialize;

    use super::*;
    use crate::{ServiceStatus, node::node_service_data_v1::NODE_SERVICE_DATA_SCHEMA_V1};

    /// Test to confirm that fields can be removed from the schema without breaking deserialization.
    /// This test checks that the `disable_mainnet_contacts` field can be removed without requiring
    /// additional logic in the deserialization process.
    ///
    /// Also adding a dummy field `dummy_field` to ensure that the deserialization
    /// process does not fail when encountering fields that are not defined in the current schema.
    #[test]
    fn fields_can_be_removed_without_breaking() {
        let json_with_deprecated_field = serde_json::json!({
            "dummy_field": "This field is not used in the v1 schema",
            "schema_version": NODE_SERVICE_DATA_SCHEMA_V1,
            "antnode_path": "/usr/bin/antnode",
            "auto_restart": true,
            "connected_peers": [
                "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            ],
            "data_dir_path": "/home/user/.local/share/safe/node/1",
            "evm_network": "ArbitrumSepoliaTest",
            "initial_peers_config": {
                "first": false,
                "local": false,
                "addrs": [],
                "network_contacts_url": [],
                "disable_mainnet_contacts": false,
                "ignore_cache": false,
                "bootstrap_cache_dir": null
            },
            "listen_addr": [
                "/ip4/127.0.0.1/udp/56215/quic-v1/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            ],
            "log_dir_path": "/home/user/.local/share/safe/node/1/logs",
            "log_format": "Default",
            "max_archived_log_files": 5,
            "max_log_files": 10,
            "metrics_port": 8080,
            "network_id": 1,
            "node_ip": "127.0.0.1",
            "node_port": 56215,
            "no_upnp": false,
            "number": 1,
            "peer_id": "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN",
            "pid": 12345,
            "relay": true,
            "rewards_address": "0x1234567890123456789012345678901234567890",
            "reward_balance": "1000000000000000000",
            "rpc_socket_addr": "127.0.0.1:8000",
            "service_name": "safenode-1",
            "status": "Running",
            "user": "safe",
            "user_mode": true,
            "version": "0.1.0"
        });

        let service_data: Result<NodeServiceData, _> =
            serde_json::from_value(json_with_deprecated_field);

        assert!(
            service_data.is_ok(),
            "Failed to deserialize data with deprecated field 'disable_mainnet_contacts': {:?}",
            service_data.err()
        );

        let data = service_data.unwrap();

        assert_eq!(data.schema_version, NODE_SERVICE_DATA_SCHEMA_LATEST);
        assert_eq!(data.service_name, "safenode-1");
        assert_eq!(data.node_port, Some(56215));

        assert!(!data.initial_peers_config.first);
        assert!(!data.initial_peers_config.local);
        assert!(data.initial_peers_config.addrs.is_empty());
        assert!(data.initial_peers_config.network_contacts_url.is_empty());
        assert!(!data.initial_peers_config.ignore_cache);
        assert!(data.initial_peers_config.bootstrap_cache_dir.is_none());
    }

    /// Test to confirm that fields can be added to the schema without breaking deserialization IF `serde(default)` is
    /// used.
    /// This test checks that the `dummy_addition` field can be added without requiring
    /// additional logic in the deserialization process.
    #[test]
    fn fields_can_be_added_without_breaking_with_serde_default() {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub struct NodeServiceDataTest {
            #[serde(default)]
            pub dummy_addition: String, // New field with serde(default)
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

        let json_with_deprecated_field = serde_json::json!({
            "schema_version": NODE_SERVICE_DATA_SCHEMA_V1,
            "antnode_path": "/usr/bin/antnode",
            "auto_restart": true,
            "connected_peers": [
                "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            ],
            "data_dir_path": "/home/user/.local/share/safe/node/1",
            "evm_network": "ArbitrumSepoliaTest",
            "initial_peers_config": {
                "first": false,
                "local": false,
                "addrs": [],
                "network_contacts_url": [],
                "ignore_cache": false,
                "bootstrap_cache_dir": null
            },
            "listen_addr": [
                "/ip4/127.0.0.1/udp/56215/quic-v1/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            ],
            "log_dir_path": "/home/user/.local/share/safe/node/1/logs",
            "log_format": "Default",
            "max_archived_log_files": 5,
            "max_log_files": 10,
            "metrics_port": 8080,
            "network_id": 1,
            "node_ip": "127.0.0.1",
            "node_port": 56215,
            "no_upnp": false,
            "number": 1,
            "peer_id": "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN",
            "pid": 12345,
            "relay": true,
            "rewards_address": "0x1234567890123456789012345678901234567890",
            "reward_balance": "1000000000000000000",
            "rpc_socket_addr": "127.0.0.1:8000",
            "service_name": "safenode-1",
            "status": "Running",
            "user": "safe",
            "user_mode": true,
            "version": "0.1.0"
        });

        let service_data: Result<NodeServiceDataTest, _> =
            serde_json::from_value(json_with_deprecated_field);
        assert!(
            service_data.is_ok(),
            "Failed to deserialize data with new field 'dummy_addition': {:?}",
            service_data.err()
        );
    }

    /// Test to confirm that a new field without `serde(default)` can break the deserialization.
    ///
    /// This test checks that the `dummy_addition` field will cause a deserialization error
    /// if it is not marked with `serde(default)`.
    #[test]
    fn fields_cannot_be_added_without_serde_default() {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub struct NodeServiceDataTest {
            pub dummy_addition: String, // New field without serde(default)
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

        let json_with_deprecated_field = serde_json::json!({
            "schema_version": NODE_SERVICE_DATA_SCHEMA_V1,
            "antnode_path": "/usr/bin/antnode",
            "auto_restart": true,
            "connected_peers": [
                "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            ],
            "data_dir_path": "/home/user/.local/share/safe/node/1",
            "evm_network": "ArbitrumSepoliaTest",
            "initial_peers_config": {
                "first": false,
                "local": false,
                "addrs": [],
                "network_contacts_url": [],
                "ignore_cache": false,
                "bootstrap_cache_dir": null
            },
            "listen_addr": [
                "/ip4/127.0.0.1/udp/56215/quic-v1/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            ],
            "log_dir_path": "/home/user/.local/share/safe/node/1/logs",
            "log_format": "Default",
            "max_archived_log_files": 5,
            "max_log_files": 10,
            "metrics_port": 8080,
            "network_id": 1,
            "node_ip": "127.0.0.1",
            "node_port": 56215,
            "no_upnp": false,
            "number": 1,
            "peer_id": "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN",
            "pid": 12345,
            "relay": true,
            "rewards_address": "0x1234567890123456789012345678901234567890",
            "reward_balance": "1000000000000000000",
            "rpc_socket_addr": "127.0.0.1:8000",
            "service_name": "safenode-1",
            "status": "Running",
            "user": "safe",
            "user_mode": true,
            "version": "0.1.0"
        });

        let service_data: Result<NodeServiceDataTest, _> =
            serde_json::from_value(json_with_deprecated_field);
        assert!(
            service_data.is_err(),
            "We should not get an Ok().Deserialization should fail without serde default",
        );
    }

    #[test]
    fn enum_variants_can_be_added_without_breaking() {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub enum TestEnum1 {
            Variant1,
            Variant2,
        }

        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub enum TestEnum2 {
            Variant1,
            Variant2,
            Variant3,
        }

        let enum1 = TestEnum1::Variant1;
        let enum1_json = serde_json::to_value(&enum1).unwrap();

        let enum2: TestEnum2 = serde_json::from_value(enum1_json).unwrap();
        assert!(matches!(enum2, TestEnum2::Variant1));
    }

    #[test]
    fn enum_variants_cannot_be_removed_without_breaking() {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub enum TestEnum1 {
            Variant1,
            Variant2,
        }

        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub enum TestEnum2 {
            Variant1,
            Variant2,
            Variant3,
        }

        let enum2 = TestEnum2::Variant3;
        let enum2_json = serde_json::to_value(&enum2).unwrap();

        let result: Result<TestEnum1, _> = serde_json::from_value(enum2_json);
        assert!(
            result.is_err(),
            "Deserialization should fail when removing variants"
        );
    }
}
