// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::constants::KAD_STREAM_PROTOCOL_ID;
use ant_protocol::messages::{QueryResponse, Response};
use libp2p::autonat::OutboundFailure;
use libp2p::identify::Event;
use libp2p::kad::{Event as KadEvent, ProgressStep, QueryId, QueryResult, QueryStats};
use libp2p::request_response::{Event as ReqEvent, Message, OutboundRequestId};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, StreamProtocol};
use thiserror::Error;

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
            SwarmEvent::Behaviour(AutonomiClientBehaviourEvent::Identify(Event::Received {
                peer_id,
                info,
                ..
            })) => self.handle_identify_received_event(peer_id, info),
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
                if finished {
                    if let Some(mut query) = self.kad().query_mut(&id) {
                        query.finish();
                    }
                }
            }
            QueryResult::PutRecord(res) => {
                trace!("PutRecord: {:?}", res);
                self.pending_tasks.update_put_record_kad(id, res)?;
            }
            QueryResult::GetProviders(res) => {
                trace!("GetProviders: {:?}", res);
            }
            QueryResult::Bootstrap(_) => {}
            QueryResult::StartProviding(_)
            | QueryResult::RepublishProvider(_)
            | QueryResult::RepublishRecord(_) => {}
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
            _ => {}
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

    fn handle_identify_received_event(
        &mut self,
        peer_id: PeerId,
        info: libp2p::identify::Info,
    ) -> Result<(), NetworkDriverError> {
        self.handle_bootstrap(peer_id, info);
        Ok(())
    }

    fn handle_bootstrap(&mut self, peer_id: PeerId, info: libp2p::identify::Info) {
        if !self.bootstrap_manager.has_observers() {
            return;
        }

        if info
            .protocols
            .contains(&StreamProtocol::new(KAD_STREAM_PROTOCOL_ID))
        {
            for listen_addr in info.listen_addrs {
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, listen_addr);

                self.bootstrap_manager.add_connected_peer(peer_id);
            }
        }
    }
}
