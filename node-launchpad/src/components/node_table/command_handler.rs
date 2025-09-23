// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::lifecycle::{CommandKind, DesiredNodeState, LifecycleState};
use super::operations::NodeOperations;
use super::state::NodeTableState;
use crate::action::{Action, NodeManagementCommand, NodeManagementResponse};
use crate::components::popup::error_popup::ErrorPopup;
use color_eyre::Result;
use tracing::{debug, error};

/// Orchestrates node management commands and responses.
pub struct NodeCommandHandler<'a> {
    state: &'a mut NodeTableState,
}

impl<'a> NodeCommandHandler<'a> {
    pub fn new(state: &'a mut NodeTableState) -> Self {
        Self { state }
    }

    pub fn handle_command(&mut self, command: NodeManagementCommand) -> Result<Option<Action>> {
        match command {
            NodeManagementCommand::RefreshRegistry => self.refresh_registry(),
            NodeManagementCommand::MaintainNodes => self.maintain_nodes(),
            NodeManagementCommand::AddNode => self.add_node(),
            NodeManagementCommand::StartNodes => self.start_nodes(),
            NodeManagementCommand::StopNodes => self.stop_nodes(),
            NodeManagementCommand::ToggleNode => self.toggle_selected_node(),
            NodeManagementCommand::RemoveNodes => self.remove_selected_node(),
            NodeManagementCommand::UpgradeNodes => self.upgrade_nodes(),
            NodeManagementCommand::ResetNodes => self.reset_nodes(),
        }
    }

    pub fn handle_response(&mut self, response: NodeManagementResponse) -> Result<Option<Action>> {
        match response {
            NodeManagementResponse::RefreshRegistry { error } => {
                if let Some(err) = error {
                    error!("RefreshRegistry operation failed: {err}");
                }
                Ok(None)
            }
            NodeManagementResponse::MaintainNodes { error } => {
                self.state
                    .controller
                    .clear_transitions_by_command(CommandKind::Maintain);
                self.error_popup_if_needed(error, "Error while managing nodes", "Please try again")
            }
            NodeManagementResponse::AddNode { error } => {
                self.state
                    .controller
                    .clear_transitions_by_command(CommandKind::Add);
                self.error_popup_if_needed(error, "Error while adding node", "Please try again")
            }
            NodeManagementResponse::StartNodes {
                service_names,
                error,
            } => self.finalize_service_command(
                service_names,
                Some(DesiredNodeState::FollowCluster),
                error,
                "Error while starting nodes",
            ),
            NodeManagementResponse::StopNodes {
                service_names,
                error,
            } => self.finalize_service_command(
                service_names,
                Some(DesiredNodeState::FollowCluster),
                error,
                "Error while stopping nodes",
            ),
            NodeManagementResponse::RemoveNodes {
                service_names,
                error,
            } => self.finalize_service_command(
                service_names,
                Some(DesiredNodeState::FollowCluster),
                error,
                "Error while removing nodes",
            ),
            NodeManagementResponse::UpgradeNodes {
                service_names,
                error,
            } => self.finalize_service_command(
                service_names,
                None,
                error,
                "Error while upgrading nodes",
            ),
            NodeManagementResponse::ResetNodes { error } => {
                self.state
                    .controller
                    .clear_transitions_by_command(CommandKind::Remove);
                self.error_popup_if_needed(error, "Error while resetting nodes", "Please try again")
            }
        }
    }

    fn refresh_registry(&mut self) -> Result<Option<Action>> {
        self.state.operations.handle_refresh_registry()?;
        Ok(None)
    }

    fn maintain_nodes(&mut self) -> Result<Option<Action>> {
        let ids: Vec<_> = self
            .state
            .controller
            .items()
            .iter()
            .map(|model| model.id.clone())
            .collect();

        for id in &ids {
            self.state
                .controller
                .mark_transition(id, CommandKind::Maintain);
        }

        match self
            .state
            .operations
            .handle_maintain_nodes(&self.state.operations_config)
        {
            Ok(Some(action)) => {
                for id in &ids {
                    self.state.controller.clear_transition(id);
                }
                Ok(Some(action))
            }
            Ok(None) => Ok(None),
            Err(err) => {
                for id in &ids {
                    self.state.controller.clear_transition(id);
                }
                Err(err)
            }
        }
    }

    fn add_node(&mut self) -> Result<Option<Action>> {
        self.state.operations.handle_add_node(
            &self.state.operations_config,
            self.state.controller.items().len() as u64,
        )
    }

    fn start_nodes(&mut self) -> Result<Option<Action>> {
        let nodes_to_start: Vec<_> = self
            .state
            .controller
            .items()
            .iter()
            .filter(|model| model.can_start())
            .map(|model| model.id.clone())
            .collect();

        self.apply_transition_command(
            nodes_to_start,
            CommandKind::Start,
            Some(DesiredNodeState::Run),
            Some(DesiredNodeState::FollowCluster),
            |ops, ids| ops.handle_start_node(ids),
            "StartNodes: No nodes available to start",
            "StartNodes operation failed",
        )?;

        Ok(None)
    }

    fn stop_nodes(&mut self) -> Result<Option<Action>> {
        let nodes_to_stop: Vec<_> = self
            .state
            .controller
            .items()
            .iter()
            .filter(|model| model.can_stop())
            .map(|model| model.id.clone())
            .collect();

        self.apply_transition_command(
            nodes_to_stop,
            CommandKind::Stop,
            Some(DesiredNodeState::Stop),
            Some(DesiredNodeState::FollowCluster),
            |ops, ids| ops.handle_stop_nodes(ids),
            "StopNodes: No nodes available to stop",
            "Failed to stop node",
        )?;

        Ok(None)
    }

    fn toggle_selected_node(&mut self) -> Result<Option<Action>> {
        let Some(selected) = self.state.controller.selected_item().cloned() else {
            return Ok(None);
        };

        if selected.is_locked() {
            debug!("Cannot toggle node {}: node is locked", selected.id);
            return Ok(None);
        }

        match selected.lifecycle {
            LifecycleState::Running | LifecycleState::Starting => {
                if selected.can_stop() {
                    let error_message = format!("Failed to stop node {}", selected.id);
                    self.apply_transition_command(
                        vec![selected.id.clone()],
                        CommandKind::Stop,
                        Some(DesiredNodeState::Stop),
                        Some(DesiredNodeState::FollowCluster),
                        |ops, ids| ops.handle_stop_nodes(ids),
                        "StopNodes: No nodes available to stop",
                        error_message.as_str(),
                    )?;
                }
            }
            LifecycleState::Stopped | LifecycleState::Unreachable { .. } => {
                if selected.can_start() {
                    let error_message = format!("Failed to start node {}", selected.id);
                    self.apply_transition_command(
                        vec![selected.id.clone()],
                        CommandKind::Start,
                        Some(DesiredNodeState::Run),
                        Some(DesiredNodeState::FollowCluster),
                        |ops, ids| ops.handle_start_node(ids),
                        "StartNodes: No nodes available to start",
                        error_message.as_str(),
                    )?;
                }
            }
            _ => {
                debug!(
                    "ToggleNode: No action taken for node {} in state {:?}",
                    selected.id, selected.lifecycle
                );
            }
        }

        Ok(None)
    }

    fn remove_selected_node(&mut self) -> Result<Option<Action>> {
        let Some(selected) = self.state.controller.selected_item().cloned() else {
            return Ok(None);
        };

        if selected.is_locked() {
            debug!("Cannot remove node {}: node is locked", selected.id);
            return Ok(None);
        }

        let error_message = format!("Failed to remove node {}", selected.id);
        self.apply_transition_command(
            vec![selected.id.clone()],
            CommandKind::Remove,
            Some(DesiredNodeState::Remove),
            Some(DesiredNodeState::FollowCluster),
            |ops, ids| ops.handle_remove_nodes(ids),
            "RemoveNodes: No node selected for removal",
            error_message.as_str(),
        )?;

        Ok(None)
    }

    fn upgrade_nodes(&mut self) -> Result<Option<Action>> {
        let nodes_to_upgrade: Vec<_> = self
            .state
            .controller
            .items()
            .iter()
            .filter(|model| model.can_upgrade())
            .map(|model| model.id.clone())
            .collect();

        self.apply_transition_command(
            nodes_to_upgrade,
            CommandKind::Maintain,
            None,
            None,
            |ops, ids| ops.handle_upgrade_nodes(ids),
            "UpgradeNodes: No nodes available to upgrade",
            "UpgradeNodes operation failed",
        )?;

        Ok(None)
    }

    fn reset_nodes(&mut self) -> Result<Option<Action>> {
        let ids: Vec<_> = self
            .state
            .controller
            .items()
            .iter()
            .map(|model| model.id.clone())
            .collect();

        for id in &ids {
            self.state
                .controller
                .mark_transition(id, CommandKind::Remove);
            self.state
                .controller
                .set_node_target(id, DesiredNodeState::Remove);
        }

        if let Err(err) = self.state.operations.handle_reset_nodes() {
            for id in &ids {
                self.state.controller.clear_transition(id);
                self.state
                    .controller
                    .set_node_target(id, DesiredNodeState::FollowCluster);
            }
            return Err(err);
        }

        Ok(None)
    }

    fn apply_transition_command<F>(
        &mut self,
        ids: Vec<String>,
        command: CommandKind,
        desired: Option<DesiredNodeState>,
        revert_on_error: Option<DesiredNodeState>,
        execute: F,
        empty_message: &str,
        error_message: &str,
    ) -> Result<()>
    where
        F: FnOnce(&mut NodeOperations, Vec<String>) -> Result<()>,
    {
        if ids.is_empty() {
            debug!("{empty_message}");
            return Ok(());
        }

        for id in &ids {
            self.state.controller.mark_transition(id, command);
            if let Some(target) = desired {
                self.state.controller.set_node_target(id, target);
            }
        }

        if let Err(err) = execute(&mut self.state.operations, ids.clone()) {
            error!("{error_message}: {err}");
            for id in &ids {
                self.state.controller.clear_transition(id);
                if let Some(target) = revert_on_error {
                    self.state.controller.set_node_target(id, target);
                }
            }
            return Err(err);
        }

        Ok(())
    }

    fn finalize_service_command(
        &mut self,
        service_names: Vec<String>,
        revert_target: Option<DesiredNodeState>,
        error: Option<String>,
        error_title: &'static str,
    ) -> Result<Option<Action>> {
        for service in &service_names {
            self.state.controller.clear_transition(service);
        }

        if let Some(err) = error {
            if let Some(target) = revert_target {
                for service in &service_names {
                    self.state.controller.set_node_target(service, target);
                }
            }

            let error_popup = ErrorPopup::new(error_title, "Please try again", &err);
            return Ok(Some(Action::ShowErrorPopup(error_popup)));
        }

        Ok(None)
    }

    fn error_popup_if_needed(
        &self,
        error: Option<String>,
        title: &'static str,
        subtitle: &'static str,
    ) -> Result<Option<Action>> {
        if let Some(err) = error {
            error!("{title}: {err}");
            let error_popup = ErrorPopup::new(title, subtitle, &err);
            Ok(Some(Action::ShowErrorPopup(error_popup)))
        } else {
            Ok(None)
        }
    }
}
