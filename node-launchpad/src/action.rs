// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    components::popup::error_popup::ErrorPopup,
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
    StoreUpnpSetting(bool),
    StorePortRange(Option<(u32, u32)>),
    StoreRewardsAddress(EvmAddress),
    StoreRunningNodeCount(u64),

    UpgradeLaunchpadActions(UpgradeLaunchpadActions),

    ShowErrorPopup(ErrorPopup),
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
    TriggerPortRangeEdit,
    TriggerRewardsAddress,
    TriggerUpdateNodes,
    TriggerResetNodes,
    TriggerAccessLogs,
    ToggleUpnpSetting,
    UpdateStorageDrive(PathBuf, String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Display, Deserialize)]
pub enum UpgradeLaunchpadActions {
    UpdateAvailable {
        current_version: String,
        latest_version: String,
    },
}

#[derive(custom_debug::Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeTableActions {
    // State updates FROM NodeTable TO Status (push only)
    StateChanged {
        node_count: u64,
        has_running_nodes: bool,
        has_nodes: bool,
    },
    RegistryUpdated {
        #[debug(skip)]
        all_nodes_data: Vec<NodeServiceData>,
    },
    NodeManagementCommand(NodeManagementCommand),
    NodeManagementResponse(NodeManagementResponse),
    TriggerRemoveNodePopup,
    TriggerNodeLogs,

    // Navigation actions
    NavigateUp,
    NavigateDown,
    NavigateHome,
    NavigateEnd,
    NavigatePageUp,
    NavigatePageDown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeManagementCommand {
    MaintainNodes,
    AddNode,
    StartNodes,
    StopNodes,
    RemoveNodes,
    ToggleNode,
    UpgradeNodes,
    ResetNodes,
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
