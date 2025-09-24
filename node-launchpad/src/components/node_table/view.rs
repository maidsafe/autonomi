use super::lifecycle::{
    CommandKind, LifecycleState, NodeId, NodeMetrics, RegistryNode, derive_lifecycle_state,
};
use super::state::NodeState;
use super::table_state::StatefulTable;
use ant_service_management::{
    ReachabilityProgress, ServiceStatus, metric::ReachabilityStatusValues,
};
use std::collections::BTreeMap;

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
        let status_snapshot = StatusSnapshot::from_registry(registry);
        let (effective_lifecycle, failure_reason) = decorate_lifecycle_with_reachability(
            lifecycle,
            registry,
            &reachability_status,
            &metrics,
            status_snapshot.failure_reason.clone(),
        );

        Self {
            id,
            lifecycle: effective_lifecycle,
            status: status_snapshot.status,
            version: status_snapshot.version,
            reachability_progress: resolve_progress(&reachability_status, status_snapshot.progress),
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

pub fn build_view_models(nodes: &BTreeMap<NodeId, NodeState>) -> Vec<NodeViewModel> {
    let mut models = Vec::with_capacity(nodes.len());

    for (id, node_state) in nodes.iter() {
        let registry_node = node_state.registry.as_ref();
        let lifecycle = derive_lifecycle_state(
            registry_node,
            node_state.desired,
            node_state.is_provisioning,
            node_state.transition.as_ref(),
        );
        let reachability_status = node_state.reachability.clone();
        let metrics = node_state.metrics.clone();
        let locked = node_state.is_locked();

        models.push(NodeViewModel::new(
            id.clone(),
            lifecycle,
            registry_node,
            reachability_status,
            metrics,
            locked,
            node_state.transition_command(),
        ));
    }

    models
}

struct StatusSnapshot {
    status: String,
    progress: ReachabilityProgress,
    failure_reason: Option<String>,
    version: String,
}

impl StatusSnapshot {
    fn from_registry(registry: Option<&RegistryNode>) -> Self {
        if let Some(node) = registry {
            Self {
                status: format!("{:?}", node.status),
                progress: node.reachability_progress.clone(),
                failure_reason: node.last_failure.as_ref().map(|f| f.reason.clone()),
                version: node.version.clone(),
            }
        } else {
            Self {
                status: "Stopped".to_string(),
                progress: ReachabilityProgress::NotRun,
                failure_reason: None,
                version: String::new(),
            }
        }
    }
}

fn resolve_progress(
    reachability_status: &ReachabilityStatusValues,
    registry_progress: ReachabilityProgress,
) -> ReachabilityProgress {
    match reachability_status.progress.clone() {
        ReachabilityProgress::NotRun => registry_progress,
        other => other,
    }
}

fn decorate_lifecycle_with_reachability(
    lifecycle: LifecycleState,
    registry: Option<&RegistryNode>,
    reachability_status: &ReachabilityStatusValues,
    metrics: &NodeMetrics,
    mut failure_reason: Option<String>,
) -> (LifecycleState, Option<String>) {
    let mut effective = lifecycle;

    if reachability_status.indicates_unreachable() {
        let reason_text = failure_reason
            .take()
            .map(|reason| format!("Error ({reason})"))
            .unwrap_or_else(|| "Unreachable".to_string());
        failure_reason = Some(reason_text.clone());
        effective = LifecycleState::Unreachable {
            reason: Some(reason_text),
        };
    } else if !metrics.endpoint_online
        && let Some(reason) = failure_reason.take()
    {
        let reason_text = format!("Error ({reason})");
        failure_reason = Some(reason_text.clone());
        effective = LifecycleState::Unreachable {
            reason: Some(reason_text),
        };
    }

    if matches!(effective, LifecycleState::Unreachable { .. })
        && registry.is_some_and(|node| node.status == ServiceStatus::Running)
        && !reachability_status.indicates_unreachable()
        && metrics.endpoint_online
    {
        effective = LifecycleState::Running;
    }

    (effective, failure_reason)
}

#[cfg(test)]
mod tests {
    use super::super::lifecycle::{DesiredNodeState, TransitionEntry};
    use super::*;
    use ant_service_management::{
        ReachabilityProgress, ServiceStatus, fs::CriticalFailure, metric::ReachabilityStatusValues,
    };
    use chrono::Utc;
    use std::time::Instant;

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

    fn base_state() -> NodeState {
        NodeState {
            registry: None,
            desired: DesiredNodeState::FollowCluster,
            transition: None,
            is_provisioning: false,
            metrics: NodeMetrics::default(),
            reachability: ReachabilityStatusValues::default(),
            bandwidth_totals: (0, 0),
            awaiting_response: false,
        }
    }

    #[test]
    fn reachability_progress_prefers_metrics_update() {
        let mut nodes = BTreeMap::new();
        let mut state = base_state();
        state.registry = Some(registry_node(ServiceStatus::Running));
        state.reachability = ReachabilityStatusValues {
            progress: ReachabilityProgress::InProgress(12),
            ..Default::default()
        };
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.iter().find(|model| model.id == "node-1").unwrap();

        assert!(matches!(
            model.reachability_progress,
            ReachabilityProgress::InProgress(12)
        ));
    }

    #[test]
    fn reachability_metrics_mark_node_unreachable_with_reason() {
        let mut nodes = BTreeMap::new();
        let mut state = base_state();
        state.registry = Some(RegistryNode {
            service_name: "node-1".to_string(),
            metrics_port: 3000,
            status: ServiceStatus::Running,
            reachability_progress: ReachabilityProgress::InProgress(50),
            last_failure: Some(CriticalFailure {
                reason: "Port unreachable".to_string(),
                date_time: Utc::now(),
            }),
            version: "0.1.0".to_string(),
        });
        state.reachability = ReachabilityStatusValues {
            progress: ReachabilityProgress::Complete,
            public: false,
            private: false,
            upnp: false,
        };
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.iter().find(|model| model.id == "node-1").unwrap();

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
        let mut state = base_state();
        state.registry = Some(RegistryNode {
            service_name: "node-1".to_string(),
            metrics_port: 3000,
            status: ServiceStatus::Stopped,
            reachability_progress: ReachabilityProgress::NotRun,
            last_failure: Some(CriticalFailure {
                reason: "Process crashed".to_string(),
                date_time: Utc::now(),
            }),
            version: "0.1.0".to_string(),
        });
        state.metrics = NodeMetrics {
            endpoint_online: false,
            ..Default::default()
        };
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.iter().find(|model| model.id == "node-1").unwrap();

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
        let mut state = base_state();
        state.registry = Some(RegistryNode {
            service_name: "node-1".to_string(),
            metrics_port: 3000,
            status: ServiceStatus::Stopped,
            reachability_progress: ReachabilityProgress::NotRun,
            last_failure: None,
            version: "0.1.0".to_string(),
        });
        state.metrics = NodeMetrics {
            endpoint_online: false,
            ..Default::default()
        };
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.iter().find(|model| model.id == "node-1").unwrap();

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
        let mut state = base_state();
        state.registry = Some(node);
        state.reachability = ReachabilityStatusValues {
            progress: ReachabilityProgress::Complete,
            public: true,
            private: false,
            upnp: false,
        };
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.iter().find(|model| model.id == "node-1").unwrap();

        assert!(matches!(model.lifecycle, LifecycleState::Running));
        assert_eq!(model.last_failure.as_deref(), Some("Unreachable"));
    }

    #[test]
    fn provisioning_without_registry_shows_adding_state() {
        let mut nodes = BTreeMap::new();
        let mut state = base_state();
        state.is_provisioning = true;
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.first().unwrap();

        assert!(matches!(model.lifecycle, LifecycleState::Adding));
    }

    #[test]
    fn removal_intent_takes_precedence_over_running_status() {
        let mut nodes = BTreeMap::new();
        let mut state = base_state();
        state.registry = Some(registry_node(ServiceStatus::Running));
        state.desired = DesiredNodeState::Remove;
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.first().unwrap();

        assert!(matches!(model.lifecycle, LifecycleState::Removing));
    }

    #[test]
    fn transition_pending_command_is_exposed() {
        let mut nodes = BTreeMap::new();
        let mut state = base_state();
        state.transition = Some(TransitionEntry {
            command: CommandKind::Start,
            started_at: Instant::now(),
        });
        state.registry = Some(registry_node(ServiceStatus::Stopped));
        nodes.insert("node-1".to_string(), state);

        let models = build_view_models(&nodes);
        let model = models.first().unwrap();

        assert_eq!(model.pending_command, Some(CommandKind::Start));
        assert!(model.is_locked());
    }
}
