// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(test)]
use super::{
    node_service_data::NodeServiceData,
    node_service_data_v1::{NodeServiceDataV1, NODE_SERVICE_DATA_SCHEMA_V1},
};
use crate::ServiceStatus;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, EvmNetwork, RewardsAddress};
use libp2p::{Multiaddr, PeerId};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
};

// Helper function to create a test V1 struct directly
fn create_test_v1_struct() -> NodeServiceDataV1 {
    NodeServiceDataV1 {
            schema_version: NODE_SERVICE_DATA_SCHEMA_V1,
            antnode_path: PathBuf::from("/usr/bin/antnode"),
            auto_restart: true,
            connected_peers: Some(vec![
                PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap(),
            ]),
            data_dir_path: PathBuf::from("/home/user/.local/share/safe/node/1"),
            evm_network: EvmNetwork::ArbitrumSepolia,
            initial_peers_config: InitialPeersConfig {
                first: false,
                local: false,
                addrs: vec![],
                network_contacts_url: vec![],
                disable_mainnet_contacts: false,
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            listen_addr: Some(vec![
                Multiaddr::from_str("/ip4/127.0.0.1/udp/56215/quic-v1/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap(),
            ]),
            log_dir_path: PathBuf::from("/home/user/.local/share/safe/node/1/logs"),
            log_format: Some(ant_logging::LogFormat::Default),
            max_archived_log_files: Some(5),
            max_log_files: Some(10),
            metrics_port: Some(8080),
            network_id: Some(1),
            node_ip: Some(Ipv4Addr::new(127, 0, 0, 1)),
            node_port: Some(56215),
            no_upnp: false,
            number: 1,
            peer_id: Some(
                PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap(),
            ),
            pid: Some(12345),
            relay: true,
            rewards_address: RewardsAddress::from_str("0x1234567890123456789012345678901234567890").unwrap(),
            reward_balance: Some(AttoTokens::from_u128(1000000000000000000u128)),
            rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000),
            service_name: "safenode-1".to_string(),
            status: ServiceStatus::Running,
            user: Some("safe_user".to_string()),
            user_mode: true,
            version: "0.1.0".to_string(),
        }
}

fn create_test_v1_json() -> serde_json::Value {
    let test_struct = create_test_v1_struct();
    serde_json::to_value(test_struct).expect("Failed to serialize test struct to JSON")
}

fn create_test_v0_json() -> serde_json::Value {
    // Start with V1 JSON and transform it to V0 format
    let v1_value = create_test_v1_json();

    // Convert to V0 format by modifying the JSON structure
    if let serde_json::Value::Object(mut map) = v1_value {
        // Remove schema_version (not present in V0)
        map.remove("schema_version");

        // Rename initial_peers_config to peers_args
        if let Some(initial_peers_config) = map.remove("initial_peers_config") {
            map.insert("peers_args".to_string(), initial_peers_config);
        }

        // Convert relay to home_network
        if let Some(relay) = map.remove("relay") {
            map.insert("home_network".to_string(), relay);
        }

        // Convert no_upnp to upnp (with inverted value)
        if let Some(serde_json::Value::Bool(no_upnp)) = map.remove("no_upnp") {
            map.insert("upnp".to_string(), serde_json::Value::Bool(!no_upnp));
        }

        serde_json::Value::Object(map)
    } else {
        panic!("Failed to convert V1 JSON to V0 format");
    }
}

#[test]
fn test_deserialize_v1_format() {
    let json_data = create_test_v1_json();
    let service_data: Result<NodeServiceData, _> = serde_json::from_value(json_data);

    assert!(
        service_data.is_ok(),
        "Failed to deserialize V1 format: {:?}",
        service_data.err()
    );
    let data = service_data.unwrap();

    // Verify a few key fields to ensure proper deserialization
    assert_eq!(data.schema_version, NODE_SERVICE_DATA_SCHEMA_V1);
    assert_eq!(data.service_name, "safenode-1");
    assert_eq!(data.node_port, Some(56215));
    assert!(!data.no_upnp);
    assert!(data.relay);

    // Verify PeerId deserialization
    let expected_peer_id =
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();
    assert_eq!(data.peer_id, Some(expected_peer_id));

    // Verify connected_peers deserialization
    assert!(data.connected_peers.is_some());
    let connected_peers = data.connected_peers.unwrap();
    assert_eq!(connected_peers.len(), 1);
    assert_eq!(connected_peers[0], expected_peer_id);
}

#[test]
fn test_deserialize_v0_format() {
    let json_data = create_test_v0_json();
    let service_data: Result<NodeServiceData, _> = serde_json::from_value(json_data);

    assert!(
        service_data.is_ok(),
        "Failed to deserialize V0 format: {:?}",
        service_data.err()
    );
    let data = service_data.unwrap();

    // Verify the automatic version upgrade
    assert_eq!(data.schema_version, NODE_SERVICE_DATA_SCHEMA_V1);

    // Verify renamed fields
    assert!(data.relay); // Was home_network in V0
    assert!(!data.no_upnp); // Was !upnp in V0

    // Verify other key fields
    assert_eq!(data.service_name, "safenode-1");
    assert_eq!(data.node_port, Some(56215));

    // Verify PeerId deserialization
    let expected_peer_id =
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();
    assert_eq!(data.peer_id, Some(expected_peer_id));
}

#[test]
fn test_peer_id_serialization() {
    let test_data = create_test_v1_struct();

    // Serialize to JSON
    let serialized = serde_json::to_value(&test_data).unwrap();

    // Check peer_id is serialized as string
    if let serde_json::Value::Object(map) = &serialized {
        if let Some(peer_id) = map.get("peer_id") {
            assert!(peer_id.is_string());
            assert_eq!(
                peer_id.as_str().unwrap(),
                "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            );
        } else {
            panic!("peer_id field missing from serialized output");
        }
    } else {
        panic!("Serialized output is not an object");
    }

    // Deserialize back and check peer_id
    let deserialized: NodeServiceData = serde_json::from_value(serialized).unwrap();
    assert_eq!(
        deserialized.peer_id.unwrap().to_string(),
        "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
    );
}

#[test]
fn test_connected_peers_serialization() {
    let test_data = create_test_v1_struct();

    // Serialize to JSON
    let serialized = serde_json::to_value(&test_data).unwrap();

    // Check connected_peers is serialized as array of strings
    if let serde_json::Value::Object(map) = &serialized {
        if let Some(serde_json::Value::Array(peers)) = map.get("connected_peers") {
            assert_eq!(peers.len(), 1);
            assert!(peers[0].is_string());
            assert_eq!(
                peers[0].as_str().unwrap(),
                "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            );
        } else {
            panic!("connected_peers field missing or not an array");
        }
    } else {
        panic!("Serialized output is not an object");
    }

    // Deserialize back and check connected_peers
    let deserialized: NodeServiceData = serde_json::from_value(serialized).unwrap();
    let connected_peers = deserialized.connected_peers.unwrap();
    assert_eq!(connected_peers.len(), 1);
    assert_eq!(
        connected_peers[0].to_string(),
        "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
    );
}

#[test]
fn test_v0_to_v1_field_transformations() {
    // Create a modified V1 struct with specific values
    let mut test_struct = create_test_v1_struct();
    test_struct.no_upnp = false;
    test_struct.relay = true;

    // Convert to V0 format manually
    let mut v0_json = serde_json::to_value(test_struct).unwrap();
    if let serde_json::Value::Object(ref mut map) = v0_json {
        // Remove schema_version
        map.remove("schema_version");

        // Rename fields and transform values
        map.remove("no_upnp");
        map.insert("upnp".to_string(), serde_json::Value::Bool(true)); // Inverted from no_upnp

        map.remove("relay");
        map.insert("home_network".to_string(), serde_json::Value::Bool(true));

        map.remove("initial_peers_config");
        map.insert(
            "peers_args".to_string(),
            serde_json::json!({
                "first": false,
                "local": false,
                "addrs": [],
                "network_contacts_url": [],
                "disable_mainnet_contacts": false,
                "ignore_cache": false,
                "bootstrap_cache_dir": null
            }),
        );
    }

    // Deserialize V0 format
    let service_data: Result<NodeServiceData, _> = serde_json::from_value(v0_json);
    assert!(
        service_data.is_ok(),
        "Failed to deserialize transformed V0: {:?}",
        service_data.err()
    );

    let data = service_data.unwrap();

    // Check transformed fields:
    // - upnp: true in V0 should become no_upnp: false in V1
    // - home_network: true in V0 should become relay: true in V1
    assert!(!data.no_upnp);
    assert!(data.relay);
}

#[test]
fn test_direct_v1_deserialization() {
    let json_data = create_test_v1_json();

    // Use the direct V1 deserialization method
    let service_data = serde_json::from_value::<NodeServiceDataV1>(json_data);

    assert!(
        service_data.is_ok(),
        "Failed to directly deserialize as V1: {:?}",
        service_data.err()
    );
    let data = service_data.unwrap();

    // Verify a few key fields
    assert_eq!(data.schema_version, NODE_SERVICE_DATA_SCHEMA_V1);
    assert_eq!(data.service_name, "safenode-1");
}
