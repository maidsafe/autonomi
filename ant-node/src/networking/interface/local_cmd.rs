// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
};

use ant_evm::{PaymentQuote, QuotingMetrics};
use ant_protocol::{
    NetworkAddress, PrettyPrintRecordKey,
    storage::{DataTypes, ValidationType},
};
use libp2p::{
    PeerId,
    core::Multiaddr,
    kad::{KBucketDistance as Distance, Record, RecordKey},
};
use tokio::sync::oneshot;

use crate::networking::Addresses;

#[derive(Debug, Eq, PartialEq, Clone)]
pub(crate) enum NodeIssue {
    /// Some connections might be considered to be critical and should be tracked.
    ConnectionIssue,
    /// Data Replication failed
    ReplicationFailure,
    /// Close nodes have reported this peer as bad
    #[allow(dead_code)]
    CloseNodesShunning,
    /// Provided a bad quote
    BadQuoting,
    /// Peer failed to pass the chunk proof verification
    FailedChunkProofCheck,
}

impl std::fmt::Display for NodeIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NodeIssue::ConnectionIssue => write!(f, "CriticalConnectionIssue"),
            NodeIssue::ReplicationFailure => write!(f, "ReplicationFailure"),
            NodeIssue::CloseNodesShunning => write!(f, "CloseNodesShunning"),
            NodeIssue::BadQuoting => write!(f, "BadQuoting"),
            NodeIssue::FailedChunkProofCheck => write!(f, "FailedChunkProofCheck"),
        }
    }
}

/// Commands to send to the Swarm
pub(crate) enum LocalSwarmCmd {
    /// Get a list of all peers in local RT, with correspondent Multiaddr info attached as well.
    GetPeersWithMultiaddr {
        sender: oneshot::Sender<Vec<(PeerId, Vec<Multiaddr>)>>,
    },
    /// Get a map where each key is the ilog2 distance of that Kbucket
    /// and each value is a vector of peers in that bucket.
    GetKBuckets {
        sender: oneshot::Sender<BTreeMap<u32, Vec<PeerId>>>,
    },
    // Get K closest peers to target from the local RoutingTable, self is included
    GetKCloseLocalPeersToTarget {
        key: NetworkAddress,
        sender: oneshot::Sender<Vec<(PeerId, Addresses)>>,
    },
    GetSwarmLocalState(oneshot::Sender<SwarmLocalState>),
    /// Check if the local RecordStore contains the provided key
    RecordStoreHasKey {
        key: RecordKey,
        sender: oneshot::Sender<bool>,
    },
    /// Get the Addresses of all the Records held locally
    GetAllLocalRecordAddresses {
        sender: oneshot::Sender<HashMap<NetworkAddress, ValidationType>>,
    },
    /// Get data from the local RecordStore
    GetLocalRecord {
        key: RecordKey,
        sender: oneshot::Sender<Option<Record>>,
    },
    /// GetLocalQuotingMetrics for this node
    /// Returns the quoting metrics and whether the record at `key` is already stored locally
    GetLocalQuotingMetrics {
        key: RecordKey,
        data_type: u32,
        data_size: usize,
        sender: oneshot::Sender<(QuotingMetrics, bool)>,
    },
    /// Notify the node received a payment.
    PaymentReceived,
    /// Put record to the local RecordStore
    PutLocalRecord {
        record: Record,
        is_client_put: bool,
    },
    /// Remove a local record from the RecordStore
    /// Typically because the write failed
    RemoveFailedLocalRecord {
        key: RecordKey,
    },
    /// Add a local record to the RecordStore's HashSet of stored records
    /// This should be done after the record has been stored to disk
    AddLocalRecordAsStored {
        key: RecordKey,
        record_type: ValidationType,
        data_type: DataTypes,
    },
    /// Add a peer to the blocklist
    AddPeerToBlockList {
        peer_id: PeerId,
    },
    /// Notify whether peer is in trouble
    RecordNodeIssue {
        peer_id: PeerId,
        issue: NodeIssue,
    },
    // Whether peer is considered as `in trouble` by self
    IsPeerShunned {
        target: NetworkAddress,
        sender: oneshot::Sender<bool>,
    },
    // Quote verification agaisnt historical collected quotes
    QuoteVerification {
        quotes: Vec<(PeerId, PaymentQuote)>,
    },
    // Notify a fetch completion
    FetchCompleted((RecordKey, ValidationType)),
    /// Triggers interval repliation
    /// NOTE: This does result in outgoing messages, but is produced locally
    TriggerIntervalReplication,
    /// Triggers unrelevant record cleanup
    TriggerIrrelevantRecordCleanup,
    /// Send peer scores (collected from storage challenge) to replication_fetcher
    NotifyPeerScores {
        peer_scores: Vec<(PeerId, bool)>,
    },
    /// Add fresh replicate records into replication_fetcher
    AddFreshReplicateRecords {
        holder: NetworkAddress,
        keys: Vec<(NetworkAddress, ValidationType)>,
    },
    /// Notify a fetched peer's version
    NotifyPeerVersion {
        peer: PeerId,
        version: String,
    },
    /// Get responsible distance range.
    GetNetworkDensity {
        sender: oneshot::Sender<Option<Distance>>,
    },
    /// Remove peer from the routing table
    RemovePeer {
        peer: PeerId,
    },
}

/// Debug impl for LocalSwarmCmd to avoid printing full Record, instead only RecodKey
/// and RecordKind are printed.
impl Debug for LocalSwarmCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocalSwarmCmd::PutLocalRecord {
                record,
                is_client_put,
            } => {
                write!(
                    f,
                    "LocalSwarmCmd::PutLocalRecord {{ key: {:?}, is_client_put: {is_client_put:?} }}",
                    PrettyPrintRecordKey::from(&record.key)
                )
            }
            LocalSwarmCmd::RemoveFailedLocalRecord { key } => {
                write!(
                    f,
                    "LocalSwarmCmd::RemoveFailedLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::AddLocalRecordAsStored {
                key,
                record_type,
                data_type,
            } => {
                write!(
                    f,
                    "LocalSwarmCmd::AddLocalRecordAsStored {{ key: {:?}, record_type: {record_type:?}, data_type: {data_type:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::GetKCloseLocalPeersToTarget { key, .. } => {
                write!(
                    f,
                    "LocalSwarmCmd::GetKCloseLocalPeersToTarget {{ key: {key:?} }}"
                )
            }
            LocalSwarmCmd::GetLocalQuotingMetrics { .. } => {
                write!(f, "LocalSwarmCmd::GetLocalQuotingMetrics")
            }
            LocalSwarmCmd::PaymentReceived => {
                write!(f, "LocalSwarmCmd::PaymentReceived")
            }
            LocalSwarmCmd::GetLocalRecord { key, .. } => {
                write!(
                    f,
                    "LocalSwarmCmd::GetLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::GetAllLocalRecordAddresses { .. } => {
                write!(f, "LocalSwarmCmd::GetAllLocalRecordAddresses")
            }
            LocalSwarmCmd::GetPeersWithMultiaddr { .. } => {
                write!(f, "LocalSwarmCmd::GetPeersWithMultiaddr")
            }
            LocalSwarmCmd::GetKBuckets { .. } => {
                write!(f, "LocalSwarmCmd::GetKBuckets")
            }
            LocalSwarmCmd::GetSwarmLocalState { .. } => {
                write!(f, "LocalSwarmCmd::GetSwarmLocalState")
            }
            LocalSwarmCmd::RecordStoreHasKey { key, .. } => {
                write!(
                    f,
                    "LocalSwarmCmd::RecordStoreHasKey {:?}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::AddPeerToBlockList { peer_id } => {
                write!(f, "LocalSwarmCmd::AddPeerToBlockList {peer_id:?}")
            }
            LocalSwarmCmd::RecordNodeIssue { peer_id, issue } => {
                write!(
                    f,
                    "LocalSwarmCmd::SendNodeStatus peer {peer_id:?}, issue: {issue:?}"
                )
            }
            LocalSwarmCmd::IsPeerShunned { target, .. } => {
                write!(f, "LocalSwarmCmd::IsPeerInTrouble target: {target:?}")
            }
            LocalSwarmCmd::QuoteVerification { quotes } => {
                write!(
                    f,
                    "LocalSwarmCmd::QuoteVerification of {} quotes",
                    quotes.len()
                )
            }
            LocalSwarmCmd::FetchCompleted((key, record_type)) => {
                write!(
                    f,
                    "LocalSwarmCmd::FetchCompleted({record_type:?} : {:?})",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::TriggerIntervalReplication => {
                write!(f, "LocalSwarmCmd::TriggerIntervalReplication")
            }
            LocalSwarmCmd::TriggerIrrelevantRecordCleanup => {
                write!(f, "LocalSwarmCmd::TriggerUnrelevantRecordCleanup")
            }
            LocalSwarmCmd::NotifyPeerScores { peer_scores } => {
                write!(f, "LocalSwarmCmd::NotifyPeerScores({peer_scores:?})")
            }
            LocalSwarmCmd::AddFreshReplicateRecords { holder, keys } => {
                write!(
                    f,
                    "LocalSwarmCmd::AddFreshReplicateRecords({holder:?}, {keys:?})"
                )
            }
            LocalSwarmCmd::NotifyPeerVersion { peer, version } => {
                write!(f, "LocalSwarmCmd::NotifyPeerVersion({peer:?}, {version:?})")
            }
            LocalSwarmCmd::GetNetworkDensity { .. } => {
                write!(f, "LocalSwarmCmd::GetNetworkDensity")
            }
            LocalSwarmCmd::RemovePeer { peer } => {
                write!(f, "LocalSwarmCmd::RemovePeer({peer:?})")
            }
        }
    }
}

/// Snapshot of information kept in the Swarm's local state
#[derive(Debug, Clone)]
pub struct SwarmLocalState {
    /// List of peers that we have an established connection with.
    pub connected_peers: Vec<PeerId>,
    /// The number of peers in the routing table
    pub peers_in_routing_table: usize,
    /// List of addresses the node is currently listening on
    pub listeners: Vec<Multiaddr>,
}
