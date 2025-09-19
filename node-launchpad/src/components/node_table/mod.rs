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

pub mod lifecycle;
pub mod operations;
pub mod state;
pub mod table_state;
pub mod widget;

use crate::action::{Action, NodeManagementCommand, NodeManagementResponse, NodeTableActions};
use crate::components::Component;
use crate::components::node_table::lifecycle::{CommandKind, DesiredNodeState, LifecycleState};
use crate::components::popup::error_popup::ErrorPopup;
use crate::focus::FocusTarget;
use crate::mode::Scene;
use crate::tui::Frame;
use color_eyre::Result;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

pub use operations::NodeOperations;
pub use state::{NavigationDirection, NodeSelectionInfo, NodeTableState};
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

        let node_registry_clone = self.state().node_registry_manager.clone();
        let action_sender_clone = tx.clone();
        debug!(
            "Returning the initial registry state and also sending NodeTableComponent::RefreshRegistry command to NodeManagement"
        );
        tokio::spawn(async move {
            let services = node_registry_clone.get_node_service_data().await;
            if let Err(err) = action_sender_clone.send(Action::NodeTableActions(
                NodeTableActions::RegistryFileUpdated {
                    all_nodes_data: services,
                },
            )) {
                error!("Failed to send initial registry state: {err}");
            }
            if let Err(err) = action_sender_clone.send(Action::NodeTableActions(
                NodeTableActions::NodeManagementCommand(NodeManagementCommand::RefreshRegistry),
            )) {
                error!("Failed to send NodeTableActions::RefreshRegistry command: {err}");
            }
        });

        // Watch for registry file changes
        let action_sender_clone = tx.clone();
        let mut node_registry_watcher = self.state.node_registry_manager.watch_registry_file()?;
        let node_registry_clone = self.state.node_registry_manager.clone();
        tokio::spawn(async move {
            while let Some(()) = node_registry_watcher.recv().await {
                let services = node_registry_clone.get_node_service_data().await;
                debug!(
                    "Node registry file has been updated. Sending NodeTableActions::RegistryUpdated event."
                );
                if let Err(e) = action_sender_clone.send(Action::NodeTableActions(
                    NodeTableActions::RegistryFileUpdated {
                        all_nodes_data: services,
                    },
                )) {
                    error!("Failed to send NodeTableActions::RegistryUpdated: {e}");
                }
            }
        });

        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            // Handle NodeTableActions directly
            Action::NodeTableActions(node_action) => match node_action {
                NodeTableActions::RegistryFileUpdated { all_nodes_data } => {
                    self.state_mut().sync_node_service_data(&all_nodes_data);
                    Ok(None)
                }
                NodeTableActions::TriggerNodeLogs => {
                    debug!("NodeTable: TriggerNodeLogs action received");
                    if self.state.controller.view.items.is_empty() {
                        debug!("No nodes available for logs viewing");
                        return Ok(None);
                    }

                    let selected_node_name = self
                        .state
                        .controller
                        .selected_item()
                        .map(|node| node.id.clone())
                        .or_else(|| {
                            self.state
                                .controller
                                .items()
                                .first()
                                .map(|node| node.id.clone())
                        })
                        .unwrap_or_else(|| "No node available".to_string());

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
                    self.state.navigate(NavigationDirection::Up(1));
                    Ok(None)
                }
                NodeTableActions::NavigateDown => {
                    self.state.navigate(NavigationDirection::Down(1));
                    Ok(None)
                }
                NodeTableActions::NavigateHome => {
                    self.state.navigate(NavigationDirection::First);
                    Ok(None)
                }
                NodeTableActions::NavigateEnd => {
                    self.state.navigate(NavigationDirection::Last);
                    Ok(None)
                }
                NodeTableActions::NavigatePageUp => {
                    self.state.navigate(NavigationDirection::Up(10));
                    Ok(None)
                }
                NodeTableActions::NavigatePageDown => {
                    self.state.navigate(NavigationDirection::Down(10));
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
            NodeManagementCommand::RefreshRegistry => {
                self.state_mut().operations.handle_refresh_registry()?;
                Ok(None)
            }
            NodeManagementCommand::MaintainNodes => {
                let ids: Vec<_> = self
                    .state
                    .controller
                    .items()
                    .iter()
                    .map(|model| model.id.clone())
                    .collect();
                for id in &ids {
                    self.state
                        .controller
                        .mark_transition(id, CommandKind::Maintain);
                }

                match self
                    .state
                    .operations
                    .handle_maintain_nodes(&self.state.operations_config)
                {
                    Ok(Some(action)) => {
                        for id in &ids {
                            self.state.controller.clear_transition(id);
                        }
                        Ok(Some(action))
                    }
                    Ok(None) => Ok(None),
                    Err(err) => {
                        for id in &ids {
                            self.state.controller.clear_transition(id);
                        }
                        Err(err)
                    }
                }
            }
            NodeManagementCommand::AddNode => self.state.operations.handle_add_node(
                &self.state.operations_config,
                self.state.controller.items().len() as u64,
            ),
            NodeManagementCommand::StartNodes => {
                let nodes_to_start: Vec<_> = self
                    .state
                    .controller
                    .items()
                    .iter()
                    .filter(|model| model.can_start())
                    .map(|model| model.id.clone())
                    .collect();

                if !nodes_to_start.is_empty() {
                    for id in &nodes_to_start {
                        self.state
                            .controller
                            .mark_transition(id, CommandKind::Start);
                        self.state
                            .controller
                            .set_node_target(id, DesiredNodeState::Run);
                    }
                    self.state
                        .operations
                        .handle_start_node(nodes_to_start.clone())
                        .inspect_err(|err| {
                            error!("StartNodes operation failed: {err}");
                            for id in &nodes_to_start {
                                self.state.controller.clear_transition(id);
                                self.state
                                    .controller
                                    .set_node_target(id, DesiredNodeState::FollowCluster);
                            }
                        })?;
                } else {
                    debug!("StartNodes: No nodes available to start");
                }
                Ok(None)
            }
            NodeManagementCommand::StopNodes => {
                let nodes_to_stop: Vec<_> = self
                    .state
                    .controller
                    .items()
                    .iter()
                    .filter(|model| model.can_stop())
                    .map(|model| model.id.clone())
                    .collect();

                if !nodes_to_stop.is_empty() {
                    for id in &nodes_to_stop {
                        self.state.controller.mark_transition(id, CommandKind::Stop);
                        self.state
                            .controller
                            .set_node_target(id, DesiredNodeState::Stop);
                    }
                    self.state
                        .operations
                        .handle_stop_nodes(nodes_to_stop.clone())
                        .inspect_err(|err| {
                            for id in &nodes_to_stop {
                                error!("Failed to stop node {id}: {err}");
                                self.state.controller.clear_transition(id);
                                self.state
                                    .controller
                                    .set_node_target(id, DesiredNodeState::FollowCluster);
                            }
                        })?;
                } else {
                    debug!("StopNodes: No nodes available to stop");
                }
                Ok(None)
            }
            NodeManagementCommand::ToggleNode => {
                if let Some(selected) = self.state.controller.selected_item().cloned() {
                    if selected.is_locked() {
                        debug!("Cannot toggle node {}: node is locked", selected.id);
                        return Ok(None);
                    }

                    match selected.lifecycle {
                        LifecycleState::Running | LifecycleState::Starting => {
                            if selected.can_stop() {
                                let service = selected.id.clone();
                                self.state
                                    .controller
                                    .mark_transition(&service, CommandKind::Stop);
                                self.state
                                    .controller
                                    .set_node_target(&service, DesiredNodeState::Stop);
                                self.state
                                    .operations
                                    .handle_stop_nodes(vec![service.clone()])
                                    .inspect_err(|err| {
                                        error!("Failed to stop node {service}: {err}");
                                        self.state.controller.clear_transition(&service);
                                        self.state.controller.set_node_target(
                                            &service,
                                            DesiredNodeState::FollowCluster,
                                        );
                                    })?;
                            }
                        }
                        LifecycleState::Stopped | LifecycleState::Unreachable { .. } => {
                            if selected.can_start() {
                                let service = selected.id.clone();
                                self.state
                                    .controller
                                    .mark_transition(&service, CommandKind::Start);
                                self.state
                                    .controller
                                    .set_node_target(&service, DesiredNodeState::Run);
                                self.state
                                    .operations
                                    .handle_start_node(vec![service.clone()])
                                    .inspect_err(|err| {
                                        error!("Failed to start node {service}: {err}");
                                        self.state.controller.clear_transition(&service);
                                        self.state.controller.set_node_target(
                                            &service,
                                            DesiredNodeState::FollowCluster,
                                        );
                                    })?;
                            }
                        }
                        _ => {
                            debug!(
                                "ToggleNode: No action taken for node {} in state {:?}",
                                selected.id, selected.lifecycle
                            );
                        }
                    }
                }
                Ok(None)
            }
            NodeManagementCommand::RemoveNodes => {
                if let Some(selected) = self.state.controller.selected_item().cloned() {
                    if selected.is_locked() {
                        debug!("Cannot remove node {}: node is locked", selected.id);
                        return Ok(None);
                    }
                    let service = selected.id.clone();
                    self.state
                        .controller
                        .mark_transition(&service, CommandKind::Remove);
                    self.state
                        .controller
                        .set_node_target(&service, DesiredNodeState::Remove);
                    self.state
                        .operations
                        .handle_remove_nodes(vec![service.clone()])
                        .inspect_err(|err| {
                            error!("Failed to remove node {service}: {err}");
                            self.state.controller.clear_transition(&service);
                            self.state
                                .controller
                                .set_node_target(&service, DesiredNodeState::FollowCluster);
                        })?;
                }
                Ok(None)
            }
            NodeManagementCommand::UpgradeNodes => {
                let nodes_to_upgrade: Vec<_> = self
                    .state
                    .controller
                    .items()
                    .iter()
                    .filter(|model| model.can_upgrade())
                    .map(|model| model.id.clone())
                    .collect();

                if !nodes_to_upgrade.is_empty() {
                    for id in &nodes_to_upgrade {
                        self.state
                            .controller
                            .mark_transition(id, CommandKind::Maintain);
                    }
                    self.state
                        .operations
                        .handle_upgrade_nodes(nodes_to_upgrade.clone())
                        .inspect_err(|err| {
                            error!("UpgradeNodes operation failed: {err}");
                            for id in &nodes_to_upgrade {
                                self.state.controller.clear_transition(id);
                            }
                        })?;
                } else {
                    debug!("UpgradeNodes: No nodes available to upgrade");
                }
                Ok(None)
            }
            NodeManagementCommand::ResetNodes => {
                let ids: Vec<_> = self
                    .state
                    .controller
                    .items()
                    .iter()
                    .map(|model| model.id.clone())
                    .collect();
                for id in &ids {
                    self.state
                        .controller
                        .mark_transition(id, CommandKind::Remove);
                    self.state
                        .controller
                        .set_node_target(id, DesiredNodeState::Remove);
                }
                if let Err(err) = self.state_mut().operations.handle_reset_nodes() {
                    for id in &ids {
                        self.state.controller.clear_transition(id);
                        self.state
                            .controller
                            .set_node_target(id, DesiredNodeState::FollowCluster);
                    }
                    return Err(err);
                }
                Ok(None)
            }
        }
    }

    fn handle_node_management_response(
        &mut self,
        response: NodeManagementResponse,
    ) -> Result<Option<Action>> {
        match response {
            NodeManagementResponse::RefreshRegistry { error } => {
                if let Some(err) = error {
                    error!("RefreshRegistry operation failed: {err}");
                }
                Ok(None)
            }
            NodeManagementResponse::MaintainNodes { error } => {
                self.state
                    .controller
                    .clear_transitions_by_command(CommandKind::Maintain);

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
                self.state
                    .controller
                    .clear_transitions_by_command(CommandKind::Add);

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
                for service in &service_names {
                    self.state.controller.clear_transition(service);
                }

                if let Some(err) = error {
                    for service in &service_names {
                        self.state
                            .controller
                            .set_node_target(service, DesiredNodeState::FollowCluster);
                    }
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
                for service in &service_names {
                    self.state.controller.clear_transition(service);
                }

                if let Some(err) = error {
                    for service in &service_names {
                        self.state
                            .controller
                            .set_node_target(service, DesiredNodeState::FollowCluster);
                    }
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
                for service in &service_names {
                    self.state.controller.clear_transition(service);
                }

                if let Some(err) = error {
                    for service in &service_names {
                        self.state
                            .controller
                            .set_node_target(service, DesiredNodeState::FollowCluster);
                    }
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
                for service in &service_names {
                    self.state.controller.clear_transition(service);
                }

                if let Some(err) = error {
                    for service in &service_names {
                        self.state
                            .controller
                            .set_node_target(service, DesiredNodeState::FollowCluster);
                    }
                    let error_popup =
                        ErrorPopup::new("Error while upgrading nodes", "Please try again", &err);
                    Ok(Some(Action::ShowErrorPopup(error_popup)))
                } else {
                    Ok(None)
                }
            }
            NodeManagementResponse::ResetNodes { error } => {
                self.state
                    .controller
                    .clear_transitions_by_command(CommandKind::Remove);

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
