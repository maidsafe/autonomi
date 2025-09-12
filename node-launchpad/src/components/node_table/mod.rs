// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! The node table operates on a registry-driven architecture where the NodeRegistry serves as the single source
//! of truth, updated exclusively by antctl. When the registry file changes, a file watcher triggers
//! `Action::NodeTableActions::RegistryUpdated` events that flow through the system. This event drives
//! `sync_node_service_data()` to update the table state, ensuring the UI always reflects the true registry state
//! rather than optimistic updates from operation results.
//!
//! When users initiate operations (add/remove/start/stop/upgrade), the table immediately locks affected nodes
//! to provide visual feedback while antctl executes sequentially. These operations flow through NodeManagement
//! to antctl, which modifies the registry. The file watcher detects these changes and triggers the sync cycle,
//! automatically unlocking nodes and updating their display status based on the new registry state. This creates
//! a reliable feedback loop where UI state changes only after actual system changes are persisted.

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
use color_eyre::Result;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

// Re-exports for convenience
pub use node_item::{NodeDisplayStatus, NodeItem};
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
        self.state.send_selection_update()?;

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
                NodeTableActions::SelectionChanged { .. } => {
                    // SelectionChanged is sent by NodeTable, not handled by it
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
                    debug!("NodeTable: Handling NodeManagementCommand: {command:?}");
                    let result = self.handle_node_management_command(command);
                    // Try to update the table selection if the selected node got locked
                    self.state.try_clear_selection_if_locked();
                    result
                }
                NodeTableActions::NodeManagementResponse(response) => {
                    debug!("NodeTable: Handling NodeManagementResponse: {response:?}");
                    let result = self.handle_node_management_response(response);
                    // Try to update the table selection if the selected node got locked
                    self.state.try_clear_selection_if_locked();
                    result
                }
                NodeTableActions::NavigateUp => {
                    self.state.navigate_previous_unlocked();
                    Ok(None)
                }
                NodeTableActions::NavigateDown => {
                    self.state.navigate_next_unlocked();
                    Ok(None)
                }
                NodeTableActions::NavigateHome => {
                    self.state.navigate_first_unlocked();
                    Ok(None)
                }
                NodeTableActions::NavigateEnd => {
                    self.state.navigate_last_unlocked();
                    Ok(None)
                }
                NodeTableActions::NavigatePageUp => {
                    for _ in 0..10 {
                        self.state.navigate_previous_unlocked();
                    }
                    Ok(None)
                }
                NodeTableActions::NavigatePageDown => {
                    for _ in 0..10 {
                        self.state.navigate_next_unlocked();
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

impl NodeTableComponent {
    fn handle_node_management_command(
        &mut self,
        command: NodeManagementCommand,
    ) -> Result<Option<Action>> {
        match command {
            NodeManagementCommand::MaintainNodes => {
                // lock all for now
                // todo move to lock only if stopping.
                for item in self.state.items.items.iter_mut() {
                    item.lock_for_operation(NodeDisplayStatus::Maintaining);
                }
                let config = operations::MaintainNodesConfig {
                    rewards_address: self.state.rewards_address.as_ref(),
                    nodes_to_start: self.state.nodes_to_start,
                    antnode_path: self.state.antnode_path.clone(),
                    upnp_enabled: self.state.upnp_enabled,
                    data_dir_path: self.state.data_dir_path.clone(),
                    network_id: self.state.network_id,
                    init_peers_config: self.state.init_peers_config.clone(),
                    port_range: self.state.port_range,
                };
                self.state.operations.handle_maintain_nodes(&config)
            }
            NodeManagementCommand::AddNode => {
                // lock all for now
                // todo implement a fake row for the new node being added
                for item in self.state.items.items.iter_mut() {
                    item.lock_for_operation(NodeDisplayStatus::Adding);
                }
                let config = operations::AddNodeConfig {
                    node_count: self.state.items.items.len() as u64,
                    available_disk_space_gb: self.state.available_disk_space_gb,
                    storage_mountpoint: &self.state.storage_mountpoint,
                    rewards_address: self.state.rewards_address.as_ref(),
                    nodes_to_start: self.state.nodes_to_start,
                    antnode_path: self.state.antnode_path.clone(),
                    upnp_enabled: self.state.upnp_enabled,
                    data_dir_path: self.state.data_dir_path.clone(),
                    network_id: self.state.network_id,
                    init_peers_config: self.state.init_peers_config.clone(),
                    port_range: self.state.port_range,
                };
                self.state.operations.handle_add_node(&config)
            }
            NodeManagementCommand::StartNodes => {
                let mut nodes_to_start = Vec::new();

                // Filter nodes that can be started and lock them
                for item in self.state.items.items.iter_mut() {
                    if item.can_start() {
                        debug!(
                            "StartNodes: Locking and starting node {}",
                            item.service_name
                        );
                        item.lock_for_operation(NodeDisplayStatus::Starting);
                        nodes_to_start.push(item.service_name.clone());
                    } else if item.is_locked() {
                        debug!("StartNodes: Skipping locked node {}", item.service_name);
                    } else {
                        debug!(
                            "StartNodes: Skipping node {} (status: {:?})",
                            item.service_name, item.node_display_status
                        );
                    }
                }

                if !nodes_to_start.is_empty() {
                    self.state.operations.handle_start_node(nodes_to_start)?;
                } else {
                    debug!("StartNodes: No nodes available to start");
                }
                Ok(None)
            }
            NodeManagementCommand::StopNodes => {
                let mut nodes_to_stop = Vec::new();

                // Filter nodes that can be stopped and lock them
                for item in self.state.items.items.iter_mut() {
                    if item.can_stop() {
                        debug!("StopNodes: Locking and stopping node {}", item.service_name);
                        item.lock_for_operation(NodeDisplayStatus::Stopping);
                        nodes_to_stop.push(item.service_name.clone());
                    } else if item.is_locked() {
                        debug!("StopNodes: Skipping locked node {}", item.service_name);
                    } else {
                        debug!(
                            "StopNodes: Skipping node {} (status: {:?})",
                            item.service_name, item.node_display_status
                        );
                    }
                }

                if !nodes_to_stop.is_empty() {
                    self.state.operations.handle_stop_nodes(nodes_to_stop)?;
                } else {
                    debug!("StopNodes: No nodes available to stop");
                }
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
                    // Check if node is locked before attempting operation
                    if node_item.is_locked() {
                        debug!("Cannot toggle node {}: Node is locked", service_name);
                        return Ok(None);
                    }

                    match node_item.node_display_status {
                        NodeDisplayStatus::Running => {
                            if node_item.can_stop() {
                                debug!("Toggling node {}: Stopping it", service_name);
                                node_item.lock_for_operation(NodeDisplayStatus::Stopping);
                                self.state
                                    .operations
                                    .handle_stop_nodes(vec![service_name])?;
                            } else {
                                debug!(
                                    "Cannot stop node {}: Node cannot accept stop operation",
                                    service_name
                                );
                            }
                        }
                        NodeDisplayStatus::Stopped | NodeDisplayStatus::Added => {
                            if node_item.can_start() {
                                debug!("Toggling node {}: Starting it", service_name);
                                node_item.lock_for_operation(NodeDisplayStatus::Starting);
                                self.state
                                    .operations
                                    .handle_start_node(vec![service_name])?;
                            } else {
                                debug!(
                                    "Cannot start node {}: Node cannot accept start operation",
                                    service_name
                                );
                            }
                        }
                        _ => {
                            debug!(
                                "Toggling node {}: No action for status {:?}",
                                service_name, node_item.node_display_status
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
                if let Some(ref service_name) = selected_node
                    && let Some(node_item) = self.state.get_node_item_mut(service_name)
                {
                    if node_item.is_locked() {
                        debug!("Cannot remove node {}: Node is locked", service_name);
                        return Ok(None);
                    }
                    debug!("RemoveNodes: Locking node for removal {}", service_name);
                    node_item.lock();
                    self.state
                        .operations
                        .handle_remove_nodes(vec![service_name.clone()])?;
                } else if selected_node.is_some() {
                    debug!("Cannot remove node: Node not found");
                }
                Ok(None)
            }
            NodeManagementCommand::UpgradeNodes => {
                let mut nodes_to_upgrade = Vec::new();

                // Filter nodes that can be upgraded and lock them
                for item in self.state.items.items.iter_mut() {
                    if item.can_upgrade() {
                        debug!(
                            "UpgradeNodes: Locking and upgrading node {}",
                            item.service_name
                        );
                        item.lock_for_operation(NodeDisplayStatus::Updating);
                        nodes_to_upgrade.push(item.service_name.clone());
                    } else if item.is_locked() {
                        debug!("UpgradeNodes: Skipping locked node {}", item.service_name);
                    } else {
                        debug!(
                            "UpgradeNodes: Skipping node {} (status: {:?})",
                            item.service_name, item.node_display_status
                        );
                    }
                }

                if !nodes_to_upgrade.is_empty() {
                    self.state
                        .operations
                        .handle_upgrade_nodes(nodes_to_upgrade)?;
                } else {
                    debug!("UpgradeNodes: No nodes available to upgrade");
                }
                Ok(None)
            }
            NodeManagementCommand::ResetNodes => {
                for item in self.state.items.items.iter_mut() {
                    item.lock_for_operation(NodeDisplayStatus::Removing);
                }
                self.state_mut().operations.handle_reset_nodes()?;
                Ok(None)
            }
        }
    }

    fn handle_node_management_response(
        &mut self,
        response: NodeManagementResponse,
    ) -> Result<Option<Action>> {
        match response {
            NodeManagementResponse::MaintainNodes { error } => {
                // unlock all nodes that were locked for maintenance operations
                for item in self.state.items.items.iter_mut() {
                    if item.is_locked() {
                        debug!("MaintainNodes: Unlocking node {}", item.service_name);
                        item.unlock();
                        // Update status based on actual service status after maintenance
                        // the item.service_status will be updated from the registry data automatically
                        item.update_node_display_status(NodeDisplayStatus::from(
                            &item.service_status,
                        ));
                    }
                }

                if let Some(err) = error {
                    error!("MaintainNodes operation failed: {err}");
                    let error_popup =
                        ErrorPopup::new("Error while managing nodes", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }
            NodeManagementResponse::AddNode { error } => {
                // unlock all nodes that were locked for add operations
                for item in self.state.items.items.iter_mut() {
                    if item.is_locked() {
                        debug!("AddNode: Unlocking node {}", item.service_name);
                        item.unlock();
                        // Update status based on actual service status after maintenance
                        // the item.service_status will be updated from the registry data automatically
                        item.update_node_display_status(NodeDisplayStatus::from(
                            &item.service_status,
                        ));
                    }
                }

                if let Some(err) = error {
                    error!("AddNode operation failed: {err}");
                    let error_popup =
                        ErrorPopup::new("Error while adding node", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }
            NodeManagementResponse::StartNodes {
                service_names,
                error,
            } => {
                for item in
                    self.state.items.items.iter_mut().filter(|item| {
                        service_names.contains(&item.service_name) && item.is_locked()
                    })
                {
                    debug!("StartNodes: Unlocking node {}", item.service_name);
                    item.unlock();
                    // Update status based on actual service status after maintenance
                    // the item.service_status will be updated from the registry data automatically
                    item.update_node_display_status(NodeDisplayStatus::from(&item.service_status));
                }

                if let Some(err) = error {
                    let error_popup =
                        ErrorPopup::new("Error while starting nodes", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }

            NodeManagementResponse::StopNodes {
                service_names,
                error,
            } => {
                for item in
                    self.state.items.items.iter_mut().filter(|item| {
                        service_names.contains(&item.service_name) && item.is_locked()
                    })
                {
                    debug!("StopNodes: Unlocking node {}", item.service_name);
                    item.unlock();
                    // Update status based on actual service status after maintenance
                    // the item.service_status will be updated from the registry data automatically
                    item.update_node_display_status(NodeDisplayStatus::from(&item.service_status));
                }

                if let Some(err) = error {
                    let error_popup =
                        ErrorPopup::new("Error while stopping nodes", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }
            NodeManagementResponse::RemoveNodes {
                service_names,
                error,
            } => {
                for item in
                    self.state.items.items.iter_mut().filter(|item| {
                        service_names.contains(&item.service_name) && item.is_locked()
                    })
                {
                    debug!("RemoveNodes: Unlocking node {}", item.service_name);
                    item.unlock();
                    // Update status based on actual service status after maintenance
                    // the item.service_status will be updated from the registry data automatically
                    //
                    // If the node has been removed successfully, then the sync method will take care of removing this
                    // item from the list in the next loop.
                    item.update_node_display_status(NodeDisplayStatus::from(&item.service_status));
                }

                if let Some(err) = error {
                    let error_popup =
                        ErrorPopup::new("Error while removing nodes", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }
            NodeManagementResponse::UpgradeNodes {
                service_names,
                error,
            } => {
                for item in
                    self.state.items.items.iter_mut().filter(|item| {
                        service_names.contains(&item.service_name) && item.is_locked()
                    })
                {
                    debug!("UpgradeNodes: Unlocking node {}", item.service_name);
                    item.unlock();
                    // Update status based on actual service status after maintenance
                    // the item.service_status will be updated from the registry data automatically
                    //
                    // If the node has been upgraded, then the sync method will take care of updating the version
                    // info and stuff
                    item.update_node_display_status(NodeDisplayStatus::from(&item.service_status));
                }

                if let Some(err) = error {
                    let error_popup =
                        ErrorPopup::new("Error while upgrading nodes", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }
            NodeManagementResponse::ResetNodes { error } => {
                for item in self.state.items.items.iter_mut() {
                    item.unlock();
                }

                if let Some(err) = error {
                    let error_popup =
                        ErrorPopup::new("Error while resetting nodes", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }
        }
    }
}
