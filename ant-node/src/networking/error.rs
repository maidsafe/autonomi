// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::storage::GraphEntryAddress;
use ant_protocol::{messages::Response, storage::RecordKind, NetworkAddress};
use libp2p::swarm::ListenError;
use libp2p::{
    kad::{self, QueryId},
    request_response::{OutboundFailure, OutboundRequestId},
    swarm::DialError,
    TransportError,
};
use std::{collections::HashMap, fmt::Debug, io, path::PathBuf};
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::Level;

const TRACING_ERROR_LEVEL: Level = Level::ERROR;

pub(super) type Result<T, E = NetworkError> = std::result::Result<T, E>;

/// Network Errors
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Dial Error")]
    DialError(#[from] DialError),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Kademlia Store error: {0}")]
    KademliaStoreError(#[from] kad::store::Error),

    #[error("Transport Error")]
    TransportError(#[from] TransportError<std::io::Error>),

    #[error("SnProtocol Error: {0}")]
    ProtocolError(#[from] ant_protocol::error::Error),

    #[error("Evm payment Error {0}")]
    EvmPaymemt(#[from] ant_evm::EvmError),

    #[error("Failed to sign the message with the PeerId keypair")]
    SigningFailed(#[from] libp2p::identity::SigningError),

    // ---------- Record Errors
    #[error("Record not stored by nodes, it could be invalid, else you should retry: {0:?}")]
    RecordNotStoredByNodes(NetworkAddress),

    // The RecordKind that was obtained did not match with the expected one
    #[error("The RecordKind obtained from the Record did not match with the expected kind: {0}")]
    RecordKindMismatch(RecordKind),

    #[error("Record header is incorrect")]
    InCorrectRecordHeader,

    #[error("The operation is not allowed on a client record store")]
    OperationNotAllowedOnClientRecordStore,

    // ---------- Chunk Errors
    #[error("Failed to verify the ChunkProof with the provided quorum")]
    FailedToVerifyChunkProof(NetworkAddress),

    // ---------- Graph Errors
    #[error("Graph entry not found: {0:?}")]
    NoGraphEntryFoundInsideRecord(GraphEntryAddress),

    // ---------- Store Error
    #[error("Not Enough Peers for Store Cost Request")]
    NotEnoughPeersForStoreCostRequest,

    #[error("No Store Cost Responses")]
    NoStoreCostResponses,

    #[error("Could not create storage dir: {path:?}, error: {source}")]
    FailedToCreateRecordStoreDir {
        path: PathBuf,
        source: std::io::Error,
    },

    // ---------- Kad Network Errors
    #[error("Network GetClosest TimedOut")]
    GetClosestTimedOut,

    // ---------- Internal Network Errors
    #[error("Could not get enough peers ({required}) to satisfy the request, found {found}")]
    NotEnoughPeers { found: usize, required: usize },

    #[error("Node Listen Address was not provided during construction")]
    ListenAddressNotProvided,

    #[cfg(feature = "open-metrics")]
    #[error("Network Metric error")]
    NetworkMetricError,

    // ---------- Channel Errors
    #[error("Outbound Error")]
    OutboundError(#[from] OutboundFailure),

    #[error("A Kademlia event has been dropped: {query_id:?} {event}")]
    ReceivedKademliaEventDropped { query_id: QueryId, event: String },

    #[error("The oneshot::sender has been dropped")]
    SenderDropped(#[from] oneshot::error::RecvError),

    #[error("Internal messaging channel was dropped")]
    InternalMsgChannelDropped,

    #[error("Response received for a request not found in our local tracking map: {0}")]
    ReceivedResponseDropped(OutboundRequestId),

    #[error("Outgoing response has been dropped due to a conn being closed or timeout: {0}")]
    OutgoingResponseDropped(Response),

    #[error("Error setting up behaviour: {0}")]
    BehaviourErr(String),
}

/// Return a list of error strings for the DialError type
pub(in crate::networking) fn dial_error_to_str(err: &DialError) -> Vec<(String, Level)> {
    match err {
        DialError::LocalPeerId { .. } => {
            vec![("DialError::LocalPeerId".to_string(), TRACING_ERROR_LEVEL)]
        }
        DialError::NoAddresses => vec![("DialError::NoAddresses".to_string(), TRACING_ERROR_LEVEL)],
        DialError::DialPeerConditionFalse(peer_condition) => {
            vec![(
                format!("DialError::DialPeerConditionFalse::{peer_condition:?}"),
                TRACING_ERROR_LEVEL,
            )]
        }
        DialError::Aborted => vec![("DialError::Aborted".to_string(), TRACING_ERROR_LEVEL)],
        DialError::WrongPeerId { .. } => {
            vec![("DialError::WrongPeerId".to_string(), TRACING_ERROR_LEVEL)]
        }
        DialError::Denied { .. } => vec![("DialError::Denied".to_string(), TRACING_ERROR_LEVEL)],
        DialError::Transport(items) => items
            .iter()
            .map(|(_, error)| {
                let (error_str, level) = transport_err_to_str(error);
                (format!("DialError::{error_str}"), level)
            })
            .collect(),
    }
}

/// Return a string for the ListenError type
pub(in crate::networking) fn listen_error_to_str(err: &ListenError) -> (String, Level) {
    match err {
        ListenError::Aborted => ("ListenError::Aborted".to_string(), TRACING_ERROR_LEVEL),

        ListenError::WrongPeerId { .. } => {
            ("ListenError::WrongPeerId".to_string(), TRACING_ERROR_LEVEL)
        }
        ListenError::LocalPeerId { .. } => {
            ("ListenError::LocalPeerId".to_string(), TRACING_ERROR_LEVEL)
        }
        ListenError::Denied { .. } => ("ListenError::Denied".to_string(), TRACING_ERROR_LEVEL),
        ListenError::Transport(transport_error) => {
            let (error_str, level) = transport_err_to_str(transport_error);
            (format!("ListenError::{error_str}"), level)
        }
    }
}

/// Return a string for the TransportError type
fn transport_err_to_str(err: &TransportError<std::io::Error>) -> (String, Level) {
    match err {
        TransportError::MultiaddrNotSupported { .. } => (
            "TransportError::MultiaddrNotSupported".to_string(),
            Level::ERROR,
        ),
        TransportError::Other(err) => {
            let some_known_errors = HashMap::from([
                (
                    "ConnectionRefused",
                    ("ConnectionRefused", TRACING_ERROR_LEVEL),
                ),
                ("HostUnreachable", ("HostUnreachable", TRACING_ERROR_LEVEL)),
                (
                    "HandshakeTimedOut",
                    ("HandshakeTimedOut", TRACING_ERROR_LEVEL),
                ),
                ("TimedOut", ("TimedOut", TRACING_ERROR_LEVEL)),
                (
                    "ResponseFromBehaviourCanceled",
                    ("ResponseFromBehaviourCanceled", TRACING_ERROR_LEVEL),
                ),
                ("ConnectionLost", ("ConnectionLost", TRACING_ERROR_LEVEL)),
                ("ConnectionClosed", ("ConnectionClosed", Level::DEBUG)),
                (
                    "ConnectionFailed",
                    ("ConnectionFailed", TRACING_ERROR_LEVEL),
                ),
                (
                    "MALFORMED_MESSAGE",
                    ("MalformedMessage", TRACING_ERROR_LEVEL),
                ),
                ("UnexpectedEof", ("UnexpectedEof", TRACING_ERROR_LEVEL)),
                ("Select(Failed)", ("Failed", TRACING_ERROR_LEVEL)),
            ]);

            let mut err_str = None;
            for (err_substr, (err_display, tracing_level)) in some_known_errors.iter() {
                if format!("{err:?}").contains(err_substr) {
                    err_str = Some((format!("TransportError::{err_display}"), *tracing_level));
                    break;
                }
            }

            err_str.unwrap_or_else(|| ("TransportError::Other".to_string(), TRACING_ERROR_LEVEL))
        }
    }
}

#[cfg(test)]
mod tests {
    use ant_protocol::{
        storage::ChunkAddress, NetworkAddress, PrettyPrintKBucketKey, PrettyPrintRecordKey,
    };
    use xor_name::XorName;

    #[test]
    fn test_client_sees_same_hex_in_errors_for_xorname_and_record_keys() {
        let mut rng = rand::thread_rng();
        let xor_name = XorName::random(&mut rng);
        let address = ChunkAddress::new(xor_name);
        let network_address = NetworkAddress::from(address);
        let record_key = network_address.to_record_key();
        let record_str = format!("{}", PrettyPrintRecordKey::from(&record_key));
        let xor_name_str = &format!("{xor_name:64x}")[0..6]; // only the first 6 chars are logged
        let xor_name_str = format!(
            "{xor_name_str}({:?})",
            PrettyPrintKBucketKey(network_address.as_kbucket_key())
        );
        println!("record_str: {record_str}");
        println!("xor_name_str: {xor_name_str}");
        assert_eq!(record_str, xor_name_str);
    }
}
