// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::enum_variant_names)]

pub type Result<T, E = Error> = std::result::Result<T, E>;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
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
    #[error("The count ({actual}) does not match the number of ports ({expected})")]
    CountMismatch { expected: u16, actual: u16 },
    #[error("Port range must be in the format 'start-end'")]
    InvalidFormat,
    #[error("End port must be greater than start port")]
    InvalidRange,
    #[error("Failed to parse number: {reason}")]
    ParseError { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    AddNode(#[from] AddNodeError),
    #[error("Failed to perform release operation: {reason}")]
    AntReleasesError { reason: String },
    #[error("Batch operation failed: {details}")]
    BatchOperationFailed { details: String },
    #[error("Failed to create directory at '{path}': {reason}")]
    DirectoryCreationFailed { path: PathBuf, reason: String },
    #[error("Failed to remove directory at '{path}': {reason}")]
    DirectoryRemovalFailed { path: PathBuf, reason: String },
    #[error("Failed to download release: {release} after maximum retries")]
    DownloadFailure { release: String },
    #[error("Failed to access environment variable '{var_name}': {reason}")]
    EnvironmentVariableAccessFailed { var_name: String, reason: String },
    #[error("Failed to build binary: {bin_name}")]
    FailedToBuildBinary { bin_name: String },
    #[error("Failed to get binary version for {bin_path}, reason: {reason}")]
    FailedToGetBinaryVersion { bin_path: PathBuf, reason: String },
    #[error("Failed to copy file from '{src}' to '{dst}': {reason}")]
    FileCopyFailed {
        src: PathBuf,
        dst: PathBuf,
        reason: String,
    },
    #[error("Failed to access file metadata for '{path}': {reason}")]
    FileMetadataAccessFailed { path: PathBuf, reason: String },
    #[error("Failed to access file or perform I/O operation: {reason}")]
    FileOperationFailed { reason: String },
    #[error("Failed to remove file '{path}': {reason}")]
    FileRemovalFailed { path: PathBuf, reason: String },
    #[error("Failed to perform I/O operation: {reason}")]
    IoError { reason: String },
    #[error("Failed to serialize/deserialize JSON: {reason}")]
    JsonError { reason: String },
    #[error("Listening address not found")]
    ListeningAddressNotFound,
    #[error(transparent)]
    MetricsActionError(#[from] ant_service_management::metric::MetricsActionError),
    #[error("Network operation '{operation}' failed for node '{node_name}': {reason}")]
    NetworkOperationFailed {
        node_name: String,
        operation: String,
        reason: String,
    },
    #[cfg(unix)]
    #[error("Unix system operation failed: {reason}")]
    NixError { reason: String },
    #[error("Failed to parse PeerId '{peer_id}': {reason}")]
    PeerIdParsingFailed { peer_id: String, reason: String },
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
    #[error("Failed to spawn process for '{binary_path}': {reason}")]
    ProcessSpawnFailed {
        binary_path: PathBuf,
        reason: String,
    },
    #[error("Registry operation '{operation}' failed: {reason}")]
    RegistryOperationFailed { operation: String, reason: String },
    #[error("Failed to parse version: {reason}")]
    SemverError { reason: String },
    #[error("Unable to remove a running service {0:?}, stop this service first before removing")]
    ServiceAlreadyRunning(Vec<String>),
    #[error("Failed to {verb} one or more services")]
    ServiceBatchOperationFailed { verb: String },
    #[error("Failed to {operation} one or more services. {suggestion}")]
    ServiceBatchOperationFailedWithSuggestion {
        operation: String,
        suggestion: String,
    },
    #[error("Failed to parse service label '{label}': {reason}")]
    ServiceLabelParsingFailed { label: String, reason: String },
    #[error(transparent)]
    ServiceManagementError(#[from] ant_service_management::Error),
    #[error("Template parsing failed: {reason}")]
    TemplateError { reason: String },
    #[error("The service '{0}' was not found")]
    ServiceNotFound(String),
    #[error("The service(s) is not running: {0:?}")]
    ServiceNotRunning(Vec<String>),
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
    #[error("Failed to determine user antnode data directory")]
    UserDataDirNotFound,
    #[error("User '{user}' does not exist on the system")]
    UserNotFound { user: String },
    #[error("Failed to parse version '{version}' from {context}: {reason}")]
    VersionParsingFailed {
        version: String,
        context: String,
        reason: String,
    },
}
