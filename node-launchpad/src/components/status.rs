// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::OptionsActions;
use crate::components::Component;
use crate::components::footer::{Footer, FooterState};
use crate::components::header::{Header, SelectedMenuItem};
use crate::components::node_table::{NodeDisplayStatus, NodeTableComponent, NodeTableConfig};
use crate::components::popup::error_popup::ErrorPopup;
use crate::components::popup::manage_nodes::{GB, GB_PER_NODE};
use crate::config::get_launchpad_nodes_data_dir_path;
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
    // UPnP enabled
    upnp_enabled: bool,
    // Port range
    port_range: Option<(u32, u32)>,
    storage_mountpoint: PathBuf,
    available_disk_space_gb: u64,
    error_popup: Option<ErrorPopup>,

    // NodeTable component (contains the state)
    node_table_component: NodeTableComponent,

    // Cached state from NodeTable
    node_count: u64,
    has_running_nodes: bool,
    has_nodes: bool,
    selected_node_status: Option<NodeDisplayStatus>,
}

pub struct StatusConfig {
    pub allocated_disk_space: u64,
    pub antnode_path: Option<PathBuf>,
    pub upnp_enabled: bool,
    pub port_range: Option<(u32, u32)>,
    pub data_dir_path: PathBuf,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
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
            upnp_enabled: config.upnp_enabled,
            port_range: config.port_range,
            error_popup: None,
            storage_mountpoint: config.storage_mountpoint.clone(),
            available_disk_space_gb: get_available_space_b(config.storage_mountpoint.as_path())?
                / GB,

            // Initialize NodeTable component
            node_table_component: NodeTableComponent::new(NodeTableConfig {
                network_id: config.network_id,
                init_peers_config: config.init_peers_config.clone(),
                antnode_path: config.antnode_path.clone(),
                data_dir_path: config.data_dir_path.clone(),
                upnp_enabled: config.upnp_enabled,
                port_range: config.port_range,
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
            selected_node_status: None,
        };

        Ok(status)
    }

    fn handle_status_key_events(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
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
        // Handle NodeTable actions directly
        if let Action::NodeTableActions(node_table_action) = action.clone() {
            match node_table_action {
                NodeTableActions::StateChanged {
                    node_count,
                    has_running_nodes,
                    has_nodes,
                } => {
                    self.node_count = node_count;
                    self.has_running_nodes = has_running_nodes;
                    self.has_nodes = has_nodes;
                    debug!(
                        "Updated cached state: node_count={}, has_nodes={}, has_running_nodes={}",
                        self.node_count, self.has_nodes, self.has_running_nodes
                    );
                    return Ok(None);
                }
                NodeTableActions::SelectionChanged {
                    selected_node_status,
                } => {
                    self.selected_node_status = selected_node_status;
                    debug!(
                        "Updated selection: selected_node_status={:?}",
                        self.selected_node_status
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
            Action::SwitchScene(Scene::Status) => {
                return Ok(Some(Action::SwitchInputMode(InputMode::Navigation)));
            }
            Action::Tick => {
                self.node_table_component
                    .state_mut()
                    .try_update_node_stats(false)?;
            }
            Action::StoreRunningNodeCount(count) => {
                self.nodes_to_start = count;
                self.node_table_component
                    .state_mut()
                    .sync_nodes_to_start(count);
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
            Action::StoreUpnpSetting(upnp_enabled) => {
                self.upnp_enabled = upnp_enabled;
                // Sync with NodeTableState
                self.node_table_component
                    .state_mut()
                    .sync_upnp_setting(upnp_enabled);
            }
            Action::StorePortRange(port_range) => {
                self.port_range = port_range;
                // Sync with NodeTableState
                self.node_table_component
                    .state_mut()
                    .sync_port_range(port_range);
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
            },
            Action::OptionsActions(OptionsActions::UpdateStorageDrive(mountpoint, _drive_name)) => {
                self.storage_mountpoint.clone_from(&mountpoint);
                self.available_disk_space_gb = get_available_space_b(mountpoint.as_path())? / GB;
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

        let connection_info = if let Some((from, to)) = self.port_range {
            format!(
                "Ports: {}-{} {}",
                from,
                to,
                if self.upnp_enabled {
                    "(UPnP)"
                } else {
                    "(Upnp Disabled)"
                }
            )
        } else {
            format!(
                "Automatic {}",
                if self.upnp_enabled {
                    "(UPnP)"
                } else {
                    "(Upnp Disabled)"
                }
            )
        };

        let connection_mode_line = vec![Span::styled(
            connection_info,
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
        let mut footer_state = FooterState {
            has_nodes: self.has_nodes,
            has_running_nodes: self.has_running_nodes,
            selected_node_status: self.selected_node_status,
            rewards_address_set: self.rewards_address.is_some(),
        };

        f.render_stateful_widget(footer, layout[3], &mut footer_state);

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
    use crate::node_stats::IndividualNodeStats;
    use crate::test_utils::*;
    use ant_service_management::{NodeServiceData, ServiceStatus};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use std::env::temp_dir;
    use std::time::Duration;

    fn create_test_status_config() -> StatusConfig {
        let temp_path = temp_dir().join("node-launchpad-test-data");
        std::fs::create_dir_all(&temp_path).expect("failed to create temp directory");
        let storage_mountpoint = temp_path.clone();

        StatusConfig {
            allocated_disk_space: 10,
            antnode_path: Some(temp_path.join("antnode")),
            upnp_enabled: true,
            port_range: Some((15000, 15100)),
            data_dir_path: temp_path.clone(),
            network_id: Some(1),
            init_peers_config: InitialPeersConfig::default(),
            storage_mountpoint,
            rewards_address: "0x1234567890123456789012345678901234567890"
                .parse::<EvmAddress>()
                .ok(),
            registry_path_override: Some(temp_path.join("registry")),
        }
    }

    fn sync_single_node(status: &mut Status, service_status: ServiceStatus) -> NodeServiceData {
        let registry = MockNodeRegistry::empty().expect("failed to create mock registry");
        let node = registry.create_test_node_service_data(0, service_status);
        status
            .node_table_component
            .state_mut()
            .sync_node_service_data(std::slice::from_ref(&node));
        node
    }

    #[tokio::test]
    async fn test_status_handle_key_events_with_error_popup() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let focus_manager = FocusManager::new(FocusTarget::Status);

        // Set up error popup
        let mut error_popup = ErrorPopup::new("Test Error", "Test error message", "Detailed error");
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
        let (actions, event_result) = status
            .handle_key_events(key_event, &focus_manager)
            .expect("handled");

        assert!(actions.is_empty(), "unexpected actions were emitted");
        assert_eq!(event_result, EventResult::Ignored);
    }

    #[tokio::test]
    async fn test_status_update_tick_action_updates_last_refresh_time() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let table_state = status.node_table_component.state_mut();
        table_state.node_stats_last_update =
            std::time::Instant::now() - NODE_STAT_UPDATE_INTERVAL - Duration::from_secs(1);
        let before = table_state.node_stats_last_update;

        status.update(Action::Tick).expect("tick handled");

        let after = status.node_table_component.state().node_stats_last_update;
        assert!(
            after > before,
            "tick should refresh last stats update instant"
        );
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
    async fn test_status_update_store_rewards_address() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let new_address = "0x1234567890abcdef1234567890abcdef12345678"
            .parse::<EvmAddress>()
            .unwrap();

        let result = status.update(Action::StoreRewardsAddress(new_address));
        assert!(result.is_ok());
        assert_eq!(status.rewards_address, Some(new_address));
        assert_eq!(
            status.node_table_component.state().rewards_address,
            Some(new_address)
        );
    }

    #[tokio::test]
    async fn test_status_update_store_upnp_setting() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StoreUpnpSetting(false));
        assert!(result.is_ok());
        assert!(!status.upnp_enabled);
        assert!(!status.node_table_component.state().upnp_enabled);
    }

    #[tokio::test]
    async fn test_status_update_store_port_range() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StorePortRange(Some((20000, 20100))));
        assert!(result.is_ok());
        assert_eq!(status.port_range, Some((20000, 20100)));
        assert_eq!(
            status.node_table_component.state().port_range,
            Some((20000, 20100))
        );
    }

    #[tokio::test]
    async fn test_status_update_nodes_stats_obtained() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();
        let node = sync_single_node(&mut status, ServiceStatus::Running);
        let new_stats = NodeStats {
            total_memory_usage_mb: 1024,
            total_rewards_wallet_balance: 100,
            individual_stats: vec![IndividualNodeStats {
                service_name: node.service_name.clone(),
                rewards_wallet_balance: 55,
                memory_usage_mb: 777,
                bandwidth_inbound_rate: 11,
                bandwidth_outbound_rate: 22,
                max_records: 33,
                peers: 44,
                connections: 5,
                ..Default::default()
            }],
        };

        let result = status.update(Action::StatusActions(StatusActions::NodesStatsObtained(
            new_stats.clone(),
        )));
        assert!(result.is_ok());
        assert_eq!(status.node_stats.total_memory_usage_mb, 1024);
        assert_eq!(status.node_stats.total_rewards_wallet_balance, 100);

        let node_item = status
            .node_table_component
            .state()
            .items
            .items
            .iter()
            .find(|item| item.service_name == node.service_name)
            .expect("node item updated");
        assert_eq!(node_item.rewards_wallet_balance, 55);
        assert_eq!(node_item.memory, 777);
        assert_eq!(node_item.records, 33);
        assert_eq!(node_item.connections, 5);
    }

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
        let error_popup = ErrorPopup::new("Test Error", "Test error message", "Detailed error");

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
    async fn test_status_update_node_table_selection_changed() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::NodeTableActions(
            NodeTableActions::SelectionChanged {
                selected_node_status: Some(NodeDisplayStatus::Running),
            },
        ));
        assert!(result.is_ok());
        assert_eq!(
            status.selected_node_status,
            Some(NodeDisplayStatus::Running)
        );
    }

    #[tokio::test]
    async fn test_status_store_running_node_count_syncs_table() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let result = status.update(Action::StoreRunningNodeCount(7));
        assert!(result.is_ok());
        assert_eq!(status.nodes_to_start, 7);
        assert_eq!(status.node_table_component.state().nodes_to_start, 7);
    }

    #[tokio::test]
    async fn test_status_drawing_with_error_popup() {
        let config = create_test_status_config();
        let mut status = Status::new(config).await.unwrap();

        let mut error_popup = ErrorPopup::new("Test Error", "Test error message", "Detailed error");
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
