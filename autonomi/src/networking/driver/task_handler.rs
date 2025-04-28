// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::PaymentQuote;
use ant_protocol::{NetworkAddress, PrettyPrintRecordKey};
use libp2p::kad::{self, PeerInfo, QueryId, Record};
use libp2p::request_response::OutboundRequestId;
use libp2p::PeerId;
use std::collections::HashMap;
use thiserror::Error;

use crate::networking::interface::NetworkTask;
use crate::networking::NetworkError;
use crate::networking::OneShotTaskResult;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum TaskHandlerError {
    #[error("No tasks matching query {0}, query might have been completed already")]
    UnknownQuery(String),
    #[error("Network client dropped, cannot send oneshot response")]
    NetworkClientDropped,
}

type QuoteDataType = u32;
type RecordAndHolders = (Option<Record>, Vec<PeerId>);

/// The [`TaskHandler`] is responsible for handling the progress in pending tasks using the results from [`crate::driver::NetworkDriver::process_swarm_event`]
/// Once a task is completed, the [`TaskHandler`] will send the result to the client [`crate::Network`] via the oneshot channel provided when the task was created
///
/// All fields in this struct are private so we know that only the code in this module can MUTATE them
#[derive(Default)]
pub(crate) struct TaskHandler {
    closest_peers: HashMap<QueryId, OneShotTaskResult<Vec<PeerInfo>>>,
    put_record: HashMap<QueryId, OneShotTaskResult<()>>,
    get_cost: HashMap<OutboundRequestId, (OneShotTaskResult<Option<PaymentQuote>>, QuoteDataType)>,
    get_record: HashMap<QueryId, OneShotTaskResult<RecordAndHolders>>,
    get_record_accumulator: HashMap<QueryId, HashMap<PeerId, Record>>,
}

impl TaskHandler {
    pub fn contains(&self, id: &QueryId) -> bool {
        self.closest_peers.contains_key(id)
            || self.get_record.contains_key(id)
            || self.put_record.contains_key(id)
    }

    pub fn contains_query(&self, id: &OutboundRequestId) -> bool {
        self.get_cost.contains_key(id)
    }

    pub fn insert_task(&mut self, id: QueryId, task: NetworkTask) {
        info!("New task: with QueryId({id}): {task:?}");
        match task {
            NetworkTask::GetClosestPeers { resp, .. } => {
                self.closest_peers.insert(id, resp);
            }
            NetworkTask::GetRecord { resp, .. } => {
                self.get_record.insert(id, resp);
            }
            NetworkTask::PutRecord { resp, .. } => {
                self.put_record.insert(id, resp);
            }
            _ => {}
        }
    }

    pub fn insert_query(&mut self, id: OutboundRequestId, task: NetworkTask) {
        info!("New query: with OutboundRequestId({id}): {task:?}");
        if let NetworkTask::GetQuote {
            resp, data_type, ..
        } = task
        {
            self.get_cost.insert(id, (resp, data_type));
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
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
            Err(kad::GetClosestPeersError::Timeout { key, peers }) => {
                trace!(
                    "QueryId({id}): GetClosestPeersError::Timeout {:?}, peers: {:?}",
                    hex::encode(key),
                    peers
                );
                responder
                    .send(Err(NetworkError::GetClosestPeersTimeout))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
        }
        Ok(())
    }

    pub fn update_get_record(
        &mut self,
        id: QueryId,
        res: Result<kad::GetRecordOk, kad::GetRecordError>,
    ) -> Result<(), TaskHandlerError> {
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
            }
            Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. }) => {
                trace!("QueryId({id}): GetRecordOk::FinishedWithNoAdditionalRecord");
                let (responder, holders) = self.take_responder_and_holders_for_task(id)?;
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
                res.map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
            Err(kad::GetRecordError::NotFound { key, closest_peers }) => {
                trace!(
                    "QueryId({id}): GetRecordError::NotFound {:?}, closest_peers: {:?}",
                    hex::encode(key),
                    closest_peers
                );
                let (responder, holders) = self.take_responder_and_holders_for_task(id)?;
                let peers = holders.keys().cloned().collect();

                responder
                    .send(Ok((None, peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
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
                let (responder, holders) = self.take_responder_and_holders_for_task(id)?;
                let peers = holders.keys().cloned().collect();

                responder
                    .send(Ok((None, peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
            Err(kad::GetRecordError::Timeout { key }) => {
                trace!(
                    "QueryId({id}): GetRecordError::Timeout {:?}",
                    hex::encode(key)
                );
                let (responder, holders) = self.take_responder_and_holders_for_task(id)?;
                let peers = holders.keys().cloned().collect();

                responder
                    .send(Err(NetworkError::GetRecordTimeout(peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
        }
        Ok(())
    }

    pub fn update_put_record(
        &mut self,
        id: QueryId,
        res: Result<kad::PutRecordOk, kad::PutRecordError>,
    ) -> Result<(), TaskHandlerError> {
        let responder = self
            .put_record
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!("QueryId {id:?}")))?;

        match res {
            Ok(kad::PutRecordOk { key: _ }) => {
                trace!("QueryId({id}): PutRecordOk");
                responder
                    .send(Ok(()))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
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
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
            Err(kad::PutRecordError::Timeout { success, .. }) => {
                trace!("QueryId({id}): PutRecordError::Timeout");
                responder
                    .send(Err(NetworkError::PutRecordTimeout(success)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
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
        let (resp, data_type) = self
            .get_cost
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!(
                "OutboundRequestId {id:?}"
            )))?;

        match verify_quote(quote_res, peer_address.clone(), data_type) {
            Ok(Some(quote)) => {
                trace!("OutboundRequestId({id}): got quote from peer {peer_address:?}");
                resp.send(Ok(Some(quote)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
                Ok(())
            }
            Ok(None) => {
                trace!("OutboundRequestId({id}): no quote needed as record already exists at peer {peer_address:?}");
                resp.send(Ok(None))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
                Ok(())
            }
            Err(e) => {
                warn!("OutboundRequestId({id}): got invalid quote from peer {peer_address:?}: {e}");
                resp.send(Err(e))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
                Ok(())
            }
        }
    }

    /// Helper function to take the responder and holders from a get record task
    fn take_responder_and_holders_for_task(
        &mut self,
        id: QueryId,
    ) -> Result<(OneShotTaskResult<RecordAndHolders>, HashMap<PeerId, Record>), TaskHandlerError>
    {
        let responder = self
            .get_record
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!("QueryId {id:?}")))?;
        let holders = self.get_record_accumulator.remove(&id).unwrap_or_default();
        Ok((responder, holders))
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
