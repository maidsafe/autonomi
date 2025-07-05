// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{error::Result, NetworkAddress};

use super::ChunkProof;
use ant_evm::PaymentQuote;
use bytes::Bytes;
use core::fmt;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// The response to a query, containing the query result.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryResponse {
    // ===== GetStoreQuote =====
    //
    /// Response to [`GetStoreQuote`]
    ///
    /// [`GetStoreQuote`]: crate::messages::Query::GetStoreQuote
    GetStoreQuote {
        /// The store cost quote for storing the next record.
        quote: Result<PaymentQuote>,
        /// Node's Peer Address
        peer_address: NetworkAddress,
        /// Storage proofs based on requested target address and difficulty
        storage_proofs: Vec<(NetworkAddress, Result<ChunkProof>)>,
    },
    CheckNodeInProblem {
        /// Address of the peer that queried
        reporter_address: NetworkAddress,
        /// Address of the target to be queried
        target_address: NetworkAddress,
        /// Status flag indicating whether the target is in trouble
        is_in_trouble: bool,
    },
    // ===== ReplicatedRecord =====
    //
    /// Response to [`GetReplicatedRecord`]
    ///
    /// [`GetReplicatedRecord`]: crate::messages::Query::GetReplicatedRecord
    GetReplicatedRecord(Result<(NetworkAddress, Bytes)>),
    // ===== ChunkExistenceProof =====
    //
    /// Response to [`GetChunkExistenceProof`]
    ///
    /// [`GetChunkExistenceProof`]: crate::messages::Query::GetChunkExistenceProof
    GetChunkExistenceProof(Vec<(NetworkAddress, Result<ChunkProof>)>),
    // ===== GetClosestPeers =====
    //
    /// Response to [`GetClosestPeers`]
    ///
    /// [`GetClosestPeers`]: crate::messages::Query::GetClosestPeers
    GetClosestPeers {
        // The target address that the original request is about.
        target: NetworkAddress,
        // `Multiaddr` is required to allow the requester to dial the peer
        // Note: the list doesn't contain the node that being queried.
        peers: Vec<(NetworkAddress, Vec<Multiaddr>)>,
        // Signature of signing the above (if requested), for future economic model usage.
        signature: Option<Vec<u8>>,
    },
    /// *** From now on, the order of variants shall be retained to be backward compatible
    // ===== GetVersion =====
    //
    /// Response to [`GetVersion`]
    ///
    /// [`GetVersion`]: crate::messages::Query::GetVersion
    GetVersion {
        peer: NetworkAddress,
        version: String,
    },
}

// Debug implementation for QueryResponse, to avoid printing Vec<u8>
impl Debug for QueryResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryResponse::GetStoreQuote {
                quote,
                peer_address,
                storage_proofs,
            } => {
                let payment_address = quote.as_ref().map(|q| q.rewards_address).ok();
                write!(
                    f,
                    "GetStoreQuote(quote: {quote:?}, from {peer_address:?} w/ payment_address: {payment_address:?}, and {} storage proofs)",
                    storage_proofs.len()
                )
            }
            QueryResponse::CheckNodeInProblem {
                reporter_address,
                target_address,
                is_in_trouble,
            } => {
                write!(
                    f,
                    "CheckNodeInProblem({reporter_address:?} report target {target_address:?} as {is_in_trouble:?} in problem"
                )
            }
            QueryResponse::GetReplicatedRecord(result) => match result {
                Ok((holder, data)) => {
                    write!(
                        f,
                        "GetReplicatedRecord(Ok((holder: {:?}, datalen: {:?})))",
                        holder,
                        data.len()
                    )
                }
                Err(err) => {
                    write!(f, "GetReplicatedRecord(Err({err:?}))")
                }
            },
            QueryResponse::GetChunkExistenceProof(proofs) => {
                let addresses: Vec<_> = proofs.iter().map(|(addr, _)| addr.clone()).collect();
                write!(f, "GetChunkExistenceProof(checked chunks: {addresses:?})")
            }
            QueryResponse::GetClosestPeers { target, peers, .. } => {
                let addresses: Vec<_> = peers.iter().map(|(addr, _)| addr.clone()).collect();
                write!(
                    f,
                    "GetClosestPeers target {target:?} close peers {addresses:?}"
                )
            }
            QueryResponse::GetVersion { peer, version } => {
                write!(f, "GetVersion peer {peer:?} has version of {version:?}")
            }
        }
    }
}

/// The response to a Cmd, containing the query result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmdResponse {
    //
    // ===== Replication =====
    //
    /// Response to replication cmd
    Replicate(Result<()>),
    /// Response to fresh replication cmd
    FreshReplicate(Result<()>),
    //
    // ===== PeerConsideredAsBad =====
    //
    /// Response to the considered as bad notification
    PeerConsideredAsBad(Result<()>),
}
