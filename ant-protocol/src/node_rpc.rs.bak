// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::eyre::Error;
use std::time::Duration;

#[derive(Debug)]
/// To be sent to the main thread in order to stop/restart the execution of the antnode app.
pub enum NodeCtrl {
    /// Request to stop the execution of the antnode app, providing an error as a reason for it.
    Stop {
        delay: Duration,
        result: StopResult,
    },
    /// Request to restart the execution of the antnode app, retrying to join the network, after the requested delay.
    /// Set `retain_peer_id` to `true` if you want to re-use the same root dir/secret keys/PeerId.
    Restart {
        delay: Duration,
        retain_peer_id: bool,
    },
    // Request to update the antnode app, and restart it, after the requested delay.
    Update(Duration),
}

#[derive(Debug)]
pub enum StopResult {
    Success(String),
    Error(Error),
}
