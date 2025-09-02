// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::config::{
    AddNodesArgs, FIXED_INTERVAL, NODES_ALL, NodeConfig, UpgradeNodesArgs, add_multiple_nodes,
    send_action,
};
use crate::action::{Action, NodeTableActions, StatusActions};
use ant_node_manager::VerbosityLevel;
use ant_service_management::{NodeRegistryManager, ServiceStatus};
use color_eyre::Result;
use std::cmp::Ordering;
use tokio::sync::mpsc::UnboundedSender;

pub async fn maintain_n_running_nodes(args: AddNodesArgs, node_registry: NodeRegistryManager) {
    let config = NodeConfig::from(&args);
    debug!(
        "Maintaining {} running nodes with the following config:",
        config.count
    );
    debug!(
        " init_peers_config: {:?}, antnode_path: {:?}, network_id: {:?}, data_dir_path: {:?}, connection_mode: {:?}",
        config.init_peers_config,
        config.antnode_path,
        config.network_id,
        config.data_dir_path,
        config.connection_mode
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
    let target_count = args.count as usize;

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

            if let Err(err) = stop_nodes_helper(node_registry.clone(), services_to_stop).await {
                error!("Error while stopping excess nodes: {err:?}");
                send_action(
                    args.action_sender.clone(),
                    Action::StatusActions(StatusActions::ErrorStoppingNodes {
                        services: vec![],
                        raw_error: err.to_string(),
                    }),
                );
            }
        }
        Ordering::Less => {
            // Need to add or start nodes
            let to_start_count = target_count - running_count;

            if to_start_count <= inactive_nodes.len() {
                // Start existing inactive nodes
                let nodes_to_start = inactive_nodes.into_iter().take(to_start_count).collect();
                info!("Starting {to_start_count} existing inactive nodes: {nodes_to_start:?}",);

                if let Err(err) =
                    start_nodes_helper(nodes_to_start, &args.action_sender, node_registry.clone())
                        .await
                {
                    error!("Error while starting nodes: {err:?}");
                    send_action(
                        args.action_sender.clone(),
                        Action::StatusActions(StatusActions::ErrorStartingNodes {
                            services: vec![],
                            raw_error: err.to_string(),
                        }),
                    );
                }
            } else {
                // Need to add new nodes
                let to_add_count = to_start_count - inactive_nodes.len();
                info!(
                    "Adding {} new nodes and starting all {} inactive nodes",
                    to_add_count,
                    inactive_nodes.len()
                );

                let _ = add_multiple_nodes(
                    &config,
                    to_add_count as u16,
                    &args.action_sender,
                    node_registry.clone(),
                    false, // Don't send completion actions for batch operations
                    true,  // Auto-start nodes in maintain operations
                )
                .await;

                // Start all inactive nodes if any
                if !inactive_nodes.is_empty()
                    && let Err(err) = start_nodes_helper(
                        inactive_nodes,
                        &args.action_sender,
                        node_registry.clone(),
                    )
                    .await
                {
                    error!("Error while starting inactive nodes: {err:?}");
                    send_action(
                        args.action_sender.clone(),
                        Action::StatusActions(StatusActions::ErrorStartingNodes {
                            services: vec![],
                            raw_error: err.to_string(),
                        }),
                    );
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

    debug!("Finished maintaining {} nodes", args.count);
    send_action(
        args.action_sender,
        Action::NodeTableActions(NodeTableActions::StartNodesCompleted {
            service_name: NODES_ALL.to_string(),
        }),
    );
}

pub async fn reset_nodes(
    action_sender: UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
    start_nodes_after_reset: bool,
) {
    if let Err(err) =
        ant_node_manager::cmd::node::reset(true, node_registry.clone(), VerbosityLevel::Minimal)
            .await
    {
        error!("Error while resetting services {err:?}");
        send_action(
            action_sender,
            Action::StatusActions(StatusActions::ErrorResettingNodes {
                raw_error: err.to_string(),
            }),
        );
    } else {
        info!("Successfully reset services");
        send_action(
            action_sender,
            Action::StatusActions(StatusActions::ResetNodesCompleted {
                trigger_start_node: start_nodes_after_reset,
            }),
        );
    }
}

pub async fn upgrade_nodes(args: UpgradeNodesArgs, node_registry: NodeRegistryManager) {
    let config = NodeConfig::from(&args);

    // First we stop the Nodes
    if let Err(err) = stop_nodes_helper(node_registry.clone(), config.service_names.clone()).await {
        error!("Error while stopping services {err:?}");
        send_action(
            args.action_sender.clone(),
            Action::StatusActions(StatusActions::ErrorUpdatingNodes {
                raw_error: err.to_string(),
            }),
        );
    }

    if let Err(err) = ant_node_manager::cmd::node::upgrade(
        config.do_not_start,
        config.antnode_path,
        config.force,
        FIXED_INTERVAL,
        node_registry.clone(),
        Default::default(),
        config.env_variables,
        config.service_names,
        config.url,
        config.version,
        VerbosityLevel::Minimal,
    )
    .await
    {
        error!("Error while updating services {err:?}");
        send_action(
            args.action_sender,
            Action::StatusActions(StatusActions::ErrorUpdatingNodes {
                raw_error: err.to_string(),
            }),
        );
    } else {
        info!("Successfully updated services");
        send_action(
            args.action_sender,
            Action::StatusActions(StatusActions::UpdateNodesCompleted),
        );
    }
}

pub async fn remove_nodes(
    services: Vec<String>,
    action_sender: UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
) {
    // First we stop the nodes
    if let Err(err) = stop_nodes_helper(node_registry.clone(), services.clone()).await {
        error!("Error while stopping services {err:?}");
        send_action(
            action_sender.clone(),
            Action::StatusActions(StatusActions::ErrorRemovingNodes {
                services: services.clone(),
                raw_error: err.to_string(),
            }),
        );
    }

    if let Err(err) = remove_nodes_helper(node_registry, services.clone()).await {
        error!("Error while removing services {err:?}");
        send_action(
            action_sender,
            Action::StatusActions(StatusActions::ErrorRemovingNodes {
                services,
                raw_error: err.to_string(),
            }),
        );
    } else {
        info!("Successfully removed services {services:?}");
        for service in services {
            send_action(
                action_sender.clone(),
                Action::NodeTableActions(NodeTableActions::RemoveNodesCompleted {
                    service_name: service,
                }),
            );
        }
    }
}

pub async fn add_node(args: AddNodesArgs, node_registry: NodeRegistryManager) {
    let config = NodeConfig::from(&args);

    if let Err(err) = add_multiple_nodes(
        &config,
        config.count,
        &args.action_sender,
        node_registry,
        true,  // Send completion actions for individual add operations
        false, // Do not start nodes after adding them for individual add operations
    )
    .await
    {
        error!("Error while adding services {err:?}");
        send_action(
            args.action_sender,
            Action::StatusActions(StatusActions::ErrorAddingNodes {
                raw_error: err.to_string(),
            }),
        );
    }
}

pub async fn start_nodes(
    services: Vec<String>,
    action_sender: UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
) {
    debug!("Starting node {services:?}");
    if let Err(err) = start_nodes_helper(services.clone(), &action_sender, node_registry).await {
        error!("Error while starting services {err:?}");
        send_action(
            action_sender,
            Action::StatusActions(StatusActions::ErrorStartingNodes {
                services,
                raw_error: err.to_string(),
            }),
        );
    } else {
        info!("Successfully started services {services:?}");
        for service in services {
            send_action(
                action_sender.clone(),
                Action::NodeTableActions(NodeTableActions::StartNodesCompleted {
                    service_name: service,
                }),
            );
        }
    }
}

pub async fn stop_nodes(
    services: Vec<String>,
    action_sender: UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
) {
    if let Err(err) = stop_nodes_helper(node_registry, services.clone()).await {
        error!("Error while stopping services {err:?}");
        send_action(
            action_sender,
            Action::StatusActions(StatusActions::ErrorStoppingNodes {
                services,
                raw_error: err.to_string(),
            }),
        );
    } else {
        info!("Successfully stopped services");
        for service in services {
            send_action(
                action_sender.clone(),
                Action::NodeTableActions(NodeTableActions::StopNodesCompleted {
                    service_name: service,
                }),
            );
        }
    }
}

// --- Helper Functions ---

pub async fn stop_nodes_helper(
    node_registry: NodeRegistryManager,
    services: Vec<String>,
) -> Result<()> {
    ant_node_manager::cmd::node::stop(
        None,
        node_registry,
        vec![],
        services,
        VerbosityLevel::Minimal,
    )
    .await
}

pub async fn remove_nodes_helper(
    node_registry: NodeRegistryManager,
    services: Vec<String>,
) -> Result<()> {
    ant_node_manager::cmd::node::remove(
        false,
        vec![],
        node_registry,
        services,
        VerbosityLevel::Minimal,
    )
    .await
}

pub async fn start_nodes_helper(
    services: Vec<String>,
    _action_sender: &UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
) -> Result<()> {
    ant_node_manager::cmd::node::start(
        FIXED_INTERVAL,
        node_registry,
        vec![],
        services,
        VerbosityLevel::Minimal,
    )
    .await
}
