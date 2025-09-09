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

// Re-exports for convenience
pub use node_item::{NodeItem, NodeStatus};
pub use operations::{AddNodeConfig, NodeOperations, StartNodesConfig};
pub use state::NodeTableState;
pub use table_state::StatefulTable;
pub use widget::{NodeTableConfig, NodeTableWidget};

use crate::action::{Action, NodeManagementResponse, NodeTableActions, StatusActions};
use crate::components::Component;
use crate::components::popup::error_popup::ErrorPopup;
use crate::focus::{EventResult, FocusManager, FocusTarget};
use crate::mode::Scene;
use crate::tui::Frame;
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

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

    fn handle_table_navigation(&mut self, key: KeyEvent) -> Result<(Vec<Action>, EventResult)> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                debug!("NodeTable: Handling Up key - calling previous()");
                let before_selected = self.state.items.state.selected();
                self.state.items.previous();
                let after_selected = self.state.items.state.selected();
                debug!(
                    "NodeTable: Selection changed from {:?} to {:?}",
                    before_selected, after_selected
                );
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::Down | KeyCode::Char('j') => {
                debug!("NodeTable: Handling Down key - calling next()");
                let before_selected = self.state.items.state.selected();
                self.state.items.next();
                let after_selected = self.state.items.state.selected();
                debug!(
                    "NodeTable: Selection changed from {:?} to {:?}",
                    before_selected, after_selected
                );
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::Home | KeyCode::Char('g') => {
                if !self.state.items.items.is_empty() {
                    self.state.items.state.select(Some(0));
                }
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.state.items.items.is_empty() {
                    self.state
                        .items
                        .state
                        .select(Some(self.state.items.items.len() - 1));
                }
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::PageUp => {
                for _ in 0..10 {
                    self.state.items.previous();
                }
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::PageDown => {
                for _ in 0..10 {
                    self.state.items.next();
                }
                Ok((vec![], EventResult::Consumed))
            }
            _ => Ok((vec![], EventResult::Ignored)),
        }
    }

    fn handle_node_operations(&mut self, key: KeyEvent) -> Result<(Vec<Action>, EventResult)> {
        match key.code {
            KeyCode::Char('+') => Ok((
                vec![Action::NodeTableActions(NodeTableActions::AddNode)],
                EventResult::Consumed,
            )),
            KeyCode::Char('-') => Ok((
                vec![Action::NodeTableActions(
                    NodeTableActions::TriggerRemoveNode,
                )],
                EventResult::Consumed,
            )),
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::NodeTableActions(NodeTableActions::StartStopNode)],
                EventResult::Consumed,
            )),
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::NodeTableActions(NodeTableActions::StartNodes)],
                EventResult::Consumed,
            )),
            KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::NodeTableActions(NodeTableActions::StopNodes)],
                EventResult::Consumed,
            )),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::NodeTableActions(NodeTableActions::RemoveNodes)],
                EventResult::Consumed,
            )),
            KeyCode::Char('l') | KeyCode::Char('L') => Ok((
                vec![Action::NodeTableActions(NodeTableActions::TriggerNodeLogs)],
                EventResult::Consumed,
            )),
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::StatusActions(StatusActions::TriggerManageNodes)],
                EventResult::Consumed,
            )),
            KeyCode::Enter => {
                if !self.state.items.items.is_empty() && self.state.items.state.selected().is_some()
                {
                    Ok((
                        vec![Action::NodeTableActions(NodeTableActions::StartStopNode)],
                        EventResult::Consumed,
                    ))
                } else {
                    Ok((vec![], EventResult::Ignored))
                }
            }
            _ => Ok((vec![], EventResult::Ignored)),
        }
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

    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        _focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        if let (actions, EventResult::Consumed) = self.handle_table_navigation(key)? {
            return Ok((actions, EventResult::Consumed));
        }

        self.handle_node_operations(key)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            // Handle NodeTableActions directly
            Action::NodeTableActions(node_action) => match node_action {
                NodeTableActions::RegistryUpdated { all_nodes_data } => {
                    self.state_mut().sync_node_service_data(&all_nodes_data);
                    self.state_mut().send_state_update()?;
                    Ok(None)
                }
                NodeTableActions::AddNode => {
                    debug!("NodeTable: Handling AddNode action");
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
                NodeTableActions::StartNodes => {
                    debug!("NodeTable: Handling StartNodes action");
                    let config = operations::StartNodesConfig {
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
                    self.state.operations.handle_start_nodes(&config)
                }
                NodeTableActions::StopNodes => {
                    debug!("NodeTable: Handling StopNodes action");
                    let running_nodes = self.state.get_running_nodes();
                    self.state
                        .operations
                        .handle_stop_nodes(running_nodes.clone())?;
                    Ok(None)
                }
                NodeTableActions::StartStopNode => {
                    debug!("NodeTable: Handling StartStopNode action");
                    let (service_name, node_locked, node_status) = {
                        if let Some(node_item) = self.state.items.selected_item() {
                            (
                                node_item.service_name.clone(),
                                node_item.locked,
                                node_item.status,
                            )
                        } else {
                            return Ok(None);
                        }
                    };

                    if node_locked {
                        debug!("Cannot start/stop node {service_name:?} while it is locked");
                        return Ok(None);
                    }

                    if node_status == NodeStatus::Removed {
                        debug!("Node {service_name} is removed. Cannot be started.");
                        return Ok(None);
                    }

                    match node_status {
                        NodeStatus::Stopped | NodeStatus::Added => {
                            debug!("Starting Node {service_name:?}");
                            self.state
                                .operations
                                .handle_start_node(vec![service_name.clone()])?;
                            if let Some(node_item) = self.state.items.selected_item_mut() {
                                node_item.status = NodeStatus::Starting;
                            }
                        }
                        NodeStatus::Running => {
                            debug!("Stopping Node {service_name:?}");
                            self.state
                                .operations
                                .handle_stop_nodes(vec![service_name.clone()])?;
                            if let Some(node_item) = self.state.items.selected_item_mut() {
                                node_item.lock();
                            }
                        }
                        _ => {
                            debug!("Cannot Start/Stop node. Node status is {node_status:?}");
                        }
                    }
                    Ok(None)
                }
                NodeTableActions::RemoveNodes => {
                    debug!("NodeTable: Handling RemoveNodes action");
                    if let Some(node_item) = self.state.items.selected_item_mut() {
                        if node_item.locked {
                            debug!(
                                "Node {} still performing operation. Cannot remove",
                                node_item.service_name
                            );
                            return Ok(None);
                        }
                        node_item.lock();
                        self.state
                            .operations
                            .handle_remove_nodes(vec![node_item.service_name.clone()])?;
                    }
                    Ok(None)
                }
                NodeTableActions::TriggerRemoveNode => {
                    debug!("NodeTable: TriggerRemoveNode action received");
                    Ok(Some(Action::SwitchScene(Scene::RemoveNodePopUp)))
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

                NodeTableActions::ResetNodes => {
                    debug!("Got NodeTableActions::ResetNodes - removing all nodes");
                    self.state_mut().operations.handle_reset_nodes()?;
                    Ok(None)
                }
                NodeTableActions::UpgradeNodeVersion => {
                    debug!("Got NodeTableActions::UpgradeNodeVersion");
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

                NodeTableActions::StateChanged { .. } => {
                    // StateChanged is sent by NodeTable, not handled by it
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
