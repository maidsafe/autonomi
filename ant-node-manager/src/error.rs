// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    #[error("The PID of the process was not found after starting it.")]
    PidNotFoundAfterStarting,
    #[error("The PID of the process was not set.")]
    PidNotSet,
    #[error("The metric port of the node is empty")]
    MetricPortEmpty,
    #[error("multithreaded join error in node registry refreshment")]
    NodeRegistryRefreshmentJoinError,
    #[error(transparent)]
    SemverError(#[from] semver::Error),
    #[error("The service(s) is already running: {0:?}")]
    ServiceAlreadyRunning(Vec<String>),
    #[error("The service(s) is not running: {0:?}")]
    ServiceNotRunning(Vec<String>),
    #[error(transparent)]
    ServiceManagementError(#[from] ant_service_management::Error),
    #[error("The service status is not as expected. Expected: {expected:?}")]
    ServiceStatusMismatch {
        expected: ant_service_management::ServiceStatus,
    },
}
