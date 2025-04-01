// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ant_evm::{PaymentQuote, QuotingMetrics};
use ant_protocol::messages::{ConnectionInfo, Request, Response};
use ant_protocol::storage::ValidationType;
use ant_protocol::{NetworkAddress, PrettyPrintKBucketKey, PrettyPrintRecordKey};
use futures::future::select_all;
use libp2p::kad::{K_VALUE, KBucketDistance, Record, RecordKey};
use libp2p::swarm::ConnectionId;
use libp2p::{Multiaddr, PeerId, identity::Keypair};
use tokio::sync::{mpsc, oneshot};

use super::driver::event::MsgResponder;
use super::error::{NetworkError, Result};
use super::interface::{LocalSwarmCmd, NetworkSwarmCmd};
use super::{send_local_swarm_cmd, send_network_swarm_cmd, Addresses, NodeIssue, SwarmLocalState};

mod init;

pub(crate) use init::NetworkBuilder;

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
    pub(crate) fn new(
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
    pub(crate) fn peer_id(&self) -> PeerId {
        self.inner.peer_id
    }

    /// Returns the `Keypair` of the instance.
    pub(crate) fn keypair(&self) -> &Keypair {
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

    /// Returns a map where each key is the ilog2 distance of that Kbucket
    /// and each value is a vector of peers in that bucket.
    /// Does not include self
    pub(crate) async fn get_kbuckets(&self) -> Result<BTreeMap<u32, Vec<PeerId>>> {
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

    /// Get `Record` from the local RecordStore
    pub(crate) async fn get_local_record(&self, key: &RecordKey) -> Result<Option<Record>> {
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

    /// Helper to send NetworkSwarmCmd
    fn send_network_swarm_cmd(&self, cmd: NetworkSwarmCmd) {
        send_network_swarm_cmd(self.network_swarm_cmd_sender().clone(), cmd);
    }

    /// Helper to send LocalSwarmCmd
    fn send_local_swarm_cmd(&self, cmd: LocalSwarmCmd) {
        send_local_swarm_cmd(self.local_swarm_cmd_sender().clone(), cmd);
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

    /// Returns the `n` closest peers to the given `XorName`, sorted by their distance to the xor_name.
    #[allow(dead_code)]
    pub(crate) async fn get_n_closest_peers(
        &self,
        key: &NetworkAddress,
        n: usize,
    ) -> Result<Vec<(PeerId, Addresses)>> {
        assert!(n <= K_VALUE.get());

        let mut closest_peers = self.get_closest_peers(key).await?;

        // Check if we have enough results
        if closest_peers.len() < n {
            return Err(NetworkError::NotEnoughPeers {
                found: closest_peers.len(),
                required: n,
            });
        }

        // Only need the `n` closest peers
        closest_peers.truncate(n);

        Ok(closest_peers)
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

/// Helper function to print formatted connection role info.
pub(crate) fn endpoint_str(endpoint: &libp2p::core::ConnectedPoint) -> String {
    match endpoint {
        libp2p::core::ConnectedPoint::Dialer { address, .. } => {
            format!("outgoing ({address})")
        }
        libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } => {
            format!("incoming ({send_back_addr})")
        }
    }
}
