// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    node_item::{NodeItem, NodeStatus},
    operations::NodeOperations,
    table_state::StatefulTable,
};
use crate::connection_mode::ConnectionMode;
use crate::error::ErrorPopup;
use ant_bootstrap::InitialPeersConfig;
use ant_service_management::{NodeRegistryManager, NodeServiceData};
use color_eyre::eyre::Result;
use std::{collections::HashSet, path::PathBuf, time::Instant};
use throbber_widgets_tui::ThrobberState;

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
    pub rewards_address: String,
    pub nodes_to_start: usize,

    // Storage info (for validation)
    pub storage_mountpoint: PathBuf,
    pub available_disk_space_gb: usize,

    // UI state
    pub error_popup: Option<ErrorPopup>,
    pub spinner_states: Vec<ThrobberState>,
}

impl NodeTableState {
    pub async fn new(config: NodeTableConfig) -> Result<Self> {
        let node_registry =
            NodeRegistryManager::load(&ant_node_manager::config::get_node_registry_path()?).await?;
        let node_services = node_registry.get_node_service_data().await;
        let node_management = crate::node_mgmt::NodeManagement::new(node_registry.clone())?;

        Ok(Self {
            items: StatefulTable::with_items(vec![]),
            node_services,
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
            error_popup: None,
            spinner_states: vec![],
        })
    }

    pub fn get_running_nodes(&self) -> Vec<String> {
        self.node_services
            .iter()
            .filter_map(|node| {
                if node.status == ant_service_management::ServiceStatus::Running {
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
            .find(|item| item.name == service_name)
    }

    pub fn update_node_state(&mut self, all_nodes_data: &[NodeServiceData]) {
        self.node_services = all_nodes_data.to_vec();

        // Filter out removed nodes from node_items
        let service_names: HashSet<String> = all_nodes_data
            .iter()
            .map(|node| node.service_name.clone())
            .collect();

        self.items
            .items
            .retain(|item| service_names.contains(&item.name));

        // Update existing items or add new ones
        for node_data in all_nodes_data {
            if let Some(existing_item) = self
                .items
                .items
                .iter_mut()
                .find(|item| item.name == node_data.service_name)
            {
                existing_item.update_status(NodeStatus::from(&node_data.status));
            } else {
                let new_item = NodeItem {
                    name: node_data.service_name.clone(),
                    status: NodeStatus::from(&node_data.status),
                    ..Default::default()
                };
                self.items.items.push(new_item);
            }
        }

        // Ensure spinner states match item count
        self.spinner_states
            .resize_with(self.items.items.len(), ThrobberState::default);

        log::debug!(
            "Node state updated. Node count changed from {} to {}",
            self.items.items.len(),
            all_nodes_data.len()
        );
    }

    /// Tries to trigger the update of node stats if the last update was more than `NODE_STAT_UPDATE_INTERVAL` ago.
    /// The result is sent via the StatusActions::NodesStatsObtained action.
    pub fn try_update_node_stats(&mut self, force_update: bool) -> Result<()> {
        use crate::components::status::NODE_STAT_UPDATE_INTERVAL;
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
        use crate::action::{Action, NodeTableActions};
        if let Some(action_sender) = &self.operations.action_sender {
            let state_action = Action::NodeTableActions(NodeTableActions::StateChanged {
                node_count: self.items.items.len(),
                has_running_nodes: !self.get_running_nodes().is_empty(),
                has_nodes: !self.items.items.is_empty(),
            });

            action_sender.send(state_action)?;
        }
        Ok(())
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
    pub rewards_address: String,
    pub nodes_to_start: usize,
    pub storage_mountpoint: PathBuf,
}
