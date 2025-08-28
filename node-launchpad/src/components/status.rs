// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{NodeTableActions, OptionsActions};
use crate::components::Component;
use crate::components::footer::{Footer, NodesToStart};
use crate::components::header::{Header, SelectedMenuItem};
use crate::components::node_table::{NodeTableComponent, NodeTableConfig, NodeTableState};
use crate::components::popup::manage_nodes::{GB, GB_PER_NODE};
use crate::components::popup::port_range::PORT_ALLOCATION;
use crate::config::get_launchpad_nodes_data_dir_path;
use crate::connection_mode::ConnectionMode;
use crate::error::ErrorPopup;
use crate::node_mgmt::PORT_MIN;
use crate::system::get_available_space_b;
use crate::{
    action::{Action, StatusActions},
    config::Config,
    focus::{EventResult, FocusManager, FocusTarget},
    mode::{InputMode, Scene},
    node_stats::NodeStats,
    style::{EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VERY_LIGHT_AZURE, VIVID_SKY_BLUE},
};
use ant_bootstrap::InitialPeersConfig;
use color_eyre::eyre::{Ok, Result};
use crossterm::event::KeyEvent;
use ratatui::text::Span;
use ratatui::{prelude::*, widgets::*};
use std::time::Duration;
use std::{path::PathBuf, vec};
use tokio::sync::mpsc::UnboundedSender;

pub const NODE_STAT_UPDATE_INTERVAL: Duration = Duration::from_secs(5);

pub struct Status {
    action_sender: Option<UnboundedSender<Action>>,
    config: Config,
    // Device Stats Section
    node_stats: NodeStats,
    // Amount of nodes
    nodes_to_start: usize,
    // Rewards address
    rewards_address: String,
    // Path where the node data is stored
    data_dir_path: PathBuf,
    // Connection mode
    connection_mode: ConnectionMode,
    // Port from
    port_from: Option<u32>,
    // Port to
    port_to: Option<u32>,
    storage_mountpoint: PathBuf,
    available_disk_space_gb: usize,
    error_popup: Option<ErrorPopup>,

    // NodeTable state
    node_table_state: NodeTableState,
    // NodeTable component
    node_table_component: NodeTableComponent,

    // Cached state from NodeTable
    node_count: usize,
    has_running_nodes: bool,
    has_nodes: bool,
}

pub struct StatusConfig {
    pub allocated_disk_space: usize,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
    pub data_dir_path: PathBuf,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
    pub storage_mountpoint: PathBuf,
    pub rewards_address: String,
}

impl Status {
    pub async fn new(config: StatusConfig) -> Result<Self> {
        let status = Self {
            action_sender: Default::default(),
            config: Default::default(),
            node_stats: NodeStats::default(),
            nodes_to_start: config.allocated_disk_space,
            rewards_address: config.rewards_address.clone(),
            data_dir_path: config.data_dir_path.clone(),
            connection_mode: config.connection_mode,
            port_from: config.port_from,
            port_to: config.port_to,
            error_popup: None,
            storage_mountpoint: config.storage_mountpoint.clone(),
            available_disk_space_gb: get_available_space_b(&config.storage_mountpoint)? / GB,

            // Initialize NodeTable state
            node_table_state: NodeTableState::new(NodeTableConfig {
                network_id: config.network_id,
                init_peers_config: config.init_peers_config.clone(),
                antnode_path: config.antnode_path.clone(),
                data_dir_path: config.data_dir_path.clone(),
                connection_mode: config.connection_mode,
                port_from: config.port_from,
                port_to: config.port_to,
                rewards_address: config.rewards_address.clone(),
                nodes_to_start: config.allocated_disk_space,
                storage_mountpoint: config.storage_mountpoint.clone(),
            })
            .await?,
            // Initialize NodeTable component
            node_table_component: NodeTableComponent::new(NodeTableConfig {
                network_id: config.network_id,
                init_peers_config: config.init_peers_config.clone(),
                antnode_path: config.antnode_path.clone(),
                data_dir_path: config.data_dir_path.clone(),
                connection_mode: config.connection_mode,
                port_from: config.port_from,
                port_to: config.port_to,
                rewards_address: config.rewards_address.clone(),
                nodes_to_start: config.allocated_disk_space,
                storage_mountpoint: config.storage_mountpoint.clone(),
            }),

            // Initialize cached state
            node_count: 0,
            has_running_nodes: false,
            has_nodes: false,
        };

        Ok(status)
    }

    fn handle_status_key_events(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        debug!("Key received in Status: {:?}", key);
        if let Some(error_popup) = &mut self.error_popup
            && error_popup.is_visible()
        {
            error_popup.handle_input(key);
            return Ok(vec![Action::SwitchInputMode(InputMode::Navigation)]);
        }

        // Node operations and table navigation are now handled by NodeTableComponent
        // through the focused event handling system
        Ok(vec![])
    }
}

impl Component for Status {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_sender = Some(tx.clone());
        // Register action sender with NodeTable operations
        self.node_table_state
            .operations
            .register_action_sender(tx.clone());

        // Register action sender with NodeTableComponent
        self.node_table_component
            .register_action_handler(tx.clone())?;

        // Update the stats to be shown as soon as the app is run
        self.node_table_state.try_update_node_stats(true)?;

        // Refresh registry on startup
        let action_sender_startup = tx.clone();
        let node_registry_clone = self.node_table_state.node_registry.clone();
        tokio::spawn(async move {
            log::debug!("Refreshing node registry on startup");
            let services = node_registry_clone.get_node_service_data().await;
            log::debug!("Registry refresh complete. Found {} nodes", services.len());
            if let Err(e) =
                action_sender_startup.send(Action::StatusActions(StatusActions::RegistryUpdated {
                    all_nodes_data: services,
                }))
            {
                log::error!("Failed to send initial registry update: {e}");
            }
        });

        // Watch for registry file changes
        let action_sender_clone = tx.clone();
        let mut node_registry_watcher =
            self.node_table_state.node_registry.watch_registry_file()?;
        let node_registry_clone = self.node_table_state.node_registry.clone();
        tokio::spawn(async move {
            while let Some(()) = node_registry_watcher.recv().await {
                let services = node_registry_clone.get_node_service_data().await;
                log::debug!(
                    "Node registry file has been updated. Sending StatusActions::RegistryUpdated event."
                );
                if let Err(e) = action_sender_clone.send(Action::StatusActions(
                    StatusActions::RegistryUpdated {
                        all_nodes_data: services,
                    },
                )) {
                    log::error!("Failed to send StatusActions::RegistryUpdated: {e}");
                }
            }
        });

        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.config = config;
        Ok(())
    }

    fn focus_target(&self) -> FocusTarget {
        FocusTarget::Status
    }

    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        debug!("Key received in Status (focused): {:?}", key);

        // Handle error popup first
        if let Some(error_popup) = &mut self.error_popup
            && error_popup.is_visible()
        {
            error_popup.handle_input(key);
            return Ok((
                vec![Action::SwitchInputMode(InputMode::Navigation)],
                EventResult::Consumed,
            ));
        }

        // Check if Status should handle this key directly
        let status_only_keys = false; // Add any Status-specific keys here if needed

        if status_only_keys && focus_manager.has_focus(&self.focus_target()) {
            // Handle Status-specific keys here
            return Ok((vec![], EventResult::Consumed));
        }

        // Delegate node table operations to NodeTableComponent when Status has focus
        if focus_manager.has_focus(&FocusTarget::Status) {
            let (node_actions, event_result) = self
                .node_table_component
                .handle_key_events(key, focus_manager)?;
            // If the NodeTable component consumed the event, return those actions
            if matches!(event_result, EventResult::Consumed) {
                return Ok((node_actions, event_result));
            }
            // Otherwise, fall through to handle Status-specific operations
        }

        // If Status has focus, handle Status-specific operations
        if focus_manager.has_focus(&self.focus_target()) {
            let actions = self.handle_status_key_events(key)?;
            let result = if actions.is_empty() {
                EventResult::Ignored
            } else {
                EventResult::Consumed
            };
            return Ok((actions, result));
        }

        Ok((vec![], EventResult::Ignored))
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        // Update NodeTableComponent first to ensure it stays synchronized
        if let Some(result_action) = self.node_table_component.update(action.clone())? {
            // NodeTableComponent returned an action, return it immediately
            return Ok(Some(result_action));
        }

        // Handle NodeTable actions directly
        if let Action::NodeTableActions(NodeTableActions::StateChanged {
            node_count,
            has_running_nodes,
            has_nodes,
        }) = action.clone()
        {
            self.node_count = node_count;
            self.has_running_nodes = has_running_nodes;
            self.has_nodes = has_nodes;
        }

        // Handle Status-specific actions
        match action {
            Action::Tick => {
                self.node_table_state.try_update_node_stats(false)?;
                self.node_table_state.send_state_update()?;
            }
            Action::SwitchScene(scene) => match scene {
                Scene::Status
                | Scene::StatusRewardsAddressPopUp
                | Scene::RemoveNodePopUp
                | Scene::UpgradeLaunchpadPopUp => {
                    // make sure we're in navigation mode
                    return Ok(Some(Action::SwitchInputMode(InputMode::Navigation)));
                }
                Scene::ManageNodesPopUp { .. } => {}
                _ => {}
            },
            Action::StoreNodesToStart(count) => {
                self.nodes_to_start = count;
                if self.nodes_to_start == 0 {
                    info!("Nodes to start set to 0. Sending command to stop all nodes.");
                    return Ok(Some(Action::StatusActions(StatusActions::StopNodes)));
                } else {
                    info!("Nodes to start set to: {count}. Sending command to start nodes");
                    return Ok(Some(Action::StatusActions(StatusActions::StartNodes)));
                }
            }
            Action::StoreRewardsAddress(rewards_address) => {
                debug!("Storing rewards address: {rewards_address:?}");
                self.rewards_address = rewards_address;
            }
            Action::StoreStorageDrive(ref drive_mountpoint, ref _drive_name) => {
                self.data_dir_path =
                    get_launchpad_nodes_data_dir_path(&drive_mountpoint.to_path_buf(), false)?;
            }
            Action::StoreConnectionMode(connection_mode) => {
                self.connection_mode = connection_mode;
            }
            Action::StorePortRange(port_from, port_range) => {
                self.port_from = Some(port_from);
                self.port_to = Some(port_range);
            }
            Action::StatusActions(status_action) => match status_action {
                StatusActions::NodesStatsObtained(stats) => {
                    self.node_stats = stats;
                }
                StatusActions::RegistryUpdated { all_nodes_data } => {
                    log::debug!(
                        "Received RegistryUpdated event with {} nodes",
                        all_nodes_data.len()
                    );
                    self.node_table_state.update_node_state(&all_nodes_data);
                    self.node_table_state.send_state_update()?;
                }
                StatusActions::TriggerManageNodes => {
                    return Ok(Some(Action::SwitchScene(Scene::ManageNodesPopUp {
                        amount_of_nodes: self.nodes_to_start,
                    })));
                }
                StatusActions::TriggerRewardsAddress => {
                    if self.rewards_address.is_empty() {
                        return Ok(Some(Action::SwitchScene(Scene::StatusRewardsAddressPopUp)));
                    } else {
                        return Ok(None);
                    }
                }
                // Handle node operations
                StatusActions::AddNode => {
                    debug!("Got action to Add node");
                    let config = crate::components::node_table::AddNodeConfig {
                        node_count: self.node_count,
                        available_disk_space_gb: self.available_disk_space_gb,
                        storage_mountpoint: &self.storage_mountpoint,
                        rewards_address: &self.rewards_address,
                        nodes_to_start: self.nodes_to_start,
                        antnode_path: self.node_table_state.antnode_path.clone(),
                        connection_mode: self.connection_mode,
                        data_dir_path: self.data_dir_path.clone(),
                        network_id: self.node_table_state.network_id,
                        init_peers_config: self.node_table_state.init_peers_config.clone(),
                        port_from: self.port_from,
                        port_to: self.port_to,
                    };
                    if let Some(result_action) =
                        self.node_table_state.operations.handle_add_node(&config)?
                    {
                        return Ok(Some(result_action));
                    }
                }
                StatusActions::StartNodes => {
                    debug!("Got action to start nodes");
                    let config = crate::components::node_table::StartNodesConfig {
                        rewards_address: &self.rewards_address,
                        nodes_to_start: self.nodes_to_start,
                        antnode_path: self.node_table_state.antnode_path.clone(),
                        connection_mode: self.connection_mode,
                        data_dir_path: self.data_dir_path.clone(),
                        network_id: self.node_table_state.network_id,
                        init_peers_config: self.node_table_state.init_peers_config.clone(),
                        port_from: self.port_from,
                        port_to: self.port_to,
                    };
                    if let Some(result_action) = self
                        .node_table_state
                        .operations
                        .handle_start_nodes(&config)?
                    {
                        return Ok(Some(result_action));
                    }
                }
                StatusActions::StopNodes => {
                    debug!("Got action to stop nodes");
                    let running_nodes = self.node_table_state.get_running_nodes();
                    self.node_table_state
                        .operations
                        .handle_stop_nodes(running_nodes)?;
                }
                StatusActions::StartStopNode => {
                    debug!("Start/Stop node");
                    if let Some(node_item) = self.node_table_state.items.selected_item_mut() {
                        if node_item.locked {
                            debug!("Node still performing operation");
                            return Ok(None);
                        }

                        let service_name = vec![node_item.name.clone()];
                        match node_item.status {
                            crate::components::node_table::NodeStatus::Stopped
                            | crate::components::node_table::NodeStatus::Added => {
                                debug!("Starting Node {:?}", node_item.name);
                                self.node_table_state
                                    .operations
                                    .handle_start_node(service_name)?;
                                node_item.status =
                                    crate::components::node_table::NodeStatus::Starting;
                            }
                            crate::components::node_table::NodeStatus::Running => {
                                debug!("Stopping Node {:?}", node_item.name);
                                self.node_table_state
                                    .operations
                                    .handle_stop_nodes(service_name)?;
                            }
                            _ => {
                                debug!(
                                    "Cannot Start/Stop node. Node status is {:?}",
                                    node_item.status
                                );
                            }
                        }
                        node_item.lock();
                    }
                }
                StatusActions::RemoveNodes => {
                    debug!("Got action to remove node");
                    if let Some(node_item) = self.node_table_state.items.selected_item_mut() {
                        if node_item.locked {
                            debug!("Node still performing operation");
                            return Ok(None);
                        }
                        node_item.lock();
                        let service_name = vec![node_item.name.clone()];
                        self.node_table_state
                            .operations
                            .handle_remove_nodes(service_name)?;
                    }
                }
                StatusActions::TriggerRemoveNode => {
                    debug!("TriggerRemoveNode action received");
                    // This should trigger the remove confirmation popup
                    return Ok(Some(Action::SwitchScene(Scene::RemoveNodePopUp)));
                }
                StatusActions::TriggerNodeLogs => {
                    debug!("TriggerNodeLogs action received");
                    if self.node_table_state.items.items.is_empty() {
                        debug!("No nodes available for logs viewing");
                        return Ok(None);
                    }

                    let selected_node_name = self
                        .node_table_state
                        .items
                        .selected_item()
                        .map(|node| node.name.clone())
                        .unwrap_or_else(|| {
                            // If no specific node is selected, use the first available node
                            self.node_table_state
                                .items
                                .items
                                .first()
                                .map(|node| node.name.clone())
                                .unwrap_or_else(|| "No node available".to_string())
                        });

                    // First set the target node, then switch to the scene
                    // Note: The app will need to handle this sequence
                    return Ok(Some(Action::SetNodeLogsTarget(selected_node_name)));
                }
                StatusActions::PreviousTableItem => {
                    self.node_table_state.items.previous();
                }
                StatusActions::NextTableItem => {
                    self.node_table_state.items.next();
                }

                // === Completion Handlers ===
                StatusActions::StartNodesCompleted { service_name } => {
                    debug!("StartNodesCompleted for service: {service_name}");
                    if service_name == "all" {
                        // Unlock all nodes that were starting
                        for item in self.node_table_state.items.items.iter_mut() {
                            if item.status == crate::components::node_table::NodeStatus::Starting {
                                item.unlock();
                                item.update_status(
                                    crate::components::node_table::NodeStatus::Running,
                                );
                            }
                        }
                    } else if let Some(node_item) =
                        self.node_table_state.get_node_item_mut(&service_name)
                    {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Running);
                    }
                    self.node_table_state.send_state_update()?;
                }
                StatusActions::StopNodesCompleted { service_name } => {
                    debug!("StopNodesCompleted for service: {service_name}");
                    if let Some(node_item) = self.node_table_state.get_node_item_mut(&service_name)
                    {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Stopped);
                    }
                    self.node_table_state.send_state_update()?;
                }
                StatusActions::AddNodesCompleted { service_name } => {
                    debug!("AddNodesCompleted for service: {service_name}");
                    if let Some(node_item) = self.node_table_state.get_node_item_mut(&service_name)
                    {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Added);
                    }
                    self.node_table_state.send_state_update()?;
                }
                StatusActions::RemoveNodesCompleted { service_name } => {
                    debug!("RemoveNodesCompleted for service: {service_name}");
                    if let Some(node_item) = self.node_table_state.get_node_item_mut(&service_name)
                    {
                        node_item.unlock();
                        node_item.update_status(crate::components::node_table::NodeStatus::Removed);
                    }
                    // Remove the node from the list
                    self.node_table_state
                        .items
                        .items
                        .retain(|item| item.name != service_name);
                    self.node_table_state.send_state_update()?;
                }

                // Ignore all other status actions that are no longer relevant
                _ => {}
            },
            Action::OptionsActions(options_action) => match options_action {
                OptionsActions::UpdateStorageDrive(mountpoint, _drive_name) => {
                    self.storage_mountpoint.clone_from(&mountpoint);
                    self.available_disk_space_gb = get_available_space_b(&mountpoint)? / GB;
                }
                OptionsActions::ResetNodes => {
                    debug!("Got OptionsActions::ResetNodes - removing all nodes");
                    // Reset all nodes by removing all of them
                    let all_service_names: Vec<String> = self
                        .node_table_state
                        .items
                        .items
                        .iter()
                        .map(|item| item.name.clone())
                        .collect();
                    if !all_service_names.is_empty() {
                        self.node_table_state
                            .operations
                            .handle_remove_nodes(all_service_names)?;
                    }
                }
                OptionsActions::UpdateNodes => {
                    let all_service_names: Vec<String> = self
                        .node_table_state
                        .items
                        .items
                        .iter()
                        .map(|item| item.name.clone())
                        .collect();
                    if !all_service_names.is_empty() {
                        self.node_table_state
                            .operations
                            .handle_upgrade_nodes(all_service_names)?;
                    }
                }
                _ => {}
            },
            Action::ShowErrorPopup(mut error_popup) => {
                error_popup.show();
                self.error_popup = Some(error_popup);
                return Ok(Some(Action::SwitchInputMode(InputMode::Entry)));
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let layout = Layout::new(
            Direction::Vertical,
            [
                // Header
                Constraint::Length(1),
                // Device status
                Constraint::Max(6),
                // Node status
                Constraint::Min(3),
                // Footer
                Constraint::Length(3),
            ],
        )
        .split(area);

        // ==== Header =====

        let header = Header::new();
        f.render_stateful_widget(header, layout[0], &mut SelectedMenuItem::Status);

        // ==== Device Status =====

        // Device Status as a block with two tables so we can shrink the screen
        // and preserve as much as we can information

        let combined_block = Block::default()
            .title(" Device Status ")
            .bold()
            .title_style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .padding(Padding::horizontal(1))
            .style(Style::default().fg(VERY_LIGHT_AZURE));

        f.render_widget(combined_block.clone(), layout[1]);

        let storage_allocated_row = Row::new(vec![
            Cell::new("Storage Allocated".to_string()).fg(GHOST_WHITE),
            Cell::new(format!("{} GB", self.nodes_to_start * GB_PER_NODE)).fg(GHOST_WHITE),
        ]);
        let memory_use_val = if self.node_stats.total_memory_usage_mb as f64 / 1024_f64 > 1.0 {
            format!(
                "{:.2} GB",
                self.node_stats.total_memory_usage_mb as f64 / 1024_f64
            )
        } else {
            format!("{} MB", self.node_stats.total_memory_usage_mb)
        };

        let memory_use_row = Row::new(vec![
            Cell::new("Memory Use".to_string()).fg(GHOST_WHITE),
            Cell::new(memory_use_val).fg(GHOST_WHITE),
        ]);

        let connection_mode_string = match self.connection_mode {
            ConnectionMode::UPnP => "UPnP".to_string(),
            ConnectionMode::CustomPorts => format!(
                "Custom Ports  {}-{}",
                self.port_from.unwrap_or(PORT_MIN),
                self.port_to.unwrap_or(PORT_MIN + PORT_ALLOCATION)
            ),
            ConnectionMode::Automatic => "Automatic".to_string(),
        };

        let connection_mode_line = vec![Span::styled(
            connection_mode_string,
            Style::default().fg(GHOST_WHITE),
        )];

        let connection_mode_row = Row::new(vec![
            Cell::new("Connection".to_string()).fg(GHOST_WHITE),
            Cell::new(Line::from(connection_mode_line)),
        ]);

        let stats_rows = vec![storage_allocated_row, memory_use_row, connection_mode_row];
        let stats_width = [Constraint::Length(5)];
        let column_constraints = [Constraint::Length(23), Constraint::Fill(1)];
        let stats_table = Table::new(stats_rows, stats_width).widths(column_constraints);

        let wallet_not_set_text = "Press [Ctrl+B] to add your Wallet Address";
        let wallet_not_set = if self.rewards_address.is_empty() {
            vec![
                Span::styled("Press ".to_string(), Style::default().fg(VIVID_SKY_BLUE)),
                Span::styled("[Ctrl+B] ".to_string(), Style::default().fg(GHOST_WHITE)),
                Span::styled(
                    "to add your ".to_string(),
                    Style::default().fg(VIVID_SKY_BLUE),
                ),
                Span::styled(
                    "Wallet Address".to_string(),
                    Style::default().fg(VIVID_SKY_BLUE).bold(),
                ),
            ]
        } else {
            vec![]
        };

        let total_attos_earned_and_wallet_row = Row::new(vec![
            Cell::new("Attos Earned".to_string()).fg(VIVID_SKY_BLUE),
            Cell::new(format!(
                "{:?}",
                self.node_stats.total_rewards_wallet_balance
            ))
            .fg(VIVID_SKY_BLUE)
            .bold(),
            Cell::new(Line::from(wallet_not_set).alignment(Alignment::Right)),
        ]);

        let attos_wallet_rows = vec![total_attos_earned_and_wallet_row];
        let attos_wallet_width = [Constraint::Length(5)];
        let wallet_column_width = if self.rewards_address.is_empty() {
            wallet_not_set_text.len() as u16
        } else {
            0
        };
        let column_constraints = [
            Constraint::Length(23),
            Constraint::Fill(1),
            Constraint::Length(wallet_column_width),
        ];
        let attos_wallet_table =
            Table::new(attos_wallet_rows, attos_wallet_width).widths(column_constraints);

        let inner_area = combined_block.inner(layout[1]);
        let device_layout = Layout::new(
            Direction::Vertical,
            vec![Constraint::Length(5), Constraint::Length(1)],
        )
        .split(inner_area);

        // Render both tables inside the combined block
        f.render_widget(stats_table, device_layout[0]);
        f.render_widget(attos_wallet_table, device_layout[1]);

        // ==== Node Status =====

        // No nodes. Empty Table.
        if !self.has_nodes || self.rewards_address.is_empty() {
            let line1 = Line::from(vec![
                Span::styled("Press ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled("[+] ", Style::default().fg(GHOST_WHITE).bold()),
                Span::styled("to Add and ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled(
                    "Start your first node ",
                    Style::default().fg(GHOST_WHITE).bold(),
                ),
                Span::styled("on this device", Style::default().fg(LIGHT_PERIWINKLE)),
            ]);

            let line2 = Line::from(vec![Span::styled(
                format!(
                    "Each node will use {GB_PER_NODE}GB of storage and a small amount of memory, \
                        CPU, and Network bandwidth. Most computers can run many nodes at once, \
                        but we recommend you add them gradually"
                ),
                Style::default().fg(LIGHT_PERIWINKLE),
            )]);

            f.render_widget(
                Paragraph::new(vec![Line::raw(""), line1, Line::raw(""), line2])
                    .wrap(Wrap { trim: false })
                    .fg(LIGHT_PERIWINKLE)
                    .block(
                        Block::default()
                            .title(Line::from(vec![
                                Span::styled(" Nodes", Style::default().fg(GHOST_WHITE).bold()),
                                Span::styled(" (0) ", Style::default().fg(LIGHT_PERIWINKLE)),
                            ]))
                            .title_style(Style::default().fg(LIGHT_PERIWINKLE))
                            .borders(Borders::ALL)
                            .border_style(style::Style::default().fg(EUCALYPTUS))
                            .padding(Padding::horizontal(1)),
                    ),
                layout[2],
            );
        } else {
            // Render NodeTable in the node area using the component
            self.node_table_component.draw(f, layout[2])?;
        }

        // ==== Footer =====

        let footer = Footer::default();
        let footer_state = if self.has_nodes || !self.rewards_address.is_empty() {
            if self.has_running_nodes {
                &mut NodesToStart::Running
            } else {
                &mut NodesToStart::NotRunning
            }
        } else {
            &mut NodesToStart::NotRunning
        };

        f.render_stateful_widget(footer, layout[3], footer_state);

        // ===== Popups =====

        // Error Popup
        if let Some(error_popup) = &self.error_popup
            && error_popup.is_visible()
        {
            error_popup.draw_error(f, area);

            return Ok(());
        }

        Ok(())
    }
}
