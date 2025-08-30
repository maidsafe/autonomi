// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::BTreeMap;

use ant_evm::{PaymentQuote, ProofOfPayment};
use ant_protocol::{
    NetworkAddress, PrettyPrintRecordKey,
    messages::{Query, Response},
    storage::{DataTypes, ValidationType},
};
use libp2p::kad::{Record, RecordKey};
use libp2p::{Multiaddr, PeerId};

use crate::networking::{Addresses, driver::event::MsgResponder};

/// Events forwarded by the underlying Network; to be used by the upper layers
pub(crate) enum NetworkEvent {
    /// Incoming `Query` from a peer
    QueryRequestReceived {
        /// Query
        query: Query,
        /// The channel to send the `Response` through
        channel: MsgResponder,
    },
    /// Handles the responses that are not awaited at the call site
    ResponseReceived {
        /// Response
        res: Response,
    },
    /// Peer has been added to the Routing Table. And the number of connected peers.
    PeerAdded(PeerId, usize),
    /// Peer has been removed from the Routing Table. And the number of connected peers.
    PeerRemoved(PeerId, usize),
    /// The peer does not support our protocol
    PeerWithUnsupportedProtocol {
        our_protocol: String,
        their_protocol: String,
    },
    /// The records bearing these keys are to be fetched from the holder or the network
    KeysToFetchForReplication(Vec<(PeerId, RecordKey)>),
    /// Started listening on a new address
    NewListenAddr(Multiaddr),
    /// Stopped listening on an address
    ExpiredListenAddresses(Vec<Multiaddr>),
    /// Report unverified record
    UnverifiedRecord(Record),
    /// Terminate Node on unrecoverable errors
    TerminateNode { reason: TerminateNodeReason },
    /// List of peer nodes that failed to fetch replication copy from.
    FailedToFetchHolders(BTreeMap<PeerId, RecordKey>),
    /// Quotes to be verified
    #[allow(dead_code)]
    QuoteVerification { quotes: Vec<(PeerId, PaymentQuote)> },
    /// Fresh replicate to fetch
    FreshReplicateToFetch {
        holder: NetworkAddress,
        keys: Vec<(
            NetworkAddress,
            DataTypes,
            ValidationType,
            Option<ProofOfPayment>,
        )>,
    },
    /// Peers of picked bucket for version query.
    PeersForVersionQuery(Vec<(PeerId, Addresses)>),
}

/// Terminate node for the following reason
#[derive(Debug, Clone)]
pub(crate) enum TerminateNodeReason {
    HardDiskWriteError,
    UpnpGatewayNotFound,
}

// Manually implement Debug as `#[debug(with = "unverified_record_fmt")]` not working as expected.
impl std::fmt::Debug for NetworkEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkEvent::QueryRequestReceived { query, .. } => {
                write!(f, "NetworkEvent::QueryRequestReceived({query:?})")
            }
            NetworkEvent::ResponseReceived { res, .. } => {
                write!(f, "NetworkEvent::ResponseReceived({res:?})")
            }
            NetworkEvent::PeerAdded(peer_id, connected_peers) => {
                write!(f, "NetworkEvent::PeerAdded({peer_id:?}, {connected_peers})")
            }
            NetworkEvent::PeerRemoved(peer_id, connected_peers) => {
                write!(
                    f,
                    "NetworkEvent::PeerRemoved({peer_id:?}, {connected_peers})"
                )
            }
            NetworkEvent::PeerWithUnsupportedProtocol {
                our_protocol,
                their_protocol,
            } => {
                write!(
                    f,
                    "NetworkEvent::PeerWithUnsupportedProtocol({our_protocol:?}, {their_protocol:?})"
                )
            }
            NetworkEvent::KeysToFetchForReplication(list) => {
                let keys_len = list.len();
                write!(f, "NetworkEvent::KeysForReplication({keys_len:?})")
            }
            NetworkEvent::NewListenAddr(addr) => {
                write!(f, "NetworkEvent::NewListenAddr({addr:?})")
            }
            NetworkEvent::ExpiredListenAddresses(addrs) => {
                write!(f, "NetworkEvent::ExpiredListenAddresses({addrs:?})")
            }
            NetworkEvent::UnverifiedRecord(record) => {
                let pretty_key = PrettyPrintRecordKey::from(&record.key);
                write!(f, "NetworkEvent::UnverifiedRecord({pretty_key:?})")
            }
            NetworkEvent::TerminateNode { reason } => {
                write!(f, "NetworkEvent::TerminateNode({reason:?})")
            }
            NetworkEvent::FailedToFetchHolders(bad_nodes) => {
                let pretty_log: Vec<_> = bad_nodes
                    .iter()
                    .map(|(peer_id, record_key)| {
                        let pretty_key = PrettyPrintRecordKey::from(record_key);
                        (peer_id, pretty_key)
                    })
                    .collect();
                write!(f, "NetworkEvent::FailedToFetchHolders({pretty_log:?})")
            }
            NetworkEvent::QuoteVerification { quotes } => {
                write!(
                    f,
                    "NetworkEvent::QuoteVerification({} quotes)",
                    quotes.len()
                )
            }
            NetworkEvent::FreshReplicateToFetch { holder, keys } => {
                write!(
                    f,
                    "NetworkEvent::FreshReplicateToFetch({holder:?}, {keys:?})"
                )
            }
            NetworkEvent::PeersForVersionQuery(peers) => {
                write!(
                    f,
                    "NetworkEvent::PeersForVersionQuery({:?})",
                    peers
                        .iter()
                        .map(|(peer, _addrs)| peer)
                        .collect::<Vec<&PeerId>>()
                )
            }
        }
    }
}

impl std::fmt::Display for TerminateNodeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminateNodeReason::HardDiskWriteError => {
                write!(f, "HardDiskWriteError")
            }
            TerminateNodeReason::UpnpGatewayNotFound => {
                write!(
                    f,
                    "UPnP gateway not found. Enable UPnP on your router to allow incoming connections or manually port forward."
                )
            }
        }
    }
}
