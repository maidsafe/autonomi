// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::connection_mode::NodeConnectionMode;
use std::fmt;

#[derive(Default, Debug, Copy, Clone, PartialEq)]
pub enum NodeStatus {
    #[default]
    Added,
    Running,
    Starting,
    Stopped,
    Removed,
    Updating,
}

impl From<&ant_service_management::ServiceStatus> for NodeStatus {
    fn from(status: &ant_service_management::ServiceStatus) -> Self {
        match status {
            ant_service_management::ServiceStatus::Added => NodeStatus::Added,
            ant_service_management::ServiceStatus::Running => NodeStatus::Running,
            ant_service_management::ServiceStatus::Stopped => NodeStatus::Stopped,
            ant_service_management::ServiceStatus::Removed => NodeStatus::Removed,
        }
    }
}

impl fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            NodeStatus::Added => write!(f, "Added"),
            NodeStatus::Running => write!(f, "Running"),
            NodeStatus::Starting => write!(f, "Starting"),
            NodeStatus::Stopped => write!(f, "Stopped"),
            NodeStatus::Removed => write!(f, "Removed"),
            NodeStatus::Updating => write!(f, "Updating"),
        }
    }
}

#[derive(Default, Debug, Clone)]
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
    pub status: NodeStatus,
    pub failure: Option<(chrono::DateTime<chrono::Utc>, String)>,
}

impl NodeItem {
    pub fn lock(&mut self) {
        self.locked = true;
    }

    pub fn unlock(&mut self) {
        self.locked = false;
    }

    pub fn update_status(&mut self, status: NodeStatus) {
        self.status = status;
    }
}
