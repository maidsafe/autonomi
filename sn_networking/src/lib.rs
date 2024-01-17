// Copyright 2023 MaidSafe.net limited.
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
mod driver;
mod error;
mod event;
#[cfg(feature = "open-metrics")]
mod metrics;
#[cfg(feature = "open-metrics")]
mod metrics_service;
mod network_discovery;
mod record_store;
mod record_store_api;
mod replication_fetcher;
mod transfers;

use self::{cmd::SwarmCmd, error::Result};
pub use self::{
    cmd::SwarmLocalState,
    driver::{GetRecordCfg, NetworkBuilder, PutRecordCfg, SwarmDriver},
    error::Error,
    event::{MsgResponder, NetworkEvent},
    record_store::NodeRecordStore,
};

use bytes::Bytes;
use futures::future::select_all;
use libp2p::{
    identity::Keypair,
    kad::{KBucketDistance, KBucketKey, Quorum, Record, RecordKey},
    multiaddr::Protocol,
    Multiaddr, PeerId,
};
use rand::Rng;
use sn_protocol::{
    error::Error as ProtocolError,
    messages::{Query, QueryResponse, Request, Response},
    storage::{RecordHeader, RecordKind, RecordType},
    NetworkAddress, PrettyPrintKBucketKey, PrettyPrintRecordKey,
};
use sn_transfers::{MainPubkey, NanoTokens, PaymentQuote};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

/// The maximum number of peers to return in a `GetClosestPeers` response.
/// This is the group size used in safe network protocol to be responsible for
/// an item in the network.
/// The peer should be present among the CLOSE_GROUP_SIZE if we're fetching the close_group(peer)
/// The size has been set to 5 for improved performance.
pub const CLOSE_GROUP_SIZE: usize = 5;

/// The range of peers that will be considered as close to a record target,
/// that a replication of the record shall be sent/accepted to/by the peer.
pub const REPLICATE_RANGE: usize = CLOSE_GROUP_SIZE + 2;

/// Majority of a given group (i.e. > 1/2).
#[inline]
pub const fn close_group_majority() -> usize {
    // Calculate the majority of the close group size by dividing it by 2 and adding 1.
    // This ensures that the majority is always greater than half.
    CLOSE_GROUP_SIZE / 2 + 1
}

/// Max duration to wait for verification
const MAX_REVERIFICATION_WAIT_TIME_S: std::time::Duration = std::time::Duration::from_millis(5000);
/// Min duration to wait for verification
const MIN_REVERIFICATION_WAIT_TIME_S: std::time::Duration = std::time::Duration::from_millis(1500);
/// Number of attempts to GET a record
const GET_RETRY_ATTEMPTS: usize = 3;
/// Number of attempts to PUT a record
const PUT_RETRY_ATTEMPTS: usize = 10;

/// Sort the provided peers by their distance to the given `NetworkAddress`.
/// Return with the closest expected number of entries if has.
#[allow(clippy::result_large_err)]
pub fn sort_peers_by_address<'a>(
    peers: &'a HashSet<PeerId>,
    address: &NetworkAddress,
    expected_entries: usize,
) -> Result<Vec<&'a PeerId>> {
    sort_peers_by_key(peers, &address.as_kbucket_key(), expected_entries)
}

/// Sort the provided peers by their distance to the given `KBucketKey`.
/// Return with the closest expected number of entries if has.
#[allow(clippy::result_large_err)]
pub fn sort_peers_by_key<'a, T>(
    peers: &'a HashSet<PeerId>,
    key: &KBucketKey<T>,
    expected_entries: usize,
) -> Result<Vec<&'a PeerId>> {
    // Check if there are enough peers to satisfy the request.
    // bail early if that's not the case
    if CLOSE_GROUP_SIZE > peers.len() {
        warn!("Not enough peers in the k-bucket to satisfy the request");
        return Err(Error::NotEnoughPeers {
            found: peers.len(),
            required: CLOSE_GROUP_SIZE,
        });
    }

    // Create a vector of tuples where each tuple is a reference to a peer and its distance to the key.
    // This avoids multiple computations of the same distance in the sorting process.
    let mut peer_distances: Vec<(&PeerId, KBucketDistance)> = Vec::with_capacity(peers.len());

    for peer_id in peers {
        let addr = NetworkAddress::from_peer(*peer_id);
        let distance = key.distance(&addr.as_kbucket_key());
        peer_distances.push((peer_id, distance));
    }

    // Sort the vector of tuples by the distance.
    peer_distances.sort_by(|a, b| a.1.cmp(&b.1));

    // Collect the sorted peers into a new vector.
    let sorted_peers: Vec<_> = peer_distances
        .into_iter()
        .take(expected_entries)
        .map(|(peer_id, _)| peer_id)
        .collect();

    Ok(sorted_peers)
}

#[derive(Clone)]
/// API to interact with the underlying Swarm
pub struct Network {
    pub swarm_cmd_sender: mpsc::Sender<SwarmCmd>,
    pub peer_id: PeerId,
    pub root_dir_path: PathBuf,
    keypair: Keypair,
}

impl Network {
    /// Signs the given data with the node's keypair.
    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>> {
        self.keypair.sign(msg).map_err(Error::from)
    }

    /// Verifies a signature for the given data and the node's public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        self.keypair.public().verify(msg, sig)
    }

    /// Dial the given peer at the given address.
    pub async fn dial(&self, addr: Multiaddr) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::Dial { addr, sender })?;
        receiver.await?
    }

    /// Stop the continuous Kademlia Bootstrapping process
    pub fn stop_bootstrapping(&self) -> Result<()> {
        self.send_swarm_cmd(SwarmCmd::StopBootstrapping)
    }

    /// Returns the closest peers to the given `XorName`, sorted by their distance to the xor_name.
    /// Excludes the client's `PeerId` while calculating the closest peers.
    pub async fn client_get_closest_peers(&self, key: &NetworkAddress) -> Result<Vec<PeerId>> {
        self.get_closest_peers(key, true).await
    }

    /// Returns the closest peers to the given `NetworkAddress`, sorted by their distance to the key.
    ///
    /// Includes our node's `PeerId` while calculating the closest peers.
    pub async fn node_get_closest_peers(&self, key: &NetworkAddress) -> Result<Vec<PeerId>> {
        self.get_closest_peers(key, false).await
    }

    /// Returns a map where each key is the ilog2 distance of that Kbucket and each value is a vector of peers in that
    /// bucket.
    /// Does not include self
    pub async fn get_kbuckets(&self) -> Result<BTreeMap<u32, Vec<PeerId>>> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetKBuckets { sender })?;
        receiver
            .await
            .map_err(|_e| Error::InternalMsgChannelDropped)
    }

    /// Returns the closest peers to the given `NetworkAddress` that is fetched from the local
    /// Routing Table. It is ordered by increasing distance of the peers
    /// Note self peer_id is not included in the result.
    pub async fn get_close_group_local_peers(&self, key: &NetworkAddress) -> Result<Vec<PeerId>> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetCloseGroupLocalPeers {
            key: key.clone(),
            sender,
        })?;

        match receiver.await {
            Ok(close_peers) => {
                // Only perform the pretty print and tracing if tracing is enabled
                if tracing::level_enabled!(tracing::Level::TRACE) {
                    let close_peers_pretty_print: Vec<_> = close_peers
                        .iter()
                        .map(|peer_id| {
                            format!(
                                "{peer_id:?}({:?})",
                                PrettyPrintKBucketKey(
                                    NetworkAddress::from_peer(*peer_id).as_kbucket_key()
                                )
                            )
                        })
                        .collect();

                    trace!(
                        "Local knowledge of close peers to {key:?} are: {close_peers_pretty_print:?}"
                    );
                }
                Ok(close_peers)
            }
            Err(err) => {
                error!("When getting local knowledge of close peers to {key:?}, failed with error {err:?}");
                Err(Error::InternalMsgChannelDropped)
            }
        }
    }

    /// Returns all the PeerId from all the KBuckets from our local Routing Table
    /// Also contains our own PeerId.
    pub async fn get_all_local_peers(&self) -> Result<Vec<PeerId>> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetAllLocalPeers { sender })?;

        receiver
            .await
            .map_err(|_e| Error::InternalMsgChannelDropped)
    }

    /// Returns all the PeerId from all the KBuckets from our local Routing Table
    /// Also contains our own PeerId.
    pub async fn get_closest_k_value_local_peers(&self) -> Result<HashSet<PeerId>> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetClosestKLocalPeers { sender })?;

        receiver
            .await
            .map_err(|_e| Error::InternalMsgChannelDropped)
    }

    pub async fn get_store_costs_from_network(
        &self,
        record_address: NetworkAddress,
    ) -> Result<(MainPubkey, PaymentQuote)> {
        // The requirement of having at least CLOSE_GROUP_SIZE
        // close nodes will be checked internally automatically.
        let mut close_nodes = self.get_closest_peers(&record_address, true).await?;

        // Sometimes we can get too many close node responses here.
        // (Seemingly libp2p can return more than expected)
        // We only want CLOSE_GROUP_SIZE peers at most
        close_nodes.sort_by(|a, b| {
            let a = NetworkAddress::from_peer(*a);
            let b = NetworkAddress::from_peer(*b);
            record_address
                .distance(&a)
                .cmp(&record_address.distance(&b))
        });

        close_nodes.truncate(close_group_majority());

        let request = Request::Query(Query::GetStoreCost(record_address.clone()));
        let responses = self
            .send_and_get_responses(close_nodes, &request, true)
            .await;

        // loop over responses, generating an average fee and storing all responses along side
        let mut all_costs = vec![];
        for response in responses.into_iter().flatten() {
            debug!(
                "StoreCostReq for {record_address:?} received response: {:?}",
                response
            );
            match response {
                Response::Query(QueryResponse::GetStoreCost {
                    quote: Ok(quote),
                    payment_address,
                }) => {
                    all_costs.push((payment_address, quote));
                }
                Response::Query(QueryResponse::GetStoreCost {
                    quote: Err(ProtocolError::RecordExists(_)),
                    payment_address,
                }) => {
                    all_costs.push((payment_address, PaymentQuote::zero()));
                }
                _ => {
                    error!("Non store cost response received,  was {:?}", response);
                }
            }
        }

        get_fees_from_store_cost_responses(all_costs)
    }

    /// Subscribe to given gossipsub topic
    pub fn subscribe_to_topic(&self, topic_id: String) -> Result<()> {
        self.send_swarm_cmd(SwarmCmd::GossipsubSubscribe(topic_id))?;
        Ok(())
    }

    /// Unsubscribe from given gossipsub topic
    pub fn unsubscribe_from_topic(&self, topic_id: String) -> Result<()> {
        self.send_swarm_cmd(SwarmCmd::GossipsubUnsubscribe(topic_id))?;
        Ok(())
    }

    /// Publish a msg on a given topic
    pub fn publish_on_topic(&self, topic_id: String, msg: Bytes) -> Result<()> {
        self.send_swarm_cmd(SwarmCmd::GossipsubPublish { topic_id, msg })?;
        Ok(())
    }

    /// Get the Record from the network
    /// Carry out re-attempts if required
    /// In case a target_record is provided, only return when fetched target.
    /// Otherwise count it as a failure when all attempts completed.
    pub async fn get_record_from_network(
        &self,
        key: RecordKey,
        cfg: &GetRecordCfg,
    ) -> Result<Record> {
        let total_attempts = if cfg.re_attempt {
            GET_RETRY_ATTEMPTS
        } else {
            1
        };

        let mut retry_attempts = 0;
        let pretty_key = PrettyPrintRecordKey::from(&key);
        while retry_attempts < total_attempts {
            retry_attempts += 1;
            info!(
                "Getting record of {pretty_key:?} attempts {retry_attempts:?}/{total_attempts:?} with cfg {cfg:?}",
            );

            let (sender, receiver) = oneshot::channel();
            self.send_swarm_cmd(SwarmCmd::GetNetworkRecord {
                key: key.clone(),
                sender,
                quorum: cfg.get_quorum,
                expected_holders: cfg.expected_holders.clone(),
            })?;

            match receiver.await.map_err(|e| {
                error!("When fetching record {pretty_key:?} , encountered a channel error {e:?}.");
                Error::InternalMsgChannelDropped
            })? {
                Ok(returned_record) => {
                    let header = RecordHeader::from_record(&returned_record)?;
                    let is_chunk = matches!(header.kind, RecordKind::Chunk);
                    info!("Record returned: {pretty_key:?}",);

                    // Returning OK whenever fulfill one of the followings:
                    // 1, No targeting record
                    // 2, Fetched record matches the targeting record (when not chunk, as they are content addressed)
                    //
                    // Returning mismatched error when: completed all attempts
                    if cfg.target_record.is_none()
                        || (cfg.target_record.is_some()
                            // we don't need to match the whole record if chunks, 
                            // payment data could differ, but chunks themselves'
                            // keys are from the chunk address
                            && (cfg.target_record == Some(returned_record.clone()) || is_chunk))
                    {
                        return Ok(returned_record);
                    } else if retry_attempts >= total_attempts {
                        info!("Error: Returned record does not match target");
                        return Err(Error::ReturnedRecordDoesNotMatch(
                            PrettyPrintRecordKey::from(&returned_record.key).into_owned(),
                        ));
                    }
                }
                Err(Error::RecordNotEnoughCopies(returned_record)) => {
                    debug!("Not enough copies found yet for {pretty_key:?}");
                    // Only return when completed all attempts
                    if retry_attempts >= total_attempts && matches!(cfg.get_quorum, Quorum::One) {
                        if cfg.target_record.is_none()
                            || (cfg.target_record.is_some()
                                && cfg.target_record == Some(returned_record.clone()))
                        {
                            return Ok(returned_record);
                        } else {
                            return Err(Error::ReturnedRecordDoesNotMatch(
                                PrettyPrintRecordKey::from(&returned_record.key).into_owned(),
                            ));
                        }
                    }
                }
                Err(Error::RecordNotFound) => {
                    // libp2p RecordNotFound does mean no holders answered.
                    // it does not actually mean the record does not exist.
                    // just that those asked did not have it
                    if retry_attempts >= total_attempts {
                        break;
                    }

                    warn!("No holder of record '{pretty_key:?}' found. Retrying the fetch ...",);
                }
                Err(Error::SplitRecord { result_map }) => {
                    error!("Getting record {pretty_key:?} attempts #{retry_attempts}/{total_attempts} , encountered split");

                    if retry_attempts >= total_attempts {
                        return Err(Error::SplitRecord { result_map });
                    }
                    warn!("Fetched split Record '{pretty_key:?}' from network!. Retrying...",);
                }
                Err(error) => {
                    error!("Getting record {pretty_key:?} attempts #{retry_attempts}/{total_attempts} , encountered {error:?}");

                    if retry_attempts >= total_attempts {
                        break;
                    }
                    warn!("Did not retrieve Record '{pretty_key:?}' from network!. Retrying...",);
                }
            }

            // wait for a bit before re-trying
            if cfg.re_attempt {
                // Generate a random duration between MAX_REVERIFICATION_WAIT_TIME_S and MIN_REVERIFICATION_WAIT_TIME_S
                let wait_duration = rand::thread_rng()
                    .gen_range(MIN_REVERIFICATION_WAIT_TIME_S..MAX_REVERIFICATION_WAIT_TIME_S);
                tokio::time::sleep(wait_duration).await;
            }
        }

        Err(Error::RecordNotFound)
    }

    /// Get the cost of storing the next record from the network
    pub async fn get_local_storecost(&self) -> Result<NanoTokens> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetLocalStoreCost { sender })?;

        receiver
            .await
            .map_err(|_e| Error::InternalMsgChannelDropped)
    }

    /// Get `Record` from the local RecordStore
    pub async fn get_local_record(&self, key: &RecordKey) -> Result<Option<Record>> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetLocalRecord {
            key: key.clone(),
            sender,
        })?;

        receiver
            .await
            .map_err(|_e| Error::InternalMsgChannelDropped)
    }

    /// Put `Record` to network
    /// Optionally verify the record is stored after putting it to network
    /// If verify is on, retry PUT_RETRY_ATTEMPTS times with a random wait between 1.5s and 5s
    pub async fn put_record(&self, record: Record, cfg: &PutRecordCfg) -> Result<()> {
        let pretty_key = PrettyPrintRecordKey::from(&record.key);
        let mut last_err = Error::FailedToVerifyRecordWasStored(pretty_key.clone().into_owned());
        let total_attempts = if cfg.re_attempt {
            PUT_RETRY_ATTEMPTS
        } else {
            1
        };
        for retry in 1..total_attempts + 1 {
            info!(
                "Attempting to PUT record with key: {pretty_key:?} to network. Attempts {retry:?}/{total_attempts:?} with cfg {cfg:?}"
            );

            let res = self.put_record_once(record.clone(), cfg).await;

            match res {
                Ok(_) => return Ok(()),
                Err(e) => {
                    warn!("Failed to PUT record with key: {pretty_key:?} to network. Attempts {retry:?}/{total_attempts:?} with error: {e:?}");
                    last_err = e;
                }
            }
        }

        Err(last_err)
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
        self.send_swarm_cmd(SwarmCmd::PutRecord {
            record: record.clone(),
            sender,
            quorum: cfg.put_quorum,
        })?;
        let response = receiver.await?;

        if let Some((_record_kind, get_cfg)) = &cfg.verification {
            // Generate a random duration between MAX_REVERIFICATION_WAIT_TIME_S and MIN_REVERIFICATION_WAIT_TIME_S
            let wait_duration = rand::thread_rng()
                .gen_range(MIN_REVERIFICATION_WAIT_TIME_S..MAX_REVERIFICATION_WAIT_TIME_S);
            // Small wait before we attempt to verify.
            // There will be `re-attempts` to be carried out within the later step anyway.
            tokio::time::sleep(wait_duration).await;
            debug!("Attempting to verify {pretty_key:?} after we've slept for {wait_duration:?}");

            // Verify the record is stored, requiring re-attempts
            self.get_record_from_network(record.key.clone(), get_cfg)
                .await
                .map_err(|e| {
                    warn!("Failed to verify record {pretty_key:?} was stored: {e:?}");
                    Error::FailedToVerifyRecordWasStored(pretty_key.clone().into_owned())
                })?;
        }

        response
    }

    /// Put `Record` to the local RecordStore
    /// Must be called after the validations are performed on the Record
    pub fn put_local_record(&self, record: Record) -> Result<()> {
        trace!(
            "Writing Record locally, for {:?} - length {:?}",
            PrettyPrintRecordKey::from(&record.key),
            record.value.len()
        );
        self.send_swarm_cmd(SwarmCmd::PutLocalRecord { record })
    }

    /// Remove a local record from the RecordStore after a failed write
    pub fn remove_failed_local_record(&self, key: RecordKey) -> Result<()> {
        trace!("Removing Record locally, for {:?}", key);
        self.send_swarm_cmd(SwarmCmd::RemoveFailedLocalRecord { key })
    }

    /// Returns true if a RecordKey is present locally in the RecordStore
    pub async fn is_record_key_present_locally(&self, key: &RecordKey) -> Result<bool> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::RecordStoreHasKey {
            key: key.clone(),
            sender,
        })?;

        receiver
            .await
            .map_err(|_e| Error::InternalMsgChannelDropped)
    }

    /// Returns the Addresses of all the locally stored Records
    pub async fn get_all_local_record_addresses(
        &self,
    ) -> Result<HashMap<NetworkAddress, RecordType>> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetAllLocalRecordAddresses { sender })?;

        receiver
            .await
            .map_err(|_e| Error::InternalMsgChannelDropped)
    }

    // Add a list of keys of a holder to Replication Fetcher.
    #[allow(clippy::mutable_key_type)] // for Bytes in NetworkAddress
    pub fn add_keys_to_replication_fetcher(
        &self,
        holder: PeerId,
        keys: HashMap<NetworkAddress, RecordType>,
    ) -> Result<()> {
        self.send_swarm_cmd(SwarmCmd::AddKeysToReplicationFetcher { holder, keys })
    }

    /// Send `Request` to the given `PeerId` and await for the response. If `self` is the recipient,
    /// then the `Request` is forwarded to itself and handled, and a corresponding `Response` is created
    /// and returned to itself. Hence the flow remains the same and there is no branching at the upper
    /// layers.
    pub async fn send_request(&self, req: Request, peer: PeerId) -> Result<Response> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::SendRequest {
            req,
            peer,
            sender: Some(sender),
        })?;
        receiver.await?
    }

    /// Send `Request` to the given `PeerId` and do _not_ await a response here.
    /// Instead the Response will be handled by the common `response_handler`
    pub fn send_req_ignore_reply(&self, req: Request, peer: PeerId) -> Result<()> {
        let swarm_cmd = SwarmCmd::SendRequest {
            req,
            peer,
            sender: None,
        };
        self.send_swarm_cmd(swarm_cmd)
    }

    /// Send a `Response` through the channel opened by the requester.
    pub fn send_response(&self, resp: Response, channel: MsgResponder) -> Result<()> {
        self.send_swarm_cmd(SwarmCmd::SendResponse { resp, channel })
    }

    /// Return a `SwarmLocalState` with some information obtained from swarm's local state.
    pub async fn get_swarm_local_state(&self) -> Result<SwarmLocalState> {
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetSwarmLocalState(sender))?;
        let state = receiver.await?;
        Ok(state)
    }

    pub fn start_handle_gossip(&self) -> Result<()> {
        self.send_swarm_cmd(SwarmCmd::GossipHandler)
    }

    // Helper to send SwarmCmd
    fn send_swarm_cmd(&self, cmd: SwarmCmd) -> Result<()> {
        let capacity = self.swarm_cmd_sender.capacity();

        let cmd_sender = self.swarm_cmd_sender.clone();

        if capacity == 0 {
            if matches!(cmd, SwarmCmd::AddKeysToReplicationFetcher { .. }) {
                // we can safely drop AddKeysToReplicationFetcher
                // it should be reattempted in a few seconds and if we can cope we'll do it.
                warn!(
                    "SwarmCmd channel is full. Dropping AddKeysToReplicationFetcher: {:?}",
                    cmd
                );
                return Ok(());
            } else {
                error!(
                    "SwarmCmd channel is full. Await capacity to send: {:?}",
                    cmd
                );
            }
        }

        // Spawn a task to send the SwarmCmd and keep this fn sync
        let _handle = tokio::spawn(async move {
            if let Err(error) = cmd_sender.send(cmd).await {
                error!("Failed to send SwarmCmd: {}", error);
            }
        });

        Ok(())
    }

    /// Returns the closest peers to the given `XorName`, sorted by their distance to the xor_name.
    /// If `client` is false, then include `self` among the `closest_peers`
    pub async fn get_closest_peers(
        &self,
        key: &NetworkAddress,
        client: bool,
    ) -> Result<Vec<PeerId>> {
        trace!("Getting the closest peers to {key:?}");
        let (sender, receiver) = oneshot::channel();
        self.send_swarm_cmd(SwarmCmd::GetClosestPeersToAddressFromNetwork {
            key: key.clone(),
            sender,
        })?;
        let k_bucket_peers = receiver.await?;

        // Count self in if among the CLOSE_GROUP_SIZE closest and sort the result
        let mut closest_peers = k_bucket_peers;
        // ensure we're not including self here
        if client {
            let _existed = closest_peers.remove(&self.peer_id);
        }
        if tracing::level_enabled!(tracing::Level::TRACE) {
            let close_peers_pretty_print: Vec<_> = closest_peers
                .iter()
                .map(|peer_id| {
                    format!(
                        "{peer_id:?}({:?})",
                        PrettyPrintKBucketKey(NetworkAddress::from_peer(*peer_id).as_kbucket_key())
                    )
                })
                .collect();

            trace!("Network knowledge of close peers to {key:?} are: {close_peers_pretty_print:?}");
        }

        let closest_peers = sort_peers_by_address(&closest_peers, key, CLOSE_GROUP_SIZE)?;
        Ok(closest_peers.into_iter().cloned().collect())
    }

    /// Send a `Request` to the provided set of peers and wait for their responses concurrently.
    /// If `get_all_responses` is true, we wait for the responses from all the peers.
    /// NB TODO: Will return an error if the request timeouts.
    /// If `get_all_responses` is false, we return the first successful response that we get
    pub async fn send_and_get_responses(
        &self,
        peers: Vec<PeerId>,
        req: &Request,
        get_all_responses: bool,
    ) -> Vec<Result<Response>> {
        debug!("send_and_get_responses for {req:?}");
        let mut list_of_futures = peers
            .iter()
            .map(|peer| Box::pin(self.send_request(req.clone(), *peer)))
            .collect::<Vec<_>>();

        let mut responses = Vec::new();
        while !list_of_futures.is_empty() {
            let (res, _, remaining_futures) = select_all(list_of_futures).await;
            let res_string = match &res {
                Ok(res) => format!("{res}"),
                Err(err) => format!("{err:?}"),
            };
            debug!("Got response for the req: {req:?}, res: {res_string}");
            if !get_all_responses && res.is_ok() {
                return vec![res];
            }
            responses.push(res);
            list_of_futures = remaining_futures;
        }

        debug!("Received all responses for {req:?}");
        responses
    }
}

/// Given `all_costs` it will return the lowest cost.
fn get_fees_from_store_cost_responses(
    mut all_costs: Vec<(MainPubkey, PaymentQuote)>,
) -> Result<(MainPubkey, PaymentQuote)> {
    // sort all costs by fee, lowest to highest
    // if there's a tie in cost, sort by pubkey
    all_costs.sort_by(|(pub_key_a, cost_a), (pub_key_b, cost_b)| {
        match cost_a.cost.cmp(&cost_b.cost) {
            std::cmp::Ordering::Equal => pub_key_a.cmp(pub_key_b),
            other => other,
        }
    });

    // get the lowest cost
    trace!("Got all costs: {all_costs:?}");
    let lowest = all_costs
        .into_iter()
        .next()
        .ok_or(Error::NoStoreCostResponses)?;
    info!("Final fees calculated as: {lowest:?}");

    Ok(lowest)
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

/// Build a `Multiaddr` with the p2p protocol filtered out.
pub(crate) fn multiaddr_strip_p2p(multiaddr: &Multiaddr) -> Multiaddr {
    multiaddr
        .iter()
        .filter(|p| !matches!(p, Protocol::P2p(_)))
        .collect()
}

#[cfg(test)]
mod tests {
    use eyre::bail;

    use super::*;
    use sn_transfers::PaymentQuote;

    #[test]
    fn test_get_fee_from_store_cost_responses() -> Result<()> {
        // for a vec of different costs of CLOSE_GROUP size
        // ensure we return the CLOSE_GROUP / 2 indexed price
        let mut costs = vec![];
        for i in 1..CLOSE_GROUP_SIZE {
            let addr = MainPubkey::new(bls::SecretKey::random().public_key());
            costs.push((
                addr,
                PaymentQuote::test_dummy(Default::default(), NanoTokens::from(i as u64)),
            ));
        }
        let expected_price = costs[0].1.cost.as_nano();
        let (_key, price) = get_fees_from_store_cost_responses(costs)?;

        assert_eq!(
            price.cost.as_nano(),
            expected_price,
            "price should be {}",
            expected_price
        );

        Ok(())
    }

    #[test]
    fn test_get_some_fee_from_store_cost_responses_even_if_one_errs_and_sufficient(
    ) -> eyre::Result<()> {
        // for a vec of different costs of CLOSE_GROUP size
        let responses_count = CLOSE_GROUP_SIZE as u64 - 1;
        let mut costs = vec![];
        for i in 1..responses_count {
            // push random MainPubkey and Nano
            let addr = MainPubkey::new(bls::SecretKey::random().public_key());
            costs.push((
                addr,
                PaymentQuote::test_dummy(Default::default(), NanoTokens::from(i)),
            ));
            println!("price added {}", i);
        }

        // this should be the lowest price
        let expected_price = costs[0].1.cost.as_nano();

        let (_key, price) = match get_fees_from_store_cost_responses(costs) {
            Err(_) => bail!("Should not have errored as we have enough responses"),
            Ok(cost) => cost,
        };

        assert_eq!(
            price.cost.as_nano(),
            expected_price,
            "price should be {}",
            expected_price
        );

        Ok(())
    }

    #[test]
    fn test_network_sign_verify() -> eyre::Result<()> {
        let (network, _, _) =
            NetworkBuilder::new(Keypair::generate_ed25519(), false, std::env::temp_dir())
                .build_client()?;
        let msg = b"test message";
        let sig = network.sign(msg)?;
        assert!(network.verify(msg, &sig));
        Ok(())
    }
}
