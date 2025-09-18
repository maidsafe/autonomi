// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::Action;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
use ant_node_manager::add_services::config::PortRange;
use ant_service_management::NodeRegistryManager;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

pub const PORT_MAX: u32 = 65535;
pub const PORT_MIN: u32 = 1024;

pub const FIXED_INTERVAL: u64 = 10_000;

pub const NODES_ALL: &str = "NODES_ALL";

#[derive(Debug)]
pub struct UpgradeNodesConfig {
    pub custom_bin_path: Option<PathBuf>,
    pub provided_env_variables: Option<Vec<(String, String)>>,
    pub service_names: Vec<String>,
    pub url: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug)]
pub struct AddNodesConfig {
    pub antnode_path: Option<PathBuf>,
    pub upnp_enabled: bool,
    pub count: u16,
    pub data_dir_path: Option<PathBuf>,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub port_range: Option<PortRange>,
    pub rewards_address: Option<EvmAddress>,
}

pub fn send_action(action_sender: UnboundedSender<Action>, action: Action) {
    if let Err(err) = action_sender.send(action) {
        error!("Error while sending action: {err:?}");
    }
}

pub async fn get_used_ports(node_registry: &NodeRegistryManager) -> Vec<u16> {
    let mut used_ports = Vec::new();
    for node in node_registry.nodes.read().await.iter() {
        let node = node.read().await;
        if let Some(port) = node.node_port {
            used_ports.push(port);
        }
    }
    debug!("Currently used ports: {used_ports:?}");
    used_ports
}

pub fn get_port_range(config: &AddNodesConfig) -> (u16, u16) {
    match &config.port_range {
        Some(PortRange::Single(port)) => (*port, *port),
        Some(PortRange::Range(start, end)) => (*start, *end),
        None => (PORT_MIN as u16, PORT_MAX as u16),
    }
}

pub fn find_next_available_port(used_ports: &[u16], current_port: &mut u16, max_port: u16) -> bool {
    while used_ports.contains(current_port) && *current_port <= max_port {
        *current_port += 1;
    }
    *current_port <= max_port
}
