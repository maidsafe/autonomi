// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::{
    NodeServiceData, ReachabilityProgress, ServiceStatus, fs::CriticalFailure,
    metric::ReachabilityStatusValues,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

/// Identifier used across registry, desired topology, and transitions.
pub type NodeId = String;

/// Immutable snapshot of the registry at a given instant.
#[derive(Clone, Debug)]
pub struct RegistrySnapshot {
    pub nodes: BTreeMap<NodeId, RegistryNode>,
    pub seen_at: Instant,
}

impl Default for RegistrySnapshot {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            seen_at: Instant::now(),
        }
    }
}

impl RegistrySnapshot {
    pub fn from_services(services: &[NodeServiceData]) -> Self {
        let nodes = services
            .iter()
            .map(|service| {
                let node = RegistryNode {
                    service_name: service.service_name.clone(),
                    status: service.status.clone(),
                    reachability_progress: service.reachability_progress.clone(),
                    last_failure: service.last_critical_failure.clone(),
                    version: service.version.clone(),
                };
                (service.service_name.clone(), node)
            })
            .collect();

        Self {
            nodes,
            seen_at: Instant::now(),
        }
    }

    pub fn running_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| node.status == ServiceStatus::Running)
            .count()
    }
}

#[derive(Clone, Debug)]
pub struct RegistryNode {
    pub service_name: String,
    pub status: ServiceStatus,
    pub reachability_progress: ReachabilityProgress,
    pub last_failure: Option<CriticalFailure>,
    pub version: String,
}

/// Desired topology expresses user intent.
#[derive(Clone, Debug, Default)]
pub struct DesiredTopology {
    /// Desired number of nodes up and running.
    pub desired_running_count: u64,
    /// Explicit per-node intent overrides (start/stop/remove).
    pub node_targets: BTreeMap<NodeId, DesiredNodeState>,
    /// Nodes that should exist but are not yet in the registry (pending add).
    pub provisioning: BTreeSet<NodeId>,
}

impl DesiredTopology {
    pub fn set_desired_running_count(&mut self, count: u64) {
        self.desired_running_count = count;
    }

    pub fn set_node_target(&mut self, id: NodeId, target: DesiredNodeState) {
        if target == DesiredNodeState::FollowCluster {
            self.node_targets.remove(&id);
        } else {
            self.node_targets.insert(id, target);
        }
    }

    pub fn mark_provisioning<I: IntoIterator<Item = NodeId>>(&mut self, ids: I) {
        self.provisioning.extend(ids);
    }

    pub fn unmark_provisioning(&mut self, id: &str) {
        self.provisioning.remove(id);
    }

    pub fn desired_state_for(&self, id: &str) -> DesiredNodeState {
        self.node_targets
            .get(id)
            .cloned()
            .unwrap_or(DesiredNodeState::FollowCluster)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
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

/// Tracks operations issued but not yet reflected in the registry.
#[derive(Clone, Debug, Default)]
pub struct TransitionState {
    pub entries: BTreeMap<NodeId, TransitionEntry>,
}

impl TransitionState {
    pub fn mark(&mut self, id: NodeId, command: CommandKind) {
        self.entries.insert(
            id,
            TransitionEntry {
                command,
                started_at: Instant::now(),
            },
        );
    }

    pub fn unmark(&mut self, id: &str) {
        self.entries.remove(id);
    }

    pub fn get(&self, id: &str) -> Option<&TransitionEntry> {
        self.entries.get(id)
    }
}

#[derive(Clone, Debug)]
pub struct TransitionEntry {
    pub command: CommandKind,
    pub started_at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
}

impl NodeViewModel {
    pub fn new(
        id: String,
        lifecycle: LifecycleState,
        registry: Option<&RegistryNode>,
        reachability_status: ReachabilityStatusValues,
        metrics: NodeMetrics,
        locked: bool,
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
    registry: &RegistrySnapshot,
    desired: &DesiredTopology,
    transitions: &TransitionState,
    reachability: &BTreeMap<NodeId, ReachabilityStatusValues>,
    metrics: &BTreeMap<NodeId, NodeMetrics>,
) -> Vec<NodeViewModel> {
    let mut ids: BTreeSet<NodeId> = registry.nodes.keys().cloned().collect();
    ids.extend(desired.provisioning.iter().cloned());

    let mut models = Vec::with_capacity(ids.len());
    for id in ids {
        let registry_node = registry.nodes.get(&id);
        let desired_state = desired.desired_state_for(&id);
        let is_provisioning = desired.provisioning.contains(&id);
        let transition = transitions.get(&id);
        let lifecycle =
            derive_lifecycle_state(registry_node, desired_state, is_provisioning, transition);
        let reachability_status = reachability.get(&id).cloned().unwrap_or_default();
        let metrics = metrics.get(&id).cloned().unwrap_or_default();
        let locked = matches!(
            transition.map(|t| &t.command),
            Some(CommandKind::Remove | CommandKind::Stop | CommandKind::Start | CommandKind::Add)
        );
        models.push(NodeViewModel::new(
            id,
            lifecycle,
            registry_node,
            reachability_status,
            metrics,
            locked,
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
            status,
            reachability_progress: ReachabilityProgress::NotRun,
            last_failure: None,
            version: "0.1.0".to_string(),
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
        let mut registry = RegistrySnapshot::default();
        registry.nodes.insert(
            "node-1".to_string(),
            RegistryNode {
                service_name: "node-1".to_string(),
                status: ServiceStatus::Running,
                reachability_progress: ReachabilityProgress::NotRun,
                last_failure: None,
                version: "0.1.0".to_string(),
            },
        );

        let desired = DesiredTopology::default();
        let transitions = TransitionState::default();
        let mut reachability = BTreeMap::new();
        reachability.insert(
            "node-1".to_string(),
            ReachabilityStatusValues {
                progress: ReachabilityProgress::InProgress(12),
                ..Default::default()
            },
        );
        let metrics = BTreeMap::new();

        let models = build_view_models(&registry, &desired, &transitions, &reachability, &metrics);
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
        let mut registry = RegistrySnapshot::default();
        registry.nodes.insert(
            "node-1".to_string(),
            RegistryNode {
                service_name: "node-1".to_string(),
                status: ServiceStatus::Running,
                reachability_progress: ReachabilityProgress::InProgress(50),
                last_failure: Some(CriticalFailure {
                    reason: "Port unreachable".to_string(),
                    date_time: Utc::now(),
                }),
                version: "0.1.0".to_string(),
            },
        );

        let desired = DesiredTopology::default();
        let transitions = TransitionState::default();
        let mut reachability = BTreeMap::new();
        reachability.insert(
            "node-1".to_string(),
            ReachabilityStatusValues {
                progress: ReachabilityProgress::Complete,
                public: false,
                private: false,
                upnp: false,
            },
        );
        let metrics = BTreeMap::new();

        let models = build_view_models(&registry, &desired, &transitions, &reachability, &metrics);
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
        let mut registry = RegistrySnapshot::default();
        registry.nodes.insert(
            "node-1".to_string(),
            RegistryNode {
                service_name: "node-1".to_string(),
                status: ServiceStatus::Stopped,
                reachability_progress: ReachabilityProgress::NotRun,
                last_failure: Some(CriticalFailure {
                    reason: "Process crashed".to_string(),
                    date_time: Utc::now(),
                }),
                version: "0.1.0".to_string(),
            },
        );

        let desired = DesiredTopology::default();
        let transitions = TransitionState::default();
        let reachability = BTreeMap::new();
        let mut metrics = BTreeMap::new();
        metrics.insert(
            "node-1".to_string(),
            NodeMetrics {
                endpoint_online: false,
                ..Default::default()
            },
        );

        let models = build_view_models(&registry, &desired, &transitions, &reachability, &metrics);
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
        let mut registry = RegistrySnapshot::default();
        registry.nodes.insert(
            "node-1".to_string(),
            RegistryNode {
                service_name: "node-1".to_string(),
                status: ServiceStatus::Stopped,
                reachability_progress: ReachabilityProgress::NotRun,
                last_failure: None,
                version: "0.1.0".to_string(),
            },
        );

        let desired = DesiredTopology::default();
        let transitions = TransitionState::default();
        let reachability = BTreeMap::new();
        let mut metrics = BTreeMap::new();
        metrics.insert(
            "node-1".to_string(),
            NodeMetrics {
                endpoint_online: false,
                ..Default::default()
            },
        );

        let models = build_view_models(&registry, &desired, &transitions, &reachability, &metrics);
        let model = models
            .iter()
            .find(|model| model.id == "node-1")
            .expect("model missing");

        assert!(matches!(model.lifecycle, LifecycleState::Stopped));
    }

    #[test]
    fn running_node_with_historic_unreachable_failure_recovers_when_metrics_ok() {
        let mut registry = RegistrySnapshot::default();
        let mut node = registry_node(ServiceStatus::Running);
        node.last_failure = Some(CriticalFailure {
            reason: "Unreachable".to_string(),
            date_time: Utc::now(),
        });
        registry.nodes.insert("node-1".to_string(), node);

        let desired = DesiredTopology::default();
        let transitions = TransitionState::default();
        let mut reachability = BTreeMap::new();
        reachability.insert(
            "node-1".to_string(),
            ReachabilityStatusValues {
                progress: ReachabilityProgress::Complete,
                public: true,
                private: false,
                upnp: false,
            },
        );
        let metrics = BTreeMap::new();

        let models = build_view_models(&registry, &desired, &transitions, &reachability, &metrics);
        let model = models
            .iter()
            .find(|model| model.id == "node-1")
            .expect("model missing");

        assert!(matches!(model.lifecycle, LifecycleState::Running));
        assert_eq!(model.last_failure.as_deref(), Some("Unreachable"));
    }
}
