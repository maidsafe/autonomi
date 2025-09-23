// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::{ReachabilityProgress, ServiceStatus, fs::CriticalFailure};
use serde::{Deserialize, Serialize};
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

fn lifecycle_from_transition(transition: Option<&TransitionEntry>) -> Option<LifecycleState> {
    let entry = transition?;
    Some(match entry.command {
        CommandKind::Start | CommandKind::Maintain => LifecycleState::Starting,
        CommandKind::Stop => LifecycleState::Stopping,
        CommandKind::Add => LifecycleState::Adding,
        CommandKind::Remove => LifecycleState::Removing,
    })
}

fn lifecycle_from_provisioning(
    is_provisioning: bool,
    registry: Option<&RegistryNode>,
) -> Option<LifecycleState> {
    (is_provisioning && registry.is_none()).then_some(LifecycleState::Adding)
}

fn lifecycle_from_registry(
    registry: Option<&RegistryNode>,
    desired: DesiredNodeState,
) -> LifecycleState {
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

/// Determines the lifecycle state for a node.
///
/// Precedence rules:
/// 1. Active transitions (`transition`) always win so in-flight actions surface immediately.
/// 2. Provisioning intent (`is_provisioning`) takes priority when no registry entry exists yet.
/// 3. Registry status + desired intent provide the steady-state fallback.
pub fn derive_lifecycle_state(
    registry: Option<&RegistryNode>,
    desired: DesiredNodeState,
    is_provisioning: bool,
    transition: Option<&TransitionEntry>,
) -> LifecycleState {
    // Precedence intentionally ordered: explicit transitions > provisioning > registry snapshot.
    if let Some(state) = lifecycle_from_transition(transition) {
        return state;
    }

    if let Some(state) = lifecycle_from_provisioning(is_provisioning, registry) {
        return state;
    }

    lifecycle_from_registry(registry, desired)
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
    fn transition_supersedes_provisioning() {
        let lifecycle = derive_lifecycle_state(
            None,
            DesiredNodeState::Run,
            true,
            Some(&TransitionEntry {
                command: CommandKind::Remove,
                started_at: Instant::now(),
            }),
        );
        assert_eq!(lifecycle, LifecycleState::Removing);
    }

    #[test]
    fn lifecycle_refreshing_when_registry_missing_and_not_provisioning() {
        let lifecycle = derive_lifecycle_state(None, DesiredNodeState::Run, false, None);
        assert_eq!(lifecycle, LifecycleState::Refreshing);
    }

    #[test]
    fn maintain_transition_is_treated_as_starting() {
        let lifecycle = derive_lifecycle_state(
            Some(&registry_node(ServiceStatus::Stopped)),
            DesiredNodeState::Run,
            false,
            Some(&TransitionEntry {
                command: CommandKind::Maintain,
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
}
