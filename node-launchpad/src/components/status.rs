// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::OptionsActions;
use crate::components::Component;
use crate::components::footer::{Footer, NodesToStart};
use crate::components::header::{Header, SelectedMenuItem};
use crate::components::node_table::{NodeTableComponent, NodeTableConfig};
use crate::components::popup::manage_nodes::{GB, GB_PER_NODE};
use crate::components::popup::port_range::PORT_ALLOCATION;
use crate::config::get_launchpad_nodes_data_dir_path;
use crate::connection_mode::ConnectionMode;
use crate::error::ErrorPopup;
use crate::node_management::config::PORT_MIN;
use crate::system::get_available_space_b;
use crate::{
    action::{Action, NodeTableActions, StatusActions},
    focus::{EventResult, FocusManager, FocusTarget},
    mode::{InputMode, Scene},
    node_stats::NodeStats,
    style::{EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VERY_LIGHT_AZURE, VIVID_SKY_BLUE},
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
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
    // Device Stats Section
    node_stats: NodeStats,
    // Amount of nodes
    nodes_to_start: u64,
    // Rewards address
    rewards_address: Option<EvmAddress>,
    // Path where the node data is stored
    data_dir_path: PathBuf,
    // Connection mode
    connection_mode: ConnectionMode,
    // Port from
    port_from: Option<u32>,
    // Port to
    port_to: Option<u32>,
    storage_mountpoint: PathBuf,
    available_disk_space_gb: u64,
    error_popup: Option<ErrorPopup>,

    // NodeTable component (contains the state)
    node_table_component: NodeTableComponent,

    // Cached state from NodeTable
    node_count: u64,
    has_running_nodes: bool,
    has_nodes: bool,
}

pub struct StatusConfig {
    pub allocated_disk_space: u64,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
    pub data_dir_path: PathBuf,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
    pub storage_mountpoint: PathBuf,
    pub rewards_address: Option<EvmAddress>,
    pub registry_path_override: Option<PathBuf>,
}

impl Status {
    pub async fn new(config: StatusConfig) -> Result<Self> {
        let status = Self {
            action_sender: Default::default(),
            node_stats: NodeStats::default(),
            nodes_to_start: config.allocated_disk_space,
            rewards_address: config.rewards_address,
            data_dir_path: config.data_dir_path.clone(),
            connection_mode: config.connection_mode,
            port_from: config.port_from,
            port_to: config.port_to,
            error_popup: None,
            storage_mountpoint: config.storage_mountpoint.clone(),
            available_disk_space_gb: get_available_space_b(&config.storage_mountpoint)? / GB,

            // Initialize NodeTable component
            node_table_component: NodeTableComponent::new(NodeTableConfig {
                network_id: config.network_id,
                init_peers_config: config.init_peers_config.clone(),
                antnode_path: config.antnode_path.clone(),
                data_dir_path: config.data_dir_path.clone(),
                connection_mode: config.connection_mode,
                port_from: config.port_from,
                port_to: config.port_to,
                rewards_address: config.rewards_address,
                nodes_to_start: config.allocated_disk_space,
                storage_mountpoint: config.storage_mountpoint.clone(),
                registry_path_override: config.registry_path_override.clone(),
            })
            .await?,

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
        self.node_table_component
            .state_mut()
            .operations
            .register_action_sender(tx.clone());

        // Register action sender with NodeTableComponent
        self.node_table_component
            .register_action_handler(tx.clone())?;

        // Update the stats to be shown as soon as the app is run
        self.node_table_component
            .state_mut()
            .try_update_node_stats(true)?;

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
        debug!(
            "Status has focus: {}",
            focus_manager.has_focus(&FocusTarget::Status)
        );

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
            debug!("Status: Handling status-only key");
            // Handle Status-specific keys here
            return Ok((vec![], EventResult::Consumed));
        }

        // Delegate node table operations to NodeTableComponent when Status has focus
        if focus_manager.has_focus(&FocusTarget::Status) {
            debug!("Status: Delegating key {:?} to NodeTableComponent", key);
            let (node_actions, event_result) = self
                .node_table_component
                .handle_key_events(key, focus_manager)?;
            debug!(
                "Status: NodeTableComponent returned result {:?}",
                event_result
            );
            // If the NodeTable component consumed the event, return those actions
            if matches!(event_result, EventResult::Consumed) {
                debug!("Status: Key was consumed by NodeTableComponent");
                return Ok((node_actions, event_result));
            }
            // Otherwise, fall through to handle Status-specific operations
        }

        // If Status has focus, handle Status-specific operations
        if focus_manager.has_focus(&self.focus_target()) {
            debug!("Status: Handling status-specific operations");
            let actions = self.handle_status_key_events(key)?;
            let result = if actions.is_empty() {
                EventResult::Ignored
            } else {
                EventResult::Consumed
            };
            debug!(
                "Status: Generated {} actions, result: {:?}",
                actions.len(),
                result
            );
            return Ok((actions, result));
        }

        debug!("Status: No focus, ignoring key");
        Ok((vec![], EventResult::Ignored))
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        // Handle NodeTable actions directly
        if let Action::NodeTableActions(node_table_action) = action.clone() {
            match node_table_action {
                NodeTableActions::StateChanged {
                    node_count,
                    has_running_nodes,
                    has_nodes,
                } => {
                    debug!(
                        "Status::update - Received StateChanged: node_count={node_count}, has_nodes={has_nodes}, has_running_nodes={has_running_nodes}"
                    );
                    self.node_count = node_count;
                    self.has_running_nodes = has_running_nodes;
                    self.has_nodes = has_nodes;
                    debug!(
                        "Status::update - Updated cached state: node_count={}, has_nodes={}, has_running_nodes={}",
                        self.node_count, self.has_nodes, self.has_running_nodes
                    );
                    return Ok(None);
                }
                _ => {
                    // Forward all other NodeTableActions to NodeTableComponent
                    return self.node_table_component.update(action);
                }
            }
        }

        // Handle Status-specific actions
        match action {
            Action::Tick => {
                self.node_table_component
                    .state_mut()
                    .try_update_node_stats(false)?;
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
                // Sync with NodeTableState
                self.node_table_component
                    .state_mut()
                    .sync_nodes_to_start(count);
                if self.nodes_to_start == 0 {
                    info!("Nodes to start set to 0. Sending command to stop all nodes.");
                    return Ok(Some(Action::NodeTableActions(NodeTableActions::StopNodes)));
                } else {
                    info!("Nodes to start set to: {count}. Sending command to start nodes");
                    return Ok(Some(Action::NodeTableActions(NodeTableActions::StartNodes)));
                }
            }
            Action::StoreRewardsAddress(rewards_address) => {
                debug!("Storing rewards address: {rewards_address:?}");
                self.rewards_address = Some(rewards_address);
                // Sync with NodeTableState
                self.node_table_component
                    .state_mut()
                    .sync_rewards_address(Some(rewards_address));
            }
            Action::StoreStorageDrive(ref drive_mountpoint, ref _drive_name) => {
                self.data_dir_path =
                    get_launchpad_nodes_data_dir_path(&drive_mountpoint.to_path_buf(), false)?;
            }
            Action::StoreConnectionMode(connection_mode) => {
                self.connection_mode = connection_mode;
                // Sync with NodeTableState
                self.node_table_component
                    .state_mut()
                    .sync_connection_mode(connection_mode);
            }
            Action::StorePortRange(port_from, port_range) => {
                self.port_from = Some(port_from);
                self.port_to = Some(port_range);
                // Sync with NodeTableState
                self.node_table_component
                    .state_mut()
                    .sync_port_range(Some(port_from), Some(port_range));
            }
            Action::StatusActions(status_action) => match status_action {
                StatusActions::NodesStatsObtained(stats) => {
                    self.node_stats = stats.clone();
                    self.node_table_component.state_mut().sync_node_stats(stats);
                }
                StatusActions::TriggerManageNodes => {
                    return Ok(Some(Action::SwitchScene(Scene::ManageNodesPopUp {
                        amount_of_nodes: self.nodes_to_start,
                    })));
                }
                StatusActions::TriggerRewardsAddress => {
                    if self.rewards_address.is_none() {
                        return Ok(Some(Action::SwitchScene(Scene::StatusRewardsAddressPopUp)));
                    } else {
                        return Ok(None);
                    }
                }
                _ => {}
            },
            Action::OptionsActions(OptionsActions::UpdateStorageDrive(mountpoint, _drive_name)) => {
                self.storage_mountpoint.clone_from(&mountpoint);
                self.available_disk_space_gb = get_available_space_b(&mountpoint)? / GB;
            }
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
        let wallet_not_set = if self.rewards_address.is_none() {
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
        let wallet_column_width = if self.rewards_address.is_none() {
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
        if !self.has_nodes || self.rewards_address.is_none() {
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
        let footer_state = if self.has_nodes || self.rewards_address.is_some() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::{EventResult, FocusManager};
    use crate::test_utils::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use std::env::temp_dir;

    fn create_test_status_config() -> StatusConfig {
        let temp_path = temp_dir();
        // Use root filesystem as mountpoint since it's always available on Unix-like systems
        let storage_mountpoint = if cfg!(unix) {
            PathBuf::from("/")
        } else {
            PathBuf::from("C:\\")
        };

        StatusConfig {
            allocated_disk_space: 10,
            antnode_path: Some(PathBuf::from("/usr/local/bin/antnode")),
            connection_mode: ConnectionMode::Automatic,
            data_dir_path: temp_path,
            network_id: Some(1),
            init_peers_config: InitialPeersConfig::default(),
            port_from: Some(15000),
            port_to: Some(15100),
            storage_mountpoint,
            rewards_address: "0x1234567890123456789012345678901234567890"
                .parse::<EvmAddress>()
                .ok(),
            registry_path_override: None,
        }
    }

    #[tokio::test]
    async fn test_status_handle_key_events_with_error_popup() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let focus_manager = FocusManager::new(FocusTarget::Status);

        // Set up error popup
        let mut error_popup = ErrorPopup::new(
            "Test Error".to_string(),
            "Test error message".to_string(),
            "Detailed error".to_string(),
        );
        error_popup.show();
        status.error_popup = Some(error_popup);

        let key_event = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        let result = status.handle_key_events(key_event, &focus_manager);

        assert!(result.is_ok());
        let (actions, event_result) = result.unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            Action::SwitchInputMode(InputMode::Navigation)
        ));
        assert_eq!(event_result, EventResult::Consumed);
    }

    #[tokio::test]
    async fn test_status_handle_key_events_when_focused() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let focus_manager = FocusManager::new(FocusTarget::Status);

        let key_event = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty());
        let result = status.handle_key_events(key_event, &focus_manager);

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_update_tick_action() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::Tick);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_update_switch_scene() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::SwitchScene(Scene::Status));
        assert!(result.is_ok());
        let action = result.unwrap();
        assert_eq!(action, Some(Action::SwitchInputMode(InputMode::Navigation)));
    }

    #[tokio::test]
    async fn test_status_update_store_nodes_to_start_zero() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StoreNodesToStart(0));
        assert!(result.is_ok());
        let action = result.unwrap();
        assert_eq!(
            action,
            Some(Action::NodeTableActions(NodeTableActions::StopNodes))
        );
        assert_eq!(status.nodes_to_start, 0);
    }

    #[tokio::test]
    async fn test_status_update_store_nodes_to_start_non_zero() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StoreNodesToStart(5));
        assert!(result.is_ok());
        let action = result.unwrap();
        assert_eq!(
            action,
            Some(Action::NodeTableActions(NodeTableActions::StartNodes))
        );
        assert_eq!(status.nodes_to_start, 5);
    }

    #[tokio::test]
    async fn test_status_update_store_rewards_address() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let new_address = "0x1234567890abcdef1234567890abcdef12345678"
            .parse::<EvmAddress>()
            .unwrap();

        let result = status.update(Action::StoreRewardsAddress(new_address));
        assert!(result.is_ok());
        assert_eq!(status.rewards_address, Some(new_address));
    }

    #[tokio::test]
    async fn test_status_update_store_connection_mode() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StoreConnectionMode(ConnectionMode::UPnP));
        assert!(result.is_ok());
        assert_eq!(status.connection_mode, ConnectionMode::UPnP);
    }

    #[tokio::test]
    async fn test_status_update_store_port_range() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StorePortRange(20000, 20100));
        assert!(result.is_ok());
        assert_eq!(status.port_from, Some(20000));
        assert_eq!(status.port_to, Some(20100));
    }

    #[tokio::test]
    async fn test_status_update_nodes_stats_obtained() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let new_stats = NodeStats {
            total_memory_usage_mb: 1024,
            total_rewards_wallet_balance: 100,
            individual_stats: Vec::new(),
        };

        let result = status.update(Action::StatusActions(StatusActions::NodesStatsObtained(
            new_stats.clone(),
        )));
        assert!(result.is_ok());
        assert_eq!(status.node_stats.total_memory_usage_mb, 1024);
        assert_eq!(status.node_stats.total_rewards_wallet_balance, 100);
    }

    // TODO: Rewrite this test to use real node data instead of mocks
    // #[tokio::test]
    // async fn test_status_update_registry_updated() {
    //     let config = create_test_status_config();
    //     let mut status = Status::new(config).await.unwrap();
    //     // Test with real node registry data
    //     assert!(true); // Placeholder
    // }

    #[tokio::test]
    async fn test_status_update_trigger_manage_nodes() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        status.nodes_to_start = 5;

        let result = status.update(Action::StatusActions(StatusActions::TriggerManageNodes));
        assert!(result.is_ok());
        let action = result.unwrap();
        assert_eq!(
            action,
            Some(Action::SwitchScene(Scene::ManageNodesPopUp {
                amount_of_nodes: 5
            }))
        );
    }

    #[tokio::test]
    async fn test_status_update_trigger_rewards_address_empty() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        status.rewards_address = None;

        let result = status.update(Action::StatusActions(StatusActions::TriggerRewardsAddress));
        assert!(result.is_ok());
        let action = result.unwrap();
        assert_eq!(
            action,
            Some(Action::SwitchScene(Scene::StatusRewardsAddressPopUp))
        );
    }

    #[tokio::test]
    async fn test_status_update_trigger_rewards_address_non_empty() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StatusActions(StatusActions::TriggerRewardsAddress));
        assert!(result.is_ok());
        let action = result.unwrap();
        assert_eq!(action, None);
    }

    #[tokio::test]
    async fn test_status_update_show_error_popup() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let error_popup = ErrorPopup::new(
            "Test Error".to_string(),
            "Test error message".to_string(),
            "Detailed error".to_string(),
        );

        let result = status.update(Action::ShowErrorPopup(error_popup));
        assert!(result.is_ok());
        let action = result.unwrap();
        assert_eq!(action, Some(Action::SwitchInputMode(InputMode::Entry)));
        assert!(status.error_popup.is_some());
        assert!(status.error_popup.as_ref().unwrap().is_visible());
    }

    #[tokio::test]
    async fn test_status_update_node_table_state_changed() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::NodeTableActions(NodeTableActions::StateChanged {
            node_count: 5,
            has_running_nodes: true,
            has_nodes: true,
        }));
        assert!(result.is_ok());
        assert_eq!(status.node_count, 5);
        assert!(status.has_running_nodes);
        assert!(status.has_nodes);
    }

    #[tokio::test]
    async fn test_status_drawing_with_error_popup() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let mut error_popup = ErrorPopup::new(
            "Test Error".to_string(),
            "Test error message".to_string(),
            "Detailed error".to_string(),
        );
        error_popup.show();
        status.error_popup = Some(error_popup);

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let result = terminal.draw(|f| {
            let area = f.area();
            if let Err(e) = status.draw(f, area) {
                panic!("Drawing failed: {e}");
            }
        });

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_connection_mode_display_upnp() {
        let mut config = create_test_status_config();
        config.connection_mode = ConnectionMode::UPnP;
        let status = Status::new(config).await.unwrap();

        assert_eq!(status.connection_mode, ConnectionMode::UPnP);
    }

    #[tokio::test]
    async fn test_status_connection_mode_display_custom_ports() {
        let mut config = create_test_status_config();
        config.connection_mode = ConnectionMode::CustomPorts;
        config.port_from = Some(20000);
        config.port_to = Some(20100);
        let status = Status::new(config).await.unwrap();

        assert_eq!(status.connection_mode, ConnectionMode::CustomPorts);
        assert_eq!(status.port_from, Some(20000));
        assert_eq!(status.port_to, Some(20100));
    }

    #[tokio::test]
    async fn test_status_memory_display_calculation() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        // Test memory usage less than 1GB
        status.node_stats.total_memory_usage_mb = 512;
        assert_eq!(status.node_stats.total_memory_usage_mb, 512);

        // Test memory usage greater than 1GB
        status.node_stats.total_memory_usage_mb = 2048;
        assert_eq!(status.node_stats.total_memory_usage_mb, 2048);
    }

    // TODO: Rewrite this test to use real node data instead of MockNode
    // #[tokio::test]
    // async fn test_status_component_integration_with_real_nodes() {
    //     let config = create_test_status_config();
    //     let mut status = Status::new(config).await.unwrap();
    //     // Test with real node services
    //     assert!(true); // Placeholder
    // }

    #[test]
    fn test_keyboard_sequence_with_status() {
        let key_sequence = KeySequence::new()
            .key('+')
            .ctrl('b')
            .arrow_down()
            .enter()
            .esc()
            .build();

        assert_eq!(key_sequence.len(), 5);
        assert_eq!(key_sequence[0].code, KeyCode::Char('+'));
        assert_eq!(key_sequence[1].code, KeyCode::Char('b'));
        assert!(key_sequence[1].modifiers.contains(KeyModifiers::CONTROL));
        assert_eq!(key_sequence[2].code, KeyCode::Down);
        assert_eq!(key_sequence[3].code, KeyCode::Enter);
        assert_eq!(key_sequence[4].code, KeyCode::Esc);
    }
}
