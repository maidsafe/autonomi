// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[derive(Debug, thiserror::Error)]
pub enum NodeManagementError {
    #[error(transparent)]
    AntNodeManager(#[from] ant_node_manager::Error),
    #[error("No ports available in the specified range up to {max_port}")]
    NoAvailablePorts { max_port: u16 },
    #[error("Rewards address has not been set")]
    RewardsAddressNotSet,
}
