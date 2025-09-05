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

use crate::action::Action;
use ant_service_management::NodeRegistryManager;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::LocalSet;

pub use config::{AddNodesConfig, UpgradeNodesConfig};

#[derive(Debug)]
pub enum NodeManagementTask {
    MaintainNodes {
        config: AddNodesConfig,
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
        config: UpgradeNodesConfig,
    },
    AddNode {
        config: AddNodesConfig,
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
                        NodeManagementTask::MaintainNodes { config: args } => {
                            handlers::maintain_n_running_nodes(args, node_registry.clone()).await;
                        }
                        NodeManagementTask::ResetNodes {
                            start_nodes_after_reset,
                            action_sender,
                        } => {
                            handlers::reset_nodes(
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
                            handlers::stop_nodes(services, action_sender, node_registry.clone())
                                .await;
                        }
                        NodeManagementTask::UpgradeNodes { config: args } => {
                            handlers::upgrade_nodes(args, node_registry.clone()).await
                        }
                        NodeManagementTask::RemoveNodes {
                            services,
                            action_sender,
                        } => {
                            handlers::remove_nodes(services, action_sender, node_registry.clone())
                                .await
                        }
                        NodeManagementTask::StartNode {
                            services,
                            action_sender,
                        } => {
                            handlers::start_nodes(services, action_sender, node_registry.clone())
                                .await
                        }
                        NodeManagementTask::AddNode { config: args } => {
                            handlers::add_node(args, node_registry.clone()).await
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
