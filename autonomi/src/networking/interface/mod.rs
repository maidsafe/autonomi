// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::{OneShotTaskResult, PeerQuoteWithStorageProof};
use ant_evm::{PaymentQuote, merkle_payments::MerklePaymentCandidateNode};
use ant_protocol::NetworkAddress;
use ant_protocol::storage::DataTypes;
use libp2p::{
    PeerId,
    kad::{PeerInfo, Quorum, Record},
};
use std::num::NonZeroUsize;

/// Task for the underlying network driver
/// Sent by the [`crate::Network`], handled by the [`crate::driver::NetworkDriver`]
///
/// - [`crate::Network`]: The client interface to the network, sends [`NetworkTask`] to the [`crate::driver::NetworkDriver`]
/// - [`NetworkTask`]: The task to be processed by the network driver
/// - [`crate::driver::NetworkDriver`]:
///     - the NetworkDriver sends the query to the network [`crate::driver::NetworkDriver::process_task`]
///     - it then waits for libp2p events and updates the pending tasks accordingly [`crate::driver::NetworkDriver::process_swarm_event`]
///     - ultimately sending the result back to the [`crate::Network`] via the oneshot channel provided when the task was created
#[derive(custom_debug::Debug)]
pub(super) enum NetworkTask {
    /// cf [`crate::driver::task_handler::TaskHandler::update_closest_peers`]
    GetClosestPeers {
        addr: NetworkAddress,
        #[debug(skip)]
        resp: OneShotTaskResult<Vec<PeerInfo>>,
        n: NonZeroUsize,
    },
    /// cf [`crate::driver::task_handler::TaskHandler::update_get_record`]
    GetRecord {
        addr: NetworkAddress,
        quorum: Quorum,
        #[debug(skip)]
        resp: OneShotTaskResult<(Option<Record>, Vec<PeerId>)>,
    },
    /// cf [`crate::driver::task_handler::TaskHandler::update_put_record_kad`]
    #[allow(dead_code)]
    PutRecordKad {
        #[debug(skip)]
        record: Record,
        /// Empty vec results in regular store to peers closest to record address
        to: Vec<PeerInfo>,
        quorum: Quorum,
        #[debug(skip)]
        resp: OneShotTaskResult<()>,
    },
    /// cf [`crate::driver::task_handler::TaskHandler::update_put_record_kad_req`]
    PutRecordReq {
        #[debug(skip)]
        record: Record,
        to: PeerInfo,
        #[debug(skip)]
        resp: OneShotTaskResult<()>,
    },
    /// cf [`crate::driver::task_handler::TaskHandler::update_get_quote`]
    GetQuote {
        addr: NetworkAddress,
        peer: PeerInfo,
        data_type: u32,
        data_size: usize,
        #[debug(skip)]
        resp: OneShotTaskResult<Option<(PeerInfo, PaymentQuote)>>,
    },
    /// cf [`crate::driver::task_handler::TaskHandler::update_get_version`]
    GetVersion {
        peer: PeerInfo,
        #[debug(skip)]
        resp: OneShotTaskResult<String>,
    },
    /// Get a record directly from a specific peer using request/response
    GetRecordFromPeer {
        addr: NetworkAddress,
        peer: PeerInfo,
        #[debug(skip)]
        resp: OneShotTaskResult<Option<Record>>,
    },
    /// Get storage proofs directly from a specific peer using request/response
    GetStorageProofsFromPeer {
        addr: NetworkAddress,
        peer: PeerInfo,
        nonce: u64,
        difficulty: usize,
        data_type: DataTypes,
        data_size: usize,
        #[debug(skip)]
        resp: OneShotTaskResult<PeerQuoteWithStorageProof>,
    },
    /// Get closest peers from a specific peer using request/response
    GetClosestPeersFromPeer {
        addr: NetworkAddress,
        peer: PeerInfo,
        num_of_peers: Option<usize>,
        #[debug(skip)]
        resp: OneShotTaskResult<Vec<(NetworkAddress, Vec<libp2p::Multiaddr>)>>,
    },
    /// Get information about the amount of connections made
    ConnectionsMade {
        #[debug(skip)]
        resp: OneShotTaskResult<usize>,
    },
    /// Get a Merkle candidate quote from a specific peer
    /// Used for Merkle batch payment system
    GetMerkleCandidateQuote {
        addr: NetworkAddress,
        peer: PeerInfo,
        data_type: u32,
        data_size: usize,
        merkle_payment_timestamp: u64,
        #[debug(skip)]
        resp: OneShotTaskResult<MerklePaymentCandidateNode>,
    },
}
