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
use serde::{de::Error as DeError, Deserialize, Deserializer, Serializer};
use std::str::FromStr;

/// Type alias for the latest version of the node service data structure.
pub type NodeServiceData = super::node_service_data_v1::NodeServiceDataV1;
/// Type alias for the latest node service data schema version.
pub const NODE_SERVICE_DATA_SCHEMA_LATEST: u32 =
    super::node_service_data_v1::NODE_SERVICE_DATA_SCHEMA_V1;

/// Custom deserialization for NodeServiceData.
/// This will perform conversion from V0 to V1 if needed.
impl<'de> Deserialize<'de> for NodeServiceData {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json_value = serde_json::Value::deserialize(deserializer)?;

        let is_v1 = match &json_value {
            serde_json::Value::Object(obj) => obj.contains_key("schema_version"),
            _ => false,
        };

        if is_v1 {
            // It's V1 format - use NodeServiceDataV1's helper method
            match super::node_service_data_v1::NodeServiceDataV1::deserialize_v1(
                &mut serde_json::de::Deserializer::from_str(&json_value.to_string()),
            ) {
                Ok(v1) => Ok(v1),
                Err(e) => Err(D::Error::custom(format!(
                    "Failed to deserialize as V1: {}",
                    e
                ))),
            }
        } else {
            // It's V0 format - deserialize and convert
            match serde_json::from_value::<super::node_service_data_v0::NodeServiceDataV0>(
                json_value,
            ) {
                Ok(v0) => {
                    // Convert V0 to V1
                    let v1: super::node_service_data_v1::NodeServiceDataV1 = v0.into();
                    Ok(v1)
                }
                Err(e) => Err(D::Error::custom(format!(
                    "Failed to deserialize as V0: {}",
                    e
                ))),
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
                    println!("Found port: {}", port);
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
