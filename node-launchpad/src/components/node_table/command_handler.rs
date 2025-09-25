// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::lifecycle::{CommandKind, DesiredNodeState, LifecycleState};
use super::state::NodeTableState;
use crate::action::{Action, NodeManagementCommand, NodeManagementResponse};
use crate::components::popup::error_popup::ErrorPopup;
use color_eyre::Result;
use tracing::{debug, error};

/// Thin coordinator that translates high-level node-management actions into
/// table state mutations plus ops calls. Keeps the UI-specific state updates in
/// one place so journeys/tests can reason about transitions easily.
pub struct NodeCommandHandler<'a> {
    state: &'a mut NodeTableState,
}

impl<'a> NodeCommandHandler<'a> {
    pub fn new(state: &'a mut NodeTableState) -> Self {
        Self { state }
    }

    /// Entry point for commands originating from the UI layer.
    /// Each arm gathers the relevant node ids, marks transitions, and invokes
    /// the corresponding operation.
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

    /// Completes the in-flight transitions once antctl (via the mock or real
    /// backend) reports back. Success paths restore the steady-state intent
    /// while errors surface a popup and roll back desired states.
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
                CommandKind::Start,
                service_names,
                Some(DesiredNodeState::FollowCluster),
                Some(DesiredNodeState::FollowCluster),
                error,
                "Error while starting nodes",
            ),
            NodeManagementResponse::StopNodes {
                service_names,
                error,
            } => self.finalize_service_command(
                CommandKind::Stop,
                service_names,
                Some(DesiredNodeState::FollowCluster),
                Some(DesiredNodeState::FollowCluster),
                error,
                "Error while stopping nodes",
            ),
            NodeManagementResponse::RemoveNodes {
                service_names,
                error,
            } => self.finalize_service_command(
                CommandKind::Remove,
                service_names,
                Some(DesiredNodeState::FollowCluster),
                Some(DesiredNodeState::FollowCluster),
                error,
                "Error while removing nodes",
            ),
            NodeManagementResponse::UpgradeNodes {
                service_names,
                error,
            } => self.finalize_service_command(
                CommandKind::Maintain,
                service_names,
                None,
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

    /// Pass-through helper used when the UI requests a registry refresh.
    fn refresh_registry(&mut self) -> Result<Option<Action>> {
        self.state.operations.handle_refresh_registry()?;
        Ok(None)
    }

    /// Request antctl to align the fleet with the configured count.
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

    /// Delegates to the add-node operation without additional bookkeeping.
    fn add_node(&mut self) -> Result<Option<Action>> {
        self.state.operations.handle_add_node(
            &self.state.operations_config,
            self.state.controller.items().len() as u64,
        )
    }

    /// Starts every node that advertises startable lifecycles. Marks the
    /// optimistic "Run" intent so the UI reflects the request immediately.
    fn start_nodes(&mut self) -> Result<Option<Action>> {
        let nodes_to_start: Vec<String> = self
            .state
            .controller
            .items()
            .iter()
            .filter(|model| model.can_start())
            .map(|model| model.id.clone())
            .collect();

        if nodes_to_start.is_empty() {
            debug!("StartNodes: No nodes available to start");
            return Ok(None);
        }

        self.mark_transition(
            &nodes_to_start,
            CommandKind::Start,
            Some(DesiredNodeState::Run),
        );

        if let Err(err) = self
            .state
            .operations
            .handle_start_node(nodes_to_start.clone())
        {
            error!("StartNodes operation failed: {err}");
            self.revert_nodes(&nodes_to_start, Some(DesiredNodeState::FollowCluster));
            return Err(err);
        }

        Ok(None)
    }

    /// Similar to `start_nodes`, but targets running nodes and sets the desired
    /// state to `Stop` until the response confirms completion.
    fn stop_nodes(&mut self) -> Result<Option<Action>> {
        let nodes_to_stop: Vec<String> = self
            .state
            .controller
            .items()
            .iter()
            .filter(|model| model.can_stop())
            .map(|model| model.id.clone())
            .collect();

        if nodes_to_stop.is_empty() {
            debug!("StopNodes: No nodes available to stop");
            return Ok(None);
        }

        self.mark_transition(
            &nodes_to_stop,
            CommandKind::Stop,
            Some(DesiredNodeState::Stop),
        );

        if let Err(err) = self
            .state
            .operations
            .handle_stop_nodes(nodes_to_stop.clone())
        {
            error!("Failed to stop node: {err}");
            self.revert_nodes(&nodes_to_stop, Some(DesiredNodeState::FollowCluster));
            return Err(err);
        }

        Ok(None)
    }

    /// Convenience layer for single-node start/stop toggling from the UI.
    /// Mirrors the multi-node helpers but works with the focused row.
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
                    let ids = vec![selected.id.clone()];
                    self.mark_transition(&ids, CommandKind::Stop, Some(DesiredNodeState::Stop));
                    if let Err(err) = self.state.operations.handle_stop_nodes(ids.clone()) {
                        error!("Failed to stop node {}: {err}", selected.id);
                        self.revert_nodes(&ids, Some(DesiredNodeState::FollowCluster));
                        return Err(err);
                    }
                }
            }
            LifecycleState::Stopped
            | LifecycleState::Added
            | LifecycleState::Unreachable { .. } => {
                if selected.can_start() {
                    let ids = vec![selected.id.clone()];
                    self.mark_transition(&ids, CommandKind::Start, Some(DesiredNodeState::Run));
                    if let Err(err) = self.state.operations.handle_start_node(ids.clone()) {
                        error!("Failed to start node {}: {err}", selected.id);
                        self.revert_nodes(&ids, Some(DesiredNodeState::FollowCluster));
                        return Err(err);
                    }
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

    /// Initiates removal of the focused node via antctl.
    fn remove_selected_node(&mut self) -> Result<Option<Action>> {
        let Some(selected) = self.state.controller.selected_item().cloned() else {
            return Ok(None);
        };

        if selected.is_locked() {
            debug!("Cannot remove node {}: node is locked", selected.id);
            return Ok(None);
        }

        let ids = vec![selected.id.clone()];
        self.mark_transition(&ids, CommandKind::Remove, Some(DesiredNodeState::Remove));

        if let Err(err) = self.state.operations.handle_remove_nodes(ids.clone()) {
            error!("Failed to remove node {}: {err}", selected.id);
            self.revert_nodes(&ids, Some(DesiredNodeState::FollowCluster));
            return Err(err);
        }

        Ok(None)
    }

    /// Requests upgrades for every eligible node.
    fn upgrade_nodes(&mut self) -> Result<Option<Action>> {
        let nodes_to_upgrade: Vec<String> = self
            .state
            .controller
            .items()
            .iter()
            .filter(|model| model.can_upgrade())
            .map(|model| model.id.clone())
            .collect();

        if nodes_to_upgrade.is_empty() {
            debug!("UpgradeNodes: No nodes available to upgrade");
            return Ok(None);
        }

        self.mark_transition(&nodes_to_upgrade, CommandKind::Maintain, None);

        if let Err(err) = self
            .state
            .operations
            .handle_upgrade_nodes(nodes_to_upgrade.clone())
        {
            error!("UpgradeNodes operation failed: {err}");
            self.revert_nodes(&nodes_to_upgrade, None);
            return Err(err);
        }

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

    /// Completes a service command by clearing transitions, setting desired
    /// state on success, or raising an error popup/rolling back intent when an
    /// error is reported.
    fn finalize_service_command(
        &mut self,
        command: CommandKind,
        service_names: Vec<String>,
        success_target: Option<DesiredNodeState>,
        error_target: Option<DesiredNodeState>,
        error: Option<String>,
        error_title: &'static str,
    ) -> Result<Option<Action>> {
        if service_names.is_empty() {
            self.state.controller.clear_transitions_by_command(command);
        } else {
            self.clear_transition(&service_names);
        }

        if let Some(err) = error {
            if let Some(target) = error_target {
                for service in &service_names {
                    self.state.controller.set_node_target(service, target);
                }
            }

            let error_popup = ErrorPopup::new(error_title, "Please try again", &err);
            return Ok(Some(Action::ShowErrorPopup(error_popup)));
        }

        if let Some(target) = success_target {
            for service in &service_names {
                self.state.controller.set_node_target(service, target);
            }
        }

        Ok(None)
    }

    /// Helper that marks each node as actively transitioning and optionally
    /// records a desired state so the UI reflects intent immediately.
    fn mark_transition(
        &mut self,
        ids: &[String],
        command: CommandKind,
        desired: Option<DesiredNodeState>,
    ) {
        for id in ids {
            self.state.controller.mark_transition(id, command);
            if let Some(target) = desired {
                self.state.controller.set_node_target(id, target);
            }
        }
    }

    /// Provides a uniform place to clear transition flags once a command
    /// completes.
    fn clear_transition(&mut self, ids: &[String]) {
        for id in ids {
            self.state.controller.clear_transition(id);
        }
    }

    /// Rolls back both the transition and any optimistic desired state when a
    /// command fails.
    fn revert_nodes(&mut self, ids: &[String], desired: Option<DesiredNodeState>) {
        self.clear_transition(ids);
        if let Some(target) = desired {
            for id in ids {
                self.state.controller.set_node_target(id, target);
            }
        }
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
