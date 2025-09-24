// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{Action, StatusActions};
use crate::components::popup::error_popup::ErrorPopup;
use crate::components::popup::manage_nodes::{GB_PER_NODE, MAX_NODE_COUNT};
use crate::node_management::config::{PORT_MAX, PORT_MIN};
use crate::node_management::{
    AddNodesConfig, NodeManagementHandle, NodeManagementTask, UpgradeNodesConfig,
};
use crate::system::get_drive_name;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmAddress;
use ant_node_manager::add_services::config::PortRange;
use color_eyre::eyre::Result;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::mpsc::UnboundedSender;

pub struct NodeOperationsConfig {
    pub available_disk_space_gb: u64,
    pub storage_mountpoint: PathBuf,
    pub rewards_address: Option<EvmAddress>,
    pub nodes_to_start: u64,
    pub antnode_path: Option<PathBuf>,
    pub upnp_enabled: bool,
    pub data_dir_path: PathBuf,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub port_range: Option<(u32, u32)>,
}

pub struct NodeOperations {
    node_management: Arc<dyn NodeManagementHandle>,
    pub action_sender: Option<UnboundedSender<Action>>,
}

impl NodeOperations {
    pub fn new(node_management: Arc<dyn NodeManagementHandle>) -> Self {
        Self {
            node_management,
            action_sender: None,
        }
    }

    pub fn register_action_sender(&mut self, sender: UnboundedSender<Action>) -> Result<()> {
        self.node_management
            .send_task(NodeManagementTask::RegisterActionSender {
                action_sender: sender.clone(),
            })?;

        self.action_sender = Some(sender);
        Ok(())
    }

    pub fn handle_refresh_registry(&mut self) -> Result<()> {
        self.node_management
            .send_task(NodeManagementTask::RefreshNodeRegistry { force: true })?;
        Ok(())
    }

    pub fn handle_add_node(
        &mut self,
        config: &NodeOperationsConfig,
        current_node_count: u64,
    ) -> Result<Option<Action>> {
        // Validation: Available space
        if GB_PER_NODE > config.available_disk_space_gb {
            let error_popup = ErrorPopup::new(
                "Cannot Add Node",
                format!("\nEach Node requires {GB_PER_NODE}GB of available space.").as_ref(),
                format!(
                    "{} has only {}GB remaining.\n\nYou can free up some space or change to different drive in the options.",
                    get_drive_name(config.storage_mountpoint.as_path())?,
                    config.available_disk_space_gb
                ).as_ref(),
            );
            return Ok(Some(Action::ShowErrorPopup(error_popup)));
        }

        // Validation: Amount of nodes
        if current_node_count + 1 > MAX_NODE_COUNT {
            let error_popup = ErrorPopup::new(
                "Cannot Add Node",
                format!("You have reached the maximum node limit ({MAX_NODE_COUNT}).").as_ref(),
                "\n Launchpad does not support more than {MAX_NODE_COUNT} nodes.",
            );
            return Ok(Some(Action::ShowErrorPopup(error_popup)));
        }

        if config.rewards_address.is_none() {
            info!("Rewards address is not set. Ask for input.");
            return Ok(Some(Action::StatusActions(
                StatusActions::TriggerRewardsAddress,
            )));
        }

        if config.nodes_to_start == 0 {
            info!("Nodes to start not set. Ask for input.");
            return Ok(Some(Action::StatusActions(
                StatusActions::TriggerManageNodes,
            )));
        }

        let port_range = if let Some((from, to)) = config.port_range {
            PortRange::Range(from as u16, to as u16)
        } else {
            PortRange::Range(PORT_MIN as u16, PORT_MAX as u16)
        };

        let add_node_args = AddNodesConfig {
            antnode_path: config.antnode_path.clone(),
            upnp_enabled: config.upnp_enabled,
            count: 1,
            data_dir_path: Some(config.data_dir_path.clone()),
            network_id: config.network_id,
            init_peers_config: config.init_peers_config.clone(),
            port_range: Some(port_range),
            rewards_address: config.rewards_address,
        };

        self.node_management
            .send_task(NodeManagementTask::AddNode {
                config: add_node_args,
            })?;

        Ok(None)
    }

    pub fn handle_maintain_nodes(
        &mut self,
        config: &NodeOperationsConfig,
    ) -> Result<Option<Action>> {
        if config.rewards_address.is_none() {
            info!("Rewards address is not set. Ask for input.");

            return Ok(Some(Action::StatusActions(
                StatusActions::TriggerRewardsAddress,
            )));
        }

        let port_range = if let Some((from, to)) = config.port_range {
            PortRange::Range(from as u16, to as u16)
        } else {
            PortRange::Range(PORT_MIN as u16, PORT_MAX as u16)
        };

        let maintain_nodes_args = AddNodesConfig {
            antnode_path: config.antnode_path.clone(),
            upnp_enabled: config.upnp_enabled,
            count: config.nodes_to_start as u16,
            data_dir_path: Some(config.data_dir_path.clone()),
            network_id: config.network_id,
            init_peers_config: config.init_peers_config.clone(),
            port_range: Some(port_range),
            rewards_address: config.rewards_address,
        };

        self.node_management
            .send_task(NodeManagementTask::MaintainNodes {
                config: maintain_nodes_args,
            })?;

        Ok(None)
    }

    pub fn handle_stop_nodes(&mut self, running_nodes: Vec<String>) -> Result<()> {
        self.node_management
            .send_task(NodeManagementTask::StopNodes {
                services: running_nodes,
            })?;
        Ok(())
    }

    pub fn handle_remove_nodes(&mut self, service_names: Vec<String>) -> Result<()> {
        self.node_management
            .send_task(NodeManagementTask::RemoveNodes {
                services: service_names,
            })?;
        Ok(())
    }

    pub fn handle_reset_nodes(&mut self) -> Result<()> {
        self.node_management
            .send_task(NodeManagementTask::ResetNodes)?;
        Ok(())
    }

    pub fn handle_start_node(&mut self, service_names: Vec<String>) -> Result<()> {
        self.node_management
            .send_task(NodeManagementTask::StartNode {
                services: service_names,
            })?;
        Ok(())
    }

    pub fn handle_upgrade_nodes(&mut self, service_names: Vec<String>) -> Result<()> {
        let upgrade_args = UpgradeNodesConfig {
            custom_bin_path: None,
            provided_env_variables: None,
            service_names,
            url: None,
            version: None,
        };

        self.node_management
            .send_task(NodeManagementTask::UpgradeNodes {
                config: upgrade_args,
            })?;
        Ok(())
    }
}
