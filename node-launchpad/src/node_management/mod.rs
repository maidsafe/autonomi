// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod config;
pub mod error;
pub mod handlers;

use crate::action::{Action, NodeManagementResponse, NodeTableActions};
use ant_service_management::NodeRegistryManager;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::LocalSet;

pub use config::{AddNodesConfig, UpgradeNodesConfig};

#[derive(Debug)]
pub enum NodeManagementTask {
    RegisterActionSender {
        action_sender: UnboundedSender<Action>,
    },
    MaintainNodes {
        config: AddNodesConfig,
    },
    ResetNodes,
    StopNodes {
        services: Vec<String>,
    },
    UpgradeNodes {
        config: UpgradeNodesConfig,
    },
    AddNode {
        config: AddNodesConfig,
    },
    RemoveNodes {
        services: Vec<String>,
    },
    StartNode {
        services: Vec<String>,
    },
}

#[derive(Clone)]
pub struct NodeManagement {
    task_sender: UnboundedSender<NodeManagementTask>,
}

impl NodeManagement {
    pub fn new(node_registry: NodeRegistryManager) -> Result<Self> {
        let (task_sender, task_recv) = mpsc::unbounded_channel();

        let rt = Builder::new_current_thread().enable_all().build()?;

        std::thread::spawn(move || {
            let local = LocalSet::new();

            local.spawn_local(async move { Self::handle_actions(task_recv, node_registry).await });

            // This will return once all senders are dropped and all
            // spawned tasks have returned.
            rt.block_on(local);
        });

        Ok(Self { task_sender })
    }

    async fn handle_actions(
        mut task_recv: mpsc::UnboundedReceiver<NodeManagementTask>,
        node_registry: NodeRegistryManager,
    ) {
        let mut action_sender_main = None;
        while let Some(new_task) = task_recv.recv().await {
            match new_task {
                NodeManagementTask::RegisterActionSender {
                    action_sender: new_sender,
                } => {
                    action_sender_main = Some(new_sender);
                }
                NodeManagementTask::MaintainNodes { config } => {
                    let Some(action_sender) = action_sender_main.as_ref() else {
                        error!(
                            "No action sender registered, cannot proceed with maintaining nodes"
                        );
                        continue;
                    };

                    let error = if let Err(err) =
                        handlers::maintain_n_running_nodes(config, node_registry.clone()).await
                    {
                        Some(err.to_string())
                    } else {
                        None
                    };

                    if let Err(err) = action_sender.send(Action::NodeTableActions(
                        NodeTableActions::NodeManagementResponse(
                            NodeManagementResponse::MaintainNodes { error },
                        ),
                    )) {
                        error!("Failed to send MaintainNodesResult action: {err:?}");
                    }
                }
                NodeManagementTask::ResetNodes => {
                    let Some(action_sender) = action_sender_main.as_ref() else {
                        error!("No action sender registered, cannot proceed with resetting nodes");
                        continue;
                    };

                    let error = if let Err(err) = handlers::reset_nodes(node_registry.clone()).await
                    {
                        Some(err.to_string())
                    } else {
                        None
                    };

                    if let Err(err) = action_sender.send(Action::NodeTableActions(
                        NodeTableActions::NodeManagementResponse(
                            NodeManagementResponse::ResetNodes { error },
                        ),
                    )) {
                        error!("Failed to send ResetNodesResult action: {err:?}");
                    }
                }
                NodeManagementTask::StopNodes { services } => {
                    let Some(action_sender) = action_sender_main.as_ref() else {
                        error!("No action sender registered, cannot proceed with stopping nodes");
                        continue;
                    };

                    let error = if let Err(err) =
                        handlers::stop_nodes(services.clone(), node_registry.clone()).await
                    {
                        Some(err.to_string())
                    } else {
                        None
                    };

                    if let Err(err) = action_sender.send(Action::NodeTableActions(
                        NodeTableActions::NodeManagementResponse(
                            NodeManagementResponse::StopNodes {
                                service_names: services,
                                error,
                            },
                        ),
                    )) {
                        error!("Failed to send StopNodesResult action: {err:?}");
                    }
                }
                NodeManagementTask::UpgradeNodes { config } => {
                    let Some(action_sender) = action_sender_main.as_ref() else {
                        error!("No action sender registered, cannot proceed with upgrading nodes");
                        continue;
                    };
                    let service_names = config.service_names.clone();

                    let error = if let Err(err) =
                        handlers::upgrade_nodes(config, node_registry.clone()).await
                    {
                        Some(err.to_string())
                    } else {
                        None
                    };
                    if let Err(err) = action_sender.send(Action::NodeTableActions(
                        NodeTableActions::NodeManagementResponse(
                            NodeManagementResponse::UpgradeNodes {
                                service_names,
                                error,
                            },
                        ),
                    )) {
                        error!("Failed to send UpgradeNodesResult action: {err:?}");
                    }
                }
                NodeManagementTask::RemoveNodes { services } => {
                    let Some(action_sender) = action_sender_main.as_ref() else {
                        error!("No action sender registered, cannot proceed with removing nodes");
                        continue;
                    };
                    let error = if let Err(err) =
                        handlers::remove_nodes(services.clone(), node_registry.clone()).await
                    {
                        Some(err.to_string())
                    } else {
                        None
                    };
                    if let Err(err) = action_sender.send(Action::NodeTableActions(
                        NodeTableActions::NodeManagementResponse(
                            NodeManagementResponse::RemoveNodes {
                                service_names: services,
                                error,
                            },
                        ),
                    )) {
                        error!("Failed to send RemoveNodesResult action: {err:?}");
                    }
                }
                NodeManagementTask::StartNode { services } => {
                    let Some(action_sender) = action_sender_main.as_ref() else {
                        error!("No action sender registered, cannot proceed with starting nodes");
                        continue;
                    };
                    let error = if let Err(err) =
                        handlers::start_nodes(services.clone(), node_registry.clone()).await
                    {
                        Some(err.to_string())
                    } else {
                        None
                    };
                    if let Err(err) = action_sender.send(Action::NodeTableActions(
                        NodeTableActions::NodeManagementResponse(
                            NodeManagementResponse::StartNodes {
                                service_names: services,
                                error,
                            },
                        ),
                    )) {
                        error!("Failed to send StartNodesResult action: {err:?}");
                    }
                }
                NodeManagementTask::AddNode { config } => {
                    let Some(action_sender) = action_sender_main.as_ref() else {
                        error!("No action sender registered, cannot proceed with adding nodes");
                        continue;
                    };
                    let error = if let Err(err) =
                        handlers::add_nodes(config, node_registry.clone()).await
                    {
                        Some(err.to_string())
                    } else {
                        None
                    };
                    if let Err(err) = action_sender.send(Action::NodeTableActions(
                        NodeTableActions::NodeManagementResponse(NodeManagementResponse::AddNode {
                            error,
                        }),
                    )) {
                        error!("Failed to send AddNodeResult action: {err:?}");
                    }
                }
            }
        }
        // If the while loop returns, then all the LocalSpawner
        // objects have been dropped.
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
