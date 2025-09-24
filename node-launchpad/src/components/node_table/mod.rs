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

mod command_handler;
pub mod lifecycle;
pub mod operations;
pub mod state;
pub mod table_state;
pub mod view;
pub mod widget;

use crate::action::{Action, NodeManagementCommand, NodeManagementResponse, NodeTableActions};
use crate::components::Component;
use crate::focus::FocusTarget;
use crate::mode::Scene;
use crate::tui::Frame;
use color_eyre::Result;
use ratatui::layout::Rect;
use std::any::Any;
use tokio::sync::mpsc::UnboundedSender;

pub use operations::NodeOperations;
pub use state::{NavigationDirection, NodeSelectionInfo, NodeTableConfig, NodeTableState};
pub use table_state::StatefulTable;
pub use view::{NodeViewModel, build_view_models};

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

    pub fn view_items(&self) -> &[NodeViewModel] {
        self.state.view_items()
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
        widget::render_node_table(area, f, &mut self.state);
        Ok(())
    }

    fn focus_target(&self) -> FocusTarget {
        FocusTarget::NodeTable
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl NodeTableComponent {
    fn handle_node_management_command(
        &mut self,
        command: NodeManagementCommand,
    ) -> Result<Option<Action>> {
        command_handler::NodeCommandHandler::new(&mut self.state).handle_command(command)
    }

    fn handle_node_management_response(
        &mut self,
        response: NodeManagementResponse,
    ) -> Result<Option<Action>> {
        command_handler::NodeCommandHandler::new(&mut self.state).handle_response(response)
    }
}
