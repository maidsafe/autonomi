// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::enum_variant_names)]

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, Clone)]
pub enum AddNodeError {
    #[error("A genesis node already exists")]
    GenesisNodeAlreadyExists,
    #[error("A genesis node can only be added as a single node")]
    MultipleGenesisNodesNotAllowed,
    #[error("Could not get filename from the antnode download path")]
    FileNameExtractionFailed,
    #[error(
        "Failed to add one or more services. However, any services that were successfully added will be usable."
    )]
    ServiceAdditionFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum PortRangeError {
    #[error("Port range must be in the format 'start-end'")]
    InvalidFormat,
    #[error("End port must be greater than start port")]
    InvalidRange,
    #[error("The count ({actual}) does not match the number of ports ({expected})")]
    CountMismatch { expected: u16, actual: u16 },
    #[error(transparent)]
    ParseError(#[from] std::num::ParseIntError),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    AddNode(#[from] AddNodeError),
    #[error(transparent)]
    AntReleasesError(#[from] ant_releases::Error),
    #[error("Batch operation failed: {details}")]
    BatchOperationFailed { details: String },
    #[error("Failed to download release: {release} after maximum retries")]
    DownloadFailure { release: String },
    #[error("Failed to get binary output")]
    FailedToGetBinary,
    #[error("Failed to build binary: {bin_name}")]
    FailedToBuildBinary { bin_name: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Listening address not found")]
    ListeningAddressNotFound,
    #[error(transparent)]
    MetricsActionError(#[from] ant_service_management::metric::MetricsActionError),
    #[cfg(unix)]
    #[error(transparent)]
    NixError(#[from] nix::errno::Errno),
    #[error("The PeerId of the node was not set for {service_name}")]
    PeerIdNotSet { service_name: String },
    #[error("The PID of the process was not found after starting it.")]
    PidNotFoundAfterStarting,
    #[error("The PID of the process was not set.")]
    PidNotSet,
    #[error("Port {port} is being used by another service")]
    PortInUse { port: u16 },
    #[error(transparent)]
    PortRange(#[from] PortRangeError),
    #[error(transparent)]
    SemverError(#[from] semver::Error),
    #[error("Unable to remove a running service {0:?}, stop this service first before removing")]
    ServiceAlreadyRunning(Vec<String>),
    #[error("Failed to {verb} one or more services")]
    ServiceBatchOperationFailed { verb: String },
    #[error(transparent)]
    ServiceManagementError(#[from] ant_service_management::Error),
    #[error("The service(s) is not running: {0:?}")]
    ServiceNotRunning(Vec<String>),
    #[error("The service '{0}' was not found")]
    ServiceNotFound(String),
    #[error("Failed to {operation} one or more services. {suggestion}")]
    ServiceOperationFailed {
        operation: String,
        suggestion: String,
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
    #[error("The service status is not as expected. Expected: {expected:?}")]
    ServiceStatusMismatch {
        expected: ant_service_management::ServiceStatus,
    },
    #[error(transparent)]
    TemplateError(#[from] indicatif::style::TemplateError),
    #[error("Failed to determine user antnode data directory")]
    UserDataDirNotFound,
    #[error("User '{user}' does not exist on the system")]
    UserNotFound { user: String },
    #[error(transparent)]
    VarError(#[from] std::env::VarError),
}
