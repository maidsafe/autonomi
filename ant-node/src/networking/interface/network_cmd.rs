// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::fmt::Debug;

use ant_protocol::messages::{ConnectionInfo, Request, Response};
use libp2p::PeerId;
use tokio::sync::oneshot;

use crate::networking::error::Result;
use crate::networking::{Addresses, NetworkAddress, driver::event::MsgResponder};

/// Commands to send to the Swarm
pub(crate) enum NetworkSwarmCmd {
    // Get closest peers from the network
    GetClosestPeersToAddressFromNetwork {
        key: NetworkAddress,
        sender: oneshot::Sender<Vec<(PeerId, Addresses)>>,
    },

    // Send Request to the PeerId.
    SendRequest {
        req: Request,
        peer: PeerId,
        /// Will try to add the address before sending the request.
        addrs: Addresses,

        // If a `sender` is provided, the requesting node will await for a `Response` from the
        // Peer. The result is then returned at the call site.
        //
        // If a `sender` is not provided, the requesting node will not wait for the Peer's
        // response. Instead we trigger a `NetworkEvent::ResponseReceived` which calls the common
        // `response_handler`
        #[allow(clippy::type_complexity)]
        sender: Option<oneshot::Sender<Result<(Response, Option<ConnectionInfo>)>>>,
    },
    SendResponse {
        resp: Response,
        channel: MsgResponder,
    },
}

/// Debug impl for NetworkSwarmCmd to avoid printing full Record, instead only RecodKey
/// and RecordKind are printed.
impl Debug for NetworkSwarmCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkSwarmCmd::GetClosestPeersToAddressFromNetwork { key, .. } => {
                write!(f, "NetworkSwarmCmd::GetClosestPeers {{ key: {key:?} }}")
            }
            NetworkSwarmCmd::SendResponse { resp, .. } => {
                write!(f, "NetworkSwarmCmd::SendResponse resp: {resp:?}")
            }
            NetworkSwarmCmd::SendRequest { req, peer, .. } => {
                write!(
                    f,
                    "NetworkSwarmCmd::SendRequest req: {req:?}, peer: {peer:?}"
                )
            }
        }
    }
}
