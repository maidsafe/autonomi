// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::upnp::UpnpSupport;
use crate::{
    connection_mode::ConnectionMode,
    mode::{InputMode, Scene},
    node_stats::NodeStats,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use strum::Display;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Display, Deserialize)]
pub enum Action {
    StatusActions(StatusActions),
    OptionsActions(OptionsActions),

    SwitchScene(Scene),
    SwitchInputMode(InputMode),

    StoreStorageDrive(PathBuf, String),
    StoreConnectionMode(ConnectionMode),
    StorePortRange(u32, u32),
    StoreRewardsAddress(String),
    StoreNodesToStart(usize),

    SetUpnpSupport(UpnpSupport),

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Display, Deserialize)]
pub enum StatusActions {
    AddNode,
    StartNodes,
    StopNodes,
    RemoveNodes,
    StartStopNode,
    StartNodesCompleted {
        service_name: String,
    },
    StopNodesCompleted {
        service_name: String,
    },
    ResetNodesCompleted {
        trigger_start_node: bool,
    },
    RemoveNodesCompleted {
        service_name: String,
    },
    AddNodesCompleted {
        service_name: String,
    },
    UpdateNodesCompleted,
    SuccessfullyDetectedNatStatus,
    ErrorWhileRunningNatDetection,
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
    TriggerNodeLogs,
    TriggerRemoveNode,

    PreviousTableItem,
    NextTableItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Display, Deserialize)]
pub enum OptionsActions {
    ResetNodes,
    UpdateNodes,

    TriggerChangeDrive,
    TriggerChangeConnectionMode,
    TriggerChangePortRange,
    TriggerRewardsAddress,
    TriggerUpdateNodes,
    TriggerResetNodes,
    TriggerAccessLogs,
    UpdateConnectionMode(ConnectionMode),
    UpdatePortRange(u32, u32),
    UpdateRewardsAddress(String),
    UpdateStorageDrive(PathBuf, String),
}
