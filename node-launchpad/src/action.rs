// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    connection_mode::ConnectionMode,
    mode::{InputMode, Scene},
    node_stats::NodeStats,
};
use ant_evm::EvmAddress;
use ant_service_management::NodeServiceData;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use strum::Display;

#[derive(custom_debug::Debug, Clone, PartialEq, Serialize, Display, Deserialize)]
pub enum Action {
    StatusActions(StatusActions),
    OptionsActions(OptionsActions),
    NodeTableActions(NodeTableActions),

    SwitchScene(Scene),
    SwitchInputMode(InputMode),

    StoreStorageDrive(PathBuf, String),
    StoreConnectionMode(ConnectionMode),
    StorePortRange(u32, u32),
    StoreRewardsAddress(EvmAddress),
    StoreNodesToStart(u64),

    UpgradeLaunchpadActions(UpgradeLaunchpadActions),

    ShowErrorPopup(crate::error::ErrorPopup),
    SetNodeLogsTarget(String),

    LogsLoaded {
        node_name: String,
        #[debug(skip)]
        logs: Vec<String>,
        total_lines: usize,
    },
    LogsLoadError {
        node_name: String,
        error: String,
    },

    Tick,
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    Refresh,
    Error(String),
    Help,
    Noop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatusActions {
    NodesStatsObtained(NodeStats),
    TriggerManageNodes,
    TriggerRewardsAddress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Display, Deserialize)]
pub enum OptionsActions {
    TriggerChangeDrive,
    TriggerChangeConnectionMode,
    TriggerChangePortRange,
    TriggerRewardsAddress,
    TriggerUpdateNodes,
    TriggerResetNodes,
    TriggerAccessLogs,
    UpdateConnectionMode(ConnectionMode),
    UpdatePortRange(u32, u32),
    UpdateStorageDrive(PathBuf, String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Display, Deserialize)]
pub enum UpgradeLaunchpadActions {
    UpdateAvailable {
        current_version: String,
        latest_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeTableActions {
    // State updates FROM NodeTable TO Status (push only)
    StateChanged {
        node_count: u64,
        has_running_nodes: bool,
        has_nodes: bool,
    },
    RegistryUpdated {
        all_nodes_data: Vec<NodeServiceData>,
    },
    NodeManagementResponse(NodeManagementResponse),

    AddNode,
    StartNodes,
    StopNodes,
    RemoveNodes,
    StartStopNode,
    TriggerRemoveNode,
    TriggerNodeLogs,
    ResetNodes,
    UpgradeNodeVersion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeManagementResponse {
    MaintainNodes {
        error: Option<String>,
    },
    AddNode {
        error: Option<String>,
    },
    StartNodes {
        service_names: Vec<String>,
        error: Option<String>,
    },
    StopNodes {
        service_names: Vec<String>,
        error: Option<String>,
    },
    RemoveNodes {
        service_names: Vec<String>,
        error: Option<String>,
    },
    UpgradeNodes {
        service_names: Vec<String>,
        error: Option<String>,
    },
    ResetNodes {
        error: Option<String>,
    },
}
