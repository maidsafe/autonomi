// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow enum variant names that end with Error - this is a common pattern with thiserror
#![allow(clippy::enum_variant_names)]

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to parse network address: {reason}")]
    AddrParseError { reason: String },
    #[error("The endpoint for the daemon has not been set")]
    DaemonEndpointNotSet,
    #[error("Failed to execute command: {reason}")]
    ExecutionFailed { reason: String },
    #[error("Failed to access file or perform I/O operation: {reason}")]
    FileOperationFailed { reason: String },
    #[error("Failed to serialize/deserialize JSON: {reason}")]
    JsonOperationFailed { reason: String },
    #[error(transparent)]
    MetricsError(#[from] crate::metric::MetricsActionError),
    #[error("Failed to parse number: {reason}")]
    NumberParsingFailed { reason: String },
    #[error(
        "Could not connect to the network using rpc endpoint '{rpc_endpoint}' within {timeout:?}"
    )]
    NodeConnectionTimedOut {
        rpc_endpoint: String,
        timeout: std::time::Duration,
    },
    #[error("Could not connect to RPC endpoint '{0}'")]
    RpcConnectionError(String),
    #[error("Could not obtain node info through RPC: {0}")]
    RpcNodeInfoError(String),
    #[error("Could not restart node through RPC: {0}")]
    RpcNodeRestartError(String),
    #[error("Could not stop node through RPC: {0}")]
    RpcNodeStopError(String),
    #[error("Could not update node through RPC: {0}")]
    RpcNodeUpdateError(String),
    #[error("Could not obtain record addresses through RPC: {0}")]
    RpcRecordAddressError(String),
    #[error("Failed to parse service label: {reason}")]
    ServiceLabelParsingFailed { reason: String },
    #[error("Service management operation failed: {reason}")]
    ServiceManagementFailed { reason: String },
    #[error("Could not find process at '{0}'")]
    ServiceProcessNotFound(String),
    #[error("The service '{0}' does not exists and cannot be removed.")]
    ServiceDoesNotExists(String),
    #[error("The user may have removed the '{0}' service outwith the node manager")]
    ServiceRemovedManually(String),
    #[error("Failed to create service user account")]
    ServiceUserAccountCreationFailed,
    #[error("String conversion failed: {reason}")]
    StringConversion { reason: String },
    #[error("Could not obtain user's data directory")]
    UserDataDirectoryNotObtainable,
    #[error("File watcher error: {0}")]
    WatcherError(String),
}

impl From<notify::Error> for Error {
    fn from(err: notify::Error) -> Self {
        Error::WatcherError(err.to_string())
    }
}
