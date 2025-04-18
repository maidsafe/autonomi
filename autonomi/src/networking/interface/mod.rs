// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::OneShotTaskResult;
use ant_evm::PaymentQuote;
use ant_protocol::NetworkAddress;
use libp2p::{
    kad::{PeerInfo, Quorum, Record},
    PeerId,
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
    /// cf [`crate::driver::task_handler::TaskHandler::update_put_record`]
    PutRecord {
        #[debug(skip)]
        record: Record,
        /// Empty vec results in regular store to peers closest to record address
        to: Vec<PeerId>,
        quorum: Quorum,
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
        resp: OneShotTaskResult<Option<PaymentQuote>>,
    },
}
