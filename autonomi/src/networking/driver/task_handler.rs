// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::interface::{Command, NetworkTask};
use crate::networking::utils::get_quorum_amount;
use crate::networking::NetworkError;
use crate::networking::OneShotTaskResult;
use ant_evm::PaymentQuote;
use ant_protocol::{NetworkAddress, PrettyPrintRecordKey};
use libp2p::kad::{self, PeerInfo, QueryId, Quorum, Record};
use libp2p::request_response::OutboundRequestId;
use libp2p::PeerId;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use xor_name::XorName;

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
#[allow(clippy::type_complexity)]
pub(crate) struct TaskHandler {
    /// Used to send commands to the network driver, like terminating kad queries.
    network_driver_cmd_sender: Sender<Command>,
    closest_peers: HashMap<QueryId, OneShotTaskResult<Vec<PeerInfo>>>,
    put_record: HashMap<QueryId, OneShotTaskResult<()>>,
    get_cost: HashMap<
        OutboundRequestId,
        (
            OneShotTaskResult<Option<(PeerInfo, PaymentQuote)>>,
            QuoteDataType,
            PeerInfo,
        ),
    >,
    get_record: HashMap<QueryId, (OneShotTaskResult<RecordAndHolders>, Quorum)>,
    get_record_accumulator: HashMap<QueryId, HashMap<XorName, (Record, HashSet<PeerId>)>>,
}

impl TaskHandler {
    pub fn new(network_driver_cmd_sender: Sender<Command>) -> Self {
        Self {
            network_driver_cmd_sender,
            closest_peers: Default::default(),
            put_record: Default::default(),
            get_cost: Default::default(),
            get_record: Default::default(),
            get_record_accumulator: Default::default(),
        }
    }

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
            NetworkTask::GetRecord { resp, quorum, .. } => {
                self.get_record.insert(id, (resp, quorum));
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
            resp,
            data_type,
            peer,
            ..
        } = task
        {
            self.get_cost.insert(id, (resp, data_type, peer));
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

    pub async fn update_get_record(
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

                let record_results = self.get_record_accumulator.entry(id).or_default();

                if let Some(peer_id) = record.peer {
                    let record_content_hash = XorName::from_content(&record.record.value);

                    let entry = record_results
                        .entry(record_content_hash)
                        .or_insert_with(|| (record.record.clone(), Default::default()));

                    entry.1.insert(peer_id);
                }

                // If we have enough holders, finish the task.
                if let Some((_resp, quorum)) = self.get_record.get(&id) {
                    let expected_holders = get_quorum_amount(quorum);

                    if record_results
                        .iter()
                        .any(|(_, (_, peers))| peers.len() >= expected_holders)
                    {
                        info!("QueryId({id}): got enough holders, finishing task");
                        self.finish_get_record(id).await?;
                    }
                }
            }
            Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. }) => {
                trace!("QueryId({id}): GetRecordOk::FinishedWithNoAdditionalRecord");
                self.finish_get_record(id).await?;
            }
            Err(kad::GetRecordError::NotFound { key, closest_peers }) => {
                trace!(
                    "QueryId({id}): GetRecordError::NotFound {:?}, closest_peers: {:?}",
                    hex::encode(key),
                    closest_peers
                );
                let ((responder, _), record_results) =
                    self.consume_get_record_task_and_results(id)?;

                let peers: Vec<_> = record_results
                    .values()
                    .flat_map(|(_, peers_set)| peers_set.iter().cloned())
                    .collect();

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
                let ((responder, _), record_results) =
                    self.consume_get_record_task_and_results(id)?;

                let peers: Vec<_> = record_results
                    .values()
                    .flat_map(|(_, peers_set)| peers_set.iter().cloned())
                    .collect();

                responder
                    .send(Ok((None, peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
            Err(kad::GetRecordError::Timeout { key }) => {
                trace!(
                    "QueryId({id}): GetRecordError::Timeout {:?}",
                    hex::encode(key)
                );
                let ((responder, _), record_results) =
                    self.consume_get_record_task_and_results(id)?;

                let peers: Vec<_> = record_results
                    .values()
                    .flat_map(|(_, peers_set)| peers_set.iter().cloned())
                    .collect();

                responder
                    .send(Err(NetworkError::GetRecordTimeout(peers)))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
            }
        }
        Ok(())
    }

    pub async fn finish_get_record(&mut self, id: QueryId) -> Result<(), TaskHandlerError> {
        let ((responder, quorum), record_results) = self.consume_get_record_task_and_results(id)?;

        self.finish_kad_query(id).await;

        let expected_holders = get_quorum_amount(&quorum);

        // Return quorum error if none of the records have enough holders.
        if !record_results
            .iter()
            .any(|(_, (_, peers))| peers.len() >= expected_holders)
        {
            responder
                .send(Err(NetworkError::GetRecordQuorumFailed {
                    got_holders: record_results.len(),
                    expected_holders,
                }))
                .map_err(|_| TaskHandlerError::NetworkClientDropped)?;

            return Ok(());
        }

        let records: Vec<_> = record_results.into_values().collect();

        let res = match &records.as_slice() {
            [] => responder.send(Ok((None, Default::default()))),
            [one] => responder.send(Ok((Some(one.0.clone()), one.1.iter().cloned().collect()))),
            [_one, _two, ..] => {
                let mut map = HashMap::new();

                for (record, peers) in records {
                    for peer in peers {
                        // Insert into the map, cloning the record (if necessary)
                        map.insert(peer, record.clone());
                    }
                }

                responder.send(Err(NetworkError::SplitRecord(map)))
            }
        };

        res.map_err(|_| TaskHandlerError::NetworkClientDropped)?;

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
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
                Ok(())
            }
            Ok(None) => {
                trace!("OutboundRequestId({id}): no quote needed as record already exists at peer {peer_address:?}");
                // Send can fail here if we already accumulated enough quotes.
                resp.send(Ok(None))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
                Ok(())
            }
            Err(e) => {
                warn!("OutboundRequestId({id}): got invalid quote from peer {peer_address:?}: {e}");
                // Send can fail here if we already accumulated enough quotes.
                resp.send(Err(e))
                    .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
                Ok(())
            }
        }
    }

    pub fn terminate_get_quote(
        &mut self,
        id: OutboundRequestId,
        peer: PeerId,
        error: libp2p::autonat::OutboundFailure,
    ) -> Result<(), TaskHandlerError> {
        let (resp, _data_type, original_peer) =
            self.get_cost
                .remove(&id)
                .ok_or(TaskHandlerError::UnknownQuery(format!(
                    "OutboundRequestId {id:?}"
                )))?;

        trace!("OutboundRequestId({id}): initially sent to peer {original_peer:?} got fatal error from peer {peer:?}: {error:?}");
        resp.send(Err(NetworkError::GetQuoteError(error.to_string())))
            .map_err(|_| TaskHandlerError::NetworkClientDropped)?;
        Ok(())
    }

    /// Helper function to take the responder and holders from a get record task
    #[allow(clippy::type_complexity)]
    fn consume_get_record_task_and_results(
        &mut self,
        id: QueryId,
    ) -> Result<
        (
            (OneShotTaskResult<RecordAndHolders>, Quorum),
            HashMap<XorName, (Record, HashSet<PeerId>)>,
        ),
        TaskHandlerError,
    > {
        let (responder, quorum) = self
            .get_record
            .remove(&id)
            .ok_or(TaskHandlerError::UnknownQuery(format!("QueryId {id:?}")))?;
        let record_results = self.get_record_accumulator.remove(&id).unwrap_or_default();
        Ok(((responder, quorum), record_results))
    }

    /// Forcefully finish a kad query.
    pub async fn finish_kad_query(&mut self, query_id: QueryId) {
        self.network_driver_cmd_sender
            .send(Command::TerminateQuery(query_id))
            .await
            .expect("Failed to send network driver command");
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
