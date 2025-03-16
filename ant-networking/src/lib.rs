// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[macro_use]
extern crate tracing;

mod bootstrap;
mod circular_vec;
mod cmd;
mod config;
mod driver;
mod error;
mod event;
mod external_address;
mod fifo_register;
mod graph;
mod log_markers;
#[cfg(feature = "open-metrics")]
mod metrics;
mod network_discovery;
mod record_store;
mod record_store_api;
mod relay_manager;
mod replication_fetcher;
pub mod time;
mod transport;

use cmd::LocalSwarmCmd;
use xor_name::XorName;

// re-export arch dependent deps for use in the crate, or above
pub use self::{
    cmd::{NodeIssue, SwarmLocalState},
    config::{GetRecordCfg, PutRecordCfg, ResponseQuorum, RetryStrategy, VerificationKind},
    driver::{NetworkBuilder, SwarmDriver, MAX_PACKET_SIZE},
    error::{GetRecordError, NetworkError},
    event::{MsgResponder, NetworkEvent},
    graph::get_graph_entry_from_record,
    record_store::NodeRecordStore,
};
#[cfg(feature = "open-metrics")]
pub use metrics::service::MetricsRegistries;
pub use time::{interval, sleep, spawn, Instant, Interval};

use self::{cmd::NetworkSwarmCmd, error::Result};
use ant_evm::{PaymentQuote, QuotingMetrics};
use ant_protocol::{
    error::Error as ProtocolError,
    messages::{ChunkProof, Nonce, Query, QueryResponse, Request, Response},
    storage::{DataTypes, Pointer, Scratchpad, ValidationType},
    NetworkAddress, PrettyPrintKBucketKey, PrettyPrintRecordKey, CLOSE_GROUP_SIZE,
};
use futures::future::select_all;
use libp2p::{
    identity::Keypair,
    kad::{KBucketDistance, KBucketKey, Record, RecordKey},
    multiaddr::Protocol,
    request_response::OutboundFailure,
    Multiaddr, PeerId,
};
use rand::Rng;
use std::{
    collections::{BTreeMap, HashMap},
    net::IpAddr,
    sync::Arc,
};
use tokio::sync::{
    mpsc::{self, Sender},
    oneshot,
};
use tokio::time::Duration;
use {
    ant_protocol::storage::GraphEntry,
    ant_protocol::storage::{
        try_deserialize_record, try_serialize_record, RecordHeader, RecordKind,
    },
    std::collections::HashSet,
};

/// Majority of a given group (i.e. > 1/2).
#[inline]
pub const fn close_group_majority() -> usize {
    // Calculate the majority of the close group size by dividing it by 2 and adding 1.
    // This ensures that the majority is always greater than half.
    CLOSE_GROUP_SIZE / 2 + 1
}

/// Max duration to wait for verification.
const MAX_WAIT_BEFORE_READING_A_PUT: Duration = Duration::from_millis(750);
/// Min duration to wait for verification
const MIN_WAIT_BEFORE_READING_A_PUT: Duration = Duration::from_millis(300);

/// Sort the provided peers by their distance to the given `NetworkAddress`.
/// Return with the closest expected number of entries if has.
pub fn sort_peers_by_address(
    peers: Vec<(PeerId, Addresses)>,
    address: &NetworkAddress,
    expected_entries: usize,
) -> Result<Vec<(PeerId, Addresses)>> {
    sort_peers_by_key(peers, &address.as_kbucket_key(), expected_entries)
}

/// Sort the provided peers by their distance to the given `KBucketKey`.
/// Return with the closest expected number of entries if has.
pub fn sort_peers_by_key<T>(
    peers: Vec<(PeerId, Addresses)>,
    key: &KBucketKey<T>,
    expected_entries: usize,
) -> Result<Vec<(PeerId, Addresses)>> {
    // Check if there are enough peers to satisfy the request.
    // bail early if that's not the case
    if CLOSE_GROUP_SIZE > peers.len() {
        warn!("Not enough peers in the k-bucket to satisfy the request");
        return Err(NetworkError::NotEnoughPeers {
            found: peers.len(),
            required: CLOSE_GROUP_SIZE,
        });
    }

    // Create a vector of tuples where each tuple is a reference to a peer and its distance to the key.
    // This avoids multiple computations of the same distance in the sorting process.
    let mut peer_distances: Vec<(PeerId, Addresses, KBucketDistance)> =
        Vec::with_capacity(peers.len());

    for (peer_id, addrs) in peers.into_iter() {
        let addr = NetworkAddress::from_peer(peer_id);
        let distance = key.distance(&addr.as_kbucket_key());
        peer_distances.push((peer_id, addrs, distance));
    }

    // Sort the vector of tuples by the distance.
    peer_distances.sort_by(|a, b| a.2.cmp(&b.2));

    // Collect the sorted peers into a new vector.
    let sorted_peers: Vec<(PeerId, Addresses)> = peer_distances
        .into_iter()
        .take(expected_entries)
        .map(|(peer_id, addrs, _)| (peer_id, addrs))
        .collect();

    Ok(sorted_peers)
}

/// A list of addresses of a peer in the routing table.
#[derive(Clone, Debug, Default)]
pub struct Addresses(pub Vec<Multiaddr>);

#[derive(Clone, Debug)]
/// API to interact with the underlying Swarm
pub struct Network {
    inner: Arc<NetworkInner>,
}

/// The actual implementation of the Network. The other is just a wrapper around this, so that we don't expose
/// the Arc from the interface.
#[derive(Debug)]
struct NetworkInner {
    network_swarm_cmd_sender: mpsc::Sender<NetworkSwarmCmd>,
    local_swarm_cmd_sender: mpsc::Sender<LocalSwarmCmd>,
    peer_id: PeerId,
    keypair: Keypair,
}

impl Network {
    pub fn new(
        network_swarm_cmd_sender: mpsc::Sender<NetworkSwarmCmd>,
        local_swarm_cmd_sender: mpsc::Sender<LocalSwarmCmd>,
        peer_id: PeerId,
        keypair: Keypair,
    ) -> Self {
        Self {
            inner: Arc::new(NetworkInner {
                network_swarm_cmd_sender,
                local_swarm_cmd_sender,
                peer_id,
                keypair,
            }),
        }
    }

    /// Returns the `PeerId` of the instance.
    pub fn peer_id(&self) -> PeerId {
        self.inner.peer_id
    }

    /// Returns the `Keypair` of the instance.
    pub fn keypair(&self) -> &Keypair {
        &self.inner.keypair
    }

    /// Get the sender to send a `NetworkSwarmCmd` to the underlying `Swarm`.
    pub(crate) fn network_swarm_cmd_sender(&self) -> &mpsc::Sender<NetworkSwarmCmd> {
        &self.inner.network_swarm_cmd_sender
    }
    /// Get the sender to send a `LocalSwarmCmd` to the underlying `Swarm`.
    pub(crate) fn local_swarm_cmd_sender(&self) -> &mpsc::Sender<LocalSwarmCmd> {
        &self.inner.local_swarm_cmd_sender
    }

    /// Signs the given data with the node's keypair.
    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>> {
        self.keypair().sign(msg).map_err(NetworkError::from)
    }

    /// Verifies a signature for the given data and the node's public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        self.keypair().public().verify(msg, sig)
    }

    /// Returns the protobuf serialised PublicKey to allow messaging out for share.
    pub fn get_pub_key(&self) -> Vec<u8> {
        self.keypair().public().encode_protobuf()
    }

    /// Returns the closest peers to the given `XorName`, sorted by their distance to the xor_name.
    /// Excludes the client's `PeerId` while calculating the closest peers.
    pub async fn client_get_all_close_peers_in_range_or_close_group(
        &self,
        key: &NetworkAddress,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        self.get_all_close_peers_in_range_or_close_group(key, true)
            .await
    }

    /// Returns the closest peers to the given `NetworkAddress`, sorted by their distance to the key.
    ///
    /// Includes our node's `PeerId` while calculating the closest peers.
    pub async fn node_get_closest_peers(
        &self,
        key: &NetworkAddress,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        self.get_all_close_peers_in_range_or_close_group(key, false)
            .await
    }

    /// Returns a list of peers in local RT and their correspondent Multiaddr.
    /// Does not include self
    pub async fn get_local_peers_with_multiaddr(&self) -> Result<Vec<(PeerId, Vec<Multiaddr>)>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetPeersWithMultiaddr { sender });
        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Returns a map where each key is the ilog2 distance of that Kbucket
    /// and each value is a vector of peers in that bucket.
    /// Does not include self
    pub async fn get_kbuckets(&self) -> Result<BTreeMap<u32, Vec<PeerId>>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetKBuckets { sender });
        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Returns all the PeerId from all the KBuckets from our local Routing Table
    /// Also contains our own PeerId.
    pub async fn get_closest_k_value_local_peers(&self) -> Result<Vec<(PeerId, Addresses)>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetClosestKLocalPeers { sender });

        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Returns X close peers to the target.
    /// Note: self is not included
    pub async fn get_close_peers_to_the_target(
        &self,
        key: NetworkAddress,
        num_of_peers: usize,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetCloseLocalPeersToTarget {
            key,
            num_of_peers,
            sender,
        });

        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Get the Chunk existence proof from the close nodes to the provided chunk address.
    /// This is to be used by client only to verify the success of the upload.
    pub async fn verify_chunk_existence(
        &self,
        chunk_address: NetworkAddress,
        mut close_nodes: Vec<(PeerId, Addresses)>,
        nonce: Nonce,
        expected_proof: ChunkProof,
        quorum: ResponseQuorum,
        _retry_strategy: RetryStrategy,
    ) -> Result<()> {
        // The above calling place shall already carried out same `re-attempts`.
        // Hence here just use a fixed number.
        let total_attempts = 2;

        let pretty_key = PrettyPrintRecordKey::from(&chunk_address.to_record_key()).into_owned();
        let expected_n_verified = quorum.get_value();

        let request = Request::Query(Query::GetChunkExistenceProof {
            key: chunk_address.clone(),
            nonce,
            difficulty: 1,
        });

        if close_nodes.is_empty() {
            close_nodes = self
                .client_get_all_close_peers_in_range_or_close_group(&chunk_address)
                .await?;
        }

        let mut retry_attempts = 0;
        while retry_attempts < total_attempts {
            retry_attempts += 1;
            info!(
                "Getting ChunkProof for {pretty_key:?}. Attempts: {retry_attempts:?}/{total_attempts:?}",
            );

            let responses = self
                .send_and_get_responses(&close_nodes, &request, true)
                .await;
            let n_verified = responses
                .into_iter()
                .filter_map(|(peer, resp)| {
                    if let Ok(Response::Query(QueryResponse::GetChunkExistenceProof(proofs))) =
                        resp
                    {
                        if proofs.is_empty() {
                            warn!("Failed to verify the ChunkProof from {peer:?}. Returned proof is empty.");
                            None
                        } else if let Ok(ref proof) = proofs[0].1 {
                            if expected_proof.verify(proof) {
                                debug!("Got a valid ChunkProof from {peer:?}");
                                Some(())
                            } else {
                                warn!("Failed to verify the ChunkProof from {peer:?}. The chunk might have been tampered?");
                                None
                            }
                        } else {
                            warn!("Failed to verify the ChunkProof from {peer:?}, returned with error {:?}", proofs[0].1);
                            None
                        }
                    } else {
                        debug!("Did not get a valid response for the ChunkProof from {peer:?}");
                        None
                    }
                })
                .count();
            debug!("Got {n_verified} verified chunk existence proofs for chunk_address {chunk_address:?}");

            if n_verified >= expected_n_verified {
                return Ok(());
            }
            warn!("The obtained {n_verified} verified proofs did not match the expected {expected_n_verified} verified proofs");
            // Sleep to avoid firing queries too close to even choke the nodes further.
            let waiting_time = if retry_attempts == 1 {
                MIN_WAIT_BEFORE_READING_A_PUT
            } else {
                MIN_WAIT_BEFORE_READING_A_PUT + MIN_WAIT_BEFORE_READING_A_PUT
            };
            sleep(waiting_time).await;
        }

        Err(NetworkError::FailedToVerifyChunkProof(
            chunk_address.clone(),
        ))
    }

    /// Get the store costs from the majority of the closest peers to the provided RecordKey.
    /// Record already exists will have a cost of zero to be returned.
    ///
    /// Ignore the quote from any peers from `ignore_peers`.
    /// This is useful if we want to repay a different PeerId on failure.
    pub async fn get_store_quote_from_network(
        &self,
        record_address: NetworkAddress,
        data_type: u32,
        data_size: usize,
        ignore_peers: Vec<PeerId>,
    ) -> Result<Vec<(PeerId, Addresses, PaymentQuote)>> {
        // The requirement of having at least CLOSE_GROUP_SIZE
        // close nodes will be checked internally automatically.
        let mut close_nodes = self
            .client_get_all_close_peers_in_range_or_close_group(&record_address)
            .await?;
        // Filter out results from the ignored peers.
        close_nodes.retain(|(peer_id, _)| !ignore_peers.contains(peer_id));
        info!(
            "For record {record_address:?} quoting {} nodes. ignore_peers is {ignore_peers:?}",
            close_nodes.len()
        );

        if close_nodes.is_empty() {
            error!("Can't get store_cost of {record_address:?}, as all close_nodes are ignored");
            return Err(NetworkError::NotEnoughPeersForStoreCostRequest);
        }

        // Client shall decide whether to carry out storage verification or not.
        let request = Request::Query(Query::GetStoreQuote {
            key: record_address.clone(),
            data_type,
            data_size,
            nonce: None,
            difficulty: 0,
        });
        let responses = self
            .send_and_get_responses(&close_nodes, &request, true)
            .await;

        // consider data to be already paid for if 1/2 of the close nodes already have it
        let mut peer_already_have_it = 0;
        let enough_peers_already_have_it = close_nodes.len() / 2;

        let mut peers_returned_error = 0;

        // loop over responses
        let mut quotes_to_pay = vec![];
        for (peer, response) in responses {
            info!("StoreCostReq for {record_address:?} received response: {response:?}");
            match response {
                Ok(Response::Query(QueryResponse::GetStoreQuote {
                    quote: Ok(quote),
                    peer_address,
                    storage_proofs,
                })) => {
                    if !storage_proofs.is_empty() {
                        debug!("Storage proofing during GetStoreQuote to be implemented.");
                    }

                    // Check the quote itself is valid.
                    if !quote.check_is_signed_by_claimed_peer(peer) {
                        warn!("Received invalid quote from {peer_address:?}, {quote:?}");
                        continue;
                    }

                    // Check if the returned data type matches the request
                    if quote.quoting_metrics.data_type != data_type {
                        warn!("Received invalid quote from {peer_address:?}, {quote:?}. Data type did not match the request.");
                        continue;
                    }

                    let Some((_peer_id, addrs)) = close_nodes
                        .iter()
                        .find(|(peer_id, _addrs)| *peer_id == peer)
                    else {
                        warn!("Can't find the addresses of the peer {peer:?}");
                        continue;
                    };

                    quotes_to_pay.push((peer, addrs.clone(), quote));
                }
                Ok(Response::Query(QueryResponse::GetStoreQuote {
                    quote: Err(ProtocolError::RecordExists(_)),
                    peer_address,
                    storage_proofs,
                })) => {
                    if !storage_proofs.is_empty() {
                        debug!("Storage proofing during GetStoreQuote to be implemented.");
                    }
                    peer_already_have_it += 1;
                    info!("Address {record_address:?} was already paid for according to {peer_address:?} ({peer_already_have_it}/{enough_peers_already_have_it})");
                    if peer_already_have_it >= enough_peers_already_have_it {
                        info!("Address {record_address:?} was already paid for according to {peer_already_have_it} peers, ending quote request");
                        return Ok(vec![]);
                    }
                }
                Err(err) => {
                    error!("Got an error while requesting quote from peer {peer:?}: {err:?}");
                    peers_returned_error += 1;
                }
                _ => {
                    error!("Got an unexpected response while requesting quote from peer {peer:?}: {response:?}");
                    peers_returned_error += 1;
                }
            }
        }

        if quotes_to_pay.is_empty() {
            error!(
                "Could not fetch any quotes. {} peers returned an error.",
                peers_returned_error
            );
            return Err(NetworkError::NoStoreCostResponses);
        }

        Ok(quotes_to_pay)
    }

    /// Get the Record from the network
    /// Carry out re-attempts if required
    /// In case a target_record is provided, only return when fetched target.
    /// Otherwise count it as a failure when all attempts completed.
    ///
    /// It also handles the split record error for GraphEntry.
    pub async fn get_record_from_network(
        &self,
        key: RecordKey,
        cfg: &GetRecordCfg,
    ) -> Result<Record> {
        let pretty_key = PrettyPrintRecordKey::from(&key);
        let mut backoff = cfg.retry_strategy.backoff().into_iter();

        loop {
            info!("Getting record from network of {pretty_key:?}. with cfg {cfg:?}",);
            let (sender, receiver) = oneshot::channel();
            self.send_network_swarm_cmd(NetworkSwarmCmd::GetNetworkRecord {
                key: key.clone(),
                sender,
                cfg: cfg.clone(),
            });
            let result = match receiver.await {
                Ok(result) => result,
                Err(err) => {
                    error!(
                        "When fetching record {pretty_key:?}, encountered a channel error {err:?}"
                    );
                    // Do not attempt retries.
                    return Err(NetworkError::InternalMsgChannelDropped);
                }
            };

            let err = match result {
                Ok(record) => {
                    info!("Record returned: {pretty_key:?}.");
                    return Ok(record);
                }
                Err(err) => err,
            };

            // log the results
            match &err {
                GetRecordError::RecordDoesNotMatch(_) => {
                    warn!("The returned record does not match target {pretty_key:?}.");
                }
                GetRecordError::NotEnoughCopies { expected, got, .. } => {
                    warn!("Not enough copies ({got}/{expected}) found yet for {pretty_key:?}.");
                }
                // libp2p RecordNotFound does mean no holders answered.
                // it does not actually mean the record does not exist.
                // just that those asked did not have it
                GetRecordError::RecordNotFound => {
                    warn!("No holder of record '{pretty_key:?}' found.");
                }
                // This is returned during SplitRecordError, we should not get this error here.
                GetRecordError::RecordKindMismatch => {
                    error!("Record kind mismatch for {pretty_key:?}. This error should not happen here.");
                }
                GetRecordError::SplitRecord { result_map } => {
                    error!("Encountered a split record for {pretty_key:?}.");
                    if let Some(record) = Self::handle_split_record_error(result_map, &key)? {
                        info!("Merged the split record for {pretty_key:?}, into a single record");
                        return Ok(record);
                    }
                }
                GetRecordError::QueryTimeout => {
                    error!("Encountered query timeout for {pretty_key:?}.");
                }
            }

            match backoff.next() {
                Some(Some(duration)) => {
                    crate::time::sleep(duration).await;
                    debug!("Getting record from network of {pretty_key:?} via backoff...");
                }
                _ => break Err(err.into()),
            }
        }
    }

    /// Handle the split record error.
    fn handle_split_record_error(
        result_map: &HashMap<XorName, (Record, HashSet<PeerId>)>,
        key: &RecordKey,
    ) -> std::result::Result<Option<Record>, NetworkError> {
        let pretty_key = PrettyPrintRecordKey::from(key);

        // attempt to deserialise and accumulate all GraphEntries
        let results_count = result_map.len();
        let mut accumulated_graphentries = HashSet::new();
        let mut valid_scratchpad: Option<Scratchpad> = None;
        let mut valid_pointer: Option<Pointer> = None;

        if results_count > 1 {
            let mut record_kind = None;
            info!("For record {pretty_key:?}, we have more than one result returned.");
            for (record, _) in result_map.values() {
                let Ok(header) = RecordHeader::from_record(record) else {
                    continue;
                };
                let kind = record_kind.get_or_insert(header.kind);
                // FIXME: the first record dictates the kind, but we should check all records are of the same kind.
                // And somehow discard the incorrect ones.
                if *kind != header.kind {
                    error!("Encountered a split record for {pretty_key:?} with different RecordHeaders. Expected {kind:?} but got {:?}. Skipping",header.kind);
                    continue;
                }

                match kind {
                    RecordKind::DataOnly(DataTypes::Chunk) | RecordKind::DataWithPayment(_) => {
                        error!("Encountered a split record for {pretty_key:?} with unexpected RecordKind {kind:?}, skipping.");
                        continue;
                    }
                    RecordKind::DataOnly(DataTypes::GraphEntry) => {
                        match get_graph_entry_from_record(record) {
                            Ok(graphentries) => {
                                accumulated_graphentries.extend(graphentries);
                                info!("For record {pretty_key:?}, we have a split record for a GraphEntry. Accumulating GraphEntry: {}", accumulated_graphentries.len());
                            }
                            Err(_) => {
                                warn!("Failed to deserialize GraphEntry for {pretty_key:?}, skipping accumulation");
                                continue;
                            }
                        }
                    }
                    RecordKind::DataOnly(DataTypes::Pointer) => {
                        info!("For record {pretty_key:?}, we have a split record for a pointer. Selecting the one with the highest count");
                        let Ok(pointer) = try_deserialize_record::<Pointer>(record) else {
                            error!(
                                "Failed to deserialize pointer {pretty_key}. Skipping accumulation"
                            );
                            continue;
                        };

                        if !pointer.verify_signature() {
                            warn!("Rejecting Pointer for {pretty_key} PUT with invalid signature");
                            continue;
                        }

                        if let Some(old) = &valid_pointer {
                            if old.counter() >= pointer.counter() {
                                info!("Rejecting Pointer for {pretty_key} with lower count than the previous one");
                                continue;
                            }
                        }
                        valid_pointer = Some(pointer);
                    }
                    RecordKind::DataOnly(DataTypes::Scratchpad) => {
                        info!("For record {pretty_key:?}, we have a split record for a scratchpad. Selecting the one with the highest count");
                        let Ok(scratchpad) = try_deserialize_record::<Scratchpad>(record) else {
                            error!(
                                "Failed to deserialize scratchpad {pretty_key}. Skipping accumulation"
                            );
                            continue;
                        };

                        if !scratchpad.verify_signature() {
                            warn!(
                                "Rejecting Scratchpad for {pretty_key} PUT with invalid signature"
                            );
                            continue;
                        }

                        if let Some(old) = &valid_scratchpad {
                            if old.counter() >= scratchpad.counter() {
                                info!("Rejecting Scratchpad for {pretty_key} with lower count than the previous one");
                                continue;
                            }
                        }
                        valid_scratchpad = Some(scratchpad);
                    }
                }
            }
        }

        // Return the accumulated GraphEntries as a single record
        if accumulated_graphentries.len() > 1 {
            info!("For record {pretty_key:?} task found split record for a GraphEntry, accumulated and sending them as a single record");
            let accumulated_graphentries = accumulated_graphentries
                .into_iter()
                .collect::<Vec<GraphEntry>>();
            let record = Record {
                key: key.clone(),
                value: try_serialize_record(&accumulated_graphentries, RecordKind::DataOnly(DataTypes::GraphEntry))
                    .map_err(|err| {
                        error!(
                            "Error while serializing the accumulated GraphEntries for {pretty_key:?}: {err:?}"
                        );
                        NetworkError::from(err)
                    })?
                    .to_vec(),
                publisher: None,
                expires: None,
            };
            return Ok(Some(record));
        } else if let Some(pointer) = valid_pointer {
            info!("For record {pretty_key:?} task found a valid pointer, returning it.");
            let record_value =
                try_serialize_record(&pointer, RecordKind::DataOnly(DataTypes::Pointer))
                    .map_err(|err| {
                        error!("Error while serializing the pointer for {pretty_key:?}: {err:?}");
                        NetworkError::from(err)
                    })?
                    .to_vec();

            let record = Record {
                key: key.clone(),
                value: record_value,
                publisher: None,
                expires: None,
            };
            return Ok(Some(record));
        } else if let Some(scratchpad) = valid_scratchpad {
            info!("For record {pretty_key:?} task found a valid scratchpad, returning it.");
            let record_value =
                try_serialize_record(&scratchpad, RecordKind::DataOnly(DataTypes::Scratchpad))
                    .map_err(|err| {
                        error!(
                            "Error while serializing the scratchpad for {pretty_key:?}: {err:?}"
                        );
                        NetworkError::from(err)
                    })?
                    .to_vec();

            let record = Record {
                key: key.clone(),
                value: record_value,
                publisher: None,
                expires: None,
            };
            return Ok(Some(record));
        }
        Ok(None)
    }

    /// Get the quoting metrics for storing the next record from the network
    pub async fn get_local_quoting_metrics(
        &self,
        key: RecordKey,
        data_type: u32,
        data_size: usize,
    ) -> Result<(QuotingMetrics, bool)> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetLocalQuotingMetrics {
            key,
            data_type,
            data_size,
            sender,
        });

        let quoting_metrics = receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)??;
        Ok(quoting_metrics)
    }

    /// Notify the node receicced a payment.
    pub fn notify_payment_received(&self) {
        self.send_local_swarm_cmd(LocalSwarmCmd::PaymentReceived);
    }

    /// Get `Record` from the local RecordStore
    pub async fn get_local_record(&self, key: &RecordKey) -> Result<Option<Record>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetLocalRecord {
            key: key.clone(),
            sender,
        });

        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Whether the target peer is considered blacklisted by self
    pub async fn is_peer_shunned(&self, target: NetworkAddress) -> Result<bool> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::IsPeerShunned { target, sender });

        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Put `Record` to network
    /// Optionally verify the record is stored after putting it to network
    /// If verify is on, we retry.
    pub async fn put_record(&self, record: Record, cfg: &PutRecordCfg) -> Result<()> {
        let pretty_key = PrettyPrintRecordKey::from(&record.key);
        let mut backoff = cfg.retry_strategy.backoff().into_iter();

        loop {
            info!(
                "Attempting to PUT record with key: {pretty_key:?} to network, with cfg {cfg:?}, retrying via backoff..."
            );

            let err = match self.put_record_once(record.clone(), cfg).await {
                Ok(_) => break Ok(()),
                Err(err) => err,
            };

            // FIXME: Skip if we get a permanent error during verification
            warn!("Failed to PUT record with key: {pretty_key:?} to network (retry via backoff) with error: {err:?}");

            match backoff.next() {
                Some(Some(duration)) => {
                    crate::time::sleep(duration).await;
                }
                _ => break Err(err),
            }
        }
    }

    async fn put_record_once(&self, record: Record, cfg: &PutRecordCfg) -> Result<()> {
        let record_key = record.key.clone();
        let pretty_key = PrettyPrintRecordKey::from(&record_key);
        info!(
            "Putting record of {} - length {:?} to network",
            pretty_key,
            record.value.len()
        );

        // Waiting for a response to avoid flushing to network too quick that causing choke
        let (sender, receiver) = oneshot::channel();
        let close_nodes = if let Some(put_record_to_peers) = &cfg.use_put_record_to {
            if put_record_to_peers.len() >= 3 {
                self.send_network_swarm_cmd(NetworkSwarmCmd::PutRecord {
                    record: record.clone(),
                    sender,
                    quorum: cfg.put_quorum,
                });
            } else {
                let peers = put_record_to_peers
                    .iter()
                    .map(|(peer_id, _addrs)| *peer_id)
                    .collect();
                self.send_network_swarm_cmd(NetworkSwarmCmd::PutRecordTo {
                    peers,
                    record: record.clone(),
                    sender,
                    quorum: cfg.put_quorum,
                });
            }
            put_record_to_peers.clone()
        } else {
            self.send_network_swarm_cmd(NetworkSwarmCmd::PutRecord {
                record: record.clone(),
                sender,
                quorum: cfg.put_quorum,
            });
            vec![]
        };

        let response = receiver.await?;

        if let Some((verification_kind, get_cfg)) = &cfg.verification {
            // Generate a random duration between MAX_WAIT_BEFORE_READING_A_PUT and MIN_WAIT_BEFORE_READING_A_PUT
            let wait_duration = rand::thread_rng()
                .gen_range(MIN_WAIT_BEFORE_READING_A_PUT..MAX_WAIT_BEFORE_READING_A_PUT);
            // Small wait before we attempt to verify.
            // There will be `re-attempts` to be carried out within the later step anyway.
            sleep(wait_duration).await;
            debug!("Attempting to verify {pretty_key:?} after we've slept for {wait_duration:?}");

            // Verify the record is stored, requiring re-attempts
            if let VerificationKind::ChunkProof {
                expected_proof,
                nonce,
            } = verification_kind
            {
                self.verify_chunk_existence(
                    NetworkAddress::from_record_key(&record_key),
                    close_nodes,
                    *nonce,
                    expected_proof.clone(),
                    get_cfg.get_quorum,
                    get_cfg.retry_strategy,
                )
                .await?;
            } else {
                match self
                    .get_record_from_network(record.key.clone(), get_cfg)
                    .await
                {
                    Ok(_) => {
                        debug!("Record {pretty_key:?} verified to be stored.");
                    }
                    Err(NetworkError::GetRecordError(GetRecordError::RecordNotFound)) => {
                        warn!("Record {pretty_key:?} not found after PUT, either rejected or not yet stored by nodes when we asked");
                        return Err(NetworkError::RecordNotStoredByNodes(
                            NetworkAddress::from_record_key(&record_key),
                        ));
                    }
                    Err(NetworkError::GetRecordError(GetRecordError::SplitRecord { .. }))
                        if matches!(verification_kind, VerificationKind::Crdt) =>
                    {
                        warn!("Record {pretty_key:?} is split, which is okay since we're dealing with CRDTs");
                    }
                    Err(e) => {
                        debug!(
                            "Failed to verify record {pretty_key:?} to be stored with error: {e:?}"
                        );
                        return Err(e);
                    }
                }
            }
        }
        response
    }

    /// Notify ReplicationFetch a fetch attempt is completed.
    /// (but it won't trigger any real writes to disk)
    pub fn notify_fetch_completed(&self, key: RecordKey, record_type: ValidationType) {
        self.send_local_swarm_cmd(LocalSwarmCmd::FetchCompleted((key, record_type)))
    }

    /// Put `Record` to the local RecordStore
    /// Must be called after the validations are performed on the Record
    pub fn put_local_record(&self, record: Record, is_client_put: bool) {
        debug!(
            "Writing Record locally, for {:?} - length {:?}",
            PrettyPrintRecordKey::from(&record.key),
            record.value.len()
        );
        self.send_local_swarm_cmd(LocalSwarmCmd::PutLocalRecord {
            record,
            is_client_put,
        })
    }

    /// Returns true if a RecordKey is present locally in the RecordStore
    pub async fn is_record_key_present_locally(&self, key: &RecordKey) -> Result<bool> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::RecordStoreHasKey {
            key: key.clone(),
            sender,
        });

        let is_present = receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)??;

        Ok(is_present)
    }

    /// Returns the Addresses of all the locally stored Records
    pub async fn get_all_local_record_addresses(
        &self,
    ) -> Result<HashMap<NetworkAddress, ValidationType>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetAllLocalRecordAddresses { sender });

        let addrs = receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)??;
        Ok(addrs)
    }

    /// Send `Request` to the given `PeerId` and await for the response. If `self` is the recipient,
    /// then the `Request` is forwarded to itself and handled, and a corresponding `Response` is created
    /// and returned to itself. Hence the flow remains the same and there is no branching at the upper
    /// layers.
    ///
    /// If an outbound issue is raised, we retry once more to send the request before returning an error.
    pub async fn send_request(
        &self,
        req: Request,
        peer: PeerId,
        addrs: Addresses,
    ) -> Result<Response> {
        let (sender, receiver) = oneshot::channel();
        let req_str = format!("{req:?}");
        // try to send the request without dialing the peer
        self.send_network_swarm_cmd(NetworkSwarmCmd::SendRequest {
            req: req.clone(),
            peer,
            addrs: None,
            sender: Some(sender),
        });
        let mut r = receiver.await?;

        if let Err(error) = &r {
            error!("Error in response: {:?}", error);

            match error {
                NetworkError::OutboundError(OutboundFailure::Io(_))
                | NetworkError::OutboundError(OutboundFailure::ConnectionClosed)
                | NetworkError::OutboundError(OutboundFailure::DialFailure) => {
                    warn!(
                        "Outbound failed for {req_str} .. {error:?}, dialing it then re-attempt."
                    );

                    self.send_network_swarm_cmd(NetworkSwarmCmd::DialPeer {
                        peer,
                        addrs: addrs.clone(),
                    });

                    // Short wait to allow connection re-established.
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                    let (sender, receiver) = oneshot::channel();
                    debug!("Reattempting to send_request {req_str} to {peer:?} by dialing the addrs manually.");
                    self.send_network_swarm_cmd(NetworkSwarmCmd::SendRequest {
                        req,
                        peer,
                        addrs: Some(addrs),
                        sender: Some(sender),
                    });

                    r = receiver.await?;
                    if let Err(error) = &r {
                        error!("Reattempt of {req_str} led to an error again (even after dialing). {error:?}");
                    }
                }
                _ => {
                    // If the record is found, we should log the error and continue
                    warn!("Error in response for {req_str}: {error:?}",);
                }
            }
        }

        r
    }

    /// Send a `Response` through the channel opened by the requester.
    pub fn send_response(&self, resp: Response, channel: MsgResponder) {
        self.send_network_swarm_cmd(NetworkSwarmCmd::SendResponse { resp, channel })
    }

    /// Return a `SwarmLocalState` with some information obtained from swarm's local state.
    pub async fn get_swarm_local_state(&self) -> Result<SwarmLocalState> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetSwarmLocalState(sender));
        let state = receiver.await?;
        Ok(state)
    }

    pub fn trigger_interval_replication(&self) {
        self.send_local_swarm_cmd(LocalSwarmCmd::TriggerIntervalReplication)
    }

    pub fn add_fresh_records_to_the_replication_fetcher(
        &self,
        holder: NetworkAddress,
        keys: Vec<(NetworkAddress, ValidationType)>,
    ) {
        self.send_local_swarm_cmd(LocalSwarmCmd::AddFreshReplicateRecords { holder, keys })
    }

    pub fn record_node_issues(&self, peer_id: PeerId, issue: NodeIssue) {
        self.send_local_swarm_cmd(LocalSwarmCmd::RecordNodeIssue { peer_id, issue });
    }

    pub fn historical_verify_quotes(&self, quotes: Vec<(PeerId, PaymentQuote)>) {
        self.send_local_swarm_cmd(LocalSwarmCmd::QuoteVerification { quotes });
    }

    pub fn trigger_irrelevant_record_cleanup(&self) {
        self.send_local_swarm_cmd(LocalSwarmCmd::TriggerIrrelevantRecordCleanup)
    }

    pub fn add_network_density_sample(&self, distance: KBucketDistance) {
        self.send_local_swarm_cmd(LocalSwarmCmd::AddNetworkDensitySample { distance })
    }

    pub fn notify_peer_scores(&self, peer_scores: Vec<(PeerId, bool)>) {
        self.send_local_swarm_cmd(LocalSwarmCmd::NotifyPeerScores { peer_scores })
    }

    pub fn notify_node_version(&self, peer: PeerId, version: String) {
        self.send_local_swarm_cmd(LocalSwarmCmd::NotifyPeerVersion { peer, version })
    }

    /// Helper to send NetworkSwarmCmd
    fn send_network_swarm_cmd(&self, cmd: NetworkSwarmCmd) {
        send_network_swarm_cmd(self.network_swarm_cmd_sender().clone(), cmd);
    }

    /// Helper to send LocalSwarmCmd
    fn send_local_swarm_cmd(&self, cmd: LocalSwarmCmd) {
        send_local_swarm_cmd(self.local_swarm_cmd_sender().clone(), cmd);
    }

    /// Returns the closest peers to the given `XorName`, sorted by their distance to the xor_name.
    /// If `client` is false, then include `self` among the `closest_peers`
    ///
    /// If less than CLOSE_GROUP_SIZE peers are found, it will return all the peers.
    pub async fn get_all_close_peers_in_range_or_close_group(
        &self,
        key: &NetworkAddress,
        client: bool,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        let pretty_key = PrettyPrintKBucketKey(key.as_kbucket_key());
        debug!("Getting the all closest peers in range of {pretty_key:?}");
        let (sender, receiver) = oneshot::channel();
        self.send_network_swarm_cmd(NetworkSwarmCmd::GetClosestPeersToAddressFromNetwork {
            key: key.clone(),
            sender,
        });

        let found_peers = receiver.await?;

        // Count self in if among the CLOSE_GROUP_SIZE closest and sort the result
        let result_len = found_peers.len();
        let mut closest_peers = found_peers;

        // ensure we're not including self here
        if client {
            // remove our peer id from the calculations here:
            closest_peers.retain(|&(x, _)| x != self.peer_id());
            if result_len != closest_peers.len() {
                info!("Remove self client from the closest_peers");
            }
        }

        if tracing::level_enabled!(tracing::Level::DEBUG) {
            let close_peers_pretty_print: Vec<_> = closest_peers
                .iter()
                .map(|(peer_id, _)| {
                    format!(
                        "{peer_id:?}({:?})",
                        PrettyPrintKBucketKey(NetworkAddress::from_peer(*peer_id).as_kbucket_key())
                    )
                })
                .collect();

            debug!(
                "Network knowledge of closest peers to {pretty_key:?} are: {close_peers_pretty_print:?}"
            );
        }

        let expanded_close_group = CLOSE_GROUP_SIZE + CLOSE_GROUP_SIZE / 2;
        let closest_peers = sort_peers_by_address(closest_peers, key, expanded_close_group)?;
        Ok(closest_peers)
    }

    /// Send a `Request` to the provided set of peers and wait for their responses concurrently.
    /// If `get_all_responses` is true, we wait for the responses from all the peers.
    /// NB TODO: Will return an error if the request timeouts.
    /// If `get_all_responses` is false, we return the first successful response that we get
    pub async fn send_and_get_responses(
        &self,
        peers: &[(PeerId, Addresses)],
        req: &Request,
        get_all_responses: bool,
    ) -> BTreeMap<PeerId, Result<Response>> {
        debug!("send_and_get_responses for {req:?}");
        let mut list_of_futures = peers
            .iter()
            .map(|(peer, addrs)| {
                Box::pin(async {
                    let resp = self.send_request(req.clone(), *peer, addrs.clone()).await;
                    (*peer, resp)
                })
            })
            .collect::<Vec<_>>();

        let mut responses = BTreeMap::new();
        while !list_of_futures.is_empty() {
            let ((peer, resp), _, remaining_futures) = select_all(list_of_futures).await;
            let resp_string = match &resp {
                Ok(resp) => format!("{resp}"),
                Err(err) => format!("{err:?}"),
            };
            debug!("Got response from {peer:?} for the req: {req:?}, resp: {resp_string}");
            if !get_all_responses && resp.is_ok() {
                return BTreeMap::from([(peer, resp)]);
            }
            responses.insert(peer, resp);
            list_of_futures = remaining_futures;
        }

        debug!("Received all responses for {req:?}");
        responses
    }
}

/// Verifies if `Multiaddr` contains IPv4 address that is not global.
/// This is used to filter out unroutable addresses from the Kademlia routing table.
pub fn multiaddr_is_global(multiaddr: &Multiaddr) -> bool {
    !multiaddr.iter().any(|addr| match addr {
        Protocol::Ip4(ip) => {
            // Based on the nightly `is_global` method (`Ipv4Addrs::is_global`), only using what is available in stable.
            // Missing `is_shared`, `is_benchmarking` and `is_reserved`.
            ip.is_unspecified()
                | ip.is_private()
                | ip.is_loopback()
                | ip.is_link_local()
                | ip.is_documentation()
                | ip.is_broadcast()
        }
        _ => false,
    })
}

/// Pop off the `/p2p/<peer_id>`. This mutates the `Multiaddr` and returns the `PeerId` if it exists.
pub(crate) fn multiaddr_pop_p2p(multiaddr: &mut Multiaddr) -> Option<PeerId> {
    if let Some(Protocol::P2p(peer_id)) = multiaddr.iter().last() {
        // Only actually strip the last protocol if it's indeed the peer ID.
        let _ = multiaddr.pop();
        Some(peer_id)
    } else {
        None
    }
}

/// Return the last `PeerId` from the `Multiaddr` if it exists.
pub(crate) fn multiaddr_get_p2p(multiaddr: &Multiaddr) -> Option<PeerId> {
    if let Some(Protocol::P2p(peer_id)) = multiaddr.iter().last() {
        Some(peer_id)
    } else {
        None
    }
}

/// Build a `Multiaddr` with the p2p protocol filtered out.
/// If it is a relayed address, then the relay's P2P address is preserved.
pub(crate) fn multiaddr_strip_p2p(multiaddr: &Multiaddr) -> Multiaddr {
    let is_relayed = multiaddr.iter().any(|p| matches!(p, Protocol::P2pCircuit));

    if is_relayed {
        // Do not add any PeerId after we've found the P2PCircuit protocol. The prior one is the relay's PeerId which
        // we should preserve.
        let mut before_relay_protocol = true;
        let mut new_multi_addr = Multiaddr::empty();
        for p in multiaddr.iter() {
            if matches!(p, Protocol::P2pCircuit) {
                before_relay_protocol = false;
            }
            if matches!(p, Protocol::P2p(_)) && !before_relay_protocol {
                continue;
            }
            new_multi_addr.push(p);
        }
        new_multi_addr
    } else {
        multiaddr
            .iter()
            .filter(|p| !matches!(p, Protocol::P2p(_)))
            .collect()
    }
}

/// Get the `IpAddr` from the `Multiaddr`
pub(crate) fn multiaddr_get_ip(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|p| match p {
        Protocol::Ip4(addr) => Some(IpAddr::V4(addr)),
        Protocol::Ip6(addr) => Some(IpAddr::V6(addr)),
        _ => None,
    })
}

pub(crate) fn multiaddr_get_port(addr: &Multiaddr) -> Option<u16> {
    addr.iter().find_map(|p| match p {
        Protocol::Udp(port) => Some(port),
        _ => None,
    })
}

pub(crate) fn send_local_swarm_cmd(swarm_cmd_sender: Sender<LocalSwarmCmd>, cmd: LocalSwarmCmd) {
    let capacity = swarm_cmd_sender.capacity();

    if capacity == 0 {
        error!(
            "SwarmCmd channel is full. Await capacity to send: {:?}",
            cmd
        );
    }

    // Spawn a task to send the SwarmCmd and keep this fn sync
    let _handle = spawn(async move {
        if let Err(error) = swarm_cmd_sender.send(cmd).await {
            error!("Failed to send SwarmCmd: {}", error);
        }
    });
}

pub(crate) fn send_network_swarm_cmd(
    swarm_cmd_sender: Sender<NetworkSwarmCmd>,
    cmd: NetworkSwarmCmd,
) {
    let capacity = swarm_cmd_sender.capacity();

    if capacity == 0 {
        error!(
            "SwarmCmd channel is full. Await capacity to send: {:?}",
            cmd
        );
    }

    // Spawn a task to send the SwarmCmd and keep this fn sync
    let _handle = spawn(async move {
        if let Err(error) = swarm_cmd_sender.send(cmd).await {
            error!("Failed to send SwarmCmd: {}", error);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_sign_verify() -> eyre::Result<()> {
        let (network, _, _) =
            NetworkBuilder::new(Keypair::generate_ed25519(), false, vec![]).build_client();
        let msg = b"test message";
        let sig = network.sign(msg)?;
        assert!(network.verify(msg, &sig));
        Ok(())
    }
}
