// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::{Action, NodeTableActions, StatusActions};
use crate::components::Component;
use crate::focus::{EventResult, FocusManager, FocusTarget};
use crate::tui::Frame;

use super::{NodeTableConfig, NodeTableState, NodeTableWidget};

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
        self.state.operations.action_sender = Some(tx);

        // Send initial state update to synchronize Status component's cached state
        // This ensures the Status component knows about nodes loaded from registry during initialization
        self.state.send_state_update()?;

        Ok(())
    }

    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        if !focus_manager.has_focus(&self.focus_target()) {
            return Ok((vec![], EventResult::Ignored));
        }

        // Handle error popup first
        if let Some(error_popup) = &mut self.state.error_popup
            && error_popup.is_visible()
        {
            error_popup.handle_input(key);
            return Ok((
                vec![Action::SwitchInputMode(crate::mode::InputMode::Navigation)],
                EventResult::Consumed,
            ));
        }

        debug!("NodeTable handling key: {key:?}");
        debug!("NodeTable has {} items", self.state.items.items.len());

        if let (actions, EventResult::Consumed) = self.handle_table_navigation(key)? {
            return Ok((actions, EventResult::Consumed));
        }

        self.handle_node_operations(key)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            // Handle NodeTableActions directly
            Action::NodeTableActions(node_action) => match node_action {
                NodeTableActions::AddNode => {
                    debug!("NodeTable: Handling AddNode action");
                    let config = super::operations::AddNodeConfig {
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
                    match self.state.operations.handle_add_node(&config) {
                        Ok(Some(result_action)) => Ok(Some(result_action)),
                        Ok(None) => Ok(None),
                        Err(e) => {
                            debug!("Failed to add node: {e:?}");
                            Ok(Some(Action::StatusActions(
                                StatusActions::ErrorAddingNodes {
                                    raw_error: e.to_string(),
                                },
                            )))
                        }
                    }
                }
                NodeTableActions::StartNodes => {
                    debug!("NodeTable: Handling StartNodes action");
                    let config = super::operations::StartNodesConfig {
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
                    match self.state.operations.handle_start_nodes(&config) {
                        Ok(Some(result_action)) => Ok(Some(result_action)),
                        Ok(None) => Ok(None),
                        Err(e) => {
                            debug!("Failed to start nodes: {e:?}");
                            Ok(Some(Action::StatusActions(
                                StatusActions::ErrorStartingNodes {
                                    services: vec!["all".to_string()],
                                    raw_error: e.to_string(),
                                },
                            )))
                        }
                    }
                }
                NodeTableActions::StopNodes => {
                    debug!("NodeTable: Handling StopNodes action");
                    let running_nodes = self.state.get_running_nodes();
                    match self
                        .state
                        .operations
                        .handle_stop_nodes(running_nodes.clone())
                    {
                        Ok(()) => Ok(None),
                        Err(e) => {
                            debug!("Failed to stop nodes: {e:?}");
                            Ok(Some(Action::StatusActions(
                                StatusActions::ErrorStoppingNodes {
                                    services: running_nodes,
                                    raw_error: e.to_string(),
                                },
                            )))
                        }
                    }
                }
                NodeTableActions::StartStopNode => {
                    debug!("NodeTable: Handling StartStopNode action");
                    let (service_name, node_locked, node_status) = {
                        if let Some(node_item) = self.state.items.selected_item() {
                            (
                                vec![node_item.service_name.clone()],
                                node_item.locked,
                                node_item.status,
                            )
                        } else {
                            return Ok(None);
                        }
                    };

                    if node_locked {
                        debug!("Node still performing operation");
                        return Ok(None);
                    }

                    match node_status {
                        crate::components::node_table::NodeStatus::Stopped
                        | crate::components::node_table::NodeStatus::Added => {
                            debug!("Starting Node {:?}", service_name[0]);
                            if let Err(e) = self
                                .state
                                .operations
                                .handle_start_node(service_name.clone())
                            {
                                debug!("Failed to start node: {e:?}");
                                return Ok(Some(Action::StatusActions(
                                    StatusActions::ErrorStartingNodes {
                                        services: service_name,
                                        raw_error: e.to_string(),
                                    },
                                )));
                            }
                            if let Some(node_item) = self.state.items.selected_item_mut() {
                                node_item.status =
                                    crate::components::node_table::NodeStatus::Starting;
                            }
                        }
                        crate::components::node_table::NodeStatus::Running => {
                            debug!("Stopping Node {:?}", service_name[0]);
                            if let Err(e) = self
                                .state
                                .operations
                                .handle_stop_nodes(service_name.clone())
                            {
                                debug!("Failed to stop node: {e:?}");
                                return Ok(Some(Action::StatusActions(
                                    StatusActions::ErrorStoppingNodes {
                                        services: service_name,
                                        raw_error: e.to_string(),
                                    },
                                )));
                            }
                            if let Some(node_item) = self.state.items.selected_item_mut() {
                                node_item.lock();
                            }
                        }
                        _ => {
                            debug!("Cannot Start/Stop node. Node status is {:?}", node_status);
                        }
                    }
                    Ok(None)
                }
                NodeTableActions::RemoveNodes => {
                    debug!("NodeTable: Handling RemoveNodes action");
                    if let Some(node_item) = self.state.items.selected_item_mut() {
                        if node_item.locked {
                            debug!("Node still performing operation");
                            return Ok(None);
                        }
                        node_item.lock();
                        let service_name = vec![node_item.service_name.clone()];
                        if let Err(e) = self
                            .state
                            .operations
                            .handle_remove_nodes(service_name.clone())
                        {
                            debug!("Failed to remove node: {e:?}");
                            node_item.unlock();
                            return Ok(Some(Action::StatusActions(
                                StatusActions::ErrorRemovingNodes {
                                    services: service_name,
                                    raw_error: e.to_string(),
                                },
                            )));
                        }
                    }
                    Ok(None)
                }
                NodeTableActions::TriggerRemoveNode => {
                    debug!("NodeTable: TriggerRemoveNode action received");
                    Ok(Some(Action::SwitchScene(
                        crate::mode::Scene::RemoveNodePopUp,
                    )))
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
                // Handle completion events
                NodeTableActions::StartNodesCompleted { service_name } => {
                    debug!("NodeTable: StartNodesCompleted for service: {service_name}");
                    use crate::node_mgmt::NODES_ALL;
                    if service_name == NODES_ALL {
                        for item in self.state.items.items.iter_mut() {
                            if item.status == crate::components::node_table::NodeStatus::Starting {
                                item.unlock();
                                item.update_status(
                                    crate::components::node_table::NodeStatus::Running,
                                );
                            }
                        }
                    } else if let Some(node_item) = self.state.get_node_item_mut(&service_name) {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Running);
                    }
                    self.state.send_state_update()?;
                    Ok(None)
                }
                NodeTableActions::StopNodesCompleted { service_name } => {
                    debug!("NodeTable: StopNodesCompleted for service: {service_name}");
                    if let Some(node_item) = self.state.get_node_item_mut(&service_name) {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Stopped);
                    }
                    self.state.send_state_update()?;
                    Ok(None)
                }
                NodeTableActions::AddNodesCompleted { service_name } => {
                    debug!("NodeTable: AddNodesCompleted for service: {service_name}");
                    if let Some(node_item) = self.state.get_node_item_mut(&service_name) {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Added);
                    }
                    self.state.send_state_update()?;
                    Ok(None)
                }
                NodeTableActions::RemoveNodesCompleted { service_name } => {
                    debug!("NodeTable: RemoveNodesCompleted for service: {service_name}");
                    if let Some(node_item) = self.state.get_node_item_mut(&service_name) {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Removed);
                    }
                    self.state
                        .items
                        .items
                        .retain(|item| item.service_name != service_name);
                    self.state.send_state_update()?;
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
