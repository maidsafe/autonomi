// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow enum variant names that end with Error - common pattern with thiserror
#![allow(clippy::enum_variant_names)]

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("The PID of the process was not found after starting it.")]
    PidNotFoundAfterStarting,
    #[error("The PID of the process was not set.")]
    PidNotSet,
    #[error(transparent)]
    SemverError(#[from] semver::Error),
    #[error("Unable to remove a running service {0:?}, stop this service first before removing")]
    ServiceAlreadyRunning(Vec<String>),
    #[error("The service(s) is not running: {0:?}")]
    ServiceNotRunning(Vec<String>),
    #[error(transparent)]
    ServiceManagementError(#[from] ant_service_management::Error),
    #[error("The service status is not as expected. Expected: {expected:?}")]
    ServiceStatusMismatch {
        expected: ant_service_management::ServiceStatus,
    },
    #[error("Service '{service_name}' progress monitoring timed out after {timeout:?}")]
    ServiceProgressTimeout {
        service_name: String,
        timeout: std::time::Duration,
    },
    #[error("Service '{service_name}' failed to start: {reason}")]
    ServiceStartupFailed {
        service_name: String,
        reason: String,
    },
}
