// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::{eyre::eyre, Result};
use libp2p::PeerId;
use serde::{de, Deserialize, Deserializer};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    str::FromStr,
};

fn deserialize_peer_socket_map<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<PeerId, SocketAddr>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: BTreeMap<String, SocketAddr> = BTreeMap::deserialize(deserializer)?;
    let s = s
        .into_iter()
        .map(|(peer_id, socket_addr)| {
            PeerId::from_str(&peer_id)
                .map_err(de::Error::custom)
                .map(|peer_id| (peer_id, socket_addr))
        })
        .collect::<std::result::Result<Vec<_>, D::Error>>()?;
    Ok(s.into_iter().collect())
}

// The contents of the file stored by sn-testnet-deploy.
#[derive(Clone, Debug, Deserialize)]
pub struct DeploymentInventory {
    pub name: String,
    pub version_info: Option<(String, String)>,
    pub branch_info: Option<(String, String)>,
    pub vm_list: Vec<(String, IpAddr)>,
    #[serde(deserialize_with = "deserialize_peer_socket_map")]
    pub rpc_endpoints: BTreeMap<PeerId, SocketAddr>,
    #[serde(deserialize_with = "deserialize_peer_socket_map")]
    pub antctld_endpoints: BTreeMap<PeerId, SocketAddr>,
    pub node_count: u16,
    pub ssh_user: String,
    pub genesis_multiaddr: String,
    pub peers: Vec<String>,
    pub uploaded_files: Vec<(String, String)>,
}

impl DeploymentInventory {
    /// Load the Deployment inventory from the SN_INVENTORY env variable.
    /// The variable can contain either the path to the inventory file or the deployment name.
    pub fn load() -> Result<Self> {
        let sn_inventory = std::env::var("SN_INVENTORY")
        .map_err(|_| eyre!("SN_INVENTORY not set. Provide either the deployment name or the direct path to the inventory.json file"))?;
        println!("SN_INVENTORY var set to {sn_inventory:?}");

        let inv = Self::load_from_str(&sn_inventory)?;
        println!("Read DeploymentInventory");
        Ok(inv)
    }

    /// Load the deployment inventory from the provided string.
    /// The string can either be a path to the inventory file or the deployment name.
    pub fn load_from_str(inv: &str) -> Result<Self> {
        let path = Self::get_deployment_path(inv)?;
        let inventory_file = std::fs::read(path)?;
        let inventory = serde_json::from_slice(&inventory_file)?;
        Ok(inventory)
    }

    // Read the path from the env variable SN_INVENTORY
    // Else read deployment name from SN_INVENTORY
    fn get_deployment_path(inv: &str) -> Result<PathBuf> {
        let path_from_env = PathBuf::from(&inv);
        if path_from_env.exists() {
            Ok(path_from_env)
        } else {
            let path = dirs_next::data_dir()
                .ok_or_else(|| eyre!("Could not obtain data_dir"))?
                .join("autonomi")
                .join("testnet-deploy")
                .join(format!("{inv}-inventory.json"));
            if path.exists() {
                Ok(path)
            } else {
                Err(eyre!(
                    "Could not obtain the deployment path from SN_INVENTORY"
                ))
            }
        }
    }
}
