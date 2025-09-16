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
use crate::node_management::NodeManagement;
use crate::{components::status::NODE_STAT_UPDATE_INTERVAL, node_stats::NodeStats};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
use ant_service_management::{NodeRegistryManager, NodeServiceData, ServiceStatus};
use color_eyre::eyre::Result;
use std::{path::PathBuf, time::Instant};
use throbber_widgets_tui::ThrobberState;
use tracing::{debug, error};

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
    pub upnp_enabled: bool,
    pub port_range: Option<(u32, u32)>,
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
            upnp_enabled: config.upnp_enabled,
            port_range: config.port_range,
            rewards_address: config.rewards_address,
            nodes_to_start: config.nodes_to_start,
            storage_mountpoint: config.storage_mountpoint.clone(),
            available_disk_space_gb: crate::system::get_available_space_b(
                config.storage_mountpoint.as_path(),
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

    pub fn send_selection_update(&self) -> Result<()> {
        if let Some(action_sender) = &self.operations.action_sender {
            let selected_node_status = self
                .items
                .selected_item()
                .map(|node| node.node_display_status);

            let selection_action = Action::NodeTableActions(NodeTableActions::SelectionChanged {
                selected_node_status,
            });

            debug!("Sending selected_node_status={selected_node_status:?}");
            action_sender.send(selection_action)?;
        } else {
            error!("No action_sender available to send SelectionChanged");
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

                // Handle selection before removing the node
                let was_selected = self.items.state.selected() == Some(pos);
                self.items.items.remove(pos);

                // Update selection if the removed node was selected
                if was_selected {
                    if self.items.items.is_empty() {
                        // No nodes left, clear selection
                        if let Err(e) = self.clear_selection() {
                            error!("Failed to clear selection after node removal: {e}");
                        }
                    } else {
                        // Select the first available unlocked node
                        debug!("Re-selecting first unlocked node after removing selected node");
                        self.navigate_first_unlocked();
                    }
                }
            }
        }

        // Ensure spinner states match item count
        self.spinner_states
            .resize_with(self.items.items.len(), ThrobberState::default);

        // If no item is selected but we have items, select the first unlocked one
        if self.items.state.selected().is_none() && !self.items.items.is_empty() {
            debug!("Auto-selecting first unlocked node since no selection exists");
            self.navigate_first_unlocked();
        }

        let running_nodes = self
            .node_services
            .iter()
            .filter(|node| node.status == ServiceStatus::Running)
            .count() as u64;

        if running_nodes != self.nodes_to_start {
            debug!(
                "Sync detected running node count change: {} -> {running_nodes}",
                self.nodes_to_start
            );
            self.nodes_to_start = running_nodes;

            if let Some(action_sender) = &self.operations.action_sender {
                if let Err(err) = action_sender.send(Action::StoreRunningNodeCount(running_nodes)) {
                    error!(
                        "Failed to propagate updated running node count ({running_nodes}): {err}"
                    );
                }
            } else {
                debug!(
                    "Action sender not registered yet; skipping propagation of running node count"
                );
            }
        }

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

    pub fn sync_upnp_setting(&mut self, upnp_enabled: bool) {
        self.upnp_enabled = upnp_enabled;
        debug!("NodeTableState: Synced upnp_enabled to {upnp_enabled:?}");
    }

    pub fn sync_port_range(&mut self, port_range: Option<(u32, u32)>) {
        self.port_range = port_range;
        debug!("NodeTableState: Synced port_range to {port_range:?}");
    }

    /// Find the index of the next unlocked node, wrapping around if needed
    fn find_next_unlocked_index(&self) -> Option<usize> {
        if self.items.items.is_empty() {
            return None;
        }

        let current = self.items.state.selected().unwrap_or(0);
        let total_items = self.items.items.len();

        // Try to find the next unlocked node
        for i in 1..=total_items {
            let next_index = (current + i) % total_items;
            if !self.items.items[next_index].is_locked() {
                return Some(next_index);
            }
        }
        None // All nodes are locked
    }

    /// Find the index of the previous unlocked node, wrapping around if needed
    fn find_previous_unlocked_index(&self) -> Option<usize> {
        if self.items.items.is_empty() {
            return None;
        }

        let current = self.items.state.selected().unwrap_or(0);
        let total_items = self.items.items.len();

        // Try to find the previous unlocked node
        for i in 1..=total_items {
            let prev_index = (current + total_items - i) % total_items;
            if !self.items.items[prev_index].is_locked() {
                return Some(prev_index);
            }
        }
        None // All nodes are locked
    }

    /// Find the index of the first unlocked node
    fn find_first_unlocked_index(&self) -> Option<usize> {
        self.items
            .items
            .iter()
            .enumerate()
            .find(|(_, item)| !item.is_locked())
            .map(|(index, _)| index)
    }

    /// Find the index of the last unlocked node  
    fn find_last_unlocked_index(&self) -> Option<usize> {
        self.items
            .items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, item)| !item.is_locked())
            .map(|(index, _)| index)
    }

    /// Navigate to the next unlocked node, wrapping around if needed
    pub fn navigate_next_unlocked(&mut self) {
        let next_index = self.find_next_unlocked_index();
        if let Err(e) = self.select_node_if_unlocked(next_index) {
            error!("Failed to navigate to next unlocked node: {e}");
        }
    }

    /// Navigate to the previous unlocked node, wrapping around if needed
    pub fn navigate_previous_unlocked(&mut self) {
        let prev_index = self.find_previous_unlocked_index();
        if let Err(e) = self.select_node_if_unlocked(prev_index) {
            error!("Failed to navigate to previous unlocked node: {e}");
        }
    }

    /// Navigate to the first unlocked node
    pub fn navigate_first_unlocked(&mut self) {
        let first_index = self.find_first_unlocked_index();
        if let Err(e) = self.select_node_if_unlocked(first_index) {
            error!("Failed to navigate to first unlocked node: {e}");
        }
    }

    /// Navigate to the last unlocked node
    pub fn navigate_last_unlocked(&mut self) {
        let last_index = self.find_last_unlocked_index();
        if let Err(e) = self.select_node_if_unlocked(last_index) {
            error!("Failed to navigate to last unlocked node: {e}");
        }
    }

    /// Core selection method - handles notification automatically
    fn set_selection(&mut self, index: Option<usize>) -> Result<()> {
        let old_selection = self.items.state.selected();
        self.items.state.select(index);
        self.items.last_selected = index;

        // Only send notification if selection actually changed
        if old_selection != index {
            self.send_selection_update()?;
        }
        Ok(())
    }

    /// Lock-aware selection - only selects if node is unlocked or clears if locked
    pub fn select_node_if_unlocked(&mut self, index: Option<usize>) -> Result<()> {
        match index {
            Some(idx) if idx < self.items.items.len() => {
                if !self.items.items[idx].is_locked() {
                    self.set_selection(Some(idx))
                } else {
                    // Node is locked, clear selection
                    self.set_selection(None)
                }
            }
            None => self.set_selection(None),
            _ => Ok(()), // Invalid index, do nothing
        }
    }

    pub fn clear_selection(&mut self) -> Result<()> {
        self.set_selection(None)
    }

    pub fn try_clear_selection_if_locked(&mut self) {
        if let Some(selected) = self.items.state.selected()
            && self.items.items[selected].is_locked()
            && let Err(e) = self.clear_selection()
        {
            error!("Failed to clear selection for locked node: {e}");
        }
    }
}

#[derive(Clone)]
pub struct NodeTableConfig {
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub antnode_path: Option<PathBuf>,
    pub data_dir_path: PathBuf,
    pub upnp_enabled: bool,
    pub port_range: Option<(u32, u32)>,
    pub rewards_address: Option<EvmAddress>,
    pub nodes_to_start: u64,
    pub storage_mountpoint: PathBuf,
    pub registry_path_override: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, NodeTableActions};
    use crate::components::node_table::NodeDisplayStatus;
    use crate::test_utils::MockNodeRegistry;
    use ant_evm::EvmAddress;
    use ant_service_management::ServiceStatus;
    use color_eyre::Result;
    use std::{fs, str::FromStr};
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    async fn build_state_with_statuses(
        statuses: &[ServiceStatus],
    ) -> Result<(NodeTableState, MockNodeRegistry, TempDir)> {
        let mut mock_registry = MockNodeRegistry::empty()?;

        for (index, status) in statuses.iter().enumerate() {
            let mut node =
                mock_registry.create_test_node_service_data(index as u64, status.clone());
            node.status = status.clone();
            mock_registry.add_node(node)?;
        }

        let temp_dir = tempfile::tempdir()?;
        let data_dir = temp_dir.path().join("data");
        fs::create_dir_all(&data_dir)?;

        let config = NodeTableConfig {
            network_id: Some(1),
            init_peers_config: InitialPeersConfig::default(),
            antnode_path: None,
            data_dir_path: data_dir,
            upnp_enabled: true,
            port_range: None,
            rewards_address: None,
            nodes_to_start: 0,
            storage_mountpoint: crate::system::get_primary_mount_point(),
            registry_path_override: Some(mock_registry.get_registry_path().clone()),
        };

        let state = NodeTableState::new(config).await?;
        Ok((state, mock_registry, temp_dir))
    }

    #[tokio::test]
    async fn sync_updates_running_node_count_from_registry() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running, ServiceStatus::Stopped]).await?;
        assert_eq!(state.nodes_to_start, 1);

        let (tx, mut rx) = mpsc::unbounded_channel();
        state.operations.action_sender = Some(tx);

        let mut updated_services = state.node_services.clone();
        if let Some(node) = updated_services
            .iter_mut()
            .find(|node| node.status == ServiceStatus::Stopped)
        {
            node.status = ServiceStatus::Running;
        }

        state.sync_node_service_data(&updated_services);
        assert_eq!(state.nodes_to_start, 2);

        match rx.try_recv() {
            Ok(Action::StoreRunningNodeCount(count)) => assert_eq!(count, 2),
            Ok(other) => panic!("Unexpected action received: {other:?}"),
            Err(err) => panic!("No action received for updated running count: {err:?}"),
        }

        state.sync_node_service_data(&updated_services);
        assert!(rx.try_recv().is_err(), "Did not expect duplicate action");

        Ok(())
    }

    #[tokio::test]
    async fn sync_removes_nodes_and_reselects() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running, ServiceStatus::Running]).await?;

        state.select_node_if_unlocked(Some(0))?;
        let removed_service = state.items.items[0].service_name.clone();
        let remaining_service = state.items.items[1].service_name.clone();

        let mut updated_services = state.node_services.clone();
        if let Some(node) = updated_services
            .iter_mut()
            .find(|node| node.service_name == removed_service)
        {
            node.status = ServiceStatus::Removed;
        }

        state.sync_node_service_data(&updated_services);

        assert_eq!(state.items.items.len(), 1);
        assert_eq!(state.items.items[0].service_name, remaining_service);
        assert_eq!(state.items.state.selected(), Some(0));
        Ok(())
    }

    #[tokio::test]
    async fn sync_updates_nodes_to_start_without_action_sender() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running, ServiceStatus::Stopped]).await?;
        assert_eq!(state.nodes_to_start, 1);

        let mut updated_services = state.node_services.clone();
        if let Some(node) = updated_services
            .iter_mut()
            .find(|node| node.status == ServiceStatus::Stopped)
        {
            node.status = ServiceStatus::Running;
        }

        state.sync_node_service_data(&updated_services);
        assert_eq!(state.nodes_to_start, 2);
        Ok(())
    }

    #[tokio::test]
    async fn navigation_skips_locked_nodes() -> Result<()> {
        let (mut state, _registry, _temp_dir) = build_state_with_statuses(&[
            ServiceStatus::Running,
            ServiceStatus::Running,
            ServiceStatus::Running,
        ])
        .await?;

        state.select_node_if_unlocked(Some(0))?;
        state.items.items[1].lock();
        state.navigate_next_unlocked();
        assert_eq!(state.items.state.selected(), Some(2));

        for item in state.items.items.iter_mut() {
            item.lock();
        }
        state.navigate_next_unlocked();
        assert_eq!(state.items.state.selected(), None);

        Ok(())
    }

    #[tokio::test]
    async fn navigate_first_and_last_unlocked() -> Result<()> {
        let (mut state, _registry, _temp_dir) = build_state_with_statuses(&[
            ServiceStatus::Running,
            ServiceStatus::Running,
            ServiceStatus::Running,
        ])
        .await?;

        state.clear_selection()?;
        state.items.items[0].lock();
        state.items.items[1].lock();
        state.navigate_first_unlocked();
        assert_eq!(state.items.state.selected(), Some(2));

        state.items.items[2].lock();
        state.items.items[0].unlock();
        state.items.items[1].unlock();
        state.clear_selection()?;
        state.navigate_last_unlocked();
        assert_eq!(state.items.state.selected(), Some(1));
        Ok(())
    }

    #[tokio::test]
    async fn sync_adds_new_nodes_and_extends_spinners() -> Result<()> {
        let (mut state, registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running]).await?;

        let original_len = state.items.items.len();
        let mut updated_services = state.node_services.clone();
        let new_node =
            registry.create_test_node_service_data(original_len as u64, ServiceStatus::Running);
        updated_services.push(new_node.clone());

        state.sync_node_service_data(&updated_services);

        assert_eq!(state.items.items.len(), original_len + 1);
        assert!(
            state
                .items
                .items
                .iter()
                .any(|item| item.service_name == new_node.service_name)
        );
        assert_eq!(state.spinner_states.len(), state.items.items.len());
        assert_eq!(state.nodes_to_start, (original_len + 1) as u64);
        Ok(())
    }

    #[tokio::test]
    async fn sync_removal_shrinks_spinner_states() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running, ServiceStatus::Running]).await?;
        assert_eq!(state.spinner_states.len(), 2);

        let removed_service = state.items.items[0].service_name.clone();
        let mut updated_services = state.node_services.clone();
        if let Some(node) = updated_services
            .iter_mut()
            .find(|node| node.service_name == removed_service)
        {
            node.status = ServiceStatus::Removed;
        }

        state.sync_node_service_data(&updated_services);
        assert_eq!(state.spinner_states.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn sync_updates_existing_node_status_and_display() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running]).await?;

        let service_name = state.items.items[0].service_name.clone();
        state.items.items[0].lock_for_operation(NodeDisplayStatus::Starting);

        let mut updated_services = state.node_services.clone();
        if let Some(node) = updated_services
            .iter_mut()
            .find(|node| node.service_name == service_name)
        {
            node.status = ServiceStatus::Stopped;
        }

        state.sync_node_service_data(&updated_services);

        let updated_item = state
            .items
            .items
            .iter()
            .find(|item| item.service_name == service_name)
            .expect("missing node after sync");
        assert_eq!(updated_item.service_status, ServiceStatus::Stopped);
        assert_eq!(updated_item.node_display_status, NodeDisplayStatus::Stopped);
        Ok(())
    }

    #[tokio::test]
    async fn send_state_and_selection_update_with_sender() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running]).await?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.operations.action_sender = Some(tx);

        state.send_state_update()?;
        match rx.try_recv() {
            Ok(Action::NodeTableActions(NodeTableActions::StateChanged {
                node_count,
                has_running_nodes,
                has_nodes,
            })) => {
                assert_eq!(node_count, 1);
                assert!(has_running_nodes);
                assert!(has_nodes);
            }
            other => panic!("Unexpected action: {other:?}"),
        }

        state.select_node_if_unlocked(Some(0))?;
        state.send_selection_update()?;
        match rx.try_recv() {
            Ok(Action::NodeTableActions(NodeTableActions::SelectionChanged {
                selected_node_status,
            })) => {
                assert_eq!(selected_node_status, Some(NodeDisplayStatus::Running));
            }
            other => panic!("Unexpected action: {other:?}"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn sync_configuration_helpers() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running]).await?;

        state.sync_nodes_to_start(42);
        assert_eq!(state.nodes_to_start, 42);

        let address = EvmAddress::from_str("0x1234567890123456789012345678901234567890")?;
        state.sync_rewards_address(Some(address));
        assert_eq!(state.rewards_address, Some(address));

        state.sync_upnp_setting(false);
        assert!(!state.upnp_enabled);

        state.sync_port_range(Some((20000, 20100)));
        assert_eq!(state.port_range, Some((20000, 20100)));

        Ok(())
    }

    #[tokio::test]
    async fn try_clear_selection_if_locked_removes_selection() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running]).await?;

        state.select_node_if_unlocked(Some(0))?;
        state.items.items[0].lock();
        state.try_clear_selection_if_locked();
        assert_eq!(state.items.state.selected(), None);
        Ok(())
    }

    #[tokio::test]
    async fn select_node_if_unlocked_skips_locked_target() -> Result<()> {
        let (mut state, _registry, _temp_dir) =
            build_state_with_statuses(&[ServiceStatus::Running, ServiceStatus::Running]).await?;

        state.items.items[1].lock();
        state.select_node_if_unlocked(Some(1))?;
        assert_eq!(state.items.state.selected(), None);
        Ok(())
    }
}
