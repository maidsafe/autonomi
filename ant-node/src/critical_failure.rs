// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Error;
use crate::networking::NetworkError;
use std::path::Path;

pub(crate) const CRITICAL_FAILURE_FILE: &str = "critical_failure.json";

#[derive(serde::Serialize, serde::Deserialize)]
// This struct is read by ant-service-management/src/fs.rs
// So any changes here need to be reflected there too.
#[derive(Clone, Debug)]
pub(crate) struct CriticalFailure {
    pub date_time: chrono::DateTime<chrono::Utc>,
    pub reason: String,
}

pub fn set_critical_failure(root_dir: &Path, error: &Error) {
    let file_path = root_dir.join(CRITICAL_FAILURE_FILE);
    let datetime_prefix = chrono::Utc::now();
    let reason = node_error_to_reason(error);
    let failure = CriticalFailure {
        date_time: datetime_prefix,
        reason,
    };
    let Ok(failure_json) = serde_json::to_string(&failure)
        .inspect_err(|err| error!("Failed to serialize when writing critical failure: {err}"))
    else {
        return;
    };
    let _ = std::fs::write(file_path, failure_json)
        .inspect_err(|err| error!("Failed to write to {CRITICAL_FAILURE_FILE}: {err}"));
    info!("Critical failure recorded: {failure:?}");
}

pub fn reset_critical_failure(root_dir: &Path) {
    let failure_path = root_dir.join(CRITICAL_FAILURE_FILE);
    if failure_path.exists() {
        if std::fs::remove_file(failure_path).is_ok() {
            info!("Critical failure file removed");
        } else {
            error!("Failed to remove critical failure file");
        }
    }
}

fn node_error_to_reason(error: &Error) -> String {
    match error {
        Error::Network(network_error) => {
            println!("Network error: {network_error}");
            let network_err_str = match network_error {
                NetworkError::DialError(_) => "DialError".to_string(),
                NetworkError::Io(_) => "IoError".to_string(),
                NetworkError::KademliaStoreError(_) => "KademliaStoreError".to_string(),
                NetworkError::TransportError(_) => "TransportError".to_string(),
                NetworkError::ProtocolError(_) => "ProtocolError".to_string(),
                NetworkError::EvmPaymemt(_) => "EvmPayment".to_string(),
                NetworkError::SigningFailed(_) => "SigningFailed".to_string(),
                NetworkError::NoListenAddressesFound => "NoListenAddressesFound".to_string(),
                NetworkError::ListenerCleanupFailed => "ListenerCleanupFailed".to_string(),
                NetworkError::ListenFailed(_) => "ListenFailed".to_string(),
                NetworkError::InCorrectRecordHeader => "InCorrectRecordHeader".to_string(),
                NetworkError::FailedToCreateRecordStoreDir { .. } => {
                    "FailedToCreateRecordStoreDir".to_string()
                }
                NetworkError::GetClosestTimedOut => "GetClosestTimedOut".to_string(),
                NetworkError::NotEnoughPeers { .. } => "NotEnoughPeers".to_string(),
                #[cfg(feature = "open-metrics")]
                NetworkError::NetworkMetricError => "NetworkMetricError".to_string(),
                NetworkError::OutboundError(_) => "OutboundError".to_string(),
                NetworkError::ReceivedKademliaEventDropped { .. } => {
                    "ReceivedKademliaEventDropped".to_string()
                }
                NetworkError::SenderDropped(_) => "SenderDropped".to_string(),
                NetworkError::InternalMsgChannelDropped => "InternalMsgChannelDropped".to_string(),
                NetworkError::ReceivedResponseDropped(_) => "ReceivedResponseDropped".to_string(),
                NetworkError::OutgoingResponseDropped(_) => "OutgoingResponseDropped".to_string(),
                NetworkError::NotEnoughBootstrapAddresses => {
                    "NotEnoughBootstrapAddresses".to_string()
                }
            };
            format!("NetworkError::{network_err_str}")
        }
        Error::FailedToGetNodePort => "FailedToGetNodePort".to_string(),
        Error::InvalidQuoteContent => "InvalidQuoteContent".to_string(),
        Error::InvalidQuoteSignature => "InvalidQuoteSignature".to_string(),
        Error::UnreachableNode => "UnreachableNode".to_string(),
        Error::PidFileWriteFailed { .. } => "PidFileWriteFailed".to_string(),
        Error::ControlChannelClosed => "ControlChannelClosed".to_string(),
        Error::NodeEventChannelClosed => "NodeEventChannelClosed".to_string(),
        Error::ControlMessageSendFailed(_) => "ControlMessageSendFailed".to_string(),
        Error::CtrlCReceived => "CtrlCReceived".to_string(),
        Error::TerminateSignalReceived(terminate_node_reason) => {
            format!("TerminateSignalReceived::{terminate_node_reason:?}")
        }
        Error::Bootstrap(_) => "Bootstrap".to_string(),
        Error::Tokio => "TokioError".to_string(),
    }
}
