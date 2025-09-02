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

#[derive(Debug, Clone, PartialEq, Serialize, Display, Deserialize)]
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
    ResetNodesCompleted {
        trigger_start_node: bool,
    },
    UpdateNodesCompleted,
    ErrorLoadingNodeRegistry {
        raw_error: String,
    },
    ErrorGettingNodeRegistryPath {
        raw_error: String,
    },
    ErrorScalingUpNodes {
        raw_error: String,
    },
    ErrorResettingNodes {
        raw_error: String,
    },
    ErrorUpdatingNodes {
        raw_error: String,
    },
    ErrorAddingNodes {
        raw_error: String,
    },
    ErrorStartingNodes {
        services: Vec<String>,
        raw_error: String,
    },
    ErrorStoppingNodes {
        services: Vec<String>,
        raw_error: String,
    },
    ErrorRemovingNodes {
        services: Vec<String>,
        raw_error: String,
    },
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

    AddNode,
    StartNodes,
    StopNodes,
    RemoveNodes,
    StartStopNode,
    TriggerRemoveNode,
    TriggerNodeLogs,
    ResetNodes,
    UpgradeNodeVersion,

    // Completion events
    StartNodesCompleted {
        service_name: String,
    },
    StopNodesCompleted {
        service_name: String,
    },
    AddNodesCompleted {
        service_name: String,
    },
    RemoveNodesCompleted {
        service_name: String,
    },
}
