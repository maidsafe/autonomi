// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::messages::{QueryResponse, Response};
use libp2p::kad::{Event as KadEvent, ProgressStep, QueryId, QueryResult, QueryStats};
use libp2p::request_response::{Event as ReqEvent, Message, OutboundRequestId};
use libp2p::swarm::SwarmEvent;
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
            SwarmEvent::Behaviour(AutonomiClientBehaviourEvent::Kademlia(
                KadEvent::OutboundQueryProgressed {
                    id,
                    result,
                    stats,
                    step,
                },
            )) => self.handle_kad_progress_event(id, result, &stats, &step),
            _other_event => {
                // trace!("Other event: {:?}", _other_event);
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
                trace!("GetRecord: {:?}", res);
                self.pending_tasks.update_get_record(id, res)?;
            }
            QueryResult::PutRecord(res) => {
                trace!("PutRecord: {:?}", res);
                self.pending_tasks.update_put_record(id, res)?;
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

        if let Response::Query(QueryResponse::GetStoreQuote {
            quote,
            peer_address,
            storage_proofs: _,
        }) = response
        {
            self.pending_tasks
                .update_get_quote(request_id, quote, peer_address)?;
        }

        Ok(())
    }
}
