// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::sync::{LazyLock, RwLock};

pub const MAINNET_ID: u8 = 1;
pub const ALPHANET_ID: u8 = 2;

/// Network identifier used to differentiate between different networks.
///
/// ## Default Value
/// Set to `1` representing the mainnet network.
///
/// ## Usage
/// - Network isolation mechanism
/// - Prevents cross-network contamination
/// - Used in protocol strings and user-agent identifiers
pub static NETWORK_ID: LazyLock<RwLock<u8>> = LazyLock::new(|| RwLock::new(1));

/// Node user-agent identifier for peer recognition and routing table management.
///
/// ## Purpose
/// Functions as a user-agent identifier (similar to HTTP user-agent headers) that
/// communicates peer type, its cargo package version, and network id to other peers.
///
/// ## Format
/// `ant/node/{ant_protocol_version}/{ant_node_version}/{network_id}`
///
/// ## Behavior
/// - Other nodes recognize this as a fellow routing participant
/// - Peers with this identifier are added to routing tables (RT)
pub fn construct_node_user_agent(node_version: String) -> String {
    format!(
        "ant/node/{ant_protocol_version}/{node_version}/{network_id}",
        ant_protocol_version = get_truncate_version_str(),
        network_id = get_network_id(),
    )
}

/// Client user-agent identifier for peer recognition and routing exclusion.
///
/// ## Purpose
/// Functions as a user-agent identifier (similar to HTTP user-agent headers) that
/// communicates peer type, its cargo package version, and network id to other peers.
///
/// ## Format
/// `ant/client/{ant_protocol_version}/{ant_client_version}/{network_id}`
///
/// ## Behavior
/// - Nodes search for "client" in this identifier and they are **excluded** from routing tables (RT)
/// - Treated as network consumers rather than routing participants
pub fn construct_client_user_agent(client_version: String) -> String {
    format!(
        "ant/client/{ant_protocol_version}/{client_version}/{network_id}",
        ant_protocol_version = get_truncate_version_str(),
        network_id = get_network_id(),
    )
}

pub static IDENTIFY_REACHABILITY_CHECK_CLIENT_VERSION_STR: LazyLock<RwLock<String>> =
    LazyLock::new(|| {
        RwLock::new(format!(
            "ant/reachability-check-client/{}/{}",
            get_truncate_version_str(),
            *NETWORK_ID
                .read()
                .expect("Failed to obtain read lock for NETWORK_ID"),
        ))
    });

/// The req/response protocol version
///
/// Defines the protocol identifier used for libp2p request-response communication that is used during
/// libp2p's multistream-select negotiation. Both peers must use identical protocol strings to establish communication.
///
/// ## Format
/// `/ant/{ant_protocol_version}/{network_id}`
///
/// ## Protocol Matching
/// - **Match**: Both peers negotiate successfully, communication proceeds
/// - **Mismatch**: Connection fails with `UnsupportedProtocols` error
///   - Different versions cannot communicate
///   - Different network IDs are isolated
///   - Connection remains open for other protocols
pub static REQ_RESPONSE_VERSION_STR: LazyLock<RwLock<String>> = LazyLock::new(|| {
    RwLock::new(format!(
        "/ant/{ant_protocol_version}/{network_id}",
        ant_protocol_version = get_truncate_version_str(),
        network_id = get_network_id()
    ))
});

/// Identify protocol version string for peer compatibility verification.
///
/// ## Purpose
/// Serves as a protocol handshake identifier ensuring peer communication compatibility.
/// Both peers must have identical values to exchange messages successfully.
///
/// ## Format
/// `ant/{ant_protocol_version}/{network_id}`
///
/// ## Compatibility Enforcement
/// - **Compatible**: Peers with matching strings can communicate
/// - **Incompatible**: Peers with different strings are:
///   - Considered incompatible
///   - Added to the blocklist
///   - Protected against cross-network contamination
pub static IDENTIFY_PROTOCOL_STR: LazyLock<RwLock<String>> = LazyLock::new(|| {
    RwLock::new(format!(
        "ant/{ant_protocol_version}/{network_id}",
        ant_protocol_version = get_truncate_version_str(),
        network_id = get_network_id(),
    ))
});

/// Update the NETWORK_ID.
///
/// Other version strings will reference this value. The default is 1, representing the mainnet.
///
/// This function should be used sparingly, ideally before the node or client is started.
///
/// Each of the version strings need to be explicitly updated here. There are scenarios where they
/// could be read before this function is called, in which case they will have the old value for
/// the lifetime of the program.
pub fn set_network_id(id: u8) {
    info!("Setting network id to: {id}");
    {
        let mut network_id = NETWORK_ID
            .write()
            .expect("Failed to obtain write lock for NETWORK_ID");
        *network_id = id;
    }

    {
        let mut req_response = REQ_RESPONSE_VERSION_STR
            .write()
            .expect("Failed to obtain write lock for REQ_RESPONSE_VERSION_STR");
        *req_response = format!("/ant/{}/{}", get_truncate_version_str(), id);
    }

    {
        let mut identify_protocol = IDENTIFY_PROTOCOL_STR
            .write()
            .expect("Failed to obtain write lock for IDENTIFY_PROTOCOL_STR");
        *identify_protocol = format!("ant/{}/{}", get_truncate_version_str(), id);
    }

    info!("Network id set to: {id} and all protocol strings updated");
}

/// Get the current NETWORK_ID as string.
pub fn get_network_id() -> u8 {
    *NETWORK_ID
        .read()
        .expect("Failed to obtain read lock for NETWORK_ID")
}

/// Get the current NETWORK_ID as string.
pub fn get_network_id_str() -> String {
    format!(
        "{}",
        *NETWORK_ID
            .read()
            .expect("Failed to obtain read lock for NETWORK_ID")
    )
}

// Protocol support shall be downward compatible for patch only version update.
// i.e. versions of `A.B.X` or `A.B.X-alpha.Y` shall be considered as a same protocol of `A.B`
pub fn get_truncate_version_str() -> String {
    let version_str = env!("CARGO_PKG_VERSION").to_string();
    let parts = version_str.split('.').collect::<Vec<_>>();
    if parts.len() >= 2 {
        format!("{}.{}", parts[0], parts[1])
    } else {
        panic!("Cannot obtain truncated version str for {version_str:?}: {parts:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_version_strings() -> Result<(), Box<dyn std::error::Error>> {
        set_network_id(3);
        println!(
            "\nNode user agent: {}",
            construct_node_user_agent("1.0.0".to_string())
        );
        println!(
            "Client user agent: {}",
            construct_client_user_agent("1.0.0".to_string())
        );
        println!(
            "REQ_RESPONSE_VERSION_STR: {}",
            *REQ_RESPONSE_VERSION_STR.read().expect(
                "Failed to 
                obtain read lock for REQ_RESPONSE_VERSION_STR"
            )
        );
        println!(
            "IDENTIFY_PROTOCOL_STR: {}",
            *IDENTIFY_PROTOCOL_STR
                .read()
                .expect("Failed to obtain read lock for IDENTIFY_PROTOCOL_STR")
        );

        // Test truncated version string
        let truncated = get_truncate_version_str();
        println!("\nTruncated version: {truncated}");

        // Test network id string
        let network_id = get_network_id_str();
        println!("Network ID string: {network_id}");

        Ok(())
    }
}
