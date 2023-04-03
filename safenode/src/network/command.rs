// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    error::Error,
    safe_msg::{SafeRequest, SafeResponse},
    EventLoop,
};
use crate::network::error::Result;
use futures::channel::oneshot;
use libp2p::{multiaddr::Protocol, request_response::ResponseChannel, Multiaddr, PeerId};
use std::collections::{hash_map, HashSet};
use tracing::warn;
use xor_name::XorName;

/// Commands to send to the Swarm
#[derive(Debug)]
pub(crate) enum CmdToSwarm {
    StartListening {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<()>>,
    },
    Dial {
        peer_id: PeerId,
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<()>>,
    },
    StoreData {
        xor_name: XorName,
        sender: oneshot::Sender<Result<()>>,
    },
    GetDataProviders {
        xor_name: XorName,
        sender: oneshot::Sender<HashSet<PeerId>>,
    },
    SendSafeRequest {
        req: SafeRequest,
        peer: PeerId,
        sender: oneshot::Sender<Result<SafeResponse>>,
    },
    SendSafeResponse {
        resp: SafeResponse,
        channel: ResponseChannel<SafeResponse>,
    },
}

impl EventLoop {
    pub(crate) fn handle_command(&mut self, command: CmdToSwarm) -> Result<(), Error> {
        match command {
            CmdToSwarm::StartListening { addr, sender } => {
                let _ = match self.swarm.listen_on(addr) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(e.into())),
                };
            }
            CmdToSwarm::Dial {
                peer_id,
                peer_addr,
                sender,
            } => {
                if let hash_map::Entry::Vacant(e) = self.pending_dial.entry(peer_id) {
                    let _routing_update = self
                        .swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, peer_addr.clone());
                    match self
                        .swarm
                        .dial(peer_addr.with(Protocol::P2p(peer_id.into())))
                    {
                        Ok(()) => {
                            let _ = e.insert(sender);
                        }
                        Err(e) => {
                            let _ = sender.send(Err(e.into()));
                        }
                    }
                } else {
                    warn!("Already dialing peer.");
                }
            }
            CmdToSwarm::StoreData { xor_name, sender } => {
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .start_providing(xor_name.0.to_vec().into())?;
                let _ = self.pending_start_providing.insert(query_id, sender);
            }
            CmdToSwarm::GetDataProviders { xor_name, sender } => {
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_providers(xor_name.0.to_vec().into());
                let _ = self.pending_get_providers.insert(query_id, sender);
            }
            CmdToSwarm::SendSafeRequest { req, peer, sender } => {
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer, req);
                let _ = self.pending_safe_requests.insert(request_id, sender);
            }
            CmdToSwarm::SendSafeResponse { resp, channel } => {
                self.swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, resp)
                    .map_err(|_| {
                        Error::Other("Connection to peer to be still open.".to_string())
                    })?;
            }
        }
        Ok(())
    }
}
