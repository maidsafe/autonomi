// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{Action, StatusActions};
use crate::connection_mode::ConnectionMode;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_node_manager::{add_services::config::PortRange, VerbosityLevel};
use ant_releases::{self, AntReleaseRepoActions, ReleaseType};
use ant_service_management::NodeRegistryManager;
use color_eyre::eyre::eyre;
use color_eyre::Result;
use std::{path::PathBuf, str::FromStr};
use tokio::runtime::Builder;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::LocalSet;

pub const PORT_MAX: u32 = 65535;
pub const PORT_MIN: u32 = 1024;

const NODE_ADD_MAX_RETRIES: u32 = 5;

pub const FIXED_INTERVAL: u64 = 60_000;

pub const NODES_ALL: &str = "NODES_ALL";

#[derive(Debug)]
pub enum NodeManagementTask {
    MaintainNodes {
        args: MaintainNodesArgs,
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
        args: MaintainNodesArgs,
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
    if let Err(err) = ant_node_manager::cmd::node::stop(
        None,
        node_registry.clone(),
        vec![],
        services.clone(),
        VerbosityLevel::Minimal,
    )
    .await
    {
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
                Action::StatusActions(StatusActions::StopNodesCompleted {
                    service_name: service,
                    all_nodes_data: node_registry.get_node_service_data().await,
                    is_nat_status_determined: node_registry.nat_status.read().await.is_some(),
                }),
            );
        }
    }
}

#[derive(Debug)]
pub struct MaintainNodesArgs {
    pub action_sender: UnboundedSender<Action>,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
    pub count: u16,
    pub data_dir_path: Option<PathBuf>,
    pub network_id: Option<u8>,
    pub owner: String,
    pub init_peers_config: InitialPeersConfig,
    pub port_range: Option<PortRange>,
    pub rewards_address: String,
    pub run_nat_detection: bool,
}

/// Maintain the specified number of nodes
async fn maintain_n_running_nodes(args: MaintainNodesArgs, node_registry: NodeRegistryManager) {
    debug!("Maintaining {} nodes", args.count);
    if args.run_nat_detection {
        run_nat_detection(&args.action_sender).await;
    }

    let config = prepare_node_config(&args);
    debug_log_config(&config, &args);

    let mut used_ports = get_used_ports(&node_registry).await;
    let (mut current_port, max_port) = get_port_range(&config.custom_ports);

    let nodes_to_add = args.count as i32 - node_registry.nodes.read().await.len() as i32;

    if nodes_to_add <= 0 {
        debug!("Scaling down nodes to {}", nodes_to_add);
        scale_down_nodes(&config, args.count, node_registry.clone()).await;
    } else {
        debug!("Scaling up nodes to {}", nodes_to_add);
        add_nodes(
            &args.action_sender,
            &config,
            nodes_to_add,
            &mut used_ports,
            &mut current_port,
            max_port,
            node_registry.clone(),
        )
        .await;
    }

    debug!("Finished maintaining {} nodes", args.count);
    send_action(
        args.action_sender,
        Action::StatusActions(StatusActions::StartNodesCompleted {
            service_name: NODES_ALL.to_string(),
            all_nodes_data: node_registry.get_node_service_data().await,
            is_nat_status_determined: node_registry.nat_status.read().await.is_some(),
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
                all_nodes_data: node_registry.get_node_service_data().await,
                is_nat_status_determined: node_registry.nat_status.read().await.is_some(),
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
    pub peer_ids: Vec<String>,
    pub provided_env_variables: Option<Vec<(String, String)>>,
    pub service_names: Vec<String>,
    pub url: Option<String>,
    pub version: Option<String>,
}

async fn upgrade_nodes(args: UpgradeNodesArgs, node_registry: NodeRegistryManager) {
    // First we stop the Nodes
    if let Err(err) = ant_node_manager::cmd::node::stop(
        None,
        node_registry.clone(),
        vec![],
        args.service_names.clone(),
        VerbosityLevel::Minimal,
    )
    .await
    {
        error!("Error while stopping services {err:?}");
        send_action(
            args.action_sender.clone(),
            Action::StatusActions(StatusActions::ErrorUpdatingNodes {
                raw_error: err.to_string(),
            }),
        );
    }

    if let Err(err) = ant_node_manager::cmd::node::upgrade(
        args.do_not_start,
        args.custom_bin_path,
        args.force,
        FIXED_INTERVAL,
        node_registry.clone(),
        args.peer_ids,
        args.provided_env_variables,
        args.service_names,
        args.url,
        args.version,
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
            Action::StatusActions(StatusActions::UpdateNodesCompleted {
                all_nodes_data: node_registry.get_node_service_data().await,
                is_nat_status_determined: node_registry.nat_status.read().await.is_some(),
            }),
        );
    }
}

async fn remove_nodes(
    services: Vec<String>,
    action_sender: UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
) {
    // First we stop the nodes
    if let Err(err) = ant_node_manager::cmd::node::stop(
        None,
        node_registry.clone(),
        vec![],
        services.clone(),
        VerbosityLevel::Minimal,
    )
    .await
    {
        error!("Error while stopping services {err:?}");
        send_action(
            action_sender.clone(),
            Action::StatusActions(StatusActions::ErrorRemovingNodes {
                services: services.clone(),
                raw_error: err.to_string(),
            }),
        );
    }

    if let Err(err) = ant_node_manager::cmd::node::remove(
        false,
        vec![],
        node_registry.clone(),
        services.clone(),
        VerbosityLevel::Minimal,
    )
    .await
    {
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
                Action::StatusActions(StatusActions::RemoveNodesCompleted {
                    service_name: service,
                    all_nodes_data: node_registry.get_node_service_data().await,
                    is_nat_status_determined: node_registry.nat_status.read().await.is_some(),
                }),
            );
        }
    }
}

async fn add_node(args: MaintainNodesArgs, node_registry: NodeRegistryManager) {
    debug!("Adding node");

    if args.run_nat_detection {
        run_nat_detection(&args.action_sender).await;
    }

    let config = prepare_node_config(&args);

    let used_ports = get_used_ports(&node_registry).await;
    let (mut current_port, max_port) = get_port_range(&config.custom_ports);

    while used_ports.contains(&current_port) && current_port <= max_port {
        current_port += 1;
    }

    if current_port > max_port {
        error!("Reached maximum port number. Unable to find an available port.");
        send_action(
            args.action_sender.clone(),
            Action::StatusActions(StatusActions::ErrorAddingNodes {
                raw_error: format!(
                    "When adding a new node we reached maximum port number ({max_port}).\nUnable to find an available port."
                ),
            }),
        );
    }

    let port_range = Some(PortRange::Single(current_port));
    match ant_node_manager::cmd::node::add(
        false, // alpha,
        false, // auto_restart,
        config.auto_set_nat_flags,
        Some(config.count),
        config.data_dir_path,
        None,       // env_variables,
        None,       // evm_network
        None,       // log_dir_path,
        None,       // log_format,
        None,       // max_archived_log_files,
        None,       // max_log_files,
        None,       // metrics_port,
        None,       // network_id
        None,       // node_ip,
        port_range, // node_port
        node_registry.clone(),
        config.init_peers_config.clone(),
        config.relay, // relay,
        false,        // reachability_check,
        RewardsAddress::from_str(config.rewards_address.as_str()).unwrap(),
        None,                        // rpc_address,
        None,                        // rpc_port,
        config.antnode_path.clone(), // src_path,
        !config.upnp,
        None, // url,
        None, // user,
        None, // version,
        VerbosityLevel::Minimal,
        false, // write_older_cache_files
    )
    .await
    {
        Err(err) => {
            error!("Error while adding services {err:?}");
            send_action(
                args.action_sender,
                Action::StatusActions(StatusActions::ErrorAddingNodes {
                    raw_error: err.to_string(),
                }),
            );
        }
        Ok(services) => {
            info!("Successfully added services: {:?}", services);
            for service in services {
                send_action(
                    args.action_sender.clone(),
                    Action::StatusActions(StatusActions::AddNodesCompleted {
                        service_name: service,
                        all_nodes_data: node_registry.get_node_service_data().await,
                        is_nat_status_determined: node_registry.nat_status.read().await.is_some(),
                    }),
                );
            }
        }
    }
}

async fn start_nodes(
    services: Vec<String>,
    action_sender: UnboundedSender<Action>,
    node_registry: NodeRegistryManager,
) {
    debug!("Starting node {:?}", services);
    if let Err(err) = ant_node_manager::cmd::node::start(
        FIXED_INTERVAL,
        node_registry.clone(),
        vec![],
        services.clone(),
        VerbosityLevel::Minimal,
    )
    .await
    {
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
                Action::StatusActions(StatusActions::StartNodesCompleted {
                    service_name: service,
                    all_nodes_data: node_registry.get_node_service_data().await,
                    is_nat_status_determined: node_registry.nat_status.read().await.is_some(),
                }),
            );
        }
    }
}

// --- Helper functions ---

fn send_action(action_sender: UnboundedSender<Action>, action: Action) {
    if let Err(err) = action_sender.send(action) {
        error!("Error while sending action: {err:?}");
    }
}

struct NodeConfig {
    antnode_path: Option<PathBuf>,
    auto_set_nat_flags: bool,
    count: u16,
    custom_ports: Option<PortRange>,
    data_dir_path: Option<PathBuf>,
    relay: bool,
    network_id: Option<u8>,
    owner: Option<String>,
    init_peers_config: InitialPeersConfig,
    rewards_address: String,
    upnp: bool,
}

/// Run the NAT detection process
async fn run_nat_detection(action_sender: &UnboundedSender<Action>) {
    info!("Running nat detection....");

    // Notify that NAT detection is starting
    if let Err(err) = action_sender.send(Action::StatusActions(StatusActions::NatDetectionStarted))
    {
        error!("Error while sending action: {err:?}");
    }

    let release_repo = <dyn AntReleaseRepoActions>::default_config();
    let version = match release_repo
        .get_latest_version(&ReleaseType::NatDetection)
        .await
    {
        Ok(v) => {
            info!("Using NAT detection version {}", v.to_string());
            v.to_string()
        }
        Err(err) => {
            info!("No NAT detection release found, using fallback version 0.1.0");
            info!("Error: {err}");
            "0.1.0".to_string()
        }
    };

    if let Err(err) = ant_node_manager::cmd::nat_detection::run_nat_detection(
        None,
        true,
        None,
        None,
        Some(version),
        VerbosityLevel::Minimal,
    )
    .await
    {
        error!("Error while running nat detection {err:?}. Registering the error.");
        if let Err(err) = action_sender.send(Action::StatusActions(
            StatusActions::ErrorWhileRunningNatDetection,
        )) {
            error!("Error while sending action: {err:?}");
        }
    } else {
        info!("Successfully ran nat detection.");
        if let Err(err) = action_sender.send(Action::StatusActions(
            StatusActions::SuccessfullyDetectedNatStatus,
        )) {
            error!("Error while sending action: {err:?}");
        }
    }
}

fn prepare_node_config(args: &MaintainNodesArgs) -> NodeConfig {
    NodeConfig {
        antnode_path: args.antnode_path.clone(),
        auto_set_nat_flags: args.connection_mode == ConnectionMode::Automatic,
        data_dir_path: args.data_dir_path.clone(),
        count: args.count,
        custom_ports: if args.connection_mode == ConnectionMode::CustomPorts {
            args.port_range.clone()
        } else {
            None
        },
        owner: if args.owner.is_empty() {
            None
        } else {
            Some(args.owner.clone())
        },
        relay: args.connection_mode == ConnectionMode::HomeNetwork,
        network_id: args.network_id,
        init_peers_config: args.init_peers_config.clone(),
        rewards_address: args.rewards_address.clone(),
        upnp: args.connection_mode == ConnectionMode::UPnP,
    }
}

/// Debug log the node config
fn debug_log_config(config: &NodeConfig, args: &MaintainNodesArgs) {
    debug!("************ STARTING NODE MAINTENANCE ************");
    debug!(
        "Maintaining {} running nodes with the following args:",
        config.count
    );
    debug!(
        " owner: {:?}, init_peers_config: {:?}, antnode_path: {:?}, network_id: {:?}",
        config.owner, config.init_peers_config, config.antnode_path, args.network_id
    );
    debug!(
        " data_dir_path: {:?}, connection_mode: {:?}",
        config.data_dir_path, args.connection_mode
    );
    debug!(
        " auto_set_nat_flags: {:?}, custom_ports: {:?}, upnp: {}, relay: {}",
        config.auto_set_nat_flags, config.custom_ports, config.upnp, config.relay
    );
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
    debug!("Currently used ports: {:?}", used_ports);
    used_ports
}

/// Get the port range (u16, u16) from the custom ports PortRange
fn get_port_range(custom_ports: &Option<PortRange>) -> (u16, u16) {
    match custom_ports {
        Some(PortRange::Single(port)) => (*port, *port),
        Some(PortRange::Range(start, end)) => (*start, *end),
        None => (PORT_MIN as u16, PORT_MAX as u16),
    }
}

/// Scale down the nodes
async fn scale_down_nodes(config: &NodeConfig, count: u16, node_registry: NodeRegistryManager) {
    match ant_node_manager::cmd::node::maintain_n_running_nodes(
        false,
        false,
        config.auto_set_nat_flags,
        count,
        config.data_dir_path.clone(),
        None,
        Some(EvmNetwork::default()),
        None,
        None,
        None,
        None,
        None,
        config.network_id,
        None,
        None, // We don't care about the port, as we are scaling down
        node_registry,
        config.init_peers_config.clone(),
        false,
        config.relay,
        RewardsAddress::from_str(config.rewards_address.as_str()).unwrap(),
        None,
        None,
        config.antnode_path.clone(),
        None,
        !config.upnp,
        None,
        None,
        VerbosityLevel::Minimal,
        FIXED_INTERVAL,
        false,
    )
    .await
    {
        Ok(_) => {
            info!("Scaling down to {} nodes", count);
        }
        Err(err) => {
            error!("Error while scaling down to {} nodes: {err:?}", count);
        }
    }
}

/// Add the specified number of nodes
async fn add_nodes(
    action_sender: &UnboundedSender<Action>,
    config: &NodeConfig,
    mut nodes_to_add: i32,
    used_ports: &mut Vec<u16>,
    current_port: &mut u16,
    max_port: u16,
    node_registry: NodeRegistryManager,
) {
    let mut retry_count = 0;

    while nodes_to_add > 0 && retry_count < NODE_ADD_MAX_RETRIES {
        // Find the next available port
        while used_ports.contains(current_port) && *current_port <= max_port {
            *current_port += 1;
        }

        if *current_port > max_port {
            error!("Reached maximum port number. Unable to find an available port.");
            send_action(
                action_sender.clone(),
                Action::StatusActions(StatusActions::ErrorScalingUpNodes {
                    raw_error: format!(
                        "Reached maximum port number ({max_port}).\nUnable to find an available port."
                    ),
                }),
            );
            break;
        }

        let port_range = Some(PortRange::Single(*current_port));
        match ant_node_manager::cmd::node::maintain_n_running_nodes(
            false,
            false,
            config.auto_set_nat_flags,
            config.count,
            config.data_dir_path.clone(),
            None,
            Some(EvmNetwork::default()),
            None,
            None,
            None,
            None,
            None,
            config.network_id,
            None,
            port_range,
            node_registry.clone(),
            config.init_peers_config.clone(),
            false,
            config.relay,
            RewardsAddress::from_str(config.rewards_address.as_str()).unwrap(),
            None,
            None,
            config.antnode_path.clone(),
            None,
            !config.upnp,
            None,
            None,
            VerbosityLevel::Minimal,
            FIXED_INTERVAL,
            false,
        )
        .await
        {
            Ok(_) => {
                info!("Successfully added a node on port {}", current_port);
                used_ports.push(*current_port);
                nodes_to_add -= 1;
                *current_port += 1;
                retry_count = 0; // Reset retry count on success
            }
            Err(err) => {
                //TODO: We should use concrete error types here instead of string matching (ant_node_manager)
                if err.to_string().contains("is being used by another service") {
                    warn!(
                        "Port {} is being used, retrying with a different port. Attempt {}/{}",
                        current_port,
                        retry_count + 1,
                        NODE_ADD_MAX_RETRIES
                    );
                } else if err
                    .to_string()
                    .contains("Failed to add one or more services")
                    && retry_count >= NODE_ADD_MAX_RETRIES
                {
                    send_action(
                        action_sender.clone(),
                        Action::StatusActions(StatusActions::ErrorScalingUpNodes {
                            raw_error: "When trying to add a node, we failed.\n\
                                 Maybe you ran out of disk space?\n\
                                 Maybe you need to change the port range?"
                                .to_string(),
                        }),
                    );
                } else if err
                    .to_string()
                    .contains("contains a virus or potentially unwanted software")
                    && retry_count >= NODE_ADD_MAX_RETRIES
                {
                    send_action(
                        action_sender.clone(),
                        Action::StatusActions(StatusActions::ErrorScalingUpNodes {
                            raw_error: "When trying to add a node, we failed.\n\
                             You may be running an old version of antnode service?\n\
                             Did you whitelisted antnode and the launchpad?"
                                .to_string(),
                        }),
                    );
                } else {
                    error!("Range of ports to be used {:?}", *current_port..max_port);
                    error!("Error while adding node on port {}: {err:?}", current_port);
                }
                // In case of error, we increase the port and the retry count
                *current_port += 1;
                retry_count += 1;
            }
        }
    }
    if retry_count >= NODE_ADD_MAX_RETRIES {
        send_action(
            action_sender.clone(),
            Action::StatusActions(StatusActions::ErrorScalingUpNodes {
                raw_error: format!(
                    "When trying to start a node, we reached the maximum amount of retries ({NODE_ADD_MAX_RETRIES}).\n\
                    Could this be a firewall blocking nodes starting or ports on your router already in use?"
                ),
            }),
        );
    }
}
