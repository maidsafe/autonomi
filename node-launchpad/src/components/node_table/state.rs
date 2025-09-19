// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    lifecycle::{
        CommandKind, DesiredNodeState, LifecycleState, NodeId, NodeMetrics, NodeViewModel,
        RegistryNode, TransitionEntry, build_view_models,
    },
    operations::NodeOperations,
    table_state::StatefulTable,
};
use crate::node_management::NodeManagement;
use crate::{action::Action, components::node_table::operations::NodeOperationsConfig};
use crate::{components::status::NODE_STAT_UPDATE_INTERVAL, node_stats::AggregatedNodeStats};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
use ant_service_management::{
    NodeRegistryManager, NodeServiceData, ServiceStatus, metric::ReachabilityStatusValues,
};
use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::{path::PathBuf, time::Instant};
use throbber_widgets_tui::ThrobberState;
use tracing::{debug, error};

pub struct NodeTableState {
    /// The manager for the node registry on disk. This struct is an Arc<RwLock<>> internally and is automatically kept
    /// in sync with the registry file on disk. This is done with the help of a file watcher.
    pub node_registry_manager: NodeRegistryManager,
    /// Operations on the nodes are performed by calling ant-node-manager lib APIs via this struct.
    pub operations: NodeOperations,
    /// Configuration for the node operations.
    pub operations_config: NodeOperationsConfig,

    pub controller: NodeStateController,

    // Stats
    pub node_stats_last_update: Instant,

    // UI state
    pub spinner_states: Vec<ThrobberState>,
    pub last_reported_running_count: u64,
}

#[derive(Clone, Debug)]
pub struct NodeState {
    pub registry: Option<RegistryNode>,
    pub desired: DesiredNodeState,
    pub transition: Option<TransitionEntry>,
    pub is_provisioning: bool,
    pub metrics: NodeMetrics,
    pub reachability: ReachabilityStatusValues,
    pub bandwidth_totals: (u64, u64),
}

impl Default for NodeState {
    fn default() -> Self {
        Self {
            registry: None,
            desired: DesiredNodeState::FollowCluster,
            transition: None,
            is_provisioning: false,
            metrics: NodeMetrics::default(),
            reachability: ReachabilityStatusValues::default(),
            bandwidth_totals: (0, 0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum NavigationDirection {
    Up(usize),
    Down(usize),
    First,
    Last,
}

/// Controller responsible for reconciling registry snapshots, user intent, and transitions.
#[derive(Debug)]
pub struct NodeStateController {
    pub nodes: BTreeMap<NodeId, NodeState>,
    pub desired_running_count: u64,
    pub locked_nodes: BTreeSet<NodeId>,
    pub view: StatefulTable<NodeViewModel>,
}

impl Default for NodeStateController {
    fn default() -> Self {
        Self {
            view: StatefulTable::with_items(vec![]),
            nodes: BTreeMap::new(),
            desired_running_count: 0,
            locked_nodes: BTreeSet::new(),
        }
    }
}

impl NodeStateController {
    pub fn with_services(services: &[NodeServiceData]) -> Self {
        let mut controller = Self::default();
        controller.apply_registry_services(services);
        controller.refresh_view();
        controller
    }

    fn apply_registry_services(&mut self, services: &[NodeServiceData]) {
        let mut seen = BTreeSet::new();
        for service in services {
            let registry_node = RegistryNode {
                service_name: service.service_name.clone(),
                metrics_port: service.metrics_port,
                status: service.status.clone(),
                reachability_progress: service.reachability_progress.clone(),
                last_failure: service.last_critical_failure.clone(),
                version: service.version.clone(),
            };

            let entry = self.nodes.entry(service.service_name.clone()).or_default();
            entry.registry = Some(registry_node);
            entry.is_provisioning = false;
            seen.insert(service.service_name.clone());
        }

        for (id, node_state) in self.nodes.iter_mut() {
            if !seen.contains(id) {
                node_state.registry = None;
            }
        }

        self.nodes.retain(|id, state| {
            state.registry.is_some()
                || state.is_provisioning
                || state.transition.is_some()
                || !matches!(state.desired, DesiredNodeState::FollowCluster)
                || self.locked_nodes.contains(id)
        });
        debug!("Applied registry services, current controller: {self:?}");
    }

    pub fn refresh_view(&mut self) {
        let selected = self.view.state.selected();
        let models = build_view_models(&self.nodes, &self.locked_nodes);
        let mut table = StatefulTable::with_items(models);
        if let Some(selected_index) = selected
            && !table.items.is_empty()
        {
            let index = selected_index.min(table.items.len().saturating_sub(1));
            table.state.select(Some(index));
            table.last_selected = Some(index);
        }
        self.view = table;
    }

    pub fn update_registry(&mut self, services: &[NodeServiceData]) {
        self.apply_registry_services(services);
        self.reconcile_transitions();
        self.refresh_view();
    }

    pub fn update_desired_running_count(&mut self, count: u64) {
        self.desired_running_count = count;
        self.refresh_view();
    }

    pub fn mark_transition(&mut self, id: &str, command: CommandKind) {
        let entry = self.nodes.entry(id.to_string()).or_default();
        entry.transition = Some(TransitionEntry {
            command,
            started_at: Instant::now(),
        });
        if matches!(command, CommandKind::Add) && entry.registry.is_none() {
            entry.is_provisioning = true;
        }
        self.locked_nodes.insert(id.to_string());
        self.refresh_view();
    }

    pub fn clear_transition(&mut self, id: &str) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.transition = None;
        }
        self.locked_nodes.remove(id);
        self.refresh_view();
    }

    pub fn clear_transitions_by_command(&mut self, command: CommandKind) {
        let mut to_clear = Vec::new();
        for (id, node) in self.nodes.iter() {
            if node
                .transition
                .as_ref()
                .is_some_and(|entry| entry.command == command)
            {
                to_clear.push(id.clone());
            }
        }
        let had_entries = !to_clear.is_empty();
        for id in to_clear {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.transition = None;
            }
            self.locked_nodes.remove(&id);
        }
        if had_entries {
            self.refresh_view();
        }
    }

    pub fn set_node_target(&mut self, id: &str, state: DesiredNodeState) {
        let entry = self.nodes.entry(id.to_string()).or_default();
        entry.desired = state;
        if !matches!(state, DesiredNodeState::Remove) {
            entry.is_provisioning = false;
        }
        if matches!(state, DesiredNodeState::FollowCluster)
            && entry.transition.is_none()
            && entry.registry.is_none()
            && !entry.is_provisioning
            && !self.locked_nodes.contains(id)
        {
            self.nodes.remove(id);
        }
        self.refresh_view();
    }

    pub fn items(&self) -> &[NodeViewModel] {
        &self.view.items
    }

    pub fn selected_item(&self) -> Option<&NodeViewModel> {
        self.view.selected_item()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.view.state.selected()
    }

    pub fn running_nodes(&self) -> Vec<RegistryNode> {
        self.nodes
            .values()
            .filter_map(|state| {
                state
                    .registry
                    .as_ref()
                    .filter(|node| node.status == ServiceStatus::Running)
                    .cloned()
            })
            .collect()
    }

    pub fn running_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|state| {
                state
                    .registry
                    .as_ref()
                    .is_some_and(|node| node.status == ServiceStatus::Running)
            })
            .count()
    }

    fn reconcile_transitions(&mut self) {
        let mut completed = Vec::new();
        for (id, node) in self.nodes.iter() {
            if self.locked_nodes.contains(id) {
                continue;
            }
            let Some(entry) = node.transition.as_ref() else {
                continue;
            };
            let registry_state = node.registry.as_ref();
            let done = match entry.command {
                CommandKind::Start => matches!(
                    registry_state.map(|n| &n.status),
                    Some(ServiceStatus::Running)
                ),
                CommandKind::Maintain => matches!(
                    registry_state.map(|n| n.status.clone()),
                    None | Some(
                        ServiceStatus::Running | ServiceStatus::Stopped | ServiceStatus::Removed
                    )
                ),
                CommandKind::Stop => !matches!(
                    registry_state.map(|n| &n.status),
                    Some(ServiceStatus::Running)
                ),
                CommandKind::Add => registry_state.is_some(),
                CommandKind::Remove => {
                    registry_state.is_none()
                        || matches!(
                            registry_state.map(|n| &n.status),
                            Some(ServiceStatus::Removed)
                        )
                }
            };

            if done {
                completed.push(id.clone());
            }
        }

        for id in completed {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.transition = None;
            }
            self.locked_nodes.remove(&id);
        }
    }
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

        let mut controller = NodeStateController::with_services(&node_services);
        controller.update_desired_running_count(config.nodes_to_start);

        let operations = NodeOperations::new(node_management);
        let operations_config = NodeOperationsConfig {
            available_disk_space_gb: crate::system::get_available_space_b(
                config.storage_mountpoint.as_path(),
            )? / crate::components::popup::manage_nodes::GB,
            storage_mountpoint: config.storage_mountpoint.clone(),
            rewards_address: config.rewards_address,
            nodes_to_start: config.nodes_to_start,
            antnode_path: config.antnode_path,
            upnp_enabled: config.upnp_enabled,
            data_dir_path: config.data_dir_path,
            network_id: config.network_id,
            init_peers_config: config.init_peers_config,
            port_range: config.port_range,
        };
        let mut state = Self {
            node_registry_manager: node_registry,
            operations,
            operations_config,
            controller,
            node_stats_last_update: Instant::now(),
            spinner_states: vec![],
            last_reported_running_count: 0,
        };

        // Populate the UI table items from the loaded node services
        // This ensures that nodes loaded from the registry are immediately visible in the UI
        state.sync_node_service_data(&node_services);

        Ok(state)
    }

    /// Tries to fetch the node stats the last update was more than `NODE_STAT_UPDATE_INTERVAL` ago.
    /// The result is sent via the StatusActions::NodesStatsObtained action.
    pub fn try_fetch_node_stats(&mut self, force_update: bool) -> Result<()> {
        if self.node_stats_last_update.elapsed() > NODE_STAT_UPDATE_INTERVAL || force_update {
            self.node_stats_last_update = Instant::now();

            if let Some(action_sender) = &self.operations.action_sender {
                crate::node_stats::AggregatedNodeStats::fetch_aggregated_node_stats(
                    self.controller.running_nodes(),
                    action_sender.clone(),
                );
            }
        }
        Ok(())
    }

    /// Synchronise controller with registry snapshot and reconcile transitions.
    pub fn sync_node_service_data(&mut self, all_nodes_data: &[NodeServiceData]) {
        self.controller.update_registry(all_nodes_data);
        self.controller
            .update_desired_running_count(self.operations_config.nodes_to_start);

        let view_len = self.controller.view.items.len();
        self.spinner_states
            .resize_with(view_len, ThrobberState::default);

        if self.controller.selected_index().is_none() && view_len > 0 {
            debug!("Auto-selecting first unlocked node since no selection exists");
            self.navigate(NavigationDirection::First)
        }

        let running_nodes = self.controller.running_count() as u64;
        if running_nodes != self.last_reported_running_count {
            if let Some(action_sender) = &self.operations.action_sender
                && let Err(err) = action_sender.send(Action::StoreRunningNodeCount(running_nodes))
            {
                error!("Failed to propagate updated running node count ({running_nodes}): {err}");
            }
            self.last_reported_running_count = running_nodes;
        }

        debug!("Node state updated. Node count changed to {view_len}");
    }

    // update the values inside node items
    pub fn sync_aggregated_node_stats(&mut self, node_stats: AggregatedNodeStats) {
        let interval_secs = NODE_STAT_UPDATE_INTERVAL.as_secs_f64().max(1.0);

        for stats in node_stats.individual_stats {
            let current_inbound_total = stats.bandwidth_inbound as u64;
            let current_outbound_total = stats.bandwidth_outbound as u64;

            let entry = self
                .controller
                .nodes
                .entry(stats.service_name.clone())
                .or_default();

            let (prev_in, prev_out) = entry.bandwidth_totals;
            let inbound_delta = current_inbound_total.saturating_sub(prev_in);
            let outbound_delta = current_outbound_total.saturating_sub(prev_out);
            let bandwidth_inbound_bps = (inbound_delta as f64 * 8.0) / interval_secs;
            let bandwidth_outbound_bps = (outbound_delta as f64 * 8.0) / interval_secs;

            entry.bandwidth_totals = (current_inbound_total, current_outbound_total);
            entry.metrics = NodeMetrics {
                rewards_wallet_balance: stats.rewards_wallet_balance as u64,
                memory_usage_mb: stats.memory_usage_mb as u64,
                bandwidth_inbound_bps,
                bandwidth_outbound_bps,
                records: stats.max_records as u64,
                peers: stats.peers as u64,
                connections: stats.connections as u64,
                endpoint_online: true,
            };
            entry.reachability = stats.reachability_status.clone();
        }

        for failed_service in node_stats.failed_to_connect {
            let entry = self
                .controller
                .nodes
                .entry(failed_service.clone())
                .or_default();
            entry.metrics = NodeMetrics {
                endpoint_online: false,
                ..Default::default()
            };
            entry.reachability = ReachabilityStatusValues::default();
            entry.bandwidth_totals = (0, 0);
        }
        self.controller.refresh_view();
        debug!("Synced node metrics with aggregated stats");
    }

    pub fn has_nodes(&self) -> bool {
        !self.controller.view.items.is_empty()
    }

    pub fn has_running_nodes(&self) -> bool {
        self.controller.running_count() > 0
    }

    pub fn selected_node(&self) -> Option<NodeSelectionInfo> {
        self.controller.selected_item().map(NodeSelectionInfo::from)
    }

    pub fn sync_rewards_address(&mut self, rewards_address: Option<EvmAddress>) {
        self.operations_config.rewards_address = rewards_address;
        debug!("Synced rewards_address to {rewards_address:?}");
    }

    pub fn sync_nodes_to_start(&mut self, nodes_to_start: u64) {
        self.operations_config.nodes_to_start = nodes_to_start;
        self.controller
            .update_desired_running_count(self.operations_config.nodes_to_start);
        debug!("Synced nodes_to_start to {nodes_to_start}");
    }

    pub fn sync_upnp_setting(&mut self, upnp_enabled: bool) {
        self.operations_config.upnp_enabled = upnp_enabled;
        debug!("Synced upnp_enabled to {upnp_enabled:?}");
    }

    pub fn sync_port_range(&mut self, port_range: Option<(u32, u32)>) {
        self.operations_config.port_range = port_range;
        debug!("Synced port_range to {port_range:?}");
    }

    /// Find the index of the next unlocked node, wrapping around if needed
    fn find_next_unlocked_index(&self) -> Option<usize> {
        let items = self.controller.items();
        if items.is_empty() {
            return None;
        }

        let current = self.controller.selected_index().unwrap_or(0);
        let total_items = items.len();

        for i in 1..=total_items {
            let next_index = (current + i) % total_items;
            if !items[next_index].is_locked() {
                return Some(next_index);
            }
        }
        None
    }

    fn find_previous_unlocked_index(&self) -> Option<usize> {
        let items = self.controller.items();
        if items.is_empty() {
            return None;
        }

        let current = self.controller.selected_index().unwrap_or(0);
        let total_items = items.len();

        for i in 1..=total_items {
            let prev_index = (current + total_items - i) % total_items;
            if !items[prev_index].is_locked() {
                return Some(prev_index);
            }
        }
        None
    }

    fn find_first_unlocked_index(&self) -> Option<usize> {
        self.controller
            .items()
            .iter()
            .enumerate()
            .find(|(_, item)| !item.is_locked())
            .map(|(idx, _)| idx)
    }

    fn find_last_unlocked_index(&self) -> Option<usize> {
        self.controller
            .items()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, item)| !item.is_locked())
            .map(|(idx, _)| idx)
    }

    pub fn navigate(&mut self, direction: NavigationDirection) {
        match direction {
            NavigationDirection::Up(steps) => {
                let count = steps.max(1);
                for _ in 0..count {
                    let prev_index = self.find_previous_unlocked_index();
                    self.select_node_if_unlocked(prev_index);
                }
            }
            NavigationDirection::Down(steps) => {
                let count = steps.max(1);
                for _ in 0..count {
                    let next_index = self.find_next_unlocked_index();
                    self.select_node_if_unlocked(next_index);
                }
            }
            NavigationDirection::First => {
                let first_index = self.find_first_unlocked_index();
                self.select_node_if_unlocked(first_index);
            }
            NavigationDirection::Last => {
                let last_index = self.find_last_unlocked_index();
                self.select_node_if_unlocked(last_index);
            }
        }
    }

    fn set_selection(&mut self, index: Option<usize>) {
        self.controller.view.state.select(index);
        self.controller.view.last_selected = index;
    }

    pub fn select_node_if_unlocked(&mut self, index: Option<usize>) {
        match index {
            Some(idx) if idx < self.controller.view.items.len() => {
                if !self.controller.view.items[idx].is_locked() {
                    self.set_selection(Some(idx))
                } else {
                    self.set_selection(None)
                }
            }
            None => self.set_selection(None),
            _ => (),
        }
    }

    pub fn clear_selection(&mut self) {
        self.set_selection(None)
    }

    pub fn try_clear_selection_if_locked(&mut self) {
        if let Some(selected) = self.controller.selected_index()
            && self.controller.items()[selected].is_locked()
        {
            self.clear_selection();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSelectionInfo {
    pub lifecycle: LifecycleState,
    pub locked: bool,
    pub can_start: bool,
    pub can_stop: bool,
}

impl From<&NodeViewModel> for NodeSelectionInfo {
    fn from(node: &NodeViewModel) -> Self {
        Self {
            lifecycle: node.lifecycle.clone(),
            locked: node.is_locked(),
            can_start: node.can_start(),
            can_stop: node.can_stop(),
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
