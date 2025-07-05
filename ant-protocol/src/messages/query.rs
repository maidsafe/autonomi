// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{messages::Nonce, NetworkAddress};
use libp2p::kad::U256;
use serde::{Deserialize, Serialize};

/// Data queries - retrieving data and inspecting their structure.
///
/// See the [`protocol`] module documentation for more details of the types supported by the Safe
/// Network, and their semantics.
///
/// [`protocol`]: crate
#[derive(Eq, PartialEq, PartialOrd, Clone, Serialize, Deserialize, Debug)]
pub enum Query {
    /// Retrieve the quote to store a record at the given address.
    /// The storage verification is optional to be undertaken
    GetStoreQuote {
        /// The Address of the record to be stored.
        key: NetworkAddress,
        /// DataTypes as represented as its `index`
        data_type: u32,
        /// Data size of the record
        data_size: usize,
        /// The random nonce that nodes use to produce the Proof (i.e., hash(record+nonce))
        /// Set to None if no need to carry out storage check.
        nonce: Option<Nonce>,
        /// Defines the expected number of answers to the challenge.
        /// Node shall try their best to fulfill the number, based on their capacity.
        /// Set to 0 to indicate not carry out any verification.
        difficulty: usize,
    },
    /// Retrieve a specific record from a specific peer.
    ///
    /// This should eventually lead to a [`GetReplicatedRecord`] response.
    ///
    /// [`GetReplicatedRecord`]: super::QueryResponse::GetReplicatedRecord
    GetReplicatedRecord {
        /// Sender of the query
        requester: NetworkAddress,
        /// Key of the record to be fetched
        key: NetworkAddress,
    },
    /// Get the proof that the chunk with the given NetworkAddress exists with the requested node.
    GetChunkExistenceProof {
        /// The Address of the chunk that we are trying to verify.
        key: NetworkAddress,
        /// The random nonce that the node uses to produce the Proof (i.e., hash(record+nonce))
        nonce: Nonce,
        /// Defines the expected number of answers to the challenge.
        /// For client publish verification, use 1 for efficiency.
        /// Node shall try their best to fulfill the number, based on their capacity.
        difficulty: usize,
    },
    /// Queries close_group peers whether the target peer is a bad_node
    CheckNodeInProblem(NetworkAddress),
    /// Query the peers in range to the target address, from the receiver's perspective.
    /// In case none of the parameters provided, returns nothing.
    /// In case both of the parameters provided, `range` is preferred to be replied.
    GetClosestPeers {
        key: NetworkAddress,
        // Shall be greater than K_VALUE, otherwise can use libp2p function directly
        num_of_peers: Option<usize>,
        // Defines the range that replied peers shall be within
        range: Option<[u8; 32]>,
        // For future econ usage,
        sign_result: bool,
    },
    /// *** From now on, the order of variants shall be retained to be backward compatible
    /// Query peer's cargo package version.
    GetVersion(NetworkAddress),
}

impl Query {
    /// Used to send a query to the close group of the address.
    pub fn dst(&self) -> NetworkAddress {
        match self {
            Query::CheckNodeInProblem(address) | Query::GetVersion(address) => address.clone(),
            // Shall not be called for this, as this is a `one-to-one` message,
            // and the destination shall be decided by the requester already.
            Query::GetStoreQuote { key, .. }
            | Query::GetReplicatedRecord { key, .. }
            | Query::GetChunkExistenceProof { key, .. }
            | Query::GetClosestPeers { key, .. } => key.clone(),
        }
    }
}

impl std::fmt::Display for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Query::GetStoreQuote {
                key,
                data_type,
                data_size,
                nonce,
                difficulty,
            } => {
                write!(
                    f,
                    "Query::GetStoreQuote({key:?} {data_type} {data_size} {nonce:?} {difficulty})"
                )
            }
            Query::GetReplicatedRecord { key, requester } => {
                write!(f, "Query::GetReplicatedRecord({requester:?} {key:?})")
            }
            Query::GetChunkExistenceProof {
                key,
                nonce,
                difficulty,
            } => {
                write!(
                    f,
                    "Query::GetChunkExistenceProof({key:?} {nonce:?} {difficulty})"
                )
            }
            Query::CheckNodeInProblem(address) => {
                write!(f, "Query::CheckNodeInProblem({address:?})")
            }
            Query::GetClosestPeers {
                key,
                num_of_peers,
                range,
                sign_result,
            } => {
                let distance = range.as_ref().map(|value| U256::from_big_endian(value));
                write!(
                    f,
                    "Query::GetClosestPeers({key:?} {num_of_peers:?} {distance:?} {sign_result})"
                )
            }
            Query::GetVersion(address) => {
                write!(f, "Query::GetVersion({address:?})")
            }
        }
    }
}
