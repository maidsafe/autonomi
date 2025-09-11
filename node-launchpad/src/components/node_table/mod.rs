// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod node_item;
pub mod operations;
pub mod state;
pub mod table_state;
pub mod widget;

use crate::action::{Action, NodeManagementCommand, NodeManagementResponse, NodeTableActions};
use crate::components::Component;
use crate::components::popup::error_popup::ErrorPopup;
use crate::focus::FocusTarget;
use crate::mode::Scene;
use crate::tui::Frame;
use ant_service_management::ServiceStatus;
use color_eyre::Result;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

// Re-exports for convenience
pub use node_item::{NodeItem, NodeStatus};
pub use operations::{AddNodeConfig, MaintainNodesConfig, NodeOperations};
pub use state::NodeTableState;
pub use table_state::StatefulTable;
pub use widget::{NodeTableConfig, NodeTableWidget};

pub struct NodeTableComponent {
    pub state: NodeTableState,
    action_sender: Option<UnboundedSender<Action>>,
}

impl NodeTableComponent {
    pub async fn new(config: NodeTableConfig) -> Result<Self> {
        let state = NodeTableState::new(config).await?;
        Ok(Self {
            state,
            action_sender: None,
        })
    }

    pub fn state(&self) -> &NodeTableState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut NodeTableState {
        &mut self.state
    }
}

impl Component for NodeTableComponent {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_sender = Some(tx.clone());
        self.state.operations.register_action_sender(tx.clone())?;

        let node_registry_clone = self.state().node_registry.clone();
        let action_sender_clone = tx.clone();
        tokio::spawn(async move {
            debug!("Refreshing node registry on startup");
            let services = node_registry_clone.get_node_service_data().await;
            debug!("Registry refresh complete. Found {} nodes", services.len());
            if let Err(e) = action_sender_clone.send(Action::NodeTableActions(
                NodeTableActions::RegistryUpdated {
                    all_nodes_data: services,
                },
            )) {
                error!("Failed to send initial registry update: {e}");
            }
        });

        // Watch for registry file changes
        let action_sender_clone = tx.clone();
        let mut node_registry_watcher = self.state.node_registry.watch_registry_file()?;
        let node_registry_clone = self.state.node_registry.clone();
        tokio::spawn(async move {
            while let Some(()) = node_registry_watcher.recv().await {
                let services = node_registry_clone.get_node_service_data().await;
                debug!(
                    "Node registry file has been updated. Sending NodeTableActions::RegistryUpdated event."
                );
                if let Err(e) = action_sender_clone.send(Action::NodeTableActions(
                    NodeTableActions::RegistryUpdated {
                        all_nodes_data: services,
                    },
                )) {
                    error!("Failed to send NodeTableActions::RegistryUpdated: {e}");
                }
            }
        });

        // Send initial state update to synchronize Status component's cached state
        // This ensures the Status component knows about nodes loaded from registry during initialization
        self.state.send_state_update()?;

        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            // Handle NodeTableActions directly
            Action::NodeTableActions(node_action) => match node_action {
                NodeTableActions::StateChanged { .. } => {
                    // StateChanged is sent by NodeTable, not handled by it
                    Ok(None)
                }
                NodeTableActions::RegistryUpdated { all_nodes_data } => {
                    self.state_mut().sync_node_service_data(&all_nodes_data);
                    self.state_mut().send_state_update()?;
                    Ok(None)
                }
                NodeTableActions::TriggerNodeLogs => {
                    debug!("NodeTable: TriggerNodeLogs action received");
                    if self.state.items.items.is_empty() {
                        debug!("No nodes available for logs viewing");
                        return Ok(None);
                    }

                    let selected_node_name = self
                        .state
                        .items
                        .selected_item()
                        .map(|node| node.service_name.clone())
                        .unwrap_or_else(|| {
                            self.state
                                .items
                                .items
                                .first()
                                .map(|node| node.service_name.clone())
                                .unwrap_or_else(|| "No node available".to_string())
                        });

                    Ok(Some(Action::SetNodeLogsTarget(selected_node_name)))
                }
                NodeTableActions::TriggerRemoveNodePopup => {
                    debug!(
                        "NodeTable: TriggerRemoveNodePopup action received, showing RemoveNodePopUp"
                    );
                    Ok(Some(Action::SwitchScene(Scene::RemoveNodePopUp)))
                }
                NodeTableActions::NodeManagementCommand(command) => {
                    debug!("NodeTable: Handling NodeManagementCommand: {:?}", command);
                    match command {
                        NodeManagementCommand::MaintainNodes => {
                            // todo how should we lock nodes here?
                            let config = operations::MaintainNodesConfig {
                                rewards_address: self.state.rewards_address.as_ref(),
                                nodes_to_start: self.state.nodes_to_start,
                                antnode_path: self.state.antnode_path.clone(),
                                connection_mode: self.state.connection_mode,
                                data_dir_path: self.state.data_dir_path.clone(),
                                network_id: self.state.network_id,
                                init_peers_config: self.state.init_peers_config.clone(),
                                port_from: self.state.port_from,
                                port_to: self.state.port_to,
                            };
                            self.state.operations.handle_maintain_nodes(&config)
                        }
                        NodeManagementCommand::AddNode => {
                            // todo how should we lock nodes here?
                            let config = operations::AddNodeConfig {
                                node_count: self.state.items.items.len() as u64,
                                available_disk_space_gb: self.state.available_disk_space_gb,
                                storage_mountpoint: &self.state.storage_mountpoint,
                                rewards_address: self.state.rewards_address.as_ref(),
                                nodes_to_start: self.state.nodes_to_start,
                                antnode_path: self.state.antnode_path.clone(),
                                connection_mode: self.state.connection_mode,
                                data_dir_path: self.state.data_dir_path.clone(),
                                network_id: self.state.network_id,
                                init_peers_config: self.state.init_peers_config.clone(),
                                port_from: self.state.port_from,
                                port_to: self.state.port_to,
                            };
                            self.state.operations.handle_add_node(&config)
                        }
                        NodeManagementCommand::StartNodes => {
                            let stopped_nodes: Vec<String> = self
                                .state
                                .node_services
                                .iter()
                                .filter_map(|node| {
                                    if node.status == ServiceStatus::Stopped
                                        || node.status == ServiceStatus::Added
                                    {
                                        Some(node.service_name.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            self.state.operations.handle_start_node(stopped_nodes)?;
                            Ok(None)
                        }
                        NodeManagementCommand::StopNodes => {
                            let running_nodes = self.state.get_running_nodes();
                            self.state
                                .operations
                                .handle_stop_nodes(running_nodes.clone())?;
                            Ok(None)
                        }
                        NodeManagementCommand::ToggleNode => {
                            // toggle the selected node from running to stopped or vice versa
                            let selected_node = self
                                .state
                                .items
                                .selected_item()
                                .map(|node| node.service_name.clone());

                            if let Some(service_name) = selected_node
                                && let Some(node_item) = self.state.get_node_item_mut(&service_name)
                            {
                                match node_item.status {
                                    NodeStatus::Running => {
                                        debug!("Toggling node {}: Stopping it", service_name);
                                        self.state
                                            .operations
                                            .handle_stop_nodes(vec![service_name])?;
                                    }
                                    NodeStatus::Stopped => {
                                        debug!("Toggling node {}: Starting it", service_name);
                                        self.state
                                            .operations
                                            .handle_start_node(vec![service_name])?;
                                    }
                                    _ => {
                                        debug!(
                                            "Toggling node {}: No action for status {:?}",
                                            service_name, node_item.status
                                        );
                                    }
                                }
                            }
                            Ok(None)
                        }
                        NodeManagementCommand::RemoveNodes => {
                            let selected_node = self
                                .state
                                .items
                                .selected_item()
                                .map(|node| node.service_name.clone());
                            if let Some(service_name) = selected_node {
                                self.state
                                    .operations
                                    .handle_remove_nodes(vec![service_name])?;
                            }
                            Ok(None)
                        }
                        NodeManagementCommand::UpgradeNodes => {
                            let all_service_names: Vec<String> = self
                                .state()
                                .items
                                .items
                                .iter()
                                .map(|item| item.service_name.clone())
                                .collect();
                            if !all_service_names.is_empty() {
                                self.state_mut()
                                    .operations
                                    .handle_upgrade_nodes(all_service_names)?;
                            }
                            Ok(None)
                        }
                        NodeManagementCommand::ResetNodes => {
                            self.state_mut().operations.handle_reset_nodes()?;
                            Ok(None)
                        }
                    }
                }
                // Handle node management responses
                NodeTableActions::NodeManagementResponse(response) => match response {
                    NodeManagementResponse::MaintainNodes { error } => {
                        if let Some(err) = error {
                            let error_popup = ErrorPopup::new(
                                "Error while managing nodes",
                                "Please try again",
                                &err,
                            );
                            Ok(Some(Action::ShowErrorPopup(error_popup)))
                        } else {
                            for item in self.state.items.items.iter_mut() {
                                if item.status == NodeStatus::Starting {
                                    item.unlock();
                                    item.update_status(NodeStatus::Running);
                                }
                            }
                            Ok(None)
                        }
                    }
                    NodeManagementResponse::AddNode { error } => {
                        if let Some(err) = error {
                            let error_popup = ErrorPopup::new(
                                "Error while adding node",
                                "Please try again",
                                &err,
                            );
                            Ok(Some(Action::ShowErrorPopup(error_popup)))
                        } else {
                            Ok(None)
                        }
                    }
                    NodeManagementResponse::StartNodes {
                        service_names,
                        error,
                    } => {
                        if let Some(err) = error {
                            let error_popup = ErrorPopup::new(
                                "Error while starting nodes",
                                "Please try again",
                                &err,
                            );
                            Ok(Some(Action::ShowErrorPopup(error_popup)))
                        } else {
                            for service_name in service_names {
                                if let Some(node_item) = self.state.get_node_item_mut(&service_name)
                                {
                                    node_item.unlock();
                                    node_item.update_status(NodeStatus::Running);
                                }
                            }
                            Ok(None)
                        }
                    }

                    NodeManagementResponse::StopNodes {
                        service_names,
                        error,
                    } => {
                        if let Some(err) = error {
                            let error_popup = ErrorPopup::new(
                                "Error while stopping nodes",
                                "Please try again",
                                &err,
                            );
                            Ok(Some(Action::ShowErrorPopup(error_popup)))
                        } else {
                            for service_name in service_names {
                                if let Some(node_item) = self.state.get_node_item_mut(&service_name)
                                {
                                    node_item.unlock();
                                    node_item.update_status(NodeStatus::Stopped);
                                }
                            }
                            Ok(None)
                        }
                    }
                    NodeManagementResponse::RemoveNodes {
                        service_names,
                        error,
                    } => {
                        if let Some(err) = error {
                            let error_popup = ErrorPopup::new(
                                "Error while removing nodes",
                                "Please try again",
                                &err,
                            );
                            Ok(Some(Action::ShowErrorPopup(error_popup)))
                        } else {
                            for service_name in service_names.iter() {
                                if let Some(node_item) = self.state.get_node_item_mut(service_name)
                                {
                                    node_item.unlock();
                                    node_item.update_status(NodeStatus::Removed);
                                }
                            }
                            self.state
                                .items
                                .items
                                .retain(|item| !service_names.contains(&item.service_name));
                            Ok(None)
                        }
                    }
                    NodeManagementResponse::UpgradeNodes {
                        service_names,
                        error,
                    } => {
                        if let Some(err) = error {
                            let error_popup = ErrorPopup::new(
                                "Error while upgrading nodes",
                                "Please try again",
                                &err,
                            );
                            Ok(Some(Action::ShowErrorPopup(error_popup)))
                        } else {
                            for service_name in service_names {
                                if let Some(node_item) = self.state.get_node_item_mut(&service_name)
                                {
                                    node_item.unlock();
                                    node_item.update_status(NodeStatus::Running);
                                }
                            }
                            Ok(None)
                        }
                    }
                    NodeManagementResponse::ResetNodes { error } => {
                        if let Some(err) = error {
                            let error_popup = ErrorPopup::new(
                                "Error while resetting nodes",
                                "Please try again",
                                &err,
                            );
                            Ok(Some(Action::ShowErrorPopup(error_popup)))
                        } else {
                            Ok(None)
                        }
                    }
                },
                // Navigation actions
                NodeTableActions::NavigateUp => {
                    debug!("NodeTable: Handling NavigateUp action - calling previous()");
                    let before_selected = self.state.items.state.selected();
                    self.state.items.previous();
                    let after_selected = self.state.items.state.selected();
                    debug!(
                        "NodeTable: Selection changed from {:?} to {:?}",
                        before_selected, after_selected
                    );
                    Ok(None)
                }
                NodeTableActions::NavigateDown => {
                    debug!("NodeTable: Handling NavigateDown action - calling next()");
                    let before_selected = self.state.items.state.selected();
                    self.state.items.next();
                    let after_selected = self.state.items.state.selected();
                    debug!(
                        "NodeTable: Selection changed from {:?} to {:?}",
                        before_selected, after_selected
                    );
                    Ok(None)
                }
                NodeTableActions::NavigateHome => {
                    if !self.state.items.items.is_empty() {
                        self.state.items.state.select(Some(0));
                    }
                    Ok(None)
                }
                NodeTableActions::NavigateEnd => {
                    if !self.state.items.items.is_empty() {
                        self.state
                            .items
                            .state
                            .select(Some(self.state.items.items.len() - 1));
                    }
                    Ok(None)
                }
                NodeTableActions::NavigatePageUp => {
                    for _ in 0..10 {
                        self.state.items.previous();
                    }
                    Ok(None)
                }
                NodeTableActions::NavigatePageDown => {
                    for _ in 0..10 {
                        self.state.items.next();
                    }
                    Ok(None)
                }
            },
            _ => Ok(None),
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let widget = NodeTableWidget;
        widget.render(area, f, &mut self.state);
        Ok(())
    }

    fn focus_target(&self) -> FocusTarget {
        FocusTarget::NodeTable
    }
}
