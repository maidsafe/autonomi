// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::ServiceStatus;

use crate::connection_mode::NodeConnectionMode;
use std::fmt;

#[derive(Default, Debug, Copy, Clone, PartialEq)]
pub enum NodeDisplayStatus {
    #[default]
    Added,
    Adding,
    Maintaining,
    Running,
    Starting,
    Stopping,
    Stopped,
    Removing,
    Removed,
    Updating,
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
            NodeDisplayStatus::Stopping => write!(f, "Stopping"),
            NodeDisplayStatus::Stopped => write!(f, "Stopped"),
            NodeDisplayStatus::Removing => write!(f, "Removing"),
            NodeDisplayStatus::Removed => write!(f, "Removed"),
            NodeDisplayStatus::Updating => write!(f, "Updating"),
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
    pub locked: bool,
    pub mode: NodeConnectionMode,
    pub node_display_status: NodeDisplayStatus,
    pub service_status: ServiceStatus,
    pub failure: Option<(chrono::DateTime<chrono::Utc>, String)>,
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
                NodeDisplayStatus::Stopped | NodeDisplayStatus::Added
            )
    }

    pub fn can_stop(&self) -> bool {
        !self.locked && matches!(self.node_display_status, NodeDisplayStatus::Running)
    }

    pub fn can_upgrade(&self) -> bool {
        !self.locked && !matches!(self.node_display_status, NodeDisplayStatus::Removed)
    }

    pub fn lock_for_operation(&mut self, operation_status: NodeDisplayStatus) {
        self.lock();
        self.update_node_display_status(operation_status);
    }
}
