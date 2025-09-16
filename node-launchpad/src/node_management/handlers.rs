// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::config::{AddNodesConfig, FIXED_INTERVAL, UpgradeNodesConfig};
use crate::node_management::config::{find_next_available_port, get_port_range, get_used_ports};
use crate::node_management::error::NodeManagementError;
use ant_node_manager::VerbosityLevel;
use ant_node_manager::add_services::config::PortRange;
use ant_service_management::control::ServiceController;
use ant_service_management::{NodeRegistryManager, ServiceStatus};
use std::{cmp::Ordering, sync::Arc};

pub async fn refresh_node_registry(
    force: bool,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    ant_node_manager::refresh_node_registry(
        node_registry,
        Arc::new(ServiceController {}),
        force,
        false, // todo should be from --local flag
        VerbosityLevel::Minimal,
        false,
    )
    .await
    .inspect_err(|err| {
        error!("Error while refreshing node registry: {err:?}");
    })?;
    info!("Node registry successfully refreshed");
    Ok(())
}

pub async fn maintain_n_running_nodes(
    config: AddNodesConfig,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    debug!(
        "Maintaining {} running nodes with the following config:",
        config.count
    );
    debug!(
        " init_peers_config: {:?}, antnode_path: {:?}, network_id: {:?}, data_dir_path: {:?}, upnp_enabled: {:?}",
        config.init_peers_config,
        config.antnode_path,
        config.network_id,
        config.data_dir_path,
        config.upnp_enabled
    );

    // Count running nodes and categorize all nodes by status
    let mut running_nodes = Vec::new();
    let mut inactive_nodes = Vec::new();

    for node in node_registry.nodes.read().await.iter() {
        let node = node.read().await;
        match node.status {
            ServiceStatus::Running => {
                running_nodes.push(node.service_name.clone());
            }
            ServiceStatus::Stopped | ServiceStatus::Added => {
                inactive_nodes.push(node.service_name.clone());
            }
            ServiceStatus::Removed => {
                // Skip removed nodes - they should not be included in any operations
                debug!("Skipping removed node: {}", node.service_name);
            }
        }
    }

    let running_count = running_nodes.len();
    let target_count = config.count as usize;

    info!(
        "Current running nodes: {running_count}, Target: {target_count}, Inactive available: {}",
        inactive_nodes.len()
    );

    match running_count.cmp(&target_count) {
        Ordering::Greater => {
            // Stop excess nodes
            let to_stop_count = running_count - target_count;
            let services_to_stop = running_nodes
                .into_iter()
                .rev() // Stop the oldest nodes first
                .take(to_stop_count)
                .collect::<Vec<_>>();

            info!("Stopping {to_stop_count} excess nodes: {services_to_stop:?}");
            stop_nodes_helper(node_registry.clone(), services_to_stop).await?;
            info!("Successfully stopped excess nodes");
        }
        Ordering::Less => {
            // Need to add or start nodes
            let to_start_count = target_count - running_count;

            if to_start_count <= inactive_nodes.len() {
                // Start existing inactive nodes
                let nodes_to_start = inactive_nodes.into_iter().take(to_start_count).collect();

                info!("Starting {to_start_count} existing inactive nodes: {nodes_to_start:?}");
                start_nodes_helper(nodes_to_start, node_registry.clone()).await?;
                info!("Successfully started existing inactive nodes");
            } else {
                // Need to add new nodes
                let to_add_count = to_start_count - inactive_nodes.len();
                info!(
                    "Adding {} new nodes and starting all {} inactive nodes",
                    to_add_count,
                    inactive_nodes.len()
                );
                let _ = add_multiple_nodes_helper(
                    &config,
                    to_add_count as u16,
                    node_registry.clone(),
                    true, // Auto-start nodes in maintain operations
                )
                .await?;

                // Start all inactive nodes if any
                if !inactive_nodes.is_empty() {
                    info!("Starting all inactive nodes: {inactive_nodes:?}");
                    start_nodes_helper(inactive_nodes, node_registry.clone()).await?;
                }
            }
        }
        Ordering::Equal => {
            info!(
                "Current node count ({}) matches target ({}). No action needed.",
                running_count, target_count
            );
        }
    }

    // Verify final state
    let mut final_running_count = 0;
    for node in node_registry.nodes.read().await.iter() {
        let node_read = node.read().await;
        if node_read.status == ServiceStatus::Running {
            final_running_count += 1;
        }
    }

    if final_running_count != target_count {
        warn!(
            "Failed to reach target node count. Expected {target_count}, but got {final_running_count}"
        );
    }

    info!("Finished maintaining {} nodes", config.count);
    Ok(())
}

pub async fn reset_nodes(node_registry: NodeRegistryManager) -> Result<(), NodeManagementError> {
    ant_node_manager::cmd::node::reset(true, node_registry.clone(), VerbosityLevel::Minimal)
        .await?;
    info!("All nodes have been reset");
    Ok(())
}

pub async fn upgrade_nodes(
    args: UpgradeNodesConfig,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    info!("Stopping nodes before upgrade: {:?}", args.service_names);
    stop_nodes_helper(node_registry.clone(), args.service_names.clone()).await?;

    info!(
        "Stopped nodes, proceeding with upgrade: {:?}",
        args.service_names
    );
    ant_node_manager::cmd::node::upgrade(
        false, // do_not_start
        args.custom_bin_path,
        false, // force
        FIXED_INTERVAL,
        node_registry.clone(),
        Default::default(),
        args.provided_env_variables,
        args.service_names.clone(),
        false, // Skip performing startup check, as we'll do it manually inside launchpad.
        args.url,
        args.version,
        VerbosityLevel::Minimal,
    )
    .await
    .inspect_err(|err| {
        error!("Error while upgrading node services {err:?}");
    })?;

    info!("Successfully upgraded nodes: {:?}", args.service_names);
    Ok(())
}

pub async fn remove_nodes(
    services: Vec<String>,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    info!("Stopping nodes before removal: {:?}", services);
    stop_nodes_helper(node_registry.clone(), services.clone()).await?;
    info!("Stopped nodes, proceeding with removal: {:?}", services);
    remove_nodes_helper(node_registry, services.clone()).await?;
    info!("Successfully removed nodes: {:?}", services);

    Ok(())
}

pub async fn add_nodes(
    config: AddNodesConfig,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    add_multiple_nodes_helper(
        &config,
        config.count,
        node_registry,
        false, // Do not start nodes after adding them for individual add operations
    )
    .await?;
    info!("Successfully added {} nodes", config.count);
    Ok(())
}

pub async fn start_nodes(
    services: Vec<String>,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    start_nodes_helper(services.clone(), node_registry).await?;
    info!("Successfully started nodes: {services:?}");
    Ok(())
}

pub async fn stop_nodes(
    services: Vec<String>,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    stop_nodes_helper(node_registry, services.clone()).await?;
    info!("Successfully stopped nodes: {services:?}");
    Ok(())
}

// --- Helper Functions ---

async fn add_multiple_nodes_helper(
    config: &AddNodesConfig,
    count: u16,
    node_registry: NodeRegistryManager,
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
        error!("No available ports found in the specified range up to {max_port}");
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

    let services =
        add_node_helper(config, count, optimal_port_range, node_registry.clone()).await?;
    info!("Successfully added {count} nodes: {services:?}",);

    // Start the newly added nodes if requested
    if start_nodes {
        info!("Starting newly added nodes: {services:?}");
        start_nodes_helper(services.clone(), node_registry.clone()).await?;
    }

    Ok(services)
}

async fn stop_nodes_helper(
    node_registry: NodeRegistryManager,
    services: Vec<String>,
) -> Result<(), NodeManagementError> {
    ant_node_manager::cmd::node::stop(
        None,
        node_registry,
        vec![],
        services,
        VerbosityLevel::Minimal,
    )
    .await
    .inspect_err(|err| {
        error!("Error while stopping nodes: {err:?}");
    })?;
    Ok(())
}

async fn remove_nodes_helper(
    node_registry: NodeRegistryManager,
    services: Vec<String>,
) -> Result<(), NodeManagementError> {
    ant_node_manager::cmd::node::remove(
        false,
        vec![],
        node_registry,
        services,
        VerbosityLevel::Minimal,
    )
    .await
    .inspect_err(|err| {
        error!("Error while removing nodes: {err:?}");
    })?;
    Ok(())
}

async fn start_nodes_helper(
    services: Vec<String>,
    node_registry: NodeRegistryManager,
) -> Result<(), NodeManagementError> {
    ant_node_manager::cmd::node::start(
        FIXED_INTERVAL,
        node_registry,
        vec![],
        services,
        false, // Skip performing startup check, as we'll do it manually inside launchpad.
        VerbosityLevel::Minimal,
    )
    .await
    .inspect_err(|err| {
        error!("Error while starting nodes: {err:?}");
    })?;
    Ok(())
}
async fn add_node_helper(
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
        !config.upnp_enabled,
        None, // url
        None, // user
        None, // version
        VerbosityLevel::Minimal,
        false, // write_older_cache_files
    )
    .await
    .inspect_err(|err| {
        error!("Error while adding nodes: {err:?}");
    })?;

    Ok(added_services)
}
