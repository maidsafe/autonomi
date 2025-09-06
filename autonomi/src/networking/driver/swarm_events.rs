// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::messages::{QueryResponse, Response};
use libp2p::autonat::OutboundFailure;
use libp2p::kad::{Event as KadEvent, ProgressStep, QueryId, QueryResult, QueryStats};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{Event as ReqEvent, Message, OutboundRequestId};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, PeerId};
use thiserror::Error;

const REQUIRED_PROTOCOLS: &[&str] = &["/autonomi/kad/"];

#[derive(Error, Debug, PartialEq, Eq)]
pub enum NetworkDriverError {
    #[error("TaskHandlerError: {0}")]
    TaskHandlerError(#[from] TaskHandlerError),
}

use super::task_handler::TaskHandlerError;
use super::{AutonomiClientBehaviourEvent, NetworkDriver};

impl NetworkDriver {
    /// Process a swarm event, ultimately handing over the event to the correct handler in [`crate::driver::task_handler::TaskHandler`]
    pub(crate) fn process_swarm_event(
        &mut self,
        swarm_event: SwarmEvent<AutonomiClientBehaviourEvent>,
    ) -> Result<(), NetworkDriverError> {
        match swarm_event {
            SwarmEvent::Behaviour(AutonomiClientBehaviourEvent::RequestResponse(
                ReqEvent::Message {
                    message:
                        Message::Response {
                            request_id,
                            response,
                        },
                    peer: _,
                    connection_id: _,
                },
            )) => self.handle_request_resp_event(request_id, response),
            SwarmEvent::Behaviour(AutonomiClientBehaviourEvent::RequestResponse(
                ReqEvent::OutboundFailure {
                    peer,
                    request_id,
                    error,
                    connection_id: _,
                },
            )) => self.handle_request_resp_outbound_failure(peer, request_id, error),
            SwarmEvent::Behaviour(AutonomiClientBehaviourEvent::Kademlia(
                KadEvent::OutboundQueryProgressed {
                    id,
                    result,
                    stats,
                    step,
                },
            )) => self.handle_kad_progress_event(id, result, &stats, &step),
            SwarmEvent::Behaviour(AutonomiClientBehaviourEvent::Identify(identify_event)) => {
                self.handle_identify_event(identify_event)
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                num_established,
                concurrent_dial_errors,
                established_in,
            } => {
                debug!(%peer_id, num_established, ?concurrent_dial_errors, "ConnectionEstablished ({connection_id:?}) in {established_in:?}: {}", endpoint_str(&endpoint));
                let _ = self.live_connected_peers.insert(
                    connection_id,
                    (peer_id, endpoint.get_remote_address().clone()),
                );
                self.connections_made += 1;
                Ok(())
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                endpoint,
                cause,
                num_established,
                connection_id,
            } => {
                debug!(%peer_id, ?connection_id, ?cause, num_established, "ConnectionClosed: {}", endpoint_str(&endpoint));
                let _ = self.live_connected_peers.remove(&connection_id);

                Ok(())
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id,
                error,
                connection_id,
            } => {
                debug!("OutgoingConnectionError to {peer_id:?} on {connection_id:?} - {error:?}");
                let _ = self.live_connected_peers.remove(&connection_id);

                Ok(())
            }
            _other_event => {
                trace!("Other event: {:?}", _other_event);
                Ok(())
            }
        }
    }

    fn handle_kad_progress_event(
        &mut self,
        id: QueryId,
        result: QueryResult,
        stats: &QueryStats,
        step: &ProgressStep,
    ) -> Result<(), NetworkDriverError> {
        // skip unknown or completed queries
        if !self.pending_tasks.contains(&id) {
            trace!("Ignore result for unknown query (possibly already completed): {id:?}");
            return Ok(());
        }

        // log info for queries we care about
        trace!(" | Kad progress event id: {:?}", id);
        trace!(" | stats: {:?}", stats);
        trace!(" | step: {:?}", step);

        match result {
            QueryResult::GetClosestPeers(res) => {
                trace!("GetClosestPeers: {:?}", res);
                self.pending_tasks.update_closest_peers(id, res)?;
            }
            QueryResult::GetRecord(res) => {
                // The result here is not logged because it can produce megabytes of text.
                trace!("GetRecord event occurred");
                let finished = self.pending_tasks.update_get_record(id, res)?;
                if finished && let Some(mut query) = self.kad().query_mut(&id) {
                    query.finish();
                }
            }
            QueryResult::PutRecord(res) => {
                trace!("PutRecord: {:?}", res);
                self.pending_tasks.update_put_record_kad(id, res)?;
            }
            QueryResult::GetProviders(res) => {
                trace!("GetProviders: {:?}", res);
            }
            _ => {
                trace!("Other Kad event: {:?}", result);
            }
        }
        Ok(())
    }

    fn handle_request_resp_event(
        &mut self,
        request_id: OutboundRequestId,
        response: Response,
    ) -> Result<(), NetworkDriverError> {
        trace!("Request response event: {:?}", response);

        // skip unknown or completed queries
        if !self.pending_tasks.contains_query(&request_id) {
            trace!("Ignore result for unknown query (possibly already completed): {request_id:?}");
            return Ok(());
        }

        match response {
            Response::Query(QueryResponse::GetStoreQuote {
                quote,
                peer_address,
                storage_proofs: _,
            }) => {
                self.pending_tasks
                    .update_get_quote(request_id, quote, peer_address)?;
            }
            Response::Query(QueryResponse::PutRecord {
                result,
                peer_address: _,
                record_addr: _,
            }) => {
                self.pending_tasks
                    .update_put_record_req(request_id, result)?;
            }

            _ => {
                trace!("Other request response event: {response:?}");
            }
        }

        Ok(())
    }

    fn handle_identify_event(
        &mut self,
        identify_event: libp2p::identify::Event,
    ) -> Result<(), NetworkDriverError> {
        trace!("Identify event: {identify_event:?}",);

        match &identify_event {
            libp2p::identify::Event::Received {
                peer_id,
                info,
                connection_id,
            } => {
                debug!(
                    "identify: received info from {peer_id:?} on {connection_id:?}. Info: {info:?}"
                );

                let banned = self.handle_blocklist(*peer_id, info);

                let Some((peer_id, addr_fom_connection)) =
                    self.live_connected_peers.get(connection_id)
                else {
                    warn!(
                        "identify: received info for peer {peer_id:?} on {connection_id:?} that is not in the live connected peers"
                    );
                    return Ok(());
                };
                if is_a_relayed_peer(info.listen_addrs.iter()) {
                    debug!("identify: peer {peer_id:?} is a relayed peer, skipping adding to RT.");
                    return Ok(());
                }

                let addr = craft_valid_multiaddr_without_p2p(addr_fom_connection);
                let Some(mut addr) = addr else {
                    warn!(
                        "identify: no valid multiaddr found for {peer_id:?} on {connection_id:?}"
                    );
                    return Ok(());
                };
                addr.push(Protocol::P2p(*peer_id));
                trace!("Peer {peer_id:?} is a normal peer, crafted valid multiaddress : {addr:?}.");

                if !banned && let Some(bootstrap_cache) = &self.bootstrap_cache {
                    let bootstrap_cache = bootstrap_cache.clone();
                    #[allow(clippy::let_underscore_future)]
                    let _ = tokio::spawn(async move { bootstrap_cache.add_addr(addr).await });
                }
            }
            _ => {
                trace!("Other identify event: {identify_event:?}");
            }
        }

        Ok(())
    }

    fn handle_request_resp_outbound_failure(
        &mut self,
        peer: PeerId,
        request_id: OutboundRequestId,
        error: OutboundFailure,
    ) -> Result<(), NetworkDriverError> {
        trace!("Request response outbound failure: {:?}", error);

        // skip unknown or completed queries
        if !self.pending_tasks.contains_query(&request_id) {
            trace!("Ignore result for unknown query (possibly already completed): {request_id:?}");
            return Ok(());
        }

        self.pending_tasks
            .terminate_query(request_id, peer, error)?;

        Ok(())
    }

    /// Check if the peer needs to be banned.
    /// Returns whether the peer was banned.
    fn handle_blocklist(&mut self, peer_id: PeerId, info: &libp2p::identify::Info) -> bool {
        // Check which required protocols are missing
        let missing_protocols: Vec<&&str> = REQUIRED_PROTOCOLS
            .iter()
            .filter(|required| {
                !info
                    .protocols
                    .iter()
                    .any(|protocol| protocol.as_ref().contains(*required))
            })
            .collect();

        if !missing_protocols.is_empty() {
            // Block the peer from any further communication.
            let _ = self.swarm.behaviour_mut().blocklist.block_peer(peer_id);
            if let Some(_dead_peer) = self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id) {
                error!(
                    "Blocking peer {peer_id:?} as it does not support mandatory protocols. Missing: {:?}",
                    missing_protocols
                );
            }
            return true;
        }

        false
    }
}

/// Helper function to print formatted connection role info.
fn endpoint_str(endpoint: &libp2p::core::ConnectedPoint) -> String {
    match endpoint {
        libp2p::core::ConnectedPoint::Dialer { address, .. } => {
            format!("outgoing ({address})")
        }
        libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } => {
            format!("incoming ({send_back_addr})")
        }
    }
}

/// Craft valid multiaddr like /ip4/68.183.39.80/udp/31055/quic-v1
fn craft_valid_multiaddr_without_p2p(addr: &Multiaddr) -> Option<Multiaddr> {
    let mut new_multiaddr = Multiaddr::empty();
    let ip = addr.iter().find_map(|p| match p {
        Protocol::Ip4(addr) => Some(addr),
        _ => None,
    })?;
    let port = multiaddr_get_port(addr)?;

    new_multiaddr.push(Protocol::Ip4(ip));
    new_multiaddr.push(Protocol::Udp(port));
    new_multiaddr.push(Protocol::QuicV1);

    Some(new_multiaddr)
}

fn multiaddr_get_port(addr: &Multiaddr) -> Option<u16> {
    addr.iter().find_map(|p| match p {
        Protocol::Udp(port) => Some(port),
        _ => None,
    })
}

fn is_a_relayed_peer<'a>(mut addrs: impl Iterator<Item = &'a Multiaddr>) -> bool {
    addrs.any(|multiaddr| multiaddr.iter().any(|p| matches!(p, Protocol::P2pCircuit)))
}
