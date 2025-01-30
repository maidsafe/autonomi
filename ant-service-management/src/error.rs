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
    AddrParseError(#[from] std::net::AddrParseError),
    #[error("The endpoint for the daemon has not been set")]
    DaemonEndpointNotSet,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    MultiAddrParseError(#[from] libp2p::multiaddr::Error),
    #[error("The registry does not contain a service named '{0}'")]
    NodeNotFound(String),
    #[error(transparent)]
    ParseIntError(#[from] std::num::ParseIntError),
    #[error(transparent)]
    PeerIdParseError(#[from] libp2p_identity::ParseError),
    #[error("Could not connect to RPC endpoint '{0}'")]
    RpcConnectionError(String),
    #[error("Could not obtain node info through RPC: {0}")]
    RpcNodeInfoError(String),
    #[error("Could not obtain network info through RPC: {0}")]
    RpcNetworkInfoError(String),
    #[error("Could not restart node through RPC: {0}")]
    RpcNodeRestartError(String),
    #[error("Could not stop node through RPC: {0}")]
    RpcNodeStopError(String),
    #[error("Could not update node through RPC: {0}")]
    RpcNodeUpdateError(String),
    #[error("Could not obtain record addresses through RPC: {0}")]
    RpcRecordAddressError(String),
    #[error("Could not find process at '{0}'")]
    ServiceProcessNotFound(String),
    #[error("The service '{0}' does not exists and cannot be removed.")]
    ServiceDoesNotExists(String),
    #[error("The user may have removed the '{0}' service outwith the node manager")]
    ServiceRemovedManually(String),
    #[error("Failed to create service user account")]
    ServiceUserAccountCreationFailed,
    #[error("Could not obtain user's data directory")]
    UserDataDirectoryNotObtainable,
    #[error("Could not connect to the metric service at port {0}")]
    MetricServiceConnectionError(String),
    #[error("Could not process the metric service inforesponse")]
    MetricServiceInfoResponseError,
    #[error("Could not process the metrics from nodeInfo '{0}'")]
    MetricServiceNodeInfoError(String),
    #[error("Could not process the metrics from NetworkInfo '{0}'")]
    MetricServiceNetworkInfoError(String),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
}
