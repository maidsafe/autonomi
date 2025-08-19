// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::NetworkError;
use crate::networking::OneShotTaskResult;
use crate::networking::interface::NetworkTask;
use crate::networking::utils::get_quorum_amount;
use ant_evm::PaymentQuote;
use ant_protocol::{NetworkAddress, PrettyPrintRecordKey};
use libp2p::PeerId;
use libp2p::kad::{self, PeerInfo, QueryId, Quorum, Record};
use libp2p::request_response::OutboundRequestId;
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
type RecordAndHolders = (Option<Record>, Vec<PeerId>);

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
    get_record: HashMap<QueryId, (OneShotTaskResult<RecordAndHolders>, Quorum)>,
    get_record_accumulator: HashMap<QueryId, HashMap<PeerId, Record>>,
}

impl TaskHandler {
    pub fn new() -> Self {
        Self {
            closest_peers: Default::default(),
            put_record_kad: Default::default(),
            put_record_req: Default::default(),
            get_cost: Default::default(),
            get_record: Default::default(),
            get_record_accumulator: Default::default(),
        }
    }

    pub fn contains(&self, id: &QueryId) -> bool {
        self.closest_peers.contains_key(id)
            || self.get_record.contains_key(id)
            || self.put_record_kad.contains_key(id)
    }

    pub fn contains_query(&self, id: &OutboundRequestId) -> bool {
        self.get_cost.contains_key(id) || self.put_record_req.contains_key(id)
    }

    pub fn insert_task(&mut self, id: QueryId, task: NetworkTask) {
        info!("New task: with QueryId({id}): {task:?}");
        match task {
            NetworkTask::GetClosestPeers { resp, .. } => {
                self.closest_peers.insert(id, resp);
            }
            NetworkTask::GetRecord { resp, quorum, .. } => {
                self.get_record.insert(id, (resp, quorum));
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

    /// Returns true if the task with QueryId is finished
    pub fn update_get_record(
        &mut self,
        id: QueryId,
        res: Result<kad::GetRecordOk, kad::GetRecordError>,
    ) -> Result<bool, TaskHandlerError> {
        match res {
            Ok(kad::GetRecordOk::FoundRecord(record)) => {
                trace!(
                    "QueryId({id}): GetRecordOk::FoundRecord {:?}",
                    PrettyPrintRecordKey::from(&record.record.key)
                );
                let holders = self.get_record_accumulator.entry(id).or_default();

                if let Some(peer_id) = record.peer {
                    holders.insert(peer_id, record.record);
                }

                // If we have enough holders, finish the task.
                if let Some((_resp, quorum)) = self.get_record.get(&id) {
                    let expected_holders = get_quorum_amount(quorum);

                    if holders.len() >= expected_holders {
                        info!("QueryId({id}): got enough holders, finishing task");
                        self.send_get_record_result(id)?;
                        return Ok(true);
                    }
                }
            }
            Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. }) => {
                trace!("QueryId({id}): GetRecordOk::FinishedWithNoAdditionalRecord");
                self.send_get_record_result(id)?;
                return Ok(true);
            }
            Err(kad::GetRecordError::NotFound { key, closest_peers }) => {
                trace!(
                    "QueryId({id}): GetRecordError::NotFound {:?}, closest_peers: {:?}",
                    hex::encode(key),
                    closest_peers
                );
                let ((responder, _), holders) = self.consume_get_record_task_and_holders(id)?;
                let peers = holders.keys().cloned().collect();

                responder
                    .send(Ok((None, peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
            Err(kad::GetRecordError::QuorumFailed {
                key,
                records,
                quorum,
            }) => {
                trace!(
                    "QueryId({id}): GetRecordError::QuorumFailed {:?}, records: {:?}, quorum: {:?}",
                    hex::encode(key),
                    records.len(),
                    quorum
                );
                let ((responder, _), holders) = self.consume_get_record_task_and_holders(id)?;
                let peers = holders.keys().cloned().collect();

                responder
                    .send(Ok((None, peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
            Err(kad::GetRecordError::Timeout { key }) => {
                trace!(
                    "QueryId({id}): GetRecordError::Timeout {:?}",
                    hex::encode(key)
                );
                let ((responder, _), holders) = self.consume_get_record_task_and_holders(id)?;
                let peers = holders.keys().cloned().collect();

                responder
                    .send(Err(NetworkError::GetRecordTimeout(peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;
            }
        }
        Ok(false)
    }

    pub fn send_get_record_result(&mut self, id: QueryId) -> Result<(), TaskHandlerError> {
        let ((responder, quorum), holders) = self.consume_get_record_task_and_holders(id)?;

        let expected_holders = get_quorum_amount(&quorum);

        if holders.len() < expected_holders {
            responder
                .send(Err(NetworkError::GetRecordQuorumFailed {
                    got_holders: holders.len(),
                    expected_holders,
                }))
                .map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id:?}")))?;

            return Ok(());
        }

        let peers = holders.keys().cloned().collect();

        let records_uniq = holders.values().cloned().fold(Vec::new(), |mut acc, x| {
            if !acc.contains(&x) {
                acc.push(x);
            }
            acc
        });

        let res = match &records_uniq[..] {
            [] => responder.send(Ok((None, peers))),
            [one] => responder.send(Ok((Some(one.clone()), peers))),
            [_one, _two, ..] => responder.send(Err(NetworkError::SplitRecord(holders))),
        };

        res.map_err(|_| TaskHandlerError::NetworkClientDropped(format!("{id}")))?;

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
        result: Result<(), ant_protocol::error::Error>,
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
            Err(ant_protocol::error::Error::OutdatedRecordCounter { counter, expected }) => {
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

    pub fn update_get_quote(
        &mut self,
        id: OutboundRequestId,
        quote_res: Result<PaymentQuote, ant_protocol::error::Error>,
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
            // Old nodes don't support the request response protocol for record puts
            // we can identify them with this error:
            // "Io(Custom { kind: UnexpectedEof, error: Eof { name: \"enum\", expect: Small(1) } })"
            // which is due to the mismatched request_response codec max_request_set configuration
            if error.to_string().contains("Small(1)") {
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
        } else {
            trace!(
                "OutboundRequestId({id}): trying to terminate unknown query, maybe it was already removed"
            );
        }
        Ok(())
    }

    /// Helper function to take the responder and holders from a get record task
    #[allow(clippy::type_complexity)]
    fn consume_get_record_task_and_holders(
        &mut self,
        id: QueryId,
    ) -> Result<
        (
            (OneShotTaskResult<RecordAndHolders>, Quorum),
            HashMap<PeerId, Record>,
        ),
        TaskHandlerError,
    > {
        let (responder, quorum) = self
            .get_record
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!("QueryId {id:?}")))?;
        let holders = self.get_record_accumulator.remove(&id).unwrap_or_default();
        Ok(((responder, quorum), holders))
    }
}

fn verify_quote(
    quote_res: Result<PaymentQuote, ant_protocol::error::Error>,
    peer_address: NetworkAddress,
    expected_data_type: QuoteDataType,
) -> Result<Option<PaymentQuote>, NetworkError> {
    let quote = match quote_res {
        Ok(quote) => quote,
        Err(ant_protocol::error::Error::RecordExists(_)) => return Ok(None),
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
