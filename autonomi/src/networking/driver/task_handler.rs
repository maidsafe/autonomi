// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::interface::NetworkTask;
use crate::networking::NetworkError;
use crate::networking::OneShotTaskResult;
use ant_evm::PaymentQuote;
use ant_protocol::{error::Error as ProtocolError, Bytes, NetworkAddress};
use libp2p::kad::{self, PeerInfo, QueryId};
use libp2p::request_response::OutboundRequestId;
use libp2p::PeerId;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum TaskHandlerError {
    #[error("No tasks matching query {0}, query might have been completed already")]
    UnknownQuery(String),
    #[error("Network client dropped, cannot send oneshot response for: {0}")]
    NetworkClientDropped(String),
}

type QuoteDataType = u32;

/// The [`TaskHandler`] is responsible for handling the progress in pending tasks using the results from [`crate::driver::NetworkDriver::process_swarm_event`]
/// Once a task is completed, the [`TaskHandler`] will send the result to the client [`crate::Network`] via the oneshot channel provided when the task was created
///
/// All fields in this struct are private so we know that only the code in this module can MUTATE them
#[allow(clippy::type_complexity)]
pub(crate) struct TaskHandler {
    closest_peers: HashMap<QueryId, OneShotTaskResult<Vec<PeerInfo>>>,
    put_record_kad: HashMap<QueryId, OneShotTaskResult<()>>,
    put_record_req: HashMap<OutboundRequestId, OneShotTaskResult<()>>,
    get_cost: HashMap<
        OutboundRequestId,
        (
            OneShotTaskResult<Option<(PeerInfo, PaymentQuote)>>,
            QuoteDataType,
            PeerInfo,
        ),
    >,
    get_record_req: HashMap<OutboundRequestId, OneShotTaskResult<Option<Vec<u8>>>>,
}

impl TaskHandler {
    pub fn new() -> Self {
        Self {
            closest_peers: Default::default(),
            put_record_kad: Default::default(),
            put_record_req: Default::default(),
            get_cost: Default::default(),
            get_record_req: Default::default(),
        }
    }

    pub fn contains(&self, id: &QueryId) -> bool {
        self.closest_peers.contains_key(id) || self.put_record_kad.contains_key(id)
    }

    pub fn contains_query(&self, id: &OutboundRequestId) -> bool {
        self.get_cost.contains_key(id)
            || self.put_record_req.contains_key(id)
            || self.get_record_req.contains_key(id)
    }

    pub fn insert_task(&mut self, id: QueryId, task: NetworkTask) {
        info!("New task: with QueryId({id}): {task:?}");
        match task {
            NetworkTask::GetClosestPeers { resp, .. } => {
                self.closest_peers.insert(id, resp);
            }
            NetworkTask::PutRecordKad { resp, .. } => {
                self.put_record_kad.insert(id, resp);
            }
            _ => {}
        }
    }

    pub fn insert_query(&mut self, id: OutboundRequestId, task: NetworkTask) {
        info!("New query: with OutboundRequestId({id}): {task:?}");
        match task {
            NetworkTask::GetQuote {
                resp,
                data_type,
                peer,
                ..
            } => {
                self.get_cost.insert(id, (resp, data_type, peer));
            }
            NetworkTask::PutRecordReq { resp, .. } => {
                self.put_record_req.insert(id, resp);
            }
            NetworkTask::GetRecordReq { resp, .. } => {
                self.get_record_req.insert(id, resp);
            }
            _ => {}
        }
    }

    pub fn update_closest_peers(
        &mut self,
        id: QueryId,
        res: Result<kad::GetClosestPeersOk, kad::GetClosestPeersError>,
    ) -> Result<(), TaskHandlerError> {
        let responder = self
            .closest_peers
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!("QueryId {id:?}")))?;

        match res {
            Ok(kad::GetClosestPeersOk { peers, .. }) => {
                responder
                    .send(Ok(peers))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
            Err(kad::GetClosestPeersError::Timeout { key, peers }) => {
                trace!(
                    "QueryId({id}): GetClosestPeersError::Timeout {:?}, peers: {:?}",
                    hex::encode(key),
                    peers
                );
                responder
                    .send(Err(NetworkError::GetClosestPeersTimeout))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
        }
        Ok(())
    }

    pub fn update_put_record_kad(
        &mut self,
        id: QueryId,
        res: Result<kad::PutRecordOk, kad::PutRecordError>,
    ) -> Result<(), TaskHandlerError> {
        let responder = self
            .put_record_kad
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!("QueryId {id:?}")))?;

        match res {
            Ok(kad::PutRecordOk { key: _ }) => {
                trace!("QueryId({id}): PutRecordOk");
                responder
                    .send(Ok(()))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
            Err(kad::PutRecordError::QuorumFailed {
                key,
                success,
                quorum,
            }) => {
                trace!(
                    "QueryId({id}): PutRecordError::QuorumFailed {:?}, success: {:?}, quorum: {:?}",
                    hex::encode(key),
                    success.len(),
                    quorum
                );
                responder
                    .send(Err(NetworkError::PutRecordQuorumFailed(success, quorum)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
            Err(kad::PutRecordError::Timeout { success, .. }) => {
                trace!("QueryId({id}): PutRecordError::Timeout");
                responder
                    .send(Err(NetworkError::PutRecordTimeout(success)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
        }
        Ok(())
    }

    pub fn update_put_record_req(
        &mut self,
        id: OutboundRequestId,
        result: Result<(), ProtocolError>,
    ) -> Result<(), TaskHandlerError> {
        let responder = self
            .put_record_req
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!(
                "OutboundRequestId {id:?}"
            )))?;

        match result {
            Ok(()) => {
                responder
                    .send(Ok(()))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
            Err(ProtocolError::OutdatedRecordCounter { counter, expected }) => {
                trace!(
                    "OutboundRequestId({id}): put record got outdated record error: counter: {counter}, expected: {expected}"
                );
                responder
                    .send(Err(NetworkError::OutdatedRecordRejected {
                        counter,
                        expected,
                    }))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
            Err(e) => {
                responder
                    .send(Err(NetworkError::PutRecordRejected(e.to_string())))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
        }
        Ok(())
    }

    pub fn update_get_record_req(
        &mut self,
        request_id: OutboundRequestId,
        result: Result<(NetworkAddress, Bytes), ant_protocol::Error>,
    ) -> Result<(), TaskHandlerError> {
        let responder =
            self.get_record_req
                .remove(&request_id)
                .ok_or(TaskHandlerError::UnknownQuery(format!(
                    "OutboundRequestId {request_id:?}"
                )))?;

        match result {
            Ok((_addr, data)) => {
                responder.send(Ok(Some(data.to_vec()))).map_err(|_| {
                    TaskHandlerError::NetworkClientDropped(
                        "OutboundRequestId {request_id:?}".to_string(),
                    )
                })?;
            }
            Err(ProtocolError::ReplicatedRecordNotFound { .. }) => {
                responder
                    .send(Ok(None))
                    .map_err(|e| TaskHandlerError::NetworkClientDropped(format!("{e:?}")))?;
            }
            Err(e) => {
                responder
                    .send(Err(NetworkError::GetRecordError(e.to_string())))
                    .map_err(|e| TaskHandlerError::NetworkClientDropped(format!("{e:?}")))?;
            }
        }
        Ok(())
    }

    pub fn update_get_quote(
        &mut self,
        id: OutboundRequestId,
        quote_res: Result<PaymentQuote, ProtocolError>,
        peer_address: NetworkAddress,
    ) -> Result<(), TaskHandlerError> {
        let (resp, data_type, peer) =
            self.get_cost
                .remove(&id)
                .ok_or(TaskHandlerError::UnknownQuery(format!(
                    "OutboundRequestId {id:?}"
                )))?;

        match verify_quote(quote_res, peer_address.clone(), data_type) {
            Ok(Some(quote)) => {
                trace!("OutboundRequestId({id}): got quote from peer {peer_address:?}");
                // Send can fail here if we already accumulated enough quotes.
                resp.send(Ok(Some((peer, quote))))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
                Ok(())
            }
            Ok(None) => {
                trace!(
                    "OutboundRequestId({id}): no quote needed as record already exists at peer {peer_address:?}"
                );
                // Send can fail here if we already accumulated enough quotes.
                resp.send(Ok(None))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
                Ok(())
            }
            Err(e) => {
                warn!("OutboundRequestId({id}): got invalid quote from peer {peer_address:?}: {e}");
                // Send can fail here if we already accumulated enough quotes.
                resp.send(Err(e))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
                Ok(())
            }
        }
    }

    pub fn terminate_query(
        &mut self,
        id: OutboundRequestId,
        peer: PeerId,
        error: libp2p::autonat::OutboundFailure,
    ) -> Result<(), TaskHandlerError> {
        // Get quote case
        if let Some((resp, _data_type, original_peer)) = self.get_cost.remove(&id) {
            trace!(
                "OutboundRequestId({id}): get quote initially sent to peer {original_peer:?} got fatal error from peer {peer:?}: {error:?}"
            );
            resp.send(Err(NetworkError::GetQuoteError(error.to_string())))
                .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
        // Put record case
        } else if let Some(responder) = self.put_record_req.remove(&id) {
            trace!(
                "OutboundRequestId({id}): put record got fatal error from peer {peer:?}: {error:?}"
            );
            if is_incompatible_network_protocol(&error) {
                trace!(
                    "OutboundRequestId({id}): put record got incompatible network protocol error from peer {peer:?}"
                );
                responder
                    .send(Err(NetworkError::IncompatibleNetworkProtocol))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            } else {
                responder
                    .send(Err(NetworkError::PutRecordRejected(error.to_string())))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }

        // Get record req case
        } else if let Some(responder) = self.get_record_req.remove(&id) {
            trace!(
                "OutboundRequestId({id}): get record got fatal error from peer {peer:?}: {error:?}"
            );
            if is_incompatible_network_protocol(&error) {
                trace!("OutboundRequestId({id}): put record got incompatible network protocol error from peer {peer:?}");
                responder
                    .send(Err(NetworkError::IncompatibleNetworkProtocol))
                    .map_err(|e| TaskHandlerError::NetworkClientDropped(format!("{e:?}")))?;
            } else {
                responder
                    .send(Err(NetworkError::GetRecordError(error.to_string())))
                    .map_err(|e| TaskHandlerError::NetworkClientDropped(format!("{e:?}")))?;
            }

        // Unknown query case
        } else {
            trace!(
                "OutboundRequestId({id}): trying to terminate unknown query, maybe it was already removed"
            );
        }
        Ok(())
    }
}

fn verify_quote(
    quote_res: Result<PaymentQuote, ProtocolError>,
    peer_address: NetworkAddress,
    expected_data_type: QuoteDataType,
) -> Result<Option<PaymentQuote>, NetworkError> {
    let quote = match quote_res {
        Ok(quote) => quote,
        Err(ProtocolError::RecordExists(_)) => return Ok(None),
        Err(e) => return Err(NetworkError::GetQuoteError(e.to_string())),
    };

    // Check the quote itself is valid
    let peer_id = peer_address
        .as_peer_id()
        .ok_or(NetworkError::InvalidQuote(format!(
            "Peer address is not a peer id: {peer_address:?}"
        )))?;
    if !quote.check_is_signed_by_claimed_peer(peer_id) {
        return Err(NetworkError::InvalidQuote(format!(
            "Quote is not signed by claimed peer: {peer_address:?}"
        )));
    }
    if quote.quoting_metrics.data_type != expected_data_type {
        return Err(NetworkError::InvalidQuote(format!(
            "Quote returned with wrong data type by peer: {peer_address:?}"
        )));
    }

    Ok(Some(quote))
}

fn is_incompatible_network_protocol(error: &libp2p::autonat::OutboundFailure) -> bool {
    // Old nodes don't support the request response protocol for record puts
    // we can identify them with this error:
    // "Io(Custom { kind: UnexpectedEof, error: Eof { name: \"enum\", expect: Small(1) } })"
    // which is due to the mismatched request_response codec max_request_set configuration
    error.to_string().contains("Small(1)")
}
