// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::large_enum_variant)]
#![allow(clippy::result_large_err)]

#[macro_use]
extern crate tracing;

mod bootstrap;
mod circular_vec;
mod cmd;
mod config;
mod driver;
mod error;
mod event;
mod log_markers;
#[cfg(feature = "open-metrics")]
mod metrics;
mod network_builder;
mod network_discovery;
mod record_store;
mod relay_manager;
mod replication_fetcher;
pub mod time;
mod transport;

use cmd::LocalSwarmCmd;

// re-export arch dependent deps for use in the crate, or above
pub use self::{
    cmd::{NodeIssue, SwarmLocalState},
    config::ResponseQuorum,
    driver::SwarmDriver,
    error::NetworkError,
    event::{MsgResponder, NetworkEvent},
    network_builder::{NetworkBuilder, MAX_PACKET_SIZE},
    record_store::NodeRecordStore,
};
#[cfg(feature = "open-metrics")]
pub use metrics::service::MetricsRegistries;
pub use time::{interval, sleep, spawn, Instant, Interval};

use self::{cmd::NetworkSwarmCmd, error::Result};
use ant_evm::{PaymentQuote, QuotingMetrics};
use ant_protocol::{
    messages::{ConnectionInfo, Request, Response},
    storage::ValidationType,
    NetworkAddress, PrettyPrintKBucketKey, PrettyPrintRecordKey, CLOSE_GROUP_SIZE,
};
use futures::future::select_all;
use libp2p::{
    identity::Keypair,
    kad::{KBucketDistance, KBucketKey, Record, RecordKey, K_VALUE},
    multiaddr::Protocol,
    request_response::OutboundFailure,
    Multiaddr, PeerId,
};
use std::{
    collections::{BTreeMap, HashMap},
    net::IpAddr,
    sync::Arc,
};
use tokio::sync::{
    mpsc::{self, Sender},
    oneshot,
};

/// Majority of a given group (i.e. > 1/2).
#[inline]
pub const fn close_group_majority() -> usize {
    // Calculate the majority of the close group size by dividing it by 2 and adding 1.
    // This ensures that the majority is always greater than half.
    CLOSE_GROUP_SIZE / 2 + 1
}

/// Sort the provided peers by their distance to the given `KBucketKey`.
/// Return with the closest expected number of entries it has.
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
        let addr = NetworkAddress::from(peer_id);
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

    /// Returns K closest local peers to the target.
    /// Target defaults to self, if not provided.
    /// Self is always included as the first entry.
    pub async fn get_k_closest_local_peers_to_the_target(
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
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;
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
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;

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
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;
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
    ) -> Result<(Response, Option<ConnectionInfo>)> {
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

                    // Default Addresses will be used for request sent to close range.
                    // For example: replication requests.
                    // In that case, we shall get the proper addrs from local then re-dial.
                    let dial_addrs = if addrs.0.is_empty() {
                        debug!("Input addrs of {peer:?} is empty, lookup from local");
                        let (sender, receiver) = oneshot::channel();

                        self.send_local_swarm_cmd(LocalSwarmCmd::GetPeersWithMultiaddr { sender });
                        let peers = receiver.await?;

                        let Some(new_addrs) = peers
                            .iter()
                            .find(|(id, _addrs)| *id == peer)
                            .map(|(_id, addrs)| addrs.clone())
                        else {
                            error!("Cann't find the addrs of peer {peer:?} from local, during the request reattempt of {req:?}.");
                            return r;
                        };
                        Addresses(new_addrs)
                    } else {
                        addrs.clone()
                    };

                    self.send_network_swarm_cmd(NetworkSwarmCmd::DialPeer {
                        peer,
                        addrs: dial_addrs.clone(),
                    });

                    // Short wait to allow connection re-established.
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                    let (sender, receiver) = oneshot::channel();
                    debug!("Reattempting to send_request {req_str} to {peer:?} by dialing the addrs manually.");
                    self.send_network_swarm_cmd(NetworkSwarmCmd::SendRequest {
                        req,
                        peer,
                        addrs: Some(dial_addrs),
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

    /// To be called when got a fresh record from client uploading.
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

    pub fn notify_peer_scores(&self, peer_scores: Vec<(PeerId, bool)>) {
        self.send_local_swarm_cmd(LocalSwarmCmd::NotifyPeerScores { peer_scores })
    }

    pub fn notify_node_version(&self, peer: PeerId, version: String) {
        self.send_local_swarm_cmd(LocalSwarmCmd::NotifyPeerVersion { peer, version })
    }

    pub fn remove_peer(&self, peer: PeerId) {
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
    pub async fn get_closest_peers(
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
    pub async fn get_n_closest_peers(
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
    pub async fn send_and_get_responses(
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
            responses.insert(peer, resp);
            list_of_futures = remaining_futures;
        }

        debug!("Received all responses for {req:?}");
        responses
    }

    /// Get the estimated network density (i.e. the responsible_distance_range).
    pub async fn get_network_density(&self) -> Result<Option<KBucketDistance>> {
        let (sender, receiver) = oneshot::channel();
        self.send_local_swarm_cmd(LocalSwarmCmd::GetNetworkDensity { sender });

        let density = receiver
            .await
            .map_err(|_e| NetworkError::InternalMsgChannelDropped)?;
        Ok(density)
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

/// Craft valid multiaddr like /ip4/68.183.39.80/udp/31055/quic-v1
/// RelayManager::craft_relay_address for relayed addr. This is for non-relayed addr.
pub(crate) fn craft_valid_multiaddr_without_p2p(addr: &Multiaddr) -> Option<Multiaddr> {
    let mut new_multiaddr = Multiaddr::empty();
    let ip = addr.iter().find_map(|p| match p {
        Protocol::Ip4(addr) => Some(addr),
        _ => None,
    })?;
    let port = multiaddr_get_port(addr)?;

    new_multiaddr.push(Protocol::Ip4(ip));
    new_multiaddr.push(Protocol::Udp(port));
    new_multiaddr.push(Protocol::QuicV1);

    Some(new_multiaddr)
}

/// Get the `IpAddr` from the `Multiaddr`
pub(crate) fn multiaddr_get_ip(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|p| match p {
        Protocol::Ip4(addr) => Some(IpAddr::V4(addr)),
        Protocol::Ip6(addr) => Some(IpAddr::V6(addr)),
        _ => None,
    })
}

#[allow(dead_code)]
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
