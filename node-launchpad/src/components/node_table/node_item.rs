// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::{
    ReachabilityProgress, ServiceStatus, fs::CriticalFailure, metric::ReachabilityStatusValues,
};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeDisplayStatus {
    #[default]
    Added,
    Adding,
    Maintaining,
    Running,
    Starting,
    ReachabilityCheck,
    Stopping,
    Stopped,
    Removing,
    Removed,
    Refreshing,
    Updating,
    Unreachable,
}

impl From<&ServiceStatus> for NodeDisplayStatus {
    fn from(status: &ServiceStatus) -> Self {
        match status {
            ServiceStatus::Added => NodeDisplayStatus::Added,
            ServiceStatus::Running => NodeDisplayStatus::Running,
            ServiceStatus::Stopped => NodeDisplayStatus::Stopped,
            ServiceStatus::Removed => NodeDisplayStatus::Removed,
        }
    }
}

impl fmt::Display for NodeDisplayStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            NodeDisplayStatus::Adding => write!(f, "Adding"),
            NodeDisplayStatus::Added => write!(f, "Added"),
            NodeDisplayStatus::Maintaining => write!(f, "Maintaining"),
            NodeDisplayStatus::Running => write!(f, "Running"),
            NodeDisplayStatus::Starting => write!(f, "Starting"),
            NodeDisplayStatus::ReachabilityCheck => write!(f, "ReachabilityCheck"),
            NodeDisplayStatus::Stopping => write!(f, "Stopping"),
            NodeDisplayStatus::Stopped => write!(f, "Stopped"),
            NodeDisplayStatus::Removing => write!(f, "Removing"),
            NodeDisplayStatus::Refreshing => write!(f, "Refreshing"),
            NodeDisplayStatus::Removed => write!(f, "Removed"),
            NodeDisplayStatus::Updating => write!(f, "Updating"),
            NodeDisplayStatus::Unreachable => write!(f, "Unreachable"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeItem {
    pub service_name: String,
    pub version: String,
    pub rewards_wallet_balance: usize,
    pub memory: usize,
    pub mbps: String,
    pub records: usize,
    pub peers: usize,
    pub connections: usize,
    pub reachability_progress: ReachabilityProgress,
    pub reachability_status: ReachabilityStatusValues,
    pub last_critical_failure: Option<CriticalFailure>,
    pub locked: bool,
    pub node_display_status: NodeDisplayStatus,
    pub service_status: ServiceStatus,
}

impl NodeItem {
    pub fn lock(&mut self) {
        self.locked = true;
    }

    pub fn unlock(&mut self) {
        self.locked = false;
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Update the display status of the node item.
    ///
    /// Note: The node display status will be overriden by `sync_node_service_data`
    /// method in `NodeTableState` when new data is fetched from the registry.
    pub fn update_node_display_status(&mut self, status: NodeDisplayStatus) {
        self.node_display_status = status;
    }

    pub fn can_start(&self) -> bool {
        !self.locked
            && matches!(
                self.node_display_status,
                NodeDisplayStatus::Stopped
                    | NodeDisplayStatus::Added
                    | NodeDisplayStatus::Unreachable
            )
    }

    /// We can safely stop a node if it is `Running` or if it is performing `ReachabilityCheck`
    pub fn can_stop(&self) -> bool {
        !self.locked
            && matches!(
                self.node_display_status,
                NodeDisplayStatus::Running | NodeDisplayStatus::ReachabilityCheck
            )
    }

    pub fn can_upgrade(&self) -> bool {
        !self.locked && !matches!(self.node_display_status, NodeDisplayStatus::Removed)
    }

    pub fn lock_for_operation(&mut self, operation_status: NodeDisplayStatus) {
        self.lock();
        self.update_node_display_status(operation_status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_service_management::ServiceStatus;

    fn build_item(status: ServiceStatus) -> NodeItem {
        NodeItem {
            service_name: "antnode-1".to_string(),
            version: "0.1.0".to_string(),
            rewards_wallet_balance: 0,
            memory: 0,
            mbps: String::new(),
            records: 0,
            peers: 0,
            connections: 0,
            reachability_progress: ReachabilityProgress::NotRun,
            reachability_status: ReachabilityStatusValues::default(),
            last_critical_failure: None,
            locked: false,
            node_display_status: NodeDisplayStatus::from(&status),
            service_status: status,
        }
    }

    #[test]
    fn can_start_and_stop_respect_status_and_lock() {
        let mut added = build_item(ServiceStatus::Added);
        assert!(added.can_start());
        added.lock();
        assert!(!added.can_start());

        let mut running = build_item(ServiceStatus::Running);
        assert!(running.can_stop());
        running.lock();
        assert!(!running.can_stop());

        let stopped = build_item(ServiceStatus::Stopped);
        assert!(stopped.can_start());
    }

    #[test]
    fn lock_for_operation_sets_display_status() {
        let mut node = build_item(ServiceStatus::Running);
        assert!(!node.is_locked());
        node.lock_for_operation(NodeDisplayStatus::Stopping);
        assert!(node.is_locked());
        assert_eq!(node.node_display_status, NodeDisplayStatus::Stopping);
    }
}
