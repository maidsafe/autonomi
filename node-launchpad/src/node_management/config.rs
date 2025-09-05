// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::error::NodeManagementError;
use crate::action::Action;
use crate::connection_mode::ConnectionMode;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
use ant_node_manager::{VerbosityLevel, add_services::config::PortRange};
use ant_service_management::NodeRegistryManager;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

pub const PORT_MAX: u32 = 65535;
pub const PORT_MIN: u32 = 1024;

pub const FIXED_INTERVAL: u64 = 60_000;

pub const NODES_ALL: &str = "NODES_ALL";

#[derive(Debug)]
pub struct UpgradeNodesConfig {
    pub action_sender: UnboundedSender<Action>,
    pub custom_bin_path: Option<PathBuf>,
    pub provided_env_variables: Option<Vec<(String, String)>>,
    pub service_names: Vec<String>,
    pub url: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug)]
pub struct AddNodesConfig {
    pub action_sender: UnboundedSender<Action>,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
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

pub fn handle_port_exhaustion(action_sender: &UnboundedSender<Action>, max_port: u16) {
    error!("Reached maximum port number. Unable to find an available port.");
    send_action(
        action_sender.clone(),
        Action::StatusActions(crate::action::StatusActions::ErrorAddingNodes {
            raw_error: format!(
                "Reached maximum port number ({max_port}).\\nUnable to find an available port."
            ),
        }),
    );
}

pub async fn add_node_with_config(
    config: &AddNodesConfig,
    count: u16,
    port_range: Option<PortRange>,
    node_registry: NodeRegistryManager,
) -> Result<Vec<String>, NodeManagementError> {
    let added_services = ant_node_manager::cmd::node::add(
        false, // alpha
        false, // auto_restart
        Some(count),
        config.data_dir_path.clone(),
        None, // env_variables
        None, // evm_network
        None, // log_dir_path
        None, // log_format
        None, // max_archived_log_files
        None, // max_log_files
        None, // metrics_port
        config.network_id,
        None, // node_ip
        port_range,
        node_registry,
        config.init_peers_config.clone(),
        config.rewards_address.ok_or_else(|| {
            error!("Something went wrong: Rewards address not set");
            NodeManagementError::RewardsAddressNotSet
        })?,
        None,  // rpc_address
        None,  // rpc_port
        false, // skip_reachability_check
        config.antnode_path.clone(),
        config.connection_mode != ConnectionMode::UPnP,
        None, // url
        None, // user
        None, // version
        VerbosityLevel::Minimal,
        false, // write_older_cache_files
    )
    .await?;

    Ok(added_services)
}

pub async fn add_multiple_nodes(
    config: &AddNodesConfig,
    count: u16,
    node_registry: NodeRegistryManager,
    send_completion_actions: bool,
    start_nodes: bool,
) -> Result<Vec<String>, NodeManagementError> {
    if count == 0 {
        return Ok(vec![]);
    }

    debug!("Adding {count} nodes");

    let used_ports = get_used_ports(&node_registry).await;
    let (mut current_port, max_port) = get_port_range(config);

    // Find first available port
    if !find_next_available_port(&used_ports, &mut current_port, max_port) {
        return Err(NodeManagementError::NoAvailablePorts { max_port });
    }

    // Calculate optimal port range for the requested count
    let optimal_port_range = if count == 1 {
        Some(PortRange::Single(current_port))
    } else {
        // Try to find a contiguous range of available ports
        let mut end_port = current_port;
        for _ in 1..count {
            let next_port = end_port + 1;
            if next_port > max_port || used_ports.contains(&next_port) {
                // Can't get a contiguous range, fall back to single port
                end_port = current_port;
                break;
            }
            end_port = next_port;
        }

        if end_port > current_port {
            Some(PortRange::Range(current_port, end_port))
        } else {
            Some(PortRange::Single(current_port))
        }
    };

    info!("Using pre-validated port range: {optimal_port_range:?}");

    // Call ant_node_manager with pre-validated ports
    match add_node_with_config(config, count, optimal_port_range, node_registry.clone()).await {
        Ok(services) => {
            info!("Successfully added {count} nodes: {services:?}",);

            // Start the newly added nodes if requested
            if start_nodes
                && let Err(err) = super::handlers::start_nodes_helper(
                    services.clone(),
                    &config.action_sender,
                    node_registry,
                )
                .await
            {
                error!("Error while starting newly added nodes: {err:?}");
                send_action(
                    config.action_sender.clone(),
                    Action::StatusActions(crate::action::StatusActions::ErrorStartingNodes {
                        services: vec![],
                        raw_error: err.to_string(),
                    }),
                );
            }

            // Send completion actions if requested
            if send_completion_actions {
                for service in &services {
                    send_action(
                        config.action_sender.clone(),
                        Action::NodeTableActions(
                            crate::action::NodeTableActions::AddNodesCompleted {
                                service_name: service.clone(),
                            },
                        ),
                    );
                }
            }

            Ok(services)
        }
        Err(err) => {
            error!("Error while adding {count} nodes: {err:?}");
            send_action(
                config.action_sender.clone(),
                Action::StatusActions(crate::action::StatusActions::ErrorAddingNodes {
                    raw_error: err.to_string(),
                }),
            );
            Err(err)
        }
    }
}
