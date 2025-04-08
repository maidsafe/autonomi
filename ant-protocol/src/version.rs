// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::sync::{LazyLock, RwLock};

/// The network_id is used to differentiate between different networks.
/// The default is set to 1 and it represents the mainnet.
pub static NETWORK_ID: LazyLock<RwLock<u8>> = LazyLock::new(|| RwLock::new(1));

/// The node version used during Identify Behaviour.
pub static IDENTIFY_NODE_VERSION_STR: LazyLock<RwLock<String>> = LazyLock::new(|| {
    RwLock::new(format!(
        "ant/node/{}/{}",
        get_truncate_version_str(),
        *NETWORK_ID
            .read()
            .expect("Failed to obtain read lock for NETWORK_ID"),
    ))
});

/// The client version used during Identify Behaviour.
pub static IDENTIFY_CLIENT_VERSION_STR: LazyLock<RwLock<String>> = LazyLock::new(|| {
    RwLock::new(format!(
        "ant/client/{}/{}",
        get_truncate_version_str(),
        *NETWORK_ID
            .read()
            .expect("Failed to obtain read lock for NETWORK_ID"),
    ))
});

/// The req/response protocol version
pub static REQ_RESPONSE_VERSION_STR: LazyLock<RwLock<String>> = LazyLock::new(|| {
    RwLock::new(format!(
        "/ant/{}/{}",
        get_truncate_version_str(),
        *NETWORK_ID
            .read()
            .expect("Failed to obtain read lock for NETWORK_ID"),
    ))
});

/// The identify protocol version
pub static IDENTIFY_PROTOCOL_STR: LazyLock<RwLock<String>> = LazyLock::new(|| {
    RwLock::new(format!(
        "ant/{}/{}",
        get_truncate_version_str(),
        *NETWORK_ID
            .read()
            .expect("Failed to obtain read lock for NETWORK_ID"),
    ))
});

/// Update the NETWORK_ID. The other version strings will reference this value.
/// By default, the network id is set to 1 which represents the mainnet.
///
/// This should be called before starting the node or client.
/// The values will be read often and this can cause issues if the values are changed after the node is started.
pub fn set_network_id(id: u8) {
    info!("Setting network id to: {id}");
    let mut network_id = NETWORK_ID
        .write()
        .expect("Failed to obtain write lock for NETWORK_ID");
    *network_id = id;
    info!("Network id set to: {id}");
}

/// Get the current NETWORK_ID as string.
pub fn get_network_id() -> String {
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
    let version_str = env!("CARGO_PKG_VERSION");
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
            "\nIDENTIFY_NODE_VERSION_STR: {}",
            *IDENTIFY_NODE_VERSION_STR
                .read()
                .expect("Failed to obtain read lock for IDENTIFY_NODE_VERSION_STR")
        );
        println!(
            "IDENTIFY_CLIENT_VERSION_STR: {}",
            *IDENTIFY_CLIENT_VERSION_STR
                .read()
                .expect("Failed to obtain read lock for IDENTIFY_CLIENT_VERSION_STR")
        );
        println!(
            "REQ_RESPONSE_VERSION_STR: {}",
            *REQ_RESPONSE_VERSION_STR
                .read()
                .expect("Failed to obtain read lock for REQ_RESPONSE_VERSION_STR")
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
        let network_id = get_network_id();
        println!("Network ID string: {network_id}");

        Ok(())
    }
}
