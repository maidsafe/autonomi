// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::SwarmDriver;
use crate::networking::driver::event::MsgResponder;
use crate::networking::interface::NetworkSwarmCmd;
use crate::networking::network::connection_action_logging;
use crate::networking::{log_markers::Marker, NetworkError, NetworkEvent};
use ant_protocol::messages::ConnectionInfo;
use ant_protocol::{
    messages::{CmdResponse, Request, Response},
    storage::ValidationType,
    NetworkAddress,
};
use libp2p::request_response::{self, Message};

impl SwarmDriver {
    /// Forwards `Request` to the upper layers using `Sender<NetworkEvent>`. Sends `Response` to the peers
    pub(super) fn handle_req_resp_events(
        &mut self,
        event: request_response::Event<Request, Response>,
    ) -> Result<(), NetworkError> {
        match event {
            request_response::Event::Message {
                message,
                peer,
                connection_id,
            } => match message {
                Message::Request {
                    request,
                    channel,
                    request_id,
                    ..
                } => {
                    // ELK logging. Do not update without proper testing.
                    let action_string = match &request {
                        Request::Cmd(cmd) => match cmd {
                            ant_protocol::messages::Cmd::Replicate { .. } => {
                                "Request::Cmd::Replicate"
                            }
                            ant_protocol::messages::Cmd::FreshReplicate { .. } => {
                                "Request::Cmd::FreshReplicate"
                            }
                            ant_protocol::messages::Cmd::PeerConsideredAsBad { .. } => {
                                "Request::Cmd::PeerConsideredAsBad"
                            }
                        },
                        Request::Query(query) => match query {
                            ant_protocol::messages::Query::PutRecord { .. } => {
                                "Request::Query::PutRecord"
                            }
                            ant_protocol::messages::Query::GetStoreQuote { .. } => {
                                "Request::Query::GetStoreQuote"
                            }
                            ant_protocol::messages::Query::GetReplicatedRecord { .. } => {
                                "Request::Query::GetReplicatedRecord"
                            }
                            ant_protocol::messages::Query::GetChunkExistenceProof { .. } => {
                                "Request::Query::GetChunkExistenceProof"
                            }
                            ant_protocol::messages::Query::CheckNodeInProblem(_) => {
                                "Request::Query::CheckNodeInProblem"
                            }
                            ant_protocol::messages::Query::GetClosestPeers { .. } => {
                                "Request::Query::GetClosestPeers"
                            }
                            ant_protocol::messages::Query::GetVersion(..) => {
                                "Request::Query::GetVersion"
                            }
                        },
                    };
                    connection_action_logging(
                        &peer,
                        &self.self_peer_id,
                        &connection_id,
                        action_string,
                    );

                    debug!("Received request {request_id:?} from peer {peer:?}, req: {request:?}");
                    // If the request is replication or quote verification,
                    // we can handle it and send the OK response here.
                    // As the handle result is unimportant to the sender.
                    match request {
                        Request::Cmd(ant_protocol::messages::Cmd::Replicate { holder, keys }) => {
                            let response = Response::Cmd(
                                ant_protocol::messages::CmdResponse::Replicate(Ok(())),
                            );

                            self.queue_network_swarm_cmd(NetworkSwarmCmd::SendResponse {
                                resp: response,
                                channel: MsgResponder::FromPeer(channel),
                            });

                            self.add_keys_to_replication_fetcher(holder, keys, false)?;
                        }
                        Request::Cmd(ant_protocol::messages::Cmd::FreshReplicate {
                            holder,
                            keys,
                        }) => {
                            let response = Response::Cmd(
                                ant_protocol::messages::CmdResponse::FreshReplicate(Ok(())),
                            );

                            self.queue_network_swarm_cmd(NetworkSwarmCmd::SendResponse {
                                resp: response,
                                channel: MsgResponder::FromPeer(channel),
                            });

                            self.send_event(NetworkEvent::FreshReplicateToFetch { holder, keys });
                        }
                        Request::Cmd(ant_protocol::messages::Cmd::PeerConsideredAsBad {
                            detected_by,
                            bad_peer,
                            bad_behaviour,
                        }) => {
                            let response = Response::Cmd(
                                ant_protocol::messages::CmdResponse::PeerConsideredAsBad(Ok(())),
                            );

                            self.queue_network_swarm_cmd(NetworkSwarmCmd::SendResponse {
                                resp: response,
                                channel: MsgResponder::FromPeer(channel),
                            });

                            let (Some(detected_by), Some(bad_peer)) =
                                (detected_by.as_peer_id(), bad_peer.as_peer_id())
                            else {
                                error!(
                                    "Could not get PeerId from detected_by or bad_peer NetworkAddress {detected_by:?}, {bad_peer:?}"
                                );
                                return Ok(());
                            };

                            if bad_peer == self.self_peer_id {
                                warn!(
                                    "Peer {detected_by:?} consider us as BAD, due to {bad_behaviour:?}."
                                );
                                self.record_metrics(Marker::FlaggedAsBadNode {
                                    flagged_by: &detected_by,
                                });
                            } else {
                                error!(
                                    "Received a bad_peer notification from {detected_by:?}, targeting {bad_peer:?}, which is not us."
                                );
                            }
                        }
                        Request::Query(query) => {
                            self.send_event(NetworkEvent::QueryRequestReceived {
                                query,
                                channel: MsgResponder::FromPeer(channel),
                            })
                        }
                    }
                }
                Message::Response {
                    request_id,
                    response,
                } => {
                    // ELK logging. Do not update without proper testing.
                    let action_string = match &response {
                        Response::Cmd(cmd_response) => match cmd_response {
                            CmdResponse::Replicate(result) => {
                                format!("Response::Cmd::Replicate::{}", result_to_str(result))
                            }
                            CmdResponse::FreshReplicate(result) => {
                                format!("Response::Cmd::FreshReplicate::{}", result_to_str(result))
                            }
                            CmdResponse::PeerConsideredAsBad(result) => format!(
                                "Response::Cmd::PeerConsideredAsBad::{}",
                                result_to_str(result)
                            ),
                        },
                        Response::Query(query_response) => match query_response {
                            ant_protocol::messages::QueryResponse::PutRecord { result, .. } => {
                                format!("Response::Query::PutRecord::{}", result_to_str(result))
                            }
                            ant_protocol::messages::QueryResponse::GetStoreQuote {
                                quote, ..
                            } => {
                                format!("Response::Query::GetStoreQuote::{}", result_to_str(quote))
                            }
                            ant_protocol::messages::QueryResponse::CheckNodeInProblem {
                                ..
                            } => "Response::Query::CheckNodeInProblem".to_string(),
                            ant_protocol::messages::QueryResponse::GetReplicatedRecord(result) => {
                                format!(
                                    "Response::Query::GetReplicatedRecord::{}",
                                    result_to_str(result)
                                )
                            }
                            ant_protocol::messages::QueryResponse::GetChunkExistenceProof(_) => {
                                "Response::Query::GetChunkExistenceProof".to_string()
                            }
                            ant_protocol::messages::QueryResponse::GetClosestPeers { .. } => {
                                "Response::Query::GetClosestPeers".to_string()
                            }
                            ant_protocol::messages::QueryResponse::GetVersion { .. } => {
                                "Response::Query::GetVersion".to_string()
                            }
                        },
                    };
                    connection_action_logging(
                        &peer,
                        &self.self_peer_id,
                        &connection_id,
                        &action_string,
                    );

                    debug!("Got response {request_id:?} from peer {peer:?}, res: {response}.");
                    if let Some(sender) = self.pending_requests.remove(&request_id) {
                        // Get the optional connection info.
                        let connection_info =
                            self.live_connected_peers.get(&connection_id).cloned().map(
                                |(peer_id, multiaddr, ..)| ConnectionInfo {
                                    peer_id,
                                    response_origin: multiaddr,
                                },
                            );

                        // The sender will be provided if the caller (Requester) is awaiting for a response
                        // at the call site.
                        // Else the Request was just sent to the peer and the Response was
                        // meant to be handled in another way and is not awaited.
                        match sender {
                            Some(sender) => sender
                                .send(Ok((response, connection_info)))
                                .map_err(|_| NetworkError::InternalMsgChannelDropped)?,
                            None => {
                                if let Response::Cmd(CmdResponse::Replicate(Ok(()))) = response {
                                    // Nothing to do, response was fine
                                    // This only exists to ensure we dont drop the handle and
                                    // exit early, potentially logging false connection woes
                                } else {
                                    // responses that are not awaited at the call site must be handled
                                    // separately
                                    self.send_event(NetworkEvent::ResponseReceived {
                                        res: response,
                                    });
                                }
                            }
                        }
                    } else {
                        warn!("Tried to remove a RequestId from pending_requests which was not inserted in the first place.
                            Use Cmd::SendRequest with sender:None if you want the Response to be fed into the common handle_response function");
                    }
                }
            },
            request_response::Event::OutboundFailure {
                request_id,
                error,
                peer,
                connection_id,
            } => {
                // ELK logging. Do not update without proper testing.
                connection_action_logging(
                    &peer,
                    &self.self_peer_id,
                    &connection_id,
                    "RequestResponse::OutboundFailure",
                );
                if let Some(sender) = self.pending_requests.remove(&request_id) {
                    match sender {
                        Some(sender) => {
                            sender
                                .send(Err(error.into()))
                                .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                        }
                        None => {
                            warn!(
                                "RequestResponse: OutboundFailure for request_id: {request_id:?} and peer: {peer:?}, with error: {error:?}"
                            );
                            return Err(NetworkError::ReceivedResponseDropped(request_id));
                        }
                    }
                } else {
                    warn!(
                        "RequestResponse: OutboundFailure for request_id: {request_id:?} and peer: {peer:?}, with error: {error:?}"
                    );
                    return Err(NetworkError::ReceivedResponseDropped(request_id));
                }
            }
            request_response::Event::InboundFailure {
                peer,
                request_id,
                error,
                connection_id,
            } => {
                // ELK logging. Do not update without proper testing.
                connection_action_logging(
                    &peer,
                    &self.self_peer_id,
                    &connection_id,
                    "RequestResponse::InboundFailure",
                );
                warn!(
                    "RequestResponse: InboundFailure for request_id: {request_id:?} and peer: {peer:?}, with error: {error:?}"
                );
            }
            request_response::Event::ResponseSent {
                peer,
                request_id,
                connection_id,
            } => {
                // ELK logging. Do not update without proper testing.
                connection_action_logging(
                    &peer,
                    &self.self_peer_id,
                    &connection_id,
                    "RequestResponse::ResponseSent",
                );
                debug!("ResponseSent for request_id: {request_id:?} and peer: {peer:?}");
            }
        }
        Ok(())
    }

    pub(crate) fn add_keys_to_replication_fetcher(
        &mut self,
        sender: NetworkAddress,
        incoming_keys: Vec<(NetworkAddress, ValidationType)>,
        is_fresh_replicate: bool,
    ) -> Result<(), NetworkError> {
        let holder = if let Some(peer_id) = sender.as_peer_id() {
            peer_id
        } else {
            warn!("Replication list sender is not a peer_id {sender:?}");
            return Ok(());
        };

        debug!(
            "Received replication list from {holder:?} of {} keys is_fresh_replicate {is_fresh_replicate:?}",
            incoming_keys.len()
        );

        // accept replication requests from the K_VALUE peers away,
        // giving us some margin for replication
        let closest_40_peers = self.get_closest_40_local_peers_to_self();
        if !closest_40_peers
            .iter()
            .any(|(peer_id, _)| peer_id == &holder)
            || holder == self.self_peer_id
        {
            let distance =
                NetworkAddress::from(holder).distance(&NetworkAddress::from(self.self_peer_id));
            info!("Holder {holder:?} is self or not in replication range. Distance is {distance:?}({:?})", distance.ilog2());
            return Ok(());
        }

        // On receive a replication_list from a close up peer, we undertake:
        //   1, For those keys that we don't have:
        //        fetch them if close enough to us
        //   2, For those GraphEntry that we have that differ in the hash, we fetch the other version
        //         and update our local copy.
        let all_keys = self
            .swarm
            .behaviour_mut()
            .kademlia
            .store_mut()
            .record_addresses_ref();
        let keys_to_fetch = self.replication_fetcher.add_keys(
            holder,
            incoming_keys,
            all_keys,
            is_fresh_replicate,
            closest_40_peers
                .iter()
                .map(|(peer_id, _addrs)| NetworkAddress::from(*peer_id))
                .collect(),
        );
        if keys_to_fetch.is_empty() {
            debug!("no waiting keys to fetch from the network");
        } else {
            self.send_event(NetworkEvent::KeysToFetchForReplication(keys_to_fetch));
        }

        Ok(())
    }
}

fn result_to_str<T>(result: &Result<T, ant_protocol::Error>) -> &'static str {
    match result {
        Ok(_) => "Ok",
        Err(_) => "Err",
    }
}
