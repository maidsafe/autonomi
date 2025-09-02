// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{Action, NodeTableActions, StatusActions};
use crate::connection_mode::ConnectionMode;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
use ant_node_manager::{VerbosityLevel, add_services::config::PortRange};
use ant_service_management::{NodeRegistryManager, ServiceStatus};
use color_eyre::Result;
use color_eyre::eyre::eyre;
use std::cmp::Ordering;
use std::path::PathBuf;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::LocalSet;

pub const PORT_MAX: u32 = 65535;
pub const PORT_MIN: u32 = 1024;

pub const FIXED_INTERVAL: u64 = 60_000;

pub const NODES_ALL: &str = "NODES_ALL";

#[derive(Debug)]
pub enum NodeManagementTask {
    MaintainNodes {
        args: AddNodesArgs,
    },
    ResetNodes {
        start_nodes_after_reset: bool,
        action_sender: UnboundedSender<Action>,
    },
    StopNodes {
        services: Vec<String>,
        action_sender: UnboundedSender<Action>,
    },
    UpgradeNodes {
        args: UpgradeNodesArgs,
    },
    AddNode {
        args: AddNodesArgs,
    },
    RemoveNodes {
        services: Vec<String>,
        action_sender: UnboundedSender<Action>,
    },
    StartNode {
        services: Vec<String>,
        action_sender: UnboundedSender<Action>,
    },
}

#[derive(Clone)]
pub struct NodeManagement {
    task_sender: mpsc::UnboundedSender<NodeManagementTask>,
}

impl NodeManagement {
    pub fn new(node_registry: NodeRegistryManager) -> Result<Self> {
        let (send, mut recv) = mpsc::unbounded_channel();

        let rt = Builder::new_current_thread().enable_all().build()?;

        std::thread::spawn(move || {
            let local = LocalSet::new();

            local.spawn_local(async move {
                while let Some(new_task) = recv.recv().await {
                    match new_task {
                        NodeManagementTask::MaintainNodes { args } => {
                            maintain_n_running_nodes(args, node_registry.clone()).await;
                        }
                        NodeManagementTask::ResetNodes {
                            start_nodes_after_reset,
                            action_sender,
                        } => {
                            reset_nodes(
                                action_sender,
                                node_registry.clone(),
                                start_nodes_after_reset,
                            )
                            .await;
                        }
                        NodeManagementTask::StopNodes {
                            services,
                            action_sender,
                        } => {
                            stop_nodes(services, action_sender, node_registry.clone()).await;
                        }
                        NodeManagementTask::UpgradeNodes { args } => {
                            upgrade_nodes(args, node_registry.clone()).await
                        }
                        NodeManagementTask::RemoveNodes {
                            services,
                            action_sender,
                        } => remove_nodes(services, action_sender, node_registry.clone()).await,
                        NodeManagementTask::StartNode {
                            services,
                            action_sender,
                        } => start_nodes(services, action_sender, node_registry.clone()).await,
                        NodeManagementTask::AddNode { args } => {
                            add_node(args, node_registry.clone()).await
                        }
                    }
                }
                // If the while loop returns, then all the LocalSpawner
                // objects have been dropped.
            });

            // This will return once all senders are dropped and all
            // spawned tasks have returned.
            rt.block_on(local);
        });

        Ok(Self { task_sender: send })
    }

    /// Send a task to the NodeManagement local set
    /// These tasks will be executed on a different thread to avoid blocking the main thread
    ///
    /// The results are returned via the standard `UnboundedSender<Action>` that is passed to each task.
    ///
    /// If this function returns an error, it means that the task could not be sent to the local set.
    pub fn send_task(&self, task: NodeManagementTask) -> Result<()> {
        self.task_sender
            .send(task)
            .inspect_err(|err| error!("The node management local set is down {err:?}"))
            .map_err(|_| eyre!("Failed to send task to the node management local set"))?;
        Ok(())
    }
}

/// Stop the specified services
async fn stop_nodes(
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

/// Maintain the specified number of nodes
async fn maintain_n_running_nodes(args: AddNodesArgs, node_registry: NodeRegistryManager) {
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

/// Reset all the nodes
async fn reset_nodes(
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

#[derive(Debug)]
pub struct UpgradeNodesArgs {
    pub action_sender: UnboundedSender<Action>,
    pub connection_timeout_s: u64,
    pub do_not_start: bool,
    pub custom_bin_path: Option<PathBuf>,
    pub force: bool,
    pub fixed_interval: Option<u64>,
    pub provided_env_variables: Option<Vec<(String, String)>>,
    pub service_names: Vec<String>,
    pub url: Option<String>,
    pub version: Option<String>,
}

async fn upgrade_nodes(args: UpgradeNodesArgs, node_registry: NodeRegistryManager) {
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

async fn remove_nodes(
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
        info!("Successfully removed services {:?}", services);
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

async fn add_node(args: AddNodesArgs, node_registry: NodeRegistryManager) {
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

async fn start_nodes(
    services: Vec<String>,
    action_sender: UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
) {
    debug!("Starting node {:?}", services);
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
        info!("Successfully started services {:?}", services);
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

// --- Helper Functions ---

/// Helper to stop nodes with error handling
async fn stop_nodes_helper(
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

/// Helper to remove nodes with error handling  
async fn remove_nodes_helper(
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

async fn start_nodes_helper(
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

fn send_action(action_sender: UnboundedSender<Action>, action: Action) {
    if let Err(err) = action_sender.send(action) {
        error!("Error while sending action: {err:?}");
    }
}

// --- Configuration and Port Management ---

#[derive(Debug, Clone)]
pub struct NodeConfig {
    // Core node settings
    pub antnode_path: Option<PathBuf>,
    pub data_dir_path: Option<PathBuf>,
    pub network_id: Option<u8>,
    pub connection_mode: ConnectionMode,
    pub port_range: Option<PortRange>,
    pub rewards_address: Option<EvmAddress>,
    pub init_peers_config: InitialPeersConfig,

    // Node operation settings
    pub count: u16,
    pub skip_reachability_check: bool,

    // Upgrade-specific settings (optional)
    pub service_names: Vec<String>,
    pub force: bool,
    pub do_not_start: bool,
    pub url: Option<String>,
    pub version: Option<String>,
    pub env_variables: Option<Vec<(String, String)>>,
}

impl From<&AddNodesArgs> for NodeConfig {
    fn from(args: &AddNodesArgs) -> Self {
        NodeConfig {
            antnode_path: args.antnode_path.clone(),
            data_dir_path: args.data_dir_path.clone(),
            network_id: args.network_id,
            connection_mode: args.connection_mode,
            port_range: args.port_range.clone(),
            rewards_address: args.rewards_address,
            init_peers_config: args.init_peers_config.clone(),
            count: args.count,
            skip_reachability_check: false,
            // Default values for upgrade-specific fields
            service_names: vec![],
            force: false,
            do_not_start: false,
            url: None,
            version: None,
            env_variables: None,
        }
    }
}

impl From<&UpgradeNodesArgs> for NodeConfig {
    fn from(args: &UpgradeNodesArgs) -> Self {
        NodeConfig {
            // Default node settings for upgrade operations
            antnode_path: args.custom_bin_path.clone(),
            data_dir_path: None,
            network_id: None,
            connection_mode: ConnectionMode::Automatic,
            port_range: None,
            rewards_address: None,
            init_peers_config: InitialPeersConfig::default(),
            count: 0,
            skip_reachability_check: false,
            // Upgrade-specific fields
            service_names: args.service_names.clone(),
            force: args.force,
            do_not_start: args.do_not_start,
            url: args.url.clone(),
            version: args.version.clone(),
            env_variables: args.provided_env_variables.clone(),
        }
    }
}

/// Add nodes using ant_node_manager with the given configuration
async fn add_node_with_config(
    config: &NodeConfig,
    port_range: Option<PortRange>,
    count: Option<u16>,
    node_registry: NodeRegistryManager,
) -> Result<Vec<String>> {
    ant_node_manager::cmd::node::add(
        false, // alpha
        false, // auto_restart
        count,
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
        config.rewards_address.unwrap(),
        None, // rpc_address
        None, // rpc_port
        config.skip_reachability_check,
        config.antnode_path.clone(),
        config.connection_mode != ConnectionMode::UPnP,
        None, // url
        None, // user
        None, // version
        VerbosityLevel::Minimal,
        false, // write_older_cache_files
    )
    .await
}

/// Get the currently used ports from the node registry
async fn get_used_ports(node_registry: &NodeRegistryManager) -> Vec<u16> {
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

/// Get the port range (u16, u16) from the config
fn get_port_range(config: &NodeConfig) -> (u16, u16) {
    match &config.port_range {
        Some(PortRange::Single(port)) => (*port, *port),
        Some(PortRange::Range(start, end)) => (*start, *end),
        None => (PORT_MIN as u16, PORT_MAX as u16),
    }
}

/// Find the next available port
fn find_next_available_port(used_ports: &[u16], current_port: &mut u16, max_port: u16) -> bool {
    while used_ports.contains(current_port) && *current_port <= max_port {
        *current_port += 1;
    }
    *current_port <= max_port
}

/// Handle port exhaustion error
fn handle_port_exhaustion(action_sender: &UnboundedSender<Action>, max_port: u16) {
    error!("Reached maximum port number. Unable to find an available port.");
    send_action(
        action_sender.clone(),
        Action::StatusActions(StatusActions::ErrorAddingNodes {
            raw_error: format!(
                "Reached maximum port number ({max_port}).\nUnable to find an available port."
            ),
        }),
    );
}

/// Add multiple nodes using ant_node_manager with proactive port management
async fn add_multiple_nodes(
    config: &NodeConfig,
    count: u16,
    action_sender: &UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
    send_completion_actions: bool,
    start_nodes: bool,
) -> Result<Vec<String>> {
    if count == 0 {
        return Ok(vec![]);
    }

    debug!("Adding {count} nodes");

    // Proactive port management: Check available ports before attempting to add nodes
    let used_ports = get_used_ports(&node_registry).await;
    let (mut current_port, max_port) = get_port_range(config);

    // Find first available port
    if !find_next_available_port(&used_ports, &mut current_port, max_port) {
        handle_port_exhaustion(action_sender, max_port);
        return Err(eyre!("No available ports in range"));
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
            // Single port fallback - ant_node_manager will find additional ports if needed
            Some(PortRange::Single(current_port))
        }
    };

    info!("Using pre-validated port range: {optimal_port_range:?}");

    // Call ant_node_manager with pre-validated ports
    match add_node_with_config(
        config,
        optimal_port_range,
        Some(count),
        node_registry.clone(),
    )
    .await
    {
        Ok(services) => {
            info!("Successfully added {count} nodes: {services:?}");

            // Start the newly added nodes if requested
            if start_nodes
                && let Err(err) =
                    start_nodes_helper(services.clone(), action_sender, node_registry).await
            {
                error!("Error while starting newly added nodes: {err:?}");
                send_action(
                    action_sender.clone(),
                    Action::StatusActions(StatusActions::ErrorStartingNodes {
                        services: vec![],
                        raw_error: err.to_string(),
                    }),
                );
            }

            // Send completion actions if requested
            if send_completion_actions {
                for service in &services {
                    send_action(
                        action_sender.clone(),
                        Action::NodeTableActions(NodeTableActions::AddNodesCompleted {
                            service_name: service.clone(),
                        }),
                    );
                }
            }

            Ok(services)
        }
        Err(err) => {
            error!("Error while adding {count} nodes: {err:?}");
            send_action(
                action_sender.clone(),
                Action::StatusActions(StatusActions::ErrorAddingNodes {
                    raw_error: err.to_string(),
                }),
            );
            Err(err)
        }
    }
}

#[derive(Debug)]
pub struct AddNodesArgs {
    pub action_sender: UnboundedSender<Action>,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
    pub count: u16,
    pub data_dir_path: Option<PathBuf>,
    pub network_id: Option<u8>,
    pub owner: Option<String>,
    pub init_peers_config: InitialPeersConfig,
    pub port_range: Option<PortRange>,
    pub rewards_address: Option<EvmAddress>,
}
