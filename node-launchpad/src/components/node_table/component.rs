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

use crate::action::{Action, StatusActions};
use crate::components::Component;
use crate::config::Config;
use crate::focus::{EventResult, FocusManager, FocusTarget};
use crate::tui::Frame;

use super::{NodeOperations, NodeTableConfig, NodeTableState, NodeTableWidget, StatefulTable};

pub struct NodeTableComponent {
    pub state: NodeTableState,
    pub config: NodeTableConfig,
    action_sender: Option<UnboundedSender<Action>>,
}

impl NodeTableComponent {
    pub fn new(config: NodeTableConfig) -> Self {
        Self {
            state: Self::create_empty_state(&config),
            config,
            action_sender: None,
        }
    }

    fn create_empty_state(config: &NodeTableConfig) -> NodeTableState {
        use crate::node_mgmt::NodeManagement;
        use ant_service_management::NodeRegistryManager;
        use std::time::Instant;

        let registry_path = config.data_dir_path.join("node_registry.json");
        let node_registry = NodeRegistryManager::empty(registry_path);
        let node_management = NodeManagement::new(node_registry.clone()).unwrap();

        NodeTableState {
            items: StatefulTable::with_items(vec![]),
            node_services: vec![],
            node_registry,
            operations: NodeOperations::new(node_management),
            node_stats_last_update: Instant::now(),
            network_id: config.network_id,
            init_peers_config: config.init_peers_config.clone(),
            antnode_path: config.antnode_path.clone(),
            data_dir_path: config.data_dir_path.clone(),
            connection_mode: config.connection_mode,
            port_from: config.port_from,
            port_to: config.port_to,
            rewards_address: config.rewards_address.clone(),
            nodes_to_start: config.nodes_to_start,
            storage_mountpoint: config.storage_mountpoint.clone(),
            available_disk_space_gb: 0,
            error_popup: None,
            spinner_states: vec![],
        }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        self.state = NodeTableState::new(self.config.clone()).await?;
        Ok(())
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
                self.state.items.previous();
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.state.items.next();
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
                vec![Action::StatusActions(StatusActions::AddNode)],
                EventResult::Consumed,
            )),
            KeyCode::Char('-') => Ok((
                vec![Action::StatusActions(StatusActions::TriggerRemoveNode)],
                EventResult::Consumed,
            )),
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::StatusActions(StatusActions::StartStopNode)],
                EventResult::Consumed,
            )),
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::StatusActions(StatusActions::StartNodes)],
                EventResult::Consumed,
            )),
            KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::StatusActions(StatusActions::StopNodes)],
                EventResult::Consumed,
            )),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok((
                vec![Action::StatusActions(StatusActions::RemoveNodes)],
                EventResult::Consumed,
            )),
            KeyCode::Char('l') | KeyCode::Char('L') => Ok((
                vec![Action::StatusActions(StatusActions::TriggerNodeLogs)],
                EventResult::Consumed,
            )),
            KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let amount_of_nodes = self.state.nodes_to_start;
                Ok((
                    vec![Action::SwitchScene(crate::mode::Scene::ManageNodesPopUp {
                        amount_of_nodes,
                    })],
                    EventResult::Consumed,
                ))
            }
            KeyCode::Enter => {
                if !self.state.items.items.is_empty() && self.state.items.state.selected().is_some()
                {
                    Ok((
                        vec![Action::StatusActions(StatusActions::StartStopNode)],
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
        self.action_sender = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, _config: Config) -> Result<()> {
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

        if let (actions, EventResult::Consumed) = self.handle_table_navigation(key)? {
            return Ok((actions, EventResult::Consumed));
        }

        self.handle_node_operations(key)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::StatusActions(StatusActions::RegistryUpdated { all_nodes_data }) => {
                self.state.node_services = all_nodes_data.clone();
                self.state.update_node_state(&all_nodes_data);
                Ok(None)
            }
            Action::StatusActions(StatusActions::NodesStatsObtained(_node_stats)) => {
                self.state.try_update_node_stats(false)?;
                Ok(None)
            }
            Action::StoreNodesToStart(count) => {
                self.state.nodes_to_start = count;
                Ok(None)
            }
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
