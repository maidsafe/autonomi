// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::components::node_table::StatefulTable;
use crate::components::node_table::state::NodeState;
use ant_service_management::{
    ReachabilityProgress, ServiceStatus, fs::CriticalFailure, metric::ReachabilityStatusValues,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

/// Identifier used across registry, desired topology, and transitions.
pub type NodeId = String;

#[derive(Clone, Debug)]
pub struct RegistryNode {
    pub service_name: String,
    pub metrics_port: u16,
    pub status: ServiceStatus,
    pub reachability_progress: ReachabilityProgress,
    pub last_failure: Option<CriticalFailure>,
    pub version: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DesiredNodeState {
    /// Follow default policy derived from desired running count.
    #[default]
    FollowCluster,
    /// Force node to be running.
    Run,
    /// Force node to remain stopped.
    Stop,
    /// Node should be removed.
    Remove,
}

#[derive(Clone, Debug)]
pub struct TransitionEntry {
    pub command: CommandKind,
    pub started_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    Start,
    Stop,
    Add,
    Remove,
    Maintain,
}

/// Lifecycle state derived from registry + intent + transitions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleState {
    Running,
    Stopped,
    Adding,
    Starting,
    Stopping,
    Removing,
    Unreachable { reason: Option<String> },
    Refreshing,
}

impl LifecycleState {
    pub fn description(&self) -> &'static str {
        match self {
            LifecycleState::Running => "Running",
            LifecycleState::Stopped => "Stopped",
            LifecycleState::Adding => "Adding",
            LifecycleState::Starting => "Starting",
            LifecycleState::Stopping => "Stopping",
            LifecycleState::Removing => "Removing",
            LifecycleState::Unreachable { .. } => "Unreachable",
            LifecycleState::Refreshing => "Refreshing",
        }
    }
}

#[derive(Clone, Debug)]
pub struct NodeMetrics {
    pub rewards_wallet_balance: u64,
    pub memory_usage_mb: u64,
    pub bandwidth_inbound_bps: f64,
    pub bandwidth_outbound_bps: f64,
    pub records: u64,
    pub peers: u64,
    pub connections: u64,
    pub endpoint_online: bool,
}

impl Default for NodeMetrics {
    fn default() -> Self {
        Self {
            rewards_wallet_balance: 0,
            memory_usage_mb: 0,
            bandwidth_inbound_bps: 0.0,
            bandwidth_outbound_bps: 0.0,
            records: 0,
            peers: 0,
            connections: 0,
            endpoint_online: true,
        }
    }
}

/// View model consumed by the TUI widgets.
#[derive(Clone, Debug)]
pub struct NodeViewModel {
    pub id: String,
    pub lifecycle: LifecycleState,
    pub status: String,
    pub version: String,
    pub reachability_progress: ReachabilityProgress,
    pub reachability_status: ReachabilityStatusValues,
    pub metrics: NodeMetrics,
    pub locked: bool,
    pub last_failure: Option<String>,
    pub pending_command: Option<CommandKind>,
}

impl std::fmt::Debug for StatefulTable<NodeViewModel> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatefulTable<NodeViewModel>")
            .field("state", &self.state)
            .field("items_count", &self.items.len())
            .field("last_selected", &self.last_selected)
            .finish()
    }
}

impl NodeViewModel {
    pub fn new(
        id: String,
        lifecycle: LifecycleState,
        registry: Option<&RegistryNode>,
        reachability_status: ReachabilityStatusValues,
        metrics: NodeMetrics,
        locked: bool,
        pending_command: Option<CommandKind>,
    ) -> Self {
        let (status, registry_progress, mut failure_reason, version) = if let Some(node) = registry
        {
            (
                format!("{:?}", node.status),
                node.reachability_progress.clone(),
                node.last_failure.as_ref().map(|f| f.reason.clone()),
                node.version.clone(),
            )
        } else {
            (
                "Stopped".to_string(),
                ReachabilityProgress::NotRun,
                None,
                String::new(),
            )
        };

        let mut effective_lifecycle = lifecycle;

        if reachability_status.indicates_unreachable() {
            let reason_text = failure_reason
                .take()
                .map(|reason| format!("Error ({reason})"))
                .unwrap_or_else(|| "Unreachable".to_string());
            failure_reason = Some(reason_text.clone());
            effective_lifecycle = LifecycleState::Unreachable {
                reason: Some(reason_text),
            };
        } else if !metrics.endpoint_online {
            if let Some(reason) = failure_reason.take() {
                let reason_text = format!("Error ({reason})");
                failure_reason = Some(reason_text.clone());
                effective_lifecycle = LifecycleState::Unreachable {
                    reason: Some(reason_text),
                };
            } else {
                failure_reason = None;
            }
        }

        if matches!(effective_lifecycle, LifecycleState::Unreachable { .. })
            && registry.is_some_and(|node| node.status == ServiceStatus::Running)
            && !reachability_status.indicates_unreachable()
            && metrics.endpoint_online
        {
            effective_lifecycle = LifecycleState::Running;
        }

        let progress = match reachability_status.progress.clone() {
            ReachabilityProgress::NotRun => registry_progress,
            other => other,
        };

        Self {
            id,
            lifecycle: effective_lifecycle,
            status,
            version,
            reachability_progress: progress,
            reachability_status,
            metrics,
            locked,
            last_failure: failure_reason,
            pending_command,
        }
    }

    pub fn can_start(&self) -> bool {
        !self.locked
            && matches!(
                self.lifecycle,
                LifecycleState::Stopped | LifecycleState::Unreachable { .. }
            )
    }

    pub fn can_stop(&self) -> bool {
        !self.locked
            && matches!(
                self.lifecycle,
                LifecycleState::Running | LifecycleState::Starting
            )
    }

    pub fn can_upgrade(&self) -> bool {
        !self.locked && !matches!(self.lifecycle, LifecycleState::Removing)
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

/// Determines the lifecycle state for a node.
pub fn derive_lifecycle_state(
    registry: Option<&RegistryNode>,
    desired: DesiredNodeState,
    is_provisioning: bool,
    transition: Option<&TransitionEntry>,
) -> LifecycleState {
    if let Some(entry) = transition {
        return match entry.command {
            CommandKind::Start | CommandKind::Maintain => LifecycleState::Starting,
            CommandKind::Stop => LifecycleState::Stopping,
            CommandKind::Add => LifecycleState::Adding,
            CommandKind::Remove => LifecycleState::Removing,
        };
    }

    if is_provisioning && registry.is_none() {
        return LifecycleState::Adding;
    }

    let Some(node) = registry else {
        return LifecycleState::Refreshing;
    };

    match (&node.status, desired) {
        (ServiceStatus::Running, DesiredNodeState::Remove) => LifecycleState::Removing,
        (ServiceStatus::Running, DesiredNodeState::Stop) => LifecycleState::Stopping,
        (ServiceStatus::Running, _) => {
            if node
                .last_failure
                .as_ref()
                .is_some_and(|failure| failure.reason.contains("Unreachable"))
            {
                LifecycleState::Unreachable {
                    reason: node.last_failure.as_ref().map(|f| f.reason.clone()),
                }
            } else {
                LifecycleState::Running
            }
        }
        (ServiceStatus::Stopped, DesiredNodeState::Run) => LifecycleState::Starting,
        (ServiceStatus::Stopped, DesiredNodeState::Remove) => LifecycleState::Removing,
        (ServiceStatus::Stopped, _) => LifecycleState::Stopped,
        (ServiceStatus::Added, DesiredNodeState::Run) => LifecycleState::Starting,
        (ServiceStatus::Added, DesiredNodeState::Remove) => LifecycleState::Removing,
        (ServiceStatus::Added, _) => LifecycleState::Stopped,
        (ServiceStatus::Removed, _) => LifecycleState::Removing,
    }
}

/// Builds a set of node view models from the data sources.
pub fn build_view_models(
    nodes: &BTreeMap<NodeId, NodeState>,
    locked_nodes: &BTreeSet<NodeId>,
) -> Vec<NodeViewModel> {
    let ids: BTreeSet<NodeId> = nodes.keys().cloned().collect();

    let mut models = Vec::with_capacity(ids.len());
    for id in ids {
        let node_state = nodes.get(&id);
        let registry_node = node_state.and_then(|state| state.registry.as_ref());
        let desired_state = node_state
            .map(|state| state.desired)
            .unwrap_or(DesiredNodeState::FollowCluster);
        let is_provisioning = node_state
            .map(|state| state.is_provisioning)
            .unwrap_or(false);
        let transition = node_state.and_then(|state| state.transition.as_ref());
        let lifecycle =
            derive_lifecycle_state(registry_node, desired_state, is_provisioning, transition);
        let reachability_status = node_state
            .map(|state| state.reachability.clone())
            .unwrap_or_default();
        let metrics = node_state
            .map(|state| state.metrics.clone())
            .unwrap_or_default();
        let locked = locked_nodes.contains(&id)
            || matches!(
                transition.map(|t| &t.command),
                Some(
                    CommandKind::Remove | CommandKind::Stop | CommandKind::Start | CommandKind::Add
                )
            );
        models.push(NodeViewModel::new(
            id,
            lifecycle,
            registry_node,
            reachability_status,
            metrics,
            locked,
            transition.map(|t| t.command),
        ));
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn registry_node(status: ServiceStatus) -> RegistryNode {
        RegistryNode {
            service_name: "node-1".to_string(),
            metrics_port: 3000,
            status,
            reachability_progress: ReachabilityProgress::NotRun,
            last_failure: None,
            version: "0.1.0".to_string(),
        }
    }

    fn node_state(
        registry: Option<RegistryNode>,
        reachability: ReachabilityStatusValues,
        metrics: NodeMetrics,
    ) -> NodeState {
        NodeState {
            registry,
            desired: Default::default(),
            transition: None,
            is_provisioning: false,
            reachability,
            metrics,
            bandwidth_totals: (0, 0),
        }
    }

    #[test]
    fn lifecycle_from_transition_takes_priority() {
        let lifecycle = derive_lifecycle_state(
            Some(&registry_node(ServiceStatus::Stopped)),
            DesiredNodeState::Run,
            false,
            Some(&TransitionEntry {
                command: CommandKind::Start,
                started_at: Instant::now(),
            }),
        );
        assert_eq!(lifecycle, LifecycleState::Starting);
    }

    #[test]
    fn lifecycle_provisioning_when_absent_and_marked() {
        let lifecycle = derive_lifecycle_state(None, DesiredNodeState::Run, true, None);
        assert_eq!(lifecycle, LifecycleState::Adding);
    }

    #[test]
    fn lifecycle_running_stop_intent_draining() {
        let lifecycle = derive_lifecycle_state(
            Some(&registry_node(ServiceStatus::Running)),
            DesiredNodeState::Stop,
            false,
            None,
        );
        assert_eq!(lifecycle, LifecycleState::Stopping);
    }

    #[test]
    fn lifecycle_failed_if_running_with_failure() {
        let mut node = registry_node(ServiceStatus::Running);
        node.last_failure = Some(CriticalFailure {
            reason: "Unreachable".to_string(),
            date_time: Utc::now(),
        });
        let lifecycle = derive_lifecycle_state(Some(&node), DesiredNodeState::Run, false, None);
        match lifecycle {
            LifecycleState::Unreachable { reason } => {
                assert_eq!(reason, Some("Unreachable".to_string()));
            }
            _ => panic!("Expected unreachable state"),
        }
    }

    #[test]
    fn reachability_progress_prefers_metrics_update() {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "node-1".to_string(),
            node_state(
                Some(registry_node(ServiceStatus::Running)),
                ReachabilityStatusValues {
                    progress: ReachabilityProgress::InProgress(12),
                    ..Default::default()
                },
                NodeMetrics::default(),
            ),
        );

        let locked_nodes = BTreeSet::new();

        let models = build_view_models(&nodes, &locked_nodes);
        let model = models
            .iter()
            .find(|model| model.id == "node-1")
            .expect("model missing");

        assert!(matches!(
            model.reachability_progress,
            ReachabilityProgress::InProgress(12)
        ));
    }

    #[test]
    fn reachability_metrics_mark_node_unreachable_with_reason() {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "node-1".to_string(),
            node_state(
                Some(RegistryNode {
                    service_name: "node-1".to_string(),
                    metrics_port: 3000,
                    status: ServiceStatus::Running,
                    reachability_progress: ReachabilityProgress::InProgress(50),
                    last_failure: Some(CriticalFailure {
                        reason: "Port unreachable".to_string(),
                        date_time: Utc::now(),
                    }),
                    version: "0.1.0".to_string(),
                }),
                ReachabilityStatusValues {
                    progress: ReachabilityProgress::Complete,
                    public: false,
                    private: false,
                    upnp: false,
                },
                NodeMetrics::default(),
            ),
        );

        let locked_nodes = BTreeSet::new();

        let models = build_view_models(&nodes, &locked_nodes);
        let model = models
            .iter()
            .find(|model| model.id == "node-1")
            .expect("model missing");

        match &model.lifecycle {
            LifecycleState::Unreachable { reason } => {
                assert_eq!(reason.as_deref(), Some("Error (Port unreachable)"));
            }
            state => panic!("Expected unreachable lifecycle, got {state:?}"),
        }
    }

    #[test]
    fn metrics_endpoint_failure_with_reason_shows_error() {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "node-1".to_string(),
            node_state(
                Some(RegistryNode {
                    service_name: "node-1".to_string(),
                    metrics_port: 3000,
                    status: ServiceStatus::Stopped,
                    reachability_progress: ReachabilityProgress::NotRun,
                    last_failure: Some(CriticalFailure {
                        reason: "Process crashed".to_string(),
                        date_time: Utc::now(),
                    }),
                    version: "0.1.0".to_string(),
                }),
                ReachabilityStatusValues::default(),
                NodeMetrics {
                    endpoint_online: false,
                    ..Default::default()
                },
            ),
        );

        let locked_nodes = BTreeSet::new();

        let models = build_view_models(&nodes, &locked_nodes);
        let model = models
            .iter()
            .find(|model| model.id == "node-1")
            .expect("model missing");

        match &model.lifecycle {
            LifecycleState::Unreachable { reason } => {
                assert_eq!(reason.as_deref(), Some("Error (Process crashed)"));
            }
            state => panic!("Expected unreachable lifecycle, got {state:?}"),
        }
    }

    #[test]
    fn metrics_endpoint_failure_without_reason_keeps_registry_state() {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "node-1".to_string(),
            node_state(
                Some(RegistryNode {
                    service_name: "node-1".to_string(),
                    metrics_port: 3000,
                    status: ServiceStatus::Stopped,
                    reachability_progress: ReachabilityProgress::NotRun,
                    last_failure: None,
                    version: "0.1.0".to_string(),
                }),
                ReachabilityStatusValues::default(),
                NodeMetrics {
                    endpoint_online: false,
                    ..Default::default()
                },
            ),
        );

        let locked_nodes = BTreeSet::new();

        let models = build_view_models(&nodes, &locked_nodes);
        let model = models
            .iter()
            .find(|model| model.id == "node-1")
            .expect("model missing");

        assert!(matches!(model.lifecycle, LifecycleState::Stopped));
    }

    #[test]
    fn running_node_with_historic_unreachable_failure_recovers_when_metrics_ok() {
        let mut node = registry_node(ServiceStatus::Running);
        node.last_failure = Some(CriticalFailure {
            reason: "Unreachable".to_string(),
            date_time: Utc::now(),
        });

        let mut nodes = BTreeMap::new();
        nodes.insert(
            "node-1".to_string(),
            node_state(
                Some(node),
                ReachabilityStatusValues {
                    progress: ReachabilityProgress::Complete,
                    public: true,
                    private: false,
                    upnp: false,
                },
                NodeMetrics::default(),
            ),
        );

        let locked_nodes = BTreeSet::new();

        let models = build_view_models(&nodes, &locked_nodes);
        let model = models
            .iter()
            .find(|model| model.id == "node-1")
            .expect("model missing");

        assert!(matches!(model.lifecycle, LifecycleState::Running));
        assert_eq!(model.last_failure.as_deref(), Some("Unreachable"));
    }
}
