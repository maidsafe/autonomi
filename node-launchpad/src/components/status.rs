// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::footer::NodesToStart;
use super::header::SelectedMenuItem;
use super::{
    footer::Footer, header::Header, popup::manage_nodes::GB_PER_NODE, utils::centered_rect_fixed,
    Component, Frame,
};
use crate::action::OptionsActions;
use crate::components::popup::port_range::PORT_ALLOCATION;
use crate::components::utils::open_logs;
use crate::config::get_launchpad_nodes_data_dir_path;
use crate::connection_mode::{ConnectionMode, NodeConnectionMode};
use crate::error::ErrorPopup;
use crate::node_mgmt::{MaintainNodesArgs, NodeManagement, NodeManagementTask, UpgradeNodesArgs};
use crate::node_mgmt::{PORT_MAX, PORT_MIN};
use crate::style::{COOL_GREY, INDIGO, SIZZLING_RED};
use crate::tui::Event;
use crate::upnp::UpnpSupport;
use crate::{
    action::{Action, StatusActions},
    config::Config,
    mode::{InputMode, Scene},
    node_stats::NodeStats,
    style::{
        clear_area, EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VERY_LIGHT_AZURE, VIVID_SKY_BLUE,
    },
};
use ant_bootstrap::InitialPeersConfig;
use ant_node_manager::add_services::config::PortRange;
use ant_node_manager::config::get_node_registry_path;
use ant_service_management::{
    control::ServiceController, NodeRegistry, NodeServiceData, ServiceStatus,
};
use color_eyre::eyre::{Ok, OptionExt, Result};
use crossterm::event::KeyEvent;
use ratatui::text::Span;
use ratatui::{prelude::*, widgets::*};
use std::fmt;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
    vec,
};
use strum::Display;
use throbber_widgets_tui::{self, Throbber, ThrobberState};
use tokio::sync::mpsc::UnboundedSender;

pub const FIXED_INTERVAL: u64 = 60_000;
pub const NODE_STAT_UPDATE_INTERVAL: Duration = Duration::from_secs(5);
/// If nat detection fails for more than 3 times, we don't want to waste time running during every node start.
const MAX_ERRORS_WHILE_RUNNING_NAT_DETECTION: usize = 3;

// Table Widths
const NODE_WIDTH: usize = 10;
const VERSION_WIDTH: usize = 7;
const ATTOS_WIDTH: usize = 5;
const MEMORY_WIDTH: usize = 7;
const MBPS_WIDTH: usize = 13;
const RECORDS_WIDTH: usize = 4;
const PEERS_WIDTH: usize = 5;
const CONNS_WIDTH: usize = 5;
const MODE_WIDTH: usize = 7;
const STATUS_WIDTH: usize = 8;
const FAILURE_WIDTH: usize = 64;
const SPINNER_WIDTH: usize = 1;

#[derive(Clone)]
pub struct Status<'a> {
    /// Whether the component is active right now, capturing keystrokes + drawing things.
    active: bool,
    action_sender: Option<UnboundedSender<Action>>,
    config: Config,
    // NAT
    is_nat_status_determined: bool,
    error_while_running_nat_detection: usize,
    // Device Stats Section
    node_stats: NodeStats,
    node_stats_last_update: Instant,
    // Nodes
    node_services: Vec<NodeServiceData>,
    items: Option<StatefulTable<NodeItem<'a>>>,
    /// To pass into node services.
    network_id: Option<u8>,
    // Node Management
    node_management: NodeManagement,
    // Amount of nodes
    nodes_to_start: usize,
    // Rewards address
    rewards_address: String,
    // Currently the node registry file does not support concurrent actions and thus can lead to
    // inconsistent state. Another solution would be to have a file lock/db.
    lock_registry: Option<LockRegistryState>,
    // Peers to pass into nodes for startup
    init_peers_config: InitialPeersConfig,
    // If path is provided, we don't fetch the binary from the network
    antnode_path: Option<PathBuf>,
    // Path where the node data is stored
    data_dir_path: PathBuf,
    // Connection mode
    connection_mode: ConnectionMode,
    // UPnP support
    upnp_support: UpnpSupport,
    // Port from
    port_from: Option<u32>,
    // Port to
    port_to: Option<u32>,
    error_popup: Option<ErrorPopup>,
}

#[derive(Clone, Display, Debug)]
pub enum LockRegistryState {
    StartingNodes,
    StoppingNodes,
    ResettingNodes,
    UpdatingNodes,
}

pub struct StatusConfig {
    pub allocated_disk_space: usize,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
    pub upnp_support: UpnpSupport,
    pub data_dir_path: PathBuf,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
    pub rewards_address: String,
}

impl Status<'_> {
    pub async fn new(config: StatusConfig) -> Result<Self> {
        let mut status = Self {
            init_peers_config: config.init_peers_config,
            action_sender: Default::default(),
            config: Default::default(),
            active: true,
            is_nat_status_determined: false,
            error_while_running_nat_detection: 0,
            network_id: config.network_id,
            node_stats: NodeStats::default(),
            node_stats_last_update: Instant::now(),
            node_services: Default::default(),
            node_management: NodeManagement::new()?,
            items: None,
            nodes_to_start: config.allocated_disk_space,
            lock_registry: None,
            rewards_address: config.rewards_address,
            antnode_path: config.antnode_path,
            data_dir_path: config.data_dir_path,
            connection_mode: config.connection_mode,
            upnp_support: config.upnp_support,
            port_from: config.port_from,
            port_to: config.port_to,
            error_popup: None,
        };

        // Nodes registry
        let now = Instant::now();
        debug!("Refreshing node registry states on startup");
        let mut node_registry = NodeRegistry::load(&get_node_registry_path()?)?;
        ant_node_manager::refresh_node_registry(
            &mut node_registry,
            &ServiceController {},
            false,
            true,
            false,
        )
        .await?;
        node_registry.save()?;
        debug!("Node registry states refreshed in {:?}", now.elapsed());
        status.load_node_registry_and_update_states()?;

        Ok(status)
    }

    fn update_node_items(&mut self, new_status: Option<NodeStatus>) -> Result<()> {
        // Iterate over existing node services and update their corresponding NodeItem
        if let Some(ref mut items) = self.items {
            for node_item in self.node_services.iter() {
                // Find the corresponding item by service name
                if let Some(item) = items
                    .items
                    .iter_mut()
                    .find(|i| i.name == node_item.service_name)
                {
                    if let Some(status) = new_status {
                        item.status = status;
                    } else if item.status == NodeStatus::Updating {
                        item.spinner_state.calc_next();
                    } else if new_status != Some(NodeStatus::Updating) {
                        // Update status based on current node status
                        item.status = match node_item.status {
                            ServiceStatus::Running => {
                                item.spinner_state.calc_next();
                                NodeStatus::Running
                            }
                            ServiceStatus::Stopped => NodeStatus::Stopped,
                            ServiceStatus::Added => NodeStatus::Added,
                            ServiceStatus::Removed => NodeStatus::Removed,
                        };

                        // Starting is not part of ServiceStatus so we do it manually
                        if let Some(LockRegistryState::StartingNodes) = self.lock_registry {
                            item.spinner_state.calc_next();
                            if item.status != NodeStatus::Running {
                                item.status = NodeStatus::Starting;
                            }
                        }
                    }

                    // Update peers count
                    item.peers = match node_item.connected_peers {
                        Some(ref peers) => peers.len(),
                        None => 0,
                    };

                    // Update individual stats if available
                    if let Some(stats) = self
                        .node_stats
                        .individual_stats
                        .iter()
                        .find(|s| s.service_name == node_item.service_name)
                    {
                        item.attos = stats.rewards_wallet_balance;
                        item.memory = stats.memory_usage_mb;
                        item.mbps = format!(
                            "↓{:0>5.0} ↑{:0>5.0}",
                            (stats.bandwidth_inbound_rate * 8) as f64 / 1_000_000.0,
                            (stats.bandwidth_outbound_rate * 8) as f64 / 1_000_000.0,
                        );
                        item.records = stats.max_records;
                        item.connections = stats.connections;
                    }
                } else {
                    // If not found, create a new NodeItem and add it to items
                    let new_item = NodeItem {
                        name: node_item.service_name.clone(),
                        version: node_item.version.to_string(),
                        attos: 0,
                        memory: 0,
                        mbps: "-".to_string(),
                        records: 0,
                        peers: 0,
                        connections: 0,
                        mode: NodeConnectionMode::from(node_item),
                        status: NodeStatus::Added, // Set initial status as Added
                        failure: node_item.get_critical_failure(),
                        spinner: Throbber::default(),
                        spinner_state: ThrobberState::default(),
                    };
                    items.items.push(new_item);
                }
            }
        } else {
            // If items is None, create a new list (fallback)
            let node_items: Vec<NodeItem> = self
                .node_services
                .iter()
                .filter_map(|node_item| {
                    if node_item.status == ServiceStatus::Removed {
                        return None;
                    }
                    // Update status based on current node status
                    let status = match node_item.status {
                        ServiceStatus::Running => NodeStatus::Running,
                        ServiceStatus::Stopped => NodeStatus::Stopped,
                        ServiceStatus::Added => NodeStatus::Added,
                        ServiceStatus::Removed => NodeStatus::Removed,
                    };

                    // Create a new NodeItem for the first time
                    Some(NodeItem {
                        name: node_item.service_name.clone().to_string(),
                        version: node_item.version.to_string(),
                        attos: 0,
                        memory: 0,
                        mbps: "-".to_string(),
                        records: 0,
                        peers: 0,
                        connections: 0,
                        mode: NodeConnectionMode::from(node_item),
                        status,
                        failure: node_item.get_critical_failure(),
                        spinner: Throbber::default(),
                        spinner_state: ThrobberState::default(),
                    })
                })
                .collect();
            self.items = Some(StatefulTable::with_items(node_items));
        }
        Ok(())
    }

    fn clear_node_items(&mut self) {
        debug!("Cleaning items on Status page");
        if let Some(items) = self.items.as_mut() {
            items.items.clear();
            debug!("Cleared the items on status page");
        }
    }

    /// Tries to trigger the update of node stats if the last update was more than `NODE_STAT_UPDATE_INTERVAL` ago.
    /// The result is sent via the StatusActions::NodesStatsObtained action.
    fn try_update_node_stats(&mut self, force_update: bool) -> Result<()> {
        if self.node_stats_last_update.elapsed() > NODE_STAT_UPDATE_INTERVAL || force_update {
            self.node_stats_last_update = Instant::now();

            NodeStats::fetch_all_node_stats(&self.node_services, self.get_actions_sender()?);
        }
        Ok(())
    }
    fn get_actions_sender(&self) -> Result<UnboundedSender<Action>> {
        self.action_sender
            .clone()
            .ok_or_eyre("Action sender not registered")
    }

    fn load_node_registry_and_update_states(&mut self) -> Result<()> {
        let node_registry = NodeRegistry::load(&get_node_registry_path()?)?;
        self.is_nat_status_determined = node_registry.nat_status.is_some();
        self.node_services = node_registry
            .nodes
            .into_iter()
            .filter(|node| node.status != ServiceStatus::Removed)
            .collect();
        info!(
            "Loaded node registry. Maintaining {:?} nodes.",
            self.node_services.len()
        );

        Ok(())
    }

    /// Only run NAT detection if we haven't determined the status yet and we haven't failed more than 3 times.
    fn should_we_run_nat_detection(&self) -> bool {
        self.connection_mode == ConnectionMode::Automatic
            && !self.is_nat_status_determined
            && self.error_while_running_nat_detection < MAX_ERRORS_WHILE_RUNNING_NAT_DETECTION
    }

    fn get_running_nodes(&self) -> Vec<String> {
        self.node_services
            .iter()
            .filter_map(|node| {
                if node.status == ServiceStatus::Running {
                    Some(node.service_name.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn get_service_names_and_peer_ids(&self) -> (Vec<String>, Vec<String>) {
        let mut service_names = Vec::new();
        let mut peers_ids = Vec::new();

        for node in &self.node_services {
            // Only include nodes with a valid peer_id
            if let Some(peer_id) = &node.peer_id {
                service_names.push(node.service_name.clone());
                peers_ids.push(peer_id.to_string().clone());
            }
        }

        (service_names, peers_ids)
    }
}

impl Component for Status<'_> {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_sender = Some(tx);

        // Update the stats to be shown as soon as the app is run
        self.try_update_node_stats(true)?;

        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.config = config;
        Ok(())
    }

    fn handle_events(&mut self, event: Option<Event>) -> Result<Vec<Action>> {
        let r = match event {
            Some(Event::Key(key_event)) => self.handle_key_events(key_event)?,
            _ => vec![],
        };
        Ok(r)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Tick => {
                self.try_update_node_stats(false)?;
                let _ = self.update_node_items(None);
            }
            Action::SwitchScene(scene) => match scene {
                Scene::Status | Scene::StatusRewardsAddressPopUp => {
                    self.active = true;
                    // make sure we're in navigation mode
                    return Ok(Some(Action::SwitchInputMode(InputMode::Navigation)));
                }
                Scene::ManageNodesPopUp => self.active = true,
                _ => self.active = false,
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
                let has_changed = self.rewards_address != rewards_address;
                let we_have_nodes = !self.node_services.is_empty();

                self.rewards_address = rewards_address;

                if we_have_nodes && has_changed {
                    debug!("Setting lock_registry to ResettingNodes");
                    self.lock_registry = Some(LockRegistryState::ResettingNodes);
                    info!("Resetting antnode services because the Rewards Address was reset.");
                    let action_sender = self.get_actions_sender()?;
                    self.node_management
                        .send_task(NodeManagementTask::ResetNodes {
                            start_nodes_after_reset: false,
                            action_sender,
                        })?;
                }
            }
            Action::StoreStorageDrive(ref drive_mountpoint, ref _drive_name) => {
                debug!("Setting lock_registry to ResettingNodes");
                self.lock_registry = Some(LockRegistryState::ResettingNodes);
                info!("Resetting antnode services because the Storage Drive was changed.");
                let action_sender = self.get_actions_sender()?;
                self.node_management
                    .send_task(NodeManagementTask::ResetNodes {
                        start_nodes_after_reset: false,
                        action_sender,
                    })?;
                self.data_dir_path =
                    get_launchpad_nodes_data_dir_path(&drive_mountpoint.to_path_buf(), false)?;
            }
            Action::StoreConnectionMode(connection_mode) => {
                debug!("Setting lock_registry to ResettingNodes");
                self.lock_registry = Some(LockRegistryState::ResettingNodes);
                self.connection_mode = connection_mode;
                info!("Resetting antnode services because the Connection Mode range was changed.");
                let action_sender = self.get_actions_sender()?;
                self.node_management
                    .send_task(NodeManagementTask::ResetNodes {
                        start_nodes_after_reset: false,
                        action_sender,
                    })?;
            }
            Action::StorePortRange(port_from, port_range) => {
                debug!("Setting lock_registry to ResettingNodes");
                self.lock_registry = Some(LockRegistryState::ResettingNodes);
                self.port_from = Some(port_from);
                self.port_to = Some(port_range);
                info!("Resetting antnode services because the Port Range was changed.");
                let action_sender = self.get_actions_sender()?;
                self.node_management
                    .send_task(NodeManagementTask::ResetNodes {
                        start_nodes_after_reset: false,
                        action_sender,
                    })?;
            }
            Action::SetUpnpSupport(ref upnp_support) => {
                debug!("Setting UPnP support: {upnp_support:?}");
                self.upnp_support = upnp_support.clone();
            }
            Action::StatusActions(status_action) => match status_action {
                StatusActions::NodesStatsObtained(stats) => {
                    self.node_stats = stats;
                }
                StatusActions::StartNodesCompleted | StatusActions::StopNodesCompleted => {
                    self.lock_registry = None;
                    self.load_node_registry_and_update_states()?;
                }
                StatusActions::UpdateNodesCompleted => {
                    self.lock_registry = None;
                    self.clear_node_items();
                    self.load_node_registry_and_update_states()?;
                    let _ = self.update_node_items(None);
                    debug!("Update nodes completed");
                }
                StatusActions::ResetNodesCompleted { trigger_start_node } => {
                    self.lock_registry = None;
                    self.load_node_registry_and_update_states()?;
                    self.clear_node_items();

                    if trigger_start_node {
                        debug!("Reset nodes completed. Triggering start nodes.");
                        return Ok(Some(Action::StatusActions(StatusActions::StartNodes)));
                    }
                    debug!("Reset nodes completed");
                }
                StatusActions::SuccessfullyDetectedNatStatus => {
                    debug!(
                        "Successfully detected nat status, is_nat_status_determined set to true"
                    );
                    self.is_nat_status_determined = true;
                }
                StatusActions::ErrorWhileRunningNatDetection => {
                    self.error_while_running_nat_detection += 1;
                    debug!(
                        "Error while running nat detection. Error count: {}",
                        self.error_while_running_nat_detection
                    );
                }
                StatusActions::ErrorLoadingNodeRegistry { raw_error }
                | StatusActions::ErrorGettingNodeRegistryPath { raw_error } => {
                    self.error_popup = Some(ErrorPopup::new(
                        "Error".to_string(),
                        "Error getting node registry path".to_string(),
                        raw_error,
                    ));
                    if let Some(error_popup) = &mut self.error_popup {
                        error_popup.show();
                    }
                    // Switch back to entry mode so we can handle key events
                    return Ok(Some(Action::SwitchInputMode(InputMode::Entry)));
                }
                StatusActions::ErrorScalingUpNodes { raw_error } => {
                    self.error_popup = Some(ErrorPopup::new(
                        "Error".to_string(),
                        "Error adding new nodes".to_string(),
                        raw_error,
                    ));
                    if let Some(error_popup) = &mut self.error_popup {
                        error_popup.show();
                    }
                    // Switch back to entry mode so we can handle key events
                    return Ok(Some(Action::SwitchInputMode(InputMode::Entry)));
                }
                StatusActions::ErrorStoppingNodes { raw_error } => {
                    self.error_popup = Some(ErrorPopup::new(
                        "Error".to_string(),
                        "Error stopping nodes".to_string(),
                        raw_error,
                    ));
                    if let Some(error_popup) = &mut self.error_popup {
                        error_popup.show();
                    }
                    // Switch back to entry mode so we can handle key events
                    return Ok(Some(Action::SwitchInputMode(InputMode::Entry)));
                }
                StatusActions::ErrorUpdatingNodes { raw_error } => {
                    self.error_popup = Some(ErrorPopup::new(
                        "Error".to_string(),
                        "Error upgrading nodes".to_string(),
                        raw_error,
                    ));
                    if let Some(error_popup) = &mut self.error_popup {
                        error_popup.show();
                    }
                    // Switch back to entry mode so we can handle key events
                    return Ok(Some(Action::SwitchInputMode(InputMode::Entry)));
                }
                StatusActions::ErrorResettingNodes { raw_error } => {
                    self.error_popup = Some(ErrorPopup::new(
                        "Error".to_string(),
                        "Error resetting nodes".to_string(),
                        raw_error,
                    ));
                    if let Some(error_popup) = &mut self.error_popup {
                        error_popup.show();
                    }
                    // Switch back to entry mode so we can handle key events
                    return Ok(Some(Action::SwitchInputMode(InputMode::Entry)));
                }
                StatusActions::TriggerManageNodes => {
                    return Ok(Some(Action::SwitchScene(Scene::ManageNodesPopUp)));
                }
                StatusActions::PreviousTableItem => {
                    if let Some(items) = &mut self.items {
                        items.previous();
                    }
                }
                StatusActions::NextTableItem => {
                    if let Some(items) = &mut self.items {
                        items.next();
                    }
                }
                StatusActions::StartNodes => {
                    debug!("Got action to start nodes");

                    if self.rewards_address.is_empty() {
                        info!("Rewards address is not set. Ask for input.");
                        return Ok(Some(Action::StatusActions(
                            StatusActions::TriggerRewardsAddress,
                        )));
                    }

                    if self.nodes_to_start == 0 {
                        info!("Nodes to start not set. Ask for input.");
                        return Ok(Some(Action::StatusActions(
                            StatusActions::TriggerManageNodes,
                        )));
                    }

                    if self.lock_registry.is_some() {
                        error!(
                            "Registry is locked ({:?}) Cannot Start nodes now.",
                            self.lock_registry
                        );
                        return Ok(None);
                    }

                    debug!("Setting lock_registry to StartingNodes");
                    self.lock_registry = Some(LockRegistryState::StartingNodes);

                    let port_range = PortRange::Range(
                        self.port_from.unwrap_or(PORT_MIN) as u16,
                        self.port_to.unwrap_or(PORT_MAX) as u16,
                    );

                    let action_sender = self.get_actions_sender()?;

                    let maintain_nodes_args = MaintainNodesArgs {
                        action_sender: action_sender.clone(),
                        antnode_path: self.antnode_path.clone(),
                        connection_mode: self.connection_mode,
                        count: self.nodes_to_start as u16,
                        data_dir_path: Some(self.data_dir_path.clone()),
                        network_id: self.network_id,
                        owner: self.rewards_address.clone(),
                        init_peers_config: self.init_peers_config.clone(),
                        port_range: Some(port_range),
                        rewards_address: self.rewards_address.clone(),
                        run_nat_detection: self.should_we_run_nat_detection(),
                    };

                    debug!("Calling maintain_n_running_nodes");

                    self.node_management
                        .send_task(NodeManagementTask::MaintainNodes {
                            args: maintain_nodes_args,
                        })?;
                }
                StatusActions::StopNodes => {
                    debug!("Got action to stop nodes");
                    if self.lock_registry.is_some() {
                        error!(
                            "Registry is locked ({:?}) Cannot Stop nodes now.",
                            self.lock_registry
                        );
                        return Ok(None);
                    }

                    let running_nodes = self.get_running_nodes();
                    debug!("Setting lock_registry to StoppingNodes");
                    self.lock_registry = Some(LockRegistryState::StoppingNodes);
                    let action_sender = self.get_actions_sender()?;
                    info!("Stopping node service: {running_nodes:?}");

                    self.node_management
                        .send_task(NodeManagementTask::StopNodes {
                            services: running_nodes,
                            action_sender,
                        })?;
                }
                StatusActions::TriggerRewardsAddress => {
                    if self.rewards_address.is_empty() {
                        return Ok(Some(Action::SwitchScene(Scene::StatusRewardsAddressPopUp)));
                    } else {
                        return Ok(None);
                    }
                }
                StatusActions::TriggerNodeLogs => {
                    if let Some(node) = self.items.as_ref().and_then(|items| items.selected_item())
                    {
                        debug!("Got action to open node logs {:?}", node.name);
                        open_logs(Some(node.name.clone()))?;
                    } else {
                        debug!("Got action to open node logs but no node was selected.");
                    }
                }
            },
            Action::OptionsActions(OptionsActions::UpdateNodes) => {
                debug!("Got action to Update Nodes");
                self.load_node_registry_and_update_states()?;
                if self.lock_registry.is_some() {
                    error!(
                        "Registry is locked ({:?}) Cannot Update nodes now. Stop them first.",
                        self.lock_registry
                    );
                    return Ok(None);
                } else {
                    debug!("Lock registry ({:?})", self.lock_registry);
                };
                debug!("Setting lock_registry to UpdatingNodes");
                self.lock_registry = Some(LockRegistryState::UpdatingNodes);
                let action_sender = self.get_actions_sender()?;
                info!("Got action to update nodes");
                let _ = self.update_node_items(Some(NodeStatus::Updating));
                let (service_names, peer_ids) = self.get_service_names_and_peer_ids();

                let upgrade_nodes_args = UpgradeNodesArgs {
                    action_sender,
                    connection_timeout_s: 5,
                    do_not_start: true,
                    custom_bin_path: None,
                    force: false,
                    fixed_interval: Some(FIXED_INTERVAL),
                    peer_ids,
                    provided_env_variables: None,
                    service_names,
                    url: None,
                    version: None,
                };
                self.node_management
                    .send_task(NodeManagementTask::UpgradeNodes {
                        args: upgrade_nodes_args,
                    })?;
            }
            Action::OptionsActions(OptionsActions::ResetNodes) => {
                debug!("Got action to reset nodes");
                if self.lock_registry.is_some() {
                    error!(
                        "Registry is locked ({:?}) Cannot Reset nodes now.",
                        self.lock_registry
                    );
                    return Ok(None);
                }

                debug!("Setting lock_registry to ResettingNodes");
                self.lock_registry = Some(LockRegistryState::ResettingNodes);
                let action_sender = self.get_actions_sender()?;
                info!("Got action to reset nodes");
                self.node_management
                    .send_task(NodeManagementTask::ResetNodes {
                        start_nodes_after_reset: false,
                        action_sender,
                    })?;
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        if !self.active {
            return Ok(());
        }

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
            ConnectionMode::HomeNetwork => "Home Network".to_string(),
            ConnectionMode::UPnP => "UPnP".to_string(),
            ConnectionMode::CustomPorts => format!(
                "Custom Ports  {}-{}",
                self.port_from.unwrap_or(PORT_MIN),
                self.port_to.unwrap_or(PORT_MIN + PORT_ALLOCATION)
            ),
            ConnectionMode::Automatic => "Automatic".to_string(),
        };

        let mut connection_mode_line = vec![Span::styled(
            connection_mode_string,
            Style::default().fg(GHOST_WHITE),
        )];

        if matches!(
            self.connection_mode,
            ConnectionMode::Automatic | ConnectionMode::UPnP
        ) {
            connection_mode_line.push(Span::styled(" (", Style::default().fg(GHOST_WHITE)));

            if self.connection_mode == ConnectionMode::Automatic {
                connection_mode_line.push(Span::styled("UPnP: ", Style::default().fg(GHOST_WHITE)));
            }

            let span = match self.upnp_support {
                UpnpSupport::Supported => {
                    Span::styled("supported", Style::default().fg(EUCALYPTUS))
                }
                UpnpSupport::Unsupported => {
                    Span::styled("disabled / unsupported", Style::default().fg(SIZZLING_RED))
                }
                UpnpSupport::Loading => {
                    Span::styled("loading..", Style::default().fg(LIGHT_PERIWINKLE))
                }
                UpnpSupport::Unknown => {
                    Span::styled("unknown", Style::default().fg(LIGHT_PERIWINKLE))
                }
            };

            connection_mode_line.push(span);

            connection_mode_line.push(Span::styled(")", Style::default().fg(GHOST_WHITE)));
        }

        let connection_mode_row = Row::new(vec![
            Cell::new("Connection".to_string()).fg(GHOST_WHITE),
            Cell::new(Line::from(connection_mode_line)),
        ]);

        let stats_rows = vec![storage_allocated_row, memory_use_row, connection_mode_row];
        let stats_width = [Constraint::Length(5)];
        let column_constraints = [Constraint::Length(23), Constraint::Fill(1)];
        let stats_table = Table::new(stats_rows, stats_width).widths(column_constraints);

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
        let column_constraints = [
            Constraint::Length(23),
            Constraint::Fill(1),
            Constraint::Length(if self.rewards_address.is_empty() {
                41 //TODO: make it dynamic with wallet_not_set
            } else {
                0
            }),
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
        if let Some(ref items) = self.items {
            if items.items.is_empty() || self.rewards_address.is_empty() {
                let line1 = Line::from(vec![
                    Span::styled("Press ", Style::default().fg(LIGHT_PERIWINKLE)),
                    Span::styled("[Ctrl+G] ", Style::default().fg(GHOST_WHITE).bold()),
                    Span::styled("to Add and ", Style::default().fg(LIGHT_PERIWINKLE)),
                    Span::styled("Start Nodes ", Style::default().fg(GHOST_WHITE).bold()),
                    Span::styled("on this device", Style::default().fg(LIGHT_PERIWINKLE)),
                ]);

                let line2 = Line::from(vec![Span::styled(
                    format!(
                        "Each node will use {}GB of storage and a small amount of memory, \
                        CPU, and Network bandwidth. Most computers can run many nodes at once, \
                        but we recommend you add them gradually",
                        GB_PER_NODE
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
                // Node/s block
                let block_nodes = Block::default()
                    .title(Line::from(vec![
                        Span::styled(" Nodes", Style::default().fg(GHOST_WHITE).bold()),
                        Span::styled(
                            format!(" ({}) ", self.nodes_to_start),
                            Style::default().fg(LIGHT_PERIWINKLE),
                        ),
                    ]))
                    .padding(Padding::new(1, 1, 0, 0))
                    .title_style(Style::default().fg(GHOST_WHITE))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(EUCALYPTUS));

                // Split the inner area of the combined block
                let inner_area = block_nodes.inner(layout[2]);

                // Column Widths
                let node_widths = [
                    Constraint::Min(NODE_WIDTH as u16),
                    Constraint::Min(VERSION_WIDTH as u16),
                    Constraint::Min(ATTOS_WIDTH as u16),
                    Constraint::Min(MEMORY_WIDTH as u16),
                    Constraint::Min(MBPS_WIDTH as u16),
                    Constraint::Min(RECORDS_WIDTH as u16),
                    Constraint::Min(PEERS_WIDTH as u16),
                    Constraint::Min(CONNS_WIDTH as u16),
                    Constraint::Min(MODE_WIDTH as u16),
                    Constraint::Min(STATUS_WIDTH as u16),
                    Constraint::Fill(FAILURE_WIDTH as u16),
                    Constraint::Max(SPINNER_WIDTH as u16),
                ];

                // Header
                let header_row = Row::new(vec![
                    Cell::new("Node").fg(COOL_GREY),
                    Cell::new("Version").fg(COOL_GREY),
                    Cell::new("Attos").fg(COOL_GREY),
                    Cell::new("Memory").fg(COOL_GREY),
                    Cell::new(
                        format!("{}{}", " ".repeat(MBPS_WIDTH - "Mbps".len()), "Mbps")
                            .fg(COOL_GREY),
                    ),
                    Cell::new("Recs").fg(COOL_GREY),
                    Cell::new("Peers").fg(COOL_GREY),
                    Cell::new("Conns").fg(COOL_GREY),
                    Cell::new("Mode").fg(COOL_GREY),
                    Cell::new("Status").fg(COOL_GREY),
                    Cell::new("Failure").fg(COOL_GREY),
                    Cell::new(" ").fg(COOL_GREY), // Spinner
                ])
                .style(Style::default().add_modifier(Modifier::BOLD));

                let mut items: Vec<Row> = Vec::new();
                if let Some(ref mut items_table) = self.items {
                    for (i, node_item) in items_table.items.iter_mut().enumerate() {
                        let is_selected = items_table.state.selected() == Some(i);
                        items.push(node_item.render_as_row(i, layout[2], f, is_selected));
                    }
                }

                // Table items
                let table = Table::new(items, node_widths)
                    .header(header_row)
                    .column_spacing(1)
                    .row_highlight_style(Style::default().bg(INDIGO))
                    .highlight_spacing(HighlightSpacing::Always);

                f.render_widget(table, inner_area);

                f.render_widget(block_nodes, layout[2]);
            }
        }

        // ==== Footer =====

        let footer = Footer::default();
        let footer_state = if let Some(ref items) = self.items {
            if !items.items.is_empty() || self.rewards_address.is_empty() {
                if !self.get_running_nodes().is_empty() {
                    &mut NodesToStart::Running
                } else {
                    &mut NodesToStart::Configured
                }
            } else {
                &mut NodesToStart::NotConfigured
            }
        } else {
            &mut NodesToStart::NotConfigured
        };
        f.render_stateful_widget(footer, layout[3], footer_state);

        // ===== Popups =====

        // Error Popup
        if let Some(error_popup) = &self.error_popup {
            if error_popup.is_visible() {
                error_popup.draw_error(f, area);

                return Ok(());
            }
        }

        // Status Popup
        if let Some(registry_state) = &self.lock_registry {
            let popup_text = match registry_state {
                LockRegistryState::StartingNodes => {
                    if self.should_we_run_nat_detection() {
                        vec![
                            Line::raw("Starting nodes..."),
                            Line::raw(""),
                            Line::raw(""),
                            Line::raw("Please wait, performing initial NAT detection"),
                            Line::raw("This may take a couple minutes."),
                        ]
                    } else {
                        // We avoid rendering the popup as we have status lines now
                        return Ok(());
                    }
                }
                LockRegistryState::StoppingNodes => {
                    vec![
                        Line::raw(""),
                        Line::raw(""),
                        Line::raw(""),
                        Line::raw("Stopping nodes..."),
                    ]
                }
                LockRegistryState::ResettingNodes => {
                    vec![
                        Line::raw(""),
                        Line::raw(""),
                        Line::raw(""),
                        Line::raw("Resetting nodes..."),
                    ]
                }
                LockRegistryState::UpdatingNodes => {
                    return Ok(());
                }
            };
            if !popup_text.is_empty() {
                let popup_area = centered_rect_fixed(50, 12, area);
                clear_area(f, popup_area);

                let popup_border = Paragraph::new("").block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Manage Nodes ")
                        .bold()
                        .title_style(Style::new().fg(VIVID_SKY_BLUE))
                        .padding(Padding::uniform(2))
                        .border_style(Style::new().fg(GHOST_WHITE)),
                );

                let centred_area = Layout::new(
                    Direction::Vertical,
                    vec![
                        // border
                        Constraint::Length(2),
                        // our text goes here
                        Constraint::Min(5),
                        // border
                        Constraint::Length(1),
                    ],
                )
                .split(popup_area);
                let text = Paragraph::new(popup_text)
                    .block(Block::default().padding(Padding::horizontal(2)))
                    .wrap(Wrap { trim: false })
                    .alignment(Alignment::Center)
                    .fg(EUCALYPTUS);
                f.render_widget(text, centred_area[1]);

                f.render_widget(popup_border, popup_area);
            }
        }

        Ok(())
    }

    fn handle_key_events(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        debug!("Key received in Status: {:?}", key);
        if let Some(error_popup) = &mut self.error_popup {
            if error_popup.is_visible() {
                error_popup.handle_input(key);
                return Ok(vec![Action::SwitchInputMode(InputMode::Navigation)]);
            }
        }
        Ok(vec![])
    }
}

#[allow(dead_code)]
#[derive(Default, Clone)]
struct StatefulTable<T> {
    state: TableState,
    items: Vec<T>,
    last_selected: Option<usize>,
}

#[allow(dead_code)]
impl<T> StatefulTable<T> {
    fn with_items(items: Vec<T>) -> Self {
        StatefulTable {
            state: TableState::default(),
            items,
            last_selected: None,
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
        self.last_selected = Some(i);
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
        self.last_selected = Some(i);
    }

    fn selected_item(&self) -> Option<&T> {
        self.state
            .selected()
            .and_then(|index| self.items.get(index))
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum NodeStatus {
    #[default]
    Added,
    Running,
    Starting,
    Stopped,
    Removed,
    Updating,
}

impl fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            NodeStatus::Added => write!(f, "Added"),
            NodeStatus::Running => write!(f, "Running"),
            NodeStatus::Starting => write!(f, "Starting"),
            NodeStatus::Stopped => write!(f, "Stopped"),
            NodeStatus::Removed => write!(f, "Removed"),
            NodeStatus::Updating => write!(f, "Updating"),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct NodeItem<'a> {
    name: String,
    version: String,
    attos: usize,
    memory: usize,
    mbps: String,
    records: usize,
    peers: usize,
    connections: usize,
    mode: NodeConnectionMode,
    status: NodeStatus,
    failure: Option<(chrono::DateTime<chrono::Utc>, String)>,
    spinner: Throbber<'a>,
    spinner_state: ThrobberState,
}

impl NodeItem<'_> {
    fn render_as_row(
        &mut self,
        index: usize,
        area: Rect,
        f: &mut Frame<'_>,
        is_selected: bool,
    ) -> Row {
        let mut row_style = if is_selected {
            Style::default().fg(GHOST_WHITE).bg(INDIGO)
        } else {
            Style::default().fg(GHOST_WHITE)
        };
        let mut spinner_state = self.spinner_state.clone();
        match self.status {
            NodeStatus::Running => {
                self.spinner = self
                    .spinner
                    .clone()
                    .throbber_style(Style::default().fg(EUCALYPTUS).add_modifier(Modifier::BOLD))
                    .throbber_set(throbber_widgets_tui::BRAILLE_SIX_DOUBLE)
                    .use_type(throbber_widgets_tui::WhichUse::Spin);
                row_style = if is_selected {
                    Style::default().fg(EUCALYPTUS).bg(INDIGO)
                } else {
                    Style::default().fg(EUCALYPTUS)
                };
            }
            NodeStatus::Starting => {
                self.spinner = self
                    .spinner
                    .clone()
                    .throbber_style(Style::default().fg(EUCALYPTUS).add_modifier(Modifier::BOLD))
                    .throbber_set(throbber_widgets_tui::BOX_DRAWING)
                    .use_type(throbber_widgets_tui::WhichUse::Spin);
            }
            NodeStatus::Stopped => {
                self.spinner = self
                    .spinner
                    .clone()
                    .throbber_style(
                        Style::default()
                            .fg(GHOST_WHITE)
                            .add_modifier(Modifier::BOLD),
                    )
                    .throbber_set(throbber_widgets_tui::BRAILLE_SIX_DOUBLE)
                    .use_type(throbber_widgets_tui::WhichUse::Full);
            }
            NodeStatus::Updating => {
                self.spinner = self
                    .spinner
                    .clone()
                    .throbber_style(
                        Style::default()
                            .fg(GHOST_WHITE)
                            .add_modifier(Modifier::BOLD),
                    )
                    .throbber_set(throbber_widgets_tui::VERTICAL_BLOCK)
                    .use_type(throbber_widgets_tui::WhichUse::Spin);
            }
            _ => {}
        };

        let failure = self.failure.as_ref().map_or_else(
            || "-".to_string(),
            |(_dt, msg)| {
                if self.status == NodeStatus::Stopped {
                    msg.clone()
                } else {
                    "-".to_string()
                }
            },
        );

        let row = vec![
            self.name.clone().to_string(),
            self.version.to_string(),
            format!(
                "{}{}",
                " ".repeat(ATTOS_WIDTH.saturating_sub(self.attos.to_string().len())),
                self.attos.to_string()
            ),
            format!(
                "{}{} MB",
                " ".repeat(MEMORY_WIDTH.saturating_sub(self.memory.to_string().len() + 4)),
                self.memory.to_string()
            ),
            format!(
                "{}{}",
                " ".repeat(MBPS_WIDTH.saturating_sub(self.mbps.to_string().len())),
                self.mbps.to_string()
            ),
            format!(
                "{}{}",
                " ".repeat(RECORDS_WIDTH.saturating_sub(self.records.to_string().len())),
                self.records.to_string()
            ),
            format!(
                "{}{}",
                " ".repeat(PEERS_WIDTH.saturating_sub(self.peers.to_string().len())),
                self.peers.to_string()
            ),
            format!(
                "{}{}",
                " ".repeat(CONNS_WIDTH.saturating_sub(self.connections.to_string().len())),
                self.connections.to_string()
            ),
            self.mode.to_string(),
            self.status.to_string(),
            failure,
        ];
        let throbber_area = Rect::new(area.width - 3, area.y + 2 + index as u16, 1, 1);

        f.render_stateful_widget(self.spinner.clone(), throbber_area, &mut spinner_state);

        Row::new(row).style(row_style)
    }
}
