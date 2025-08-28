// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{Action, StatusActions};
use crate::components::popup::manage_nodes::{GB_PER_NODE, MAX_NODE_COUNT};
use crate::connection_mode::ConnectionMode;
use crate::error::ErrorPopup;
use crate::node_mgmt::{
    MaintainNodesArgs, NodeManagement, NodeManagementTask, PORT_MAX, PORT_MIN, UpgradeNodesArgs,
};
use crate::system::get_drive_name;
use ant_bootstrap::InitialPeersConfig;
use ant_node_manager::add_services::config::PortRange;
use color_eyre::eyre::Result;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

pub struct AddNodeConfig<'a> {
    pub node_count: u64,
    pub available_disk_space_gb: u64,
    pub storage_mountpoint: &'a PathBuf,
    pub rewards_address: &'a str,
    pub nodes_to_start: u64,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
    pub data_dir_path: PathBuf,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
}

pub struct StartNodesConfig<'a> {
    pub rewards_address: &'a str,
    pub nodes_to_start: u64,
    pub antnode_path: Option<PathBuf>,
    pub connection_mode: ConnectionMode,
    pub data_dir_path: PathBuf,
    pub network_id: Option<u8>,
    pub init_peers_config: InitialPeersConfig,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
}

pub struct NodeOperations {
    node_management: NodeManagement,
    pub action_sender: Option<UnboundedSender<Action>>,
}

impl NodeOperations {
    pub fn new(node_management: NodeManagement) -> Self {
        Self {
            node_management,
            action_sender: None,
        }
    }

    pub fn register_action_sender(&mut self, sender: UnboundedSender<Action>) {
        self.action_sender = Some(sender);
    }

    fn get_actions_sender(&self) -> Result<UnboundedSender<Action>> {
        self.action_sender
            .clone()
            .ok_or_else(|| color_eyre::eyre::eyre!("Action sender not registered"))
    }

    pub fn handle_add_node(&mut self, config: &AddNodeConfig) -> Result<Option<Action>> {
        // Validation: Available space
        if GB_PER_NODE > config.available_disk_space_gb {
            let error_popup = ErrorPopup::new(
                "Cannot Add Node".to_string(),
                format!("\nEach Node requires {GB_PER_NODE}GB of available space."),
                format!(
                    "{} has only {}GB remaining.\n\nYou can free up some space or change to different drive in the options.",
                    get_drive_name(config.storage_mountpoint)?,
                    config.available_disk_space_gb
                ),
            );
            return Ok(Some(Action::ShowErrorPopup(error_popup)));
        }

        // Validation: Amount of nodes
        if config.node_count + 1 > MAX_NODE_COUNT {
            let error_popup = ErrorPopup::new(
                "Cannot Add Node".to_string(),
                format!(
                    "There are not enough ports available in your\ncustom port range to start another node ({MAX_NODE_COUNT})."
                ),
                "\nVisit autonomi.com/support/port-error for help".to_string(),
            );
            return Ok(Some(Action::ShowErrorPopup(error_popup)));
        }

        if config.rewards_address.is_empty() {
            return Ok(Some(Action::StatusActions(
                StatusActions::TriggerRewardsAddress,
            )));
        }

        if config.nodes_to_start == 0 {
            return Ok(Some(Action::StatusActions(
                StatusActions::TriggerManageNodes,
            )));
        }

        let port_range = PortRange::Range(
            config.port_from.unwrap_or(PORT_MIN) as u16,
            config.port_to.unwrap_or(PORT_MAX) as u16,
        );

        let action_sender = self.get_actions_sender()?;
        let add_node_args = MaintainNodesArgs {
            action_sender: action_sender.clone(),
            antnode_path: config.antnode_path.clone(),
            connection_mode: config.connection_mode,
            count: 1,
            data_dir_path: Some(config.data_dir_path.clone()),
            network_id: config.network_id,
            owner: config.rewards_address.to_string(),
            init_peers_config: config.init_peers_config.clone(),
            port_range: Some(port_range),
            rewards_address: config.rewards_address.to_string(),
        };

        self.node_management
            .send_task(NodeManagementTask::AddNode {
                args: add_node_args,
            })?;

        Ok(None)
    }

    pub fn handle_start_nodes(&mut self, config: &StartNodesConfig) -> Result<Option<Action>> {
        if config.rewards_address.is_empty() {
            return Ok(Some(Action::StatusActions(
                StatusActions::TriggerRewardsAddress,
            )));
        }

        if config.nodes_to_start == 0 {
            return Ok(Some(Action::StatusActions(
                StatusActions::TriggerManageNodes,
            )));
        }

        let port_range = PortRange::Range(
            config.port_from.unwrap_or(PORT_MIN) as u16,
            config.port_to.unwrap_or(PORT_MAX) as u16,
        );

        let action_sender = self.get_actions_sender()?;
        let maintain_nodes_args = MaintainNodesArgs {
            action_sender: action_sender.clone(),
            antnode_path: config.antnode_path.clone(),
            connection_mode: config.connection_mode,
            count: config.nodes_to_start as u16,
            data_dir_path: Some(config.data_dir_path.clone()),
            network_id: config.network_id,
            owner: config.rewards_address.to_string(),
            init_peers_config: config.init_peers_config.clone(),
            port_range: Some(port_range),
            rewards_address: config.rewards_address.to_string(),
        };

        self.node_management
            .send_task(NodeManagementTask::MaintainNodes {
                args: maintain_nodes_args,
            })?;

        Ok(None)
    }

    pub fn handle_stop_nodes(&mut self, running_nodes: Vec<String>) -> Result<()> {
        let action_sender = self.get_actions_sender()?;
        self.node_management
            .send_task(NodeManagementTask::StopNodes {
                services: running_nodes,
                action_sender,
            })?;
        Ok(())
    }

    pub fn handle_remove_nodes(&mut self, service_names: Vec<String>) -> Result<()> {
        let action_sender = self.get_actions_sender()?;
        self.node_management
            .send_task(NodeManagementTask::RemoveNodes {
                services: service_names,
                action_sender,
            })?;
        Ok(())
    }

    pub fn handle_start_node(&mut self, service_names: Vec<String>) -> Result<()> {
        let action_sender = self.get_actions_sender()?;
        self.node_management
            .send_task(NodeManagementTask::StartNode {
                services: service_names,
                action_sender,
            })?;
        Ok(())
    }

    pub fn handle_upgrade_nodes(&mut self, service_names: Vec<String>) -> Result<()> {
        let action_sender = self.get_actions_sender()?;
        let upgrade_args = UpgradeNodesArgs {
            action_sender,
            connection_timeout_s: 30,
            do_not_start: false,
            custom_bin_path: None,
            force: false,
            fixed_interval: None,
            peer_ids: vec![],
            provided_env_variables: None,
            service_names,
            url: None,
            version: None,
        };

        self.node_management
            .send_task(NodeManagementTask::UpgradeNodes { args: upgrade_args })?;
        Ok(())
    }
}
