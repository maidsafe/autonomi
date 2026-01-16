// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use ant_evm::{PaymentQuote, QuotingMetrics};
use ant_protocol::messages::{ConnectionInfo, Request, Response};
use ant_protocol::storage::{DataTypes, ValidationType};
use ant_protocol::{NetworkAddress, PrettyPrintKBucketKey, PrettyPrintRecordKey};
use exponential_backoff::Backoff;
use futures::StreamExt;
use futures::future::select_all;
use futures::stream::FuturesUnordered;
use libp2p::kad::{KBucketDistance, Record, RecordKey};
use libp2p::swarm::ConnectionId;
use libp2p::{Multiaddr, PeerId, identity::Keypair};
use tokio::sync::{mpsc, oneshot};
use tokio::time::sleep;
use xor_name::XorName;

use super::driver::event::MsgResponder;
use super::error::{NetworkError, Result};
use super::interface::{LocalSwarmCmd, NetworkSwarmCmd};
use super::{Addresses, NetworkEvent, NodeIssue, SwarmLocalState};

mod init;

/// Number of retry attempts for get_n_closest_peers_with_retries (initial + retries)
const CLOSEST_PEERS_RETRY_ATTEMPTS: u32 = 2;
/// Minimum backoff wait time in seconds between retry attempts
const CLOSEST_PEERS_RETRY_MIN_WAIT_SECS: u64 = 2;
/// Maximum backoff wait time in seconds between retry attempts
const CLOSEST_PEERS_RETRY_MAX_WAIT_SECS: u64 = 8;

pub(crate) use init::NetworkConfig;

#[derive(Clone, Debug)]
/// API to interact with the underlying Swarm
pub(crate) struct Network {
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
    /// Initialize the network
    /// This will start the network driver in a background thread, which is a long-running task that runs until the [`Network`] is dropped
    /// The [`Network`] is cheaply cloneable, prefer cloning over creating new instances to avoid creating multiple network drivers
    pub(crate) fn init(config: NetworkConfig) -> Result<(Self, mpsc::Receiver<NetworkEvent>)> {
        let peer_id = PeerId::from(config.keypair.public());
        let keypair = config.keypair.clone();
        let shutdown_rx = config.shutdown_rx.clone();

        // setup the swarm driver
        let (swarm_driver, network_event_receiver) = init::init_driver(config)?;

        // create a new network instance
        let network = Network {
            inner: Arc::new(NetworkInner {
                network_swarm_cmd_sender: swarm_driver.network_cmd_sender.clone(),
                local_swarm_cmd_sender: swarm_driver.local_cmd_sender.clone(),
                peer_id,
                keypair,
            }),
        };

        // Run the swarm driver as a background task
        let _swarm_driver_task = tokio::spawn(swarm_driver.run(shutdown_rx));

        Ok((network, network_event_receiver))
    }

    /// Returns the `PeerId` of the instance.
    pub(crate) fn peer_id(&self) -> PeerId {
        self.inner.peer_id
    }

    /// Returns the `Keypair` of the instance.
    pub(crate) fn keypair(&self) -> &Keypair {
        &self.inner.keypair
    }

    /// Signs the given data with the node's keypair.
    pub(crate) fn sign(&self, msg: &[u8]) -> Result<Vec<u8>> {
        self.keypair().sign(msg).map_err(NetworkError::from)
    }

    /// Verifies a signature for the given data and the node's public key.
    pub(crate) fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        self.keypair().public().verify(msg, sig)
    }

    /// Returns the protobuf serialised PublicKey to allow messaging out for share.
    pub(crate) fn get_pub_key(&self) -> Vec<u8> {
        self.keypair().public().encode_protobuf()
    }

    /// Returns a list of peers in local RT and their correspondent Multiaddr.
    /// Does not include self
    pub(crate) async fn get_local_peers_with_multiaddr(
        &self,
    ) -> Result<Vec<(PeerId, Vec<Multiaddr>)>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetPeersWithMultiaddr { sender });
        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Returns a two-element tuple, where the first element is a map where each key is the ilog2
    /// distance of that Kbucket and each value is a vector of peers in that bucket, and the second
    /// element is the estimated network size.
    ///
    /// Does not include self
    pub(crate) async fn get_kbuckets(&self) -> Result<(BTreeMap<u32, Vec<PeerId>>, usize)> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetKBuckets { sender });
        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Returns K closest local peers to the target.
    /// Target defaults to self, if not provided.
    /// Self is always included as the first entry.
    pub(crate) async fn get_k_closest_local_peers_to_the_target(
        &self,
        key: Option<NetworkAddress>,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        let target = if let Some(target) = key {
            target
        } else {
            NetworkAddress::from(self.peer_id())
        };

        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetKCloseLocalPeersToTarget {
            sender,
            key: target,
        });

        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Get the quoting metrics for storing the next record from the network
    pub(crate) async fn get_local_quoting_metrics(
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
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;
        Ok(quoting_metrics)
    }

    /// Notify the node receicced a payment.
    pub(crate) fn notify_payment_received(&self) {
        self.send_local_swarm_cmd(LocalSwarmCmd::PaymentReceived);
    }

    /// Add an entry to the paid-for list after payment verification.
    pub(crate) fn notify_paid_for_entry_added(&self, xor_name: XorName, data_type: DataTypes) {
        self.send_local_swarm_cmd(LocalSwarmCmd::AddPaidForEntry {
            xor_name,
            data_type,
        });
    }

    pub(crate) fn notify_record_not_at_target_location(&self) {
        self.send_local_swarm_cmd(LocalSwarmCmd::RecordNotAtTargetLocation);
    }

    /// Get `Record` from the local RecordStore
    pub(crate) async fn get_local_record(&self, key: &RecordKey) -> Result<Option<Record>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetLocalRecord {
            key: key.clone(),
            sender,
        });

        receiver
            .await
            .map_err(|e| NetworkError::EventChannelFailure(format!("{e:?}")))
    }

    /// Whether the target peer is considered blacklisted by self
    pub(crate) async fn is_peer_shunned(&self, target: NetworkAddress) -> Result<bool> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::IsPeerShunned { target, sender });

        receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)
    }

    /// Notify ReplicationFetch a fetch attempt is completed.
    /// (but it won't trigger any real writes to disk)
    pub(crate) fn notify_fetch_completed(&self, key: RecordKey, record_type: ValidationType) {
        self.send_local_swarm_cmd(LocalSwarmCmd::FetchCompleted((key, record_type)))
    }

    /// Put `Record` to the local RecordStore
    /// Must be called after the validations are performed on the Record
    pub(crate) fn put_local_record(&self, record: Record, is_client_put: bool) {
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
    pub(crate) async fn is_record_key_present_locally(&self, key: &RecordKey) -> Result<bool> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::RecordStoreHasKey {
            key: key.clone(),
            sender,
        });

        let is_present = receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;

        Ok(is_present)
    }

    /// Returns the Addresses of all the locally stored Records
    pub(crate) async fn get_all_local_record_addresses(
        &self,
    ) -> Result<HashMap<NetworkAddress, ValidationType>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetAllLocalRecordAddresses { sender });

        let addrs = receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;
        Ok(addrs)
    }

    /// Send `Request` to the given `PeerId` and await for the response. If `self` is the recipient,
    /// then the `Request` is forwarded to itself and handled, and a corresponding `Response` is created
    /// and returned to itself. Hence the flow remains the same and there is no branching at the upper
    /// layers.
    pub(crate) async fn send_request(
        &self,
        req: Request,
        peer: PeerId,
        addrs: Addresses,
    ) -> Result<(Response, Option<ConnectionInfo>)> {
        let (sender, receiver) = oneshot::channel();

        self.send_network_swarm_cmd(NetworkSwarmCmd::SendRequest {
            req: req.clone(),
            peer,
            addrs,
            sender: Some(sender),
        });
        receiver.await?
    }

    /// Send a `Response` through the channel opened by the requester.
    pub(crate) fn send_response(&self, resp: Response, channel: MsgResponder) {
        self.send_network_swarm_cmd(NetworkSwarmCmd::SendResponse { resp, channel })
    }

    /// Return a `SwarmLocalState` with some information obtained from swarm's local state.
    pub(crate) async fn get_swarm_local_state(&self) -> Result<SwarmLocalState> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetSwarmLocalState(sender));
        let state = receiver.await?;
        Ok(state)
    }

    pub(crate) fn trigger_interval_replication(&self) {
        self.send_local_swarm_cmd(LocalSwarmCmd::TriggerIntervalReplication)
    }

    /// Add a peer to the networking blocklist.
    pub(crate) fn add_peer_to_blocklist(&self, peer: PeerId) {
        self.send_local_swarm_cmd(LocalSwarmCmd::AddPeerToBlockList { peer_id: peer })
    }

    /// To be called when got a fresh record from client uploading.
    pub(crate) fn add_fresh_records_to_the_replication_fetcher(
        &self,
        holder: NetworkAddress,
        keys: Vec<(NetworkAddress, ValidationType)>,
    ) {
        self.send_local_swarm_cmd(LocalSwarmCmd::AddFreshReplicateRecords { holder, keys })
    }

    pub(crate) fn record_node_issues(&self, peer_id: PeerId, issue: NodeIssue) {
        self.send_local_swarm_cmd(LocalSwarmCmd::RecordNodeIssue { peer_id, issue });
    }

    pub(crate) fn historical_verify_quotes(&self, quotes: Vec<(PeerId, PaymentQuote)>) {
        self.send_local_swarm_cmd(LocalSwarmCmd::QuoteVerification { quotes });
    }

    pub(crate) fn trigger_irrelevant_record_cleanup(&self) {
        self.send_local_swarm_cmd(LocalSwarmCmd::TriggerIrrelevantRecordCleanup)
    }

    pub(crate) fn notify_peer_scores(&self, peer_scores: Vec<(PeerId, bool)>) {
        self.send_local_swarm_cmd(LocalSwarmCmd::NotifyPeerScores { peer_scores })
    }

    pub(crate) fn notify_node_version(&self, peer: PeerId, version: String) {
        self.send_local_swarm_cmd(LocalSwarmCmd::NotifyPeerVersion { peer, version })
    }

    pub(crate) fn remove_peer(&self, peer: PeerId) {
        self.send_local_swarm_cmd(LocalSwarmCmd::RemovePeer { peer })
    }

    /// Get closest peers from a specific peer using request/response
    /// Returns a list of `(NetworkAddress, Vec<Multiaddr>)` tuples
    pub(crate) async fn get_closest_peers_from_peer(
        &self,
        addr: NetworkAddress,
        peer: (PeerId, Addresses),
        num_of_peers: Option<usize>,
    ) -> Result<Vec<(NetworkAddress, Vec<Multiaddr>)>> {
        use ant_protocol::messages::{Query, QueryResponse};

        let req = Request::Query(Query::GetClosestPeers {
            key: addr.clone(),
            num_of_peers,
            range: None,
            sign_result: true,
        });

        let (resp, _) = self.send_request(req, peer.0, peer.1).await?;

        match resp {
            Response::Query(QueryResponse::GetClosestPeers {
                target: _,
                peers,
                signature: _,
            }) => Ok(peers),
            _ => Err(NetworkError::EventChannelFailure(
                "Unexpected response type".to_string(),
            )),
        }
    }

    /// Returns the closest peers to the given `XorName`, sorted by their distance to the xor_name.
    #[allow(dead_code)]
    pub(crate) async fn get_closest_peers(
        &self,
        key: &NetworkAddress,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        let pretty_key = PrettyPrintKBucketKey(key.as_kbucket_key());
        debug!("Getting the all closest peers in range of {pretty_key:?}");
        let (sender, receiver) = oneshot::channel();
        self.send_network_swarm_cmd(NetworkSwarmCmd::GetClosestPeersToAddressFromNetwork {
            key: key.clone(),
            sender,
            n: None,
        });

        let closest_peers = receiver.await?;

        // Error out when fetched result is empty, indicating a timed out network query.
        if closest_peers.is_empty() {
            return Err(NetworkError::GetClosestTimedOut);
        }

        if tracing::level_enabled!(tracing::Level::DEBUG) {
            let close_peers_pretty_print: Vec<_> = closest_peers
                .iter()
                .map(|(peer_id, _)| {
                    format!(
                        "{peer_id:?}({:?})",
                        PrettyPrintKBucketKey(NetworkAddress::from(*peer_id).as_kbucket_key())
                    )
                })
                .collect();

            debug!(
                "Network knowledge of closest peers to {pretty_key:?} are: {close_peers_pretty_print:?}"
            );
        }

        Ok(closest_peers)
    }

    /// Returns the closest peers with multi-stage verification based on majority knowledge.
    /// This function verifies the candidates by:
    /// 1. Getting N candidates via Kademlia (if `n` provided, requests that many)
    /// 2. Querying each candidate for their view of closest peers
    /// 3. Candidates are collected from the aggregated results, preferring high witness among the close group
    /// 4. Peers are returned in the ascending order of distance to the target
    ///
    /// If `n` is provided, requests that many peers from Kademlia and returns up to that many.
    /// Otherwise uses the default Kademlia count.
    ///
    /// This is more accurate but slower than `get_closest_peers` due to the additional verification round-trips.
    /// Use this for critical operations like Merkle payment topology verification.
    pub(crate) async fn get_closest_peers_with_majority_knowledge(
        &self,
        key: &NetworkAddress,
        n: Option<usize>,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        let pretty_key = PrettyPrintKBucketKey(key.as_kbucket_key());
        debug!("Getting the all closest peers in range of {pretty_key:?}");
        let (sender, receiver) = oneshot::channel();
        self.send_network_swarm_cmd(NetworkSwarmCmd::GetClosestPeersToAddressFromNetwork {
            key: key.clone(),
            sender,
            n: n.and_then(NonZeroUsize::new),
        });

        let candidates = receiver.await?;

        // Error out when fetched result is empty, indicating a timed out network query.
        if candidates.is_empty() {
            return Err(NetworkError::GetClosestTimedOut);
        }

        if tracing::level_enabled!(tracing::Level::DEBUG) {
            let close_peers_pretty_print: Vec<_> = candidates
                .iter()
                .map(|(peer_id, _)| {
                    format!(
                        "{peer_id:?}({:?})",
                        PrettyPrintKBucketKey(NetworkAddress::from(*peer_id).as_kbucket_key())
                    )
                })
                .collect();

            debug!(
                "Initial candidates from Kad query targeting {pretty_key:?}: {close_peers_pretty_print:?}"
            );
        }

        // Verify candidates by querying them individually for their closest peers
        let mut query_tasks = vec![];
        for peer in &candidates {
            let network = self.clone();
            let addr = key.clone();
            let peer_clone = peer.clone();
            let n_value = candidates.len() + 2;
            query_tasks.push(async move {
                let result = network
                    .get_closest_peers_from_peer(addr, peer_clone.clone(), Some(n_value))
                    .await;
                (peer_clone.0, result)
            });
        }

        // Process queries concurrently (with a reasonable limit)
        let mut tasks: FuturesUnordered<_> = query_tasks.into_iter().collect();

        let mut peer_counts: HashMap<PeerId, usize> = HashMap::new();

        while let Some((responder_peer_id, result)) = tasks.next().await {
            if let Ok(peers_list) = result {
                // Log the responder and their returned peer list
                trace!("Closegroup to {pretty_key:?} responded from peer {responder_peer_id:?}:");

                *peer_counts.entry(responder_peer_id).or_insert(0) += 1;

                // Count appearances in the response
                for (peer_addr, _addrs) in peers_list {
                    if let Some(peer_id) = peer_addr.as_peer_id() {
                        let distance = key.distance(&peer_addr);
                        trace!("  Reported peer: {peer_id:?}, distance: {distance:?}");

                        *peer_counts.entry(peer_id).or_insert(0) += 1;
                    }
                }
            } else {
                info!("Failed to get closest peers from node {responder_peer_id:?}");
            }
        }

        // =============================================================================
        // PEER SELECTION ALGORITHM
        // =============================================================================
        // This algorithm selects the N closest verified peers using a multi-tier approach:
        //
        // 1. BUILD POPULAR PEERS LIST:
        //    - Identify "popular" peers: those seen more than n/2 times in peer responses
        //    - These are considered more trustworthy as multiple nodes agree they exist
        //
        // 2. TIER 1 - CANDIDATES IN POPULAR (highest priority):
        //    - Select peers that appear in BOTH the original Kad `candidates` AND `popular_peer_ids`
        //    - These are the most reliable: both Kad and peer consensus agree
        //
        // 3. TIER 2 - CANDIDATES BEYOND POPULAR RANGE:
        //    - If Tier 1 doesn't fill N slots, add peers from `candidates` that are farther than the farthest popular peer
        //    - These extend coverage beyond the popular consensus zone
        //
        // 4. TIER 3 - REMAINING CANDIDATES:
        //    - If still not enough, fill remaining slots with unselected `candidates`
        //    - Pick closest first (sorted by distance to target)
        //
        // Final result is sorted by distance to target address.
        // =============================================================================

        let n = candidates.len();
        let popularity_threshold = n / 2;

        // Step 1: Build popular_peer_ids - peers seen more than n/2 times
        let popular_peer_ids: std::collections::HashSet<PeerId> = peer_counts
            .iter()
            .filter(|&(_, &count)| count > popularity_threshold)
            .map(|(peer_id, _)| *peer_id)
            .collect();

        debug!(
            "Found {} popular peers (seen > {} times) for {pretty_key:?}",
            popular_peer_ids.len(),
            popularity_threshold
        );

        // Helper to compute distance
        let get_distance = |peer_id: &PeerId| -> KBucketDistance {
            let peer_addr = NetworkAddress::from(*peer_id);
            key.distance(&peer_addr)
        };

        // Find the farthest popular peer distance (if any popular peers exist)
        let farthest_popular_distance = popular_peer_ids.iter().map(get_distance).max();

        let mut verified_candidates: Vec<(PeerId, Addresses)> = Vec::with_capacity(n);
        let mut selected_peer_ids: std::collections::HashSet<PeerId> =
            std::collections::HashSet::new();

        // Step 2: Tier 1 - Pick peers in BOTH candidates AND popular_peer_ids
        let mut tier1_peers: Vec<_> = candidates
            .iter()
            .filter(|(peer_id, _)| popular_peer_ids.contains(peer_id))
            .collect();
        // Sort by distance (closest first)
        tier1_peers.sort_by_key(|(peer_id, _)| get_distance(peer_id));

        for (peer_id, addrs) in tier1_peers {
            if verified_candidates.len() >= n {
                break;
            }
            trace!(
                "Tier1 selected: {:?}, distance: {:?}",
                peer_id,
                get_distance(peer_id)
            );
            verified_candidates.push((*peer_id, addrs.clone()));
            let _ = selected_peer_ids.insert(*peer_id);
        }

        debug!(
            "Tier1 (candidates in popular): selected {}/{} peers for {pretty_key:?}",
            verified_candidates.len(),
            n
        );

        // Step 3: Tier 2 - Pick candidates farther than the farthest popular peer
        if verified_candidates.len() < n
            && let Some(farthest_popular) = farthest_popular_distance
        {
            let mut tier2_peers: Vec<_> = candidates
                .iter()
                .filter(|(peer_id, _)| {
                    !selected_peer_ids.contains(peer_id) && get_distance(peer_id) > farthest_popular
                })
                .collect();
            // Sort by distance (closest first among those beyond popular range)
            tier2_peers.sort_by_key(|(peer_id, _)| get_distance(peer_id));

            for (peer_id, addrs) in tier2_peers {
                if verified_candidates.len() >= n {
                    break;
                }
                trace!(
                    "Tier2 selected: {:?}, distance: {:?}",
                    peer_id,
                    get_distance(peer_id)
                );
                verified_candidates.push((*peer_id, addrs.clone()));
                let _ = selected_peer_ids.insert(*peer_id);
            }

            debug!(
                "Tier2 (candidates beyond popular): selected {}/{} peers total for {pretty_key:?}",
                verified_candidates.len(),
                n
            );
        }

        // Step 4: Tier 3 - Fill remaining slots with unselected candidates (closest first)
        if verified_candidates.len() < n {
            let mut tier3_peers: Vec<_> = candidates
                .iter()
                .filter(|(peer_id, _)| !selected_peer_ids.contains(peer_id))
                .collect();
            // Sort by distance (closest first)
            tier3_peers.sort_by_key(|(peer_id, _)| get_distance(peer_id));

            for (peer_id, addrs) in tier3_peers {
                if verified_candidates.len() >= n {
                    break;
                }
                trace!(
                    "Tier3 selected: {:?}, distance: {:?}",
                    peer_id,
                    get_distance(peer_id)
                );
                verified_candidates.push((*peer_id, addrs.clone()));
                let _ = selected_peer_ids.insert(*peer_id);
            }

            debug!(
                "Tier3 (remaining candidates): selected {}/{} peers total for {pretty_key:?}",
                verified_candidates.len(),
                n
            );
        }

        // Sort final candidates by distance to target (closest first)
        verified_candidates.sort_by_key(|(peer_id, _)| {
            let peer_addr = NetworkAddress::from(*peer_id);
            key.distance(&peer_addr)
        });

        debug!(
            "Final {} verified candidates sorted by distance to {pretty_key:?}",
            verified_candidates.len()
        );

        Ok(verified_candidates)
    }

    /// Get closest peers with majority knowledge verification and retries.
    ///
    /// This function wraps `get_closest_peers_with_majority_knowledge` with retry logic,
    /// attempting up to 2 times (initial attempt + 1 retry) with exponential backoff.
    ///
    /// Uses the same backoff strategy as the client-side retry functions:
    /// - First attempt is immediate
    /// - Second attempt waits 2 seconds after the first failure
    ///
    /// This is the recommended function for Merkle payment topology verification
    /// to handle transient network failures.
    pub(crate) async fn get_closest_peers_with_retries(
        &self,
        key: &NetworkAddress,
        n: Option<usize>,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        let min_wait = Duration::from_secs(CLOSEST_PEERS_RETRY_MIN_WAIT_SECS);
        let max_wait = Some(Duration::from_secs(CLOSEST_PEERS_RETRY_MAX_WAIT_SECS));
        let backoff = Backoff::new(CLOSEST_PEERS_RETRY_ATTEMPTS, min_wait, max_wait);

        for duration in backoff {
            match self.get_closest_peers_with_majority_knowledge(key, n).await {
                Ok(peers) => return Ok(peers),
                Err(err) => {
                    warn!(
                        "Get closest peers with majority knowledge failed for {key:?}: {err:?}, \
                         retrying in {duration:?}"
                    );
                    match duration {
                        Some(retry_delay) => sleep(retry_delay).await,
                        None => return Err(err),
                    }
                }
            }
        }

        // This should not be reached given the backoff configuration,
        // but handle it gracefully by returning the timeout error
        Err(NetworkError::GetClosestTimedOut)
    }

    /// Send a `Request` to the provided set of peers and wait for their responses concurrently.
    /// If `get_all_responses` is true, we wait for the responses from all the peers.
    /// If `get_all_responses` is false, we return the first successful response that we get
    pub(crate) async fn send_and_get_responses(
        &self,
        peers: &[(PeerId, Addresses)],
        req: &Request,
        get_all_responses: bool,
    ) -> BTreeMap<PeerId, Result<(Response, Option<ConnectionInfo>)>> {
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
                Ok(resp) => format!("{resp:?}"),
                Err(err) => format!("{err:?}"),
            };
            debug!("Got response from {peer:?} for the req: {req:?}, resp: {resp_string}");
            if !get_all_responses && resp.is_ok() {
                return BTreeMap::from([(peer, resp)]);
            }
            let _ = responses.insert(peer, resp);
            list_of_futures = remaining_futures;
        }

        debug!("Received all responses for {req:?}");
        responses
    }

    /// Get the estimated network density (i.e. the responsible_distance_range).
    pub(crate) async fn get_network_density(&self) -> Result<Option<KBucketDistance>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetNetworkDensity { sender });

        let density = receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;
        Ok(density)
    }

    /// Helper to send NetworkSwarmCmd
    fn send_network_swarm_cmd(&self, cmd: NetworkSwarmCmd) {
        let swarm_cmd_sender = self.inner.network_swarm_cmd_sender.clone();
        let capacity = swarm_cmd_sender.capacity();

        if capacity == 0 {
            error!(
                "SwarmCmd channel is full. Await capacity to send: {:?}",
                cmd
            );
        }

        // Spawn a task to send the SwarmCmd and keep this fn sync
        let _handle = tokio::spawn(async move {
            if let Err(error) = swarm_cmd_sender.send(cmd).await {
                error!("Failed to send SwarmCmd: {}", error);
            }
        });
    }

    /// Helper to send LocalSwarmCmd
    fn send_local_swarm_cmd(&self, cmd: LocalSwarmCmd) {
        let swarm_cmd_sender = self.inner.local_swarm_cmd_sender.clone();
        send_local_swarm_cmd(swarm_cmd_sender, cmd);
    }
}

pub(crate) fn send_local_swarm_cmd(
    swarm_cmd_sender: mpsc::Sender<LocalSwarmCmd>,
    cmd: LocalSwarmCmd,
) {
    let capacity = swarm_cmd_sender.capacity();

    if capacity == 0 {
        error!(
            "SwarmCmd channel is full. Await capacity to send: {:?}",
            cmd
        );
    }

    // Spawn a task to send the SwarmCmd and keep this fn sync
    let _handle = tokio::spawn(async move {
        if let Err(error) = swarm_cmd_sender.send(cmd).await {
            error!("Failed to send SwarmCmd: {}", error);
        }
    });
}

// A standard way to log connection id & the action performed on it.
pub(crate) fn connection_action_logging(
    remote_peer_id: &PeerId,
    self_peer_id: &PeerId,
    connection_id: &ConnectionId,
    action_string: &str,
) {
    // ELK logging. Do not update without proper testing.
    info!(
        "Action: {action_string}, performed on: {connection_id:?}, remote_peer_id: {remote_peer_id:?}, self_peer_id: {self_peer_id:?}"
    );
}
