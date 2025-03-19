// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use serde::{Deserialize, Serialize};

use crate::connection_mode::ConnectionMode;

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scene {
    #[default]
    Status,
    Options,
    Help,
    ChangeDrivePopUp,
    ChangeConnectionModePopUp,
    ChangePortsPopUp {
        connection_mode_old_value: Option<ConnectionMode>,
    },
    StatusRewardsAddressPopUp,
    OptionsRewardsAddressPopUp,
    ManageNodesPopUp {
        amount_of_nodes: usize,
    },
    ResetNodesPopUp,
    UpgradeNodesPopUp,
    RemoveNodePopUp,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputMode {
    #[default]
    Navigation,
    Entry,
}
