// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    node_item::{NodeDisplayStatus, NodeItem},
    operations::NodeOperations,
    table_state::StatefulTable,
};
use crate::action::{Action, NodeTableActions};
use crate::{components::status::NODE_STAT_UPDATE_INTERVAL, node_stats::NodeStats};
use crate::{connection_mode::ConnectionMode, node_management::NodeManagement};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
use ant_service_management::{NodeRegistryManager, NodeServiceData, ServiceStatus};
use color_eyre::eyre::Result;
use std::{path::PathBuf, time::Instant};
use throbber_widgets_tui::ThrobberState;
use tracing::debug;

pub struct NodeTableState {
    // Node data
    pub items: StatefulTable<NodeItem>,
    pub node_services: Vec<NodeServiceData>,
    pub node_registry: NodeRegistryManager,
    pub operations: NodeOperations,

    // Stats
    pub node_stats_last_update: Instant,

    // Configuration
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub antnode_path: Option<PathBuf>,
    pub data_dir_path: PathBuf,
    pub connection_mode: ConnectionMode,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
    pub rewards_address: Option<EvmAddress>,
    pub nodes_to_start: u64,

    // Storage info (for validation)
    pub storage_mountpoint: PathBuf,
    pub available_disk_space_gb: u64,

    // UI state
    pub spinner_states: Vec<ThrobberState>,
}

impl NodeTableState {
    pub async fn new(config: NodeTableConfig) -> Result<Self> {
        let registry_path = if let Some(override_path) = &config.registry_path_override {
            override_path.clone()
        } else {
            ant_node_manager::config::get_node_registry_path()?
        };
        let node_registry = NodeRegistryManager::load(&registry_path).await?;
        let node_services = node_registry.get_node_service_data().await;
        let node_management = NodeManagement::new(node_registry.clone())?;

        let mut state = Self {
            items: StatefulTable::with_items(vec![]),
            node_services: node_services.clone(),
            node_registry,
            operations: NodeOperations::new(node_management),
            node_stats_last_update: Instant::now(),
            network_id: config.network_id,
            init_peers_config: config.init_peers_config,
            antnode_path: config.antnode_path,
            data_dir_path: config.data_dir_path,
            connection_mode: config.connection_mode,
            port_from: config.port_from,
            port_to: config.port_to,
            rewards_address: config.rewards_address,
            nodes_to_start: config.nodes_to_start,
            storage_mountpoint: config.storage_mountpoint.clone(),
            available_disk_space_gb: crate::system::get_available_space_b(
                &config.storage_mountpoint,
            )? / crate::components::popup::manage_nodes::GB,
            spinner_states: vec![],
        };

        // Populate the UI table items from the loaded node services
        // This ensures that nodes loaded from the registry are immediately visible in the UI
        state.sync_node_service_data(&node_services);

        Ok(state)
    }

    pub fn get_running_nodes(&self) -> Vec<String> {
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

    pub fn get_node_item_mut(&mut self, service_name: &str) -> Option<&mut NodeItem> {
        self.items
            .items
            .iter_mut()
            .find(|item| item.service_name == service_name)
    }

    /// Tries to trigger the update of node stats if the last update was more than `NODE_STAT_UPDATE_INTERVAL` ago.
    /// The result is sent via the StatusActions::NodesStatsObtained action.
    pub fn try_update_node_stats(&mut self, force_update: bool) -> Result<()> {
        if self.node_stats_last_update.elapsed() > NODE_STAT_UPDATE_INTERVAL || force_update {
            self.node_stats_last_update = Instant::now();

            if let Some(action_sender) = &self.operations.action_sender {
                crate::node_stats::NodeStats::fetch_all_node_stats(
                    &self.node_services,
                    action_sender.clone(),
                );
            }
        }
        Ok(())
    }

    pub fn send_state_update(&self) -> Result<()> {
        if let Some(action_sender) = &self.operations.action_sender {
            let node_count = self.items.items.len() as u64;
            let has_running_nodes = !self.get_running_nodes().is_empty();
            let has_nodes = !self.items.items.is_empty();

            let state_action = Action::NodeTableActions(NodeTableActions::StateChanged {
                node_count,
                has_running_nodes,
                has_nodes,
            });

            action_sender.send(state_action)?;
        } else {
            error!("NodeTableState::send_state_update - No action_sender available");
        }
        Ok(())
    }

    pub fn sync_node_service_data(&mut self, all_nodes_data: &[NodeServiceData]) {
        self.node_services = all_nodes_data.to_vec();
        let mut removed_nodes = vec![];

        // Update existing items or add new ones
        for node_data in all_nodes_data {
            if let Some(existing_item) = self
                .items
                .items
                .iter_mut()
                .find(|item| item.service_name == node_data.service_name)
            {
                if node_data.status == ServiceStatus::Removed {
                    removed_nodes.push(existing_item.service_name.clone());
                    continue;
                }

                // if no status change, then the sync node service data is for a different node, so skip here.
                if node_data.status == existing_item.service_status {
                    continue;
                }

                // Even if the node item is locked, we can still update its display status to the latest one obtained
                // from the registry, so that the UI reflects the true state of the node.
                // NodeManagementResponse will do the unlocking later.
                let node_display_status = NodeDisplayStatus::from(&node_data.status);
                existing_item.service_status = node_data.status.clone();
                existing_item.version = node_data.version.clone();
                existing_item.node_display_status = node_display_status;
            } else {
                if node_data.status == ServiceStatus::Removed {
                    continue;
                }
                let new_item = NodeItem {
                    service_name: node_data.service_name.clone(),
                    service_status: node_data.status.clone(),
                    node_display_status: NodeDisplayStatus::from(&node_data.status),
                    version: node_data.version.clone(),
                    rewards_wallet_balance: 0,
                    memory: 0,
                    mbps: 0.to_string(),
                    records: 0,
                    peers: 0,
                    connections: 0,
                    locked: false,
                    // todo remove this field completely
                    mode: crate::connection_mode::NodeConnectionMode::Manual,
                    failure: None,
                };
                debug!(
                    "Registry sync: Adding new node {} with status {:?}",
                    new_item.service_name, new_item.node_display_status
                );
                self.items.items.push(new_item);
            }
        }

        // Remove nodes that are no longer present in the registry or marked as Removed
        for service_name in removed_nodes {
            if let Some(pos) = self
                .items
                .items
                .iter()
                .position(|item| item.service_name == service_name)
            {
                debug!(
                    "Registry sync: Removing node {service_name} as it has ServiceStatus::Removed ",
                );
                self.items.items.remove(pos);
            }
        }

        // Ensure spinner states match item count
        self.spinner_states
            .resize_with(self.items.items.len(), ThrobberState::default);

        debug!(
            "Node state updated. Node count changed from {} to {}",
            self.items.items.len(),
            all_nodes_data.len()
        );
    }

    // update the values inside node items
    pub fn sync_node_stats(&mut self, node_stats: NodeStats) {
        for stats in node_stats.individual_stats {
            if let Some(item) = self
                .items
                .items
                .iter_mut()
                .find(|item| item.service_name == stats.service_name)
            {
                item.rewards_wallet_balance = stats.rewards_wallet_balance;
                item.memory = stats.memory_usage_mb;
                item.mbps = format!(
                    "↓{:0>5.0} ↑{:0>5.0}",
                    (stats.bandwidth_inbound_rate * 8) as f64 / 1_000_000.0,
                    (stats.bandwidth_outbound_rate * 8) as f64 / 1_000_000.0,
                );
                item.records = stats.max_records;
                item.connections = stats.connections;
            }
        }
        debug!("NodeTableState: Synced node items with the node stats");
    }

    pub fn sync_rewards_address(&mut self, rewards_address: Option<EvmAddress>) {
        self.rewards_address = rewards_address;
        debug!("NodeTableState: Synced rewards_address to {rewards_address:?}");
    }

    pub fn sync_nodes_to_start(&mut self, nodes_to_start: u64) {
        self.nodes_to_start = nodes_to_start;
        debug!("NodeTableState: Synced nodes_to_start to {nodes_to_start}");
    }

    pub fn sync_connection_mode(&mut self, connection_mode: ConnectionMode) {
        self.connection_mode = connection_mode;
        debug!("NodeTableState: Synced connection_mode to {connection_mode:?}");
    }

    pub fn sync_port_range(&mut self, port_from: Option<u32>, port_to: Option<u32>) {
        self.port_from = port_from;
        self.port_to = port_to;
        debug!("NodeTableState: Synced port_range to {port_from:?}-{port_to:?}");
    }

    /// Navigate to the next unlocked node, wrapping around if needed
    pub fn navigate_next_unlocked(&mut self) {
        if self.items.items.is_empty() {
            return;
        }

        let current = self.items.state.selected().unwrap_or(0);
        let total_items = self.items.items.len();

        // Try to find the next unlocked node
        for i in 1..=total_items {
            let next_index = (current + i) % total_items;
            if !self.items.items[next_index].is_locked() {
                self.items.state.select(Some(next_index));
                self.items.last_selected = Some(next_index);
                return;
            }
        }

        // If all nodes are locked, clear selection
        self.clear_selection();
    }

    /// Navigate to the previous unlocked node, wrapping around if needed
    pub fn navigate_previous_unlocked(&mut self) {
        if self.items.items.is_empty() {
            return;
        }

        let current = self.items.state.selected().unwrap_or(0);
        let total_items = self.items.items.len();

        // Try to find the previous unlocked node
        for i in 1..=total_items {
            let prev_index = (current + total_items - i) % total_items;
            if !self.items.items[prev_index].is_locked() {
                self.items.state.select(Some(prev_index));
                self.items.last_selected = Some(prev_index);
                return;
            }
        }

        // If all nodes are locked, clear selection
        self.clear_selection();
    }

    /// Navigate to the first unlocked node
    pub fn navigate_first_unlocked(&mut self) {
        if self.items.items.is_empty() {
            return;
        }

        for (index, item) in self.items.items.iter().enumerate() {
            if !item.is_locked() {
                self.items.state.select(Some(index));
                self.items.last_selected = Some(index);
                debug!("Navigate: Selected first unlocked node at index {index}",);
                return;
            }
        }

        // If all nodes are locked, clear selection
        self.clear_selection();
    }

    /// Navigate to the last unlocked node
    pub fn navigate_last_unlocked(&mut self) {
        if self.items.items.is_empty() {
            return;
        }

        for (index, item) in self.items.items.iter().enumerate().rev() {
            if !item.is_locked() {
                self.items.state.select(Some(index));
                self.items.last_selected = Some(index);
                debug!("Navigate: Selected last unlocked node at index {index}",);
                return;
            }
        }

        // If all nodes are locked, clear selection
        self.clear_selection();
    }

    pub fn clear_selection(&mut self) {
        self.items.state.select(None);
        self.items.last_selected = None;
    }
}

#[derive(Clone)]
pub struct NodeTableConfig {
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub antnode_path: Option<PathBuf>,
    pub data_dir_path: PathBuf,
    pub connection_mode: ConnectionMode,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
    pub rewards_address: Option<EvmAddress>,
    pub nodes_to_start: u64,
    pub storage_mountpoint: PathBuf,
    pub registry_path_override: Option<PathBuf>,
}
