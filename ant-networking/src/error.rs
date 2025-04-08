// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::storage::GraphEntryAddress;
use ant_protocol::{messages::Response, storage::RecordKind, NetworkAddress, PrettyPrintRecordKey};
use libp2p::{
    kad::{self, QueryId, Record},
    request_response::{OutboundFailure, OutboundRequestId},
    swarm::DialError,
    PeerId, TransportError,
};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    io,
    path::PathBuf,
};
use thiserror::Error;
use tokio::sync::oneshot;
use xor_name::XorName;

pub(super) type Result<T, E = NetworkError> = std::result::Result<T, E>;

/// GetRecord Query errors
#[derive(Error, Clone)]
pub enum GetRecordError {
    #[error("Get Record completed with non enough copies")]
    NotEnoughCopies {
        record: Record,
        expected: usize,
        got: usize,
    },
    #[error("Network query timed out")]
    QueryTimeout,
    #[error("Record retrieved from the network does not match the provided target record.")]
    RecordDoesNotMatch(Record),
    #[error("The record kind for the split records did not match")]
    RecordKindMismatch,
    #[error("Record not found in the network")]
    RecordNotFound,
    // Avoid logging the whole `Record` content by accident.
    /// The split record error will be handled at the network layer.
    /// For GraphEntry, it accumulates them
    #[error("Split Record has {} different copies", result_map.len())]
    SplitRecord {
        result_map: HashMap<XorName, (Record, HashSet<PeerId>)>,
    },
}

impl Debug for GetRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEnoughCopies {
                record,
                expected,
                got,
            } => {
                let pretty_key = PrettyPrintRecordKey::from(&record.key);
                f.debug_struct("NotEnoughCopies")
                    .field("record_key", &pretty_key)
                    .field("expected", &expected)
                    .field("got", &got)
                    .finish()
            }
            Self::QueryTimeout => write!(f, "QueryTimeout"),
            Self::RecordDoesNotMatch(record) => {
                let pretty_key = PrettyPrintRecordKey::from(&record.key);
                f.debug_tuple("RecordDoesNotMatch")
                    .field(&pretty_key)
                    .finish()
            }
            Self::RecordKindMismatch => write!(f, "RecordKindMismatch"),
            Self::RecordNotFound => write!(f, "RecordNotFound"),
            Self::SplitRecord { result_map } => f
                .debug_struct("SplitRecord")
                .field("result_map_count", &result_map.len())
                .finish(),
        }
    }
}

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
    // GetRecord query errors
    #[error("GetRecord Query Error {0:?}")]
    GetRecordError(#[from] GetRecordError),
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

#[cfg(test)]
mod tests {
    use ant_protocol::{storage::ChunkAddress, NetworkAddress, PrettyPrintKBucketKey};
    use xor_name::XorName;

    use super::*;

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
