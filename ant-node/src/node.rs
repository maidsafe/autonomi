// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    error::Result, event::NodeEventsChannel, quote::quotes_verification, Marker, NodeEvent,
};
#[cfg(feature = "open-metrics")]
use crate::metrics::NodeMetricsRecorder;
use crate::RunningNode;
use ant_bootstrap::BootstrapCacheStore;
use ant_evm::EvmNetwork;
use ant_evm::RewardsAddress;
use ant_networking::Addresses;
#[cfg(feature = "open-metrics")]
use ant_networking::MetricsRegistries;
use ant_networking::{
    time::sleep, Instant, Network, NetworkBuilder, NetworkError, NetworkEvent, NodeIssue,
    SwarmDriver,
};
use ant_protocol::{
    error::Error as ProtocolError,
    messages::{ChunkProof, CmdResponse, Nonce, Query, QueryResponse, Request, Response},
    storage::ValidationType,
    NetworkAddress, PrettyPrintRecordKey, CLOSE_GROUP_SIZE,
};
use bytes::Bytes;
use itertools::Itertools;
use libp2p::{identity::Keypair, kad::U256, request_response::OutboundFailure, Multiaddr, PeerId};
use num_traits::cast::ToPrimitive;
use rand::{
    rngs::{OsRng, StdRng},
    thread_rng, Rng, SeedableRng,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::watch;
use tokio::{
    sync::mpsc::Receiver,
    task::{spawn, JoinSet},
};

/// Interval to trigger replication of all records to all peers.
/// This is the max time it should take. Minimum interval at any node will be half this
pub const PERIODIC_REPLICATION_INTERVAL_MAX_S: u64 = 180;

/// Interval to trigger storage challenge.
/// This is the max time it should take. Minimum interval at any node will be half this
const STORE_CHALLENGE_INTERVAL_MAX_S: u64 = 7200;

/// Interval to update the nodes uptime metric
const UPTIME_METRICS_UPDATE_INTERVAL: Duration = Duration::from_secs(10);

/// Interval to clean up unrelevant records
const UNRELEVANT_RECORDS_CLEANUP_INTERVAL: Duration = Duration::from_secs(3600);

/// Highest score to achieve from each metric sub-sector during StorageChallenge.
const HIGHEST_SCORE: usize = 100;

/// Any nodes bearing a score below this shall be considered as bad.
/// Max is to be 100 * 100
const MIN_ACCEPTABLE_HEALTHY_SCORE: usize = 3000;

/// in ms, expecting average StorageChallenge complete time to be around 250ms.
const TIME_STEP: usize = 20;

/// Interval to carryout network density sampling
/// This is the max time it should take. Minimum interval at any node will be half this
const NETWORK_DENSITY_SAMPLING_INTERVAL_MAX_S: u64 = 200;

/// Helper to build and run a Node
pub struct NodeBuilder {
    bootstrap_cache: Option<BootstrapCacheStore>,
    initial_peers: Vec<Multiaddr>,
    identity_keypair: Keypair,
    evm_address: RewardsAddress,
    evm_network: EvmNetwork,
    addr: SocketAddr,
    local: bool,
    root_dir: PathBuf,
    #[cfg(feature = "open-metrics")]
    /// Set to Some to enable the metrics server
    metrics_server_port: Option<u16>,
    /// Enable hole punching for nodes connecting from home networks.
    is_behind_home_network: bool,
    upnp: bool,
}

impl NodeBuilder {
    /// Instantiate the builder. The initial peers can either be supplied via the `initial_peers` method
    /// or fetched from the bootstrap cache set using `bootstrap_cache` method.
    pub fn new(
        identity_keypair: Keypair,
        initial_peers: Vec<Multiaddr>,
        evm_address: RewardsAddress,
        evm_network: EvmNetwork,
        addr: SocketAddr,
        root_dir: PathBuf,
    ) -> Self {
        Self {
            bootstrap_cache: None,
            initial_peers,
            identity_keypair,
            evm_address,
            evm_network,
            addr,
            local: false,
            root_dir,
            #[cfg(feature = "open-metrics")]
            metrics_server_port: None,
            is_behind_home_network: false,
            upnp: false,
        }
    }

    /// Set the flag to indicate if the node is running in local mode
    pub fn local(&mut self, local: bool) {
        self.local = local;
    }

    #[cfg(feature = "open-metrics")]
    /// Set the port for the OpenMetrics server. Defaults to a random port if not set
    pub fn metrics_server_port(&mut self, port: Option<u16>) {
        self.metrics_server_port = port;
    }

    /// Set the initialized bootstrap cache.
    pub fn bootstrap_cache(&mut self, cache: BootstrapCacheStore) {
        self.bootstrap_cache = Some(cache);
    }

    /// Set the flag to indicate if the node is behind a home network
    pub fn is_behind_home_network(&mut self, is_behind_home_network: bool) {
        self.is_behind_home_network = is_behind_home_network;
    }

    /// Set the flag to enable UPnP for the node
    pub fn upnp(&mut self, upnp: bool) {
        self.upnp = upnp;
    }

    /// Asynchronously runs a new node instance, setting up the swarm driver,
    /// creating a data storage, and handling network events. Returns the
    /// created `RunningNode` which contains a `NodeEventsChannel` for listening
    /// to node-related events.
    ///
    /// # Returns
    ///
    /// A `RunningNode` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if there is a problem initializing the `SwarmDriver`.
    pub fn build_and_run(self) -> Result<RunningNode> {
        let mut network_builder =
            NetworkBuilder::new(self.identity_keypair, self.local, self.initial_peers);

        #[cfg(feature = "open-metrics")]
        let metrics_recorder = if self.metrics_server_port.is_some() {
            // metadata registry
            let mut metrics_registries = MetricsRegistries::default();
            let metrics_recorder = NodeMetricsRecorder::new(&mut metrics_registries);

            network_builder.metrics_registries(metrics_registries);

            Some(metrics_recorder)
        } else {
            None
        };

        network_builder.listen_addr(self.addr);
        #[cfg(feature = "open-metrics")]
        network_builder.metrics_server_port(self.metrics_server_port);
        network_builder.is_behind_home_network(self.is_behind_home_network);
        if let Some(cache) = self.bootstrap_cache {
            network_builder.bootstrap_cache(cache);
        }

        network_builder.upnp(self.upnp);

        let (network, network_event_receiver, swarm_driver) =
            network_builder.build_node(self.root_dir.clone())?;

        let node_events_channel = NodeEventsChannel::default();

        let node = NodeInner {
            network: network.clone(),
            events_channel: node_events_channel.clone(),
            reward_address: self.evm_address,
            #[cfg(feature = "open-metrics")]
            metrics_recorder,
            evm_network: self.evm_network,
        };

        let node = Node {
            inner: Arc::new(node),
        };

        // Create a shutdown signal channel
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Run the node
        node.run(swarm_driver, network_event_receiver, shutdown_rx);

        let running_node = RunningNode {
            shutdown_sender: shutdown_tx,
            network,
            node_events_channel,
            root_dir_path: self.root_dir,
            rewards_address: self.evm_address,
        };

        Ok(running_node)
    }
}

/// `Node` represents a single node in the distributed network. It handles
/// network events, processes incoming requests, interacts with the data
/// storage, and broadcasts node-related events.
#[derive(Clone)]
pub(crate) struct Node {
    inner: Arc<NodeInner>,
}

/// The actual implementation of the Node. The other is just a wrapper around this, so that we don't expose
/// the Arc from the interface.
struct NodeInner {
    events_channel: NodeEventsChannel,
    network: Network,
    #[cfg(feature = "open-metrics")]
    metrics_recorder: Option<NodeMetricsRecorder>,
    reward_address: RewardsAddress,
    evm_network: EvmNetwork,
}

impl Node {
    /// Returns the NodeEventsChannel
    pub(crate) fn events_channel(&self) -> &NodeEventsChannel {
        &self.inner.events_channel
    }

    /// Returns the instance of Network
    pub(crate) fn network(&self) -> &Network {
        &self.inner.network
    }

    #[cfg(feature = "open-metrics")]
    /// Returns a reference to the NodeMetricsRecorder if the `open-metrics` feature flag is enabled
    /// This is used to record various metrics for the node.
    pub(crate) fn metrics_recorder(&self) -> Option<&NodeMetricsRecorder> {
        self.inner.metrics_recorder.as_ref()
    }

    /// Returns the reward address of the node
    pub(crate) fn reward_address(&self) -> &RewardsAddress {
        &self.inner.reward_address
    }

    pub(crate) fn evm_network(&self) -> &EvmNetwork {
        &self.inner.evm_network
    }

    /// Runs a task for the provided `SwarmDriver` and spawns a task to process for `NetworkEvents`.
    /// Returns both tasks as JoinHandle<()>.
    fn run(
        self,
        swarm_driver: SwarmDriver,
        mut network_event_receiver: Receiver<NetworkEvent>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let mut rng = StdRng::from_entropy();

        let peers_connected = Arc::new(AtomicUsize::new(0));

        let _swarm_driver_task = spawn(swarm_driver.run(shutdown_rx.clone()));
        let _node_task = spawn(async move {
            // use a random inactivity timeout to ensure that the nodes do not sync when messages
            // are being transmitted.
            let replication_interval: u64 = rng.gen_range(
                PERIODIC_REPLICATION_INTERVAL_MAX_S / 2..PERIODIC_REPLICATION_INTERVAL_MAX_S,
            );
            let replication_interval_time = Duration::from_secs(replication_interval);
            debug!("Replication interval set to {replication_interval_time:?}");

            let mut replication_interval = tokio::time::interval(replication_interval_time);
            let _ = replication_interval.tick().await; // first tick completes immediately

            let mut uptime_metrics_update_interval =
                tokio::time::interval(UPTIME_METRICS_UPDATE_INTERVAL);
            let _ = uptime_metrics_update_interval.tick().await; // first tick completes immediately

            let mut irrelevant_records_cleanup_interval =
                tokio::time::interval(UNRELEVANT_RECORDS_CLEANUP_INTERVAL);
            let _ = irrelevant_records_cleanup_interval.tick().await; // first tick completes immediately

            // use a random neighbour storage challenge ticker to ensure
            // neighbours do not carryout challenges at the same time
            let storage_challenge_interval: u64 =
                rng.gen_range(STORE_CHALLENGE_INTERVAL_MAX_S / 2..STORE_CHALLENGE_INTERVAL_MAX_S);
            let storage_challenge_interval_time = Duration::from_secs(storage_challenge_interval);
            debug!("Storage challenge interval set to {storage_challenge_interval_time:?}");

            let mut storage_challenge_interval =
                tokio::time::interval(storage_challenge_interval_time);
            let _ = storage_challenge_interval.tick().await; // first tick completes immediately

            // use a random network density sampling ticker to ensure
            // neighbours do not carryout sampling at the same time
            let network_density_sampling_interval: u64 = rng.gen_range(
                NETWORK_DENSITY_SAMPLING_INTERVAL_MAX_S / 2
                    ..NETWORK_DENSITY_SAMPLING_INTERVAL_MAX_S,
            );
            let network_density_sampling_interval_time =
                Duration::from_secs(network_density_sampling_interval);
            debug!(
                "Network density sampling interval set to {network_density_sampling_interval:?}"
            );

            let mut network_density_sampling_interval =
                tokio::time::interval(network_density_sampling_interval_time);
            let _ = network_density_sampling_interval.tick().await; // first tick completes immediately

            loop {
                let peers_connected = &peers_connected;

                tokio::select! {
                    // Check for a shutdown command.
                    result = shutdown_rx.changed() => {
                        if result.is_ok() && *shutdown_rx.borrow() || result.is_err() {
                            info!("Shutdown signal received or sender dropped. Exiting network events loop.");
                            break;
                        }
                    },
                    net_event = network_event_receiver.recv() => {
                        match net_event {
                            Some(event) => {
                                let start = Instant::now();
                                let event_string = format!("{event:?}");

                                self.handle_network_event(event, peers_connected);
                                trace!("Handled non-blocking network event in {:?}: {:?}", start.elapsed(), event_string);

                            }
                            None => {
                                error!("The `NetworkEvent` channel is closed");
                                self.events_channel().broadcast(NodeEvent::ChannelClosed);
                                break;
                            }
                        }
                    }
                    // runs every replication_interval time
                    _ = replication_interval.tick() => {
                        let start = Instant::now();
                        let network = self.network().clone();
                        self.record_metrics(Marker::IntervalReplicationTriggered);

                        let _handle = spawn(async move {
                            Self::try_interval_replication(network);
                            trace!("Periodic replication took {:?}", start.elapsed());
                        });
                    }
                    _ = uptime_metrics_update_interval.tick() => {
                        #[cfg(feature = "open-metrics")]
                        if let Some(metrics_recorder) = self.metrics_recorder() {
                            let _ = metrics_recorder.uptime.set(metrics_recorder.started_instant.elapsed().as_secs() as i64);
                        }
                    }
                    _ = irrelevant_records_cleanup_interval.tick() => {
                        let network = self.network().clone();

                        let _handle = spawn(async move {
                            Self::trigger_irrelevant_record_cleanup(network);
                        });
                    }
                    // runs every storage_challenge_interval time
                    _ = storage_challenge_interval.tick() => {
                        let start = Instant::now();
                        debug!("Periodic storage challenge triggered");
                        let network = self.network().clone();

                        let _handle = spawn(async move {
                            Self::storage_challenge(network).await;
                            trace!("Periodic storage challenge took {:?}", start.elapsed());
                        });
                    }
                    _ = network_density_sampling_interval.tick() => {
                        // The following shall be used by client only to support RBS.
                        // Due to the concern of the extra resource usage that incurred.
                        continue;

                        // let start = Instant::now();
                        // debug!("Periodic network density sampling triggered");
                        // let network = self.network().clone();

                        // let _handle = spawn(async move {
                        //     Self::network_density_sampling(network).await;
                        //     trace!("Periodic network density sampling took {:?}", start.elapsed());
                        // });
                    }
                }
            }
        });
    }

    /// Calls Marker::log() to insert the marker into the log files.
    /// Also calls NodeMetrics::record() to record the metric if the `open-metrics` feature flag is enabled.
    pub(crate) fn record_metrics(&self, marker: Marker) {
        marker.log();
        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = self.metrics_recorder() {
            metrics_recorder.record(marker)
        }
    }

    // **** Private helpers *****

    /// Handle a network event.
    /// Spawns a thread for any likely long running tasks
    fn handle_network_event(&self, event: NetworkEvent, peers_connected: &Arc<AtomicUsize>) {
        let start = Instant::now();
        let event_string = format!("{event:?}");
        let event_header;
        debug!("Handling NetworkEvent {event_string:?}");

        match event {
            NetworkEvent::PeerAdded(peer_id, connected_peers) => {
                event_header = "PeerAdded";
                // increment peers_connected and send ConnectedToNetwork event if have connected to K_VALUE peers
                let _ = peers_connected.fetch_add(1, Ordering::SeqCst);
                if peers_connected.load(Ordering::SeqCst) == CLOSE_GROUP_SIZE {
                    self.events_channel()
                        .broadcast(NodeEvent::ConnectedToNetwork);
                }

                self.record_metrics(Marker::PeersInRoutingTable(connected_peers));
                self.record_metrics(Marker::PeerAddedToRoutingTable(&peer_id));

                // try query peer version
                let network = self.network().clone();
                let _handle = spawn(async move {
                    Self::try_query_peer_version(network, peer_id, Default::default()).await;
                });

                // try replication here
                let network = self.network().clone();
                self.record_metrics(Marker::IntervalReplicationTriggered);
                let _handle = spawn(async move {
                    Self::try_interval_replication(network);
                });
            }
            NetworkEvent::PeerRemoved(peer_id, connected_peers) => {
                event_header = "PeerRemoved";
                self.record_metrics(Marker::PeersInRoutingTable(connected_peers));
                self.record_metrics(Marker::PeerRemovedFromRoutingTable(&peer_id));

                let self_id = self.network().peer_id();
                let distance =
                    NetworkAddress::from(self_id).distance(&NetworkAddress::from(peer_id));
                info!("Node {self_id:?} removed peer from routing table: {peer_id:?}. It has a {:?} distance to us.", distance.ilog2());

                let network = self.network().clone();
                self.record_metrics(Marker::IntervalReplicationTriggered);
                let _handle = spawn(async move {
                    Self::try_interval_replication(network);
                });
            }
            NetworkEvent::PeerWithUnsupportedProtocol { .. } => {
                event_header = "PeerWithUnsupportedProtocol";
            }
            NetworkEvent::NewListenAddr(_) => {
                event_header = "NewListenAddr";
            }
            NetworkEvent::ResponseReceived { res } => {
                event_header = "ResponseReceived";
                if let Err(err) = self.handle_response(res) {
                    error!("Error while handling NetworkEvent::ResponseReceived {err:?}");
                }
            }
            NetworkEvent::KeysToFetchForReplication(keys) => {
                event_header = "KeysToFetchForReplication";
                self.record_metrics(Marker::fetching_keys_for_replication(&keys));

                if let Err(err) = self.fetch_replication_keys_without_wait(keys) {
                    error!("Error while trying to fetch replicated data {err:?}");
                }
            }
            NetworkEvent::QueryRequestReceived { query, channel } => {
                event_header = "QueryRequestReceived";
                let network = self.network().clone();
                let payment_address = *self.reward_address();

                let _handle = spawn(async move {
                    let res = Self::handle_query(&network, query, payment_address).await;
                    debug!("Sending response {res:?}");

                    network.send_response(res, channel);
                });
            }
            NetworkEvent::UnverifiedRecord(record) => {
                event_header = "UnverifiedRecord";
                // queries can be long running and require validation, so we spawn a task to handle them
                let self_clone = self.clone();
                let _handle = spawn(async move {
                    let key = PrettyPrintRecordKey::from(&record.key).into_owned();
                    match self_clone.validate_and_store_record(record).await {
                        Ok(()) => debug!("UnverifiedRecord {key} has been stored"),
                        Err(err) => {
                            self_clone.record_metrics(Marker::RecordRejected(&key, &err));
                        }
                    }
                });
            }
            NetworkEvent::TerminateNode { reason } => {
                event_header = "TerminateNode";
                error!("Received termination from swarm_driver due to {reason:?}");
                self.events_channel()
                    .broadcast(NodeEvent::TerminateNode(format!("{reason}")));
            }
            NetworkEvent::FailedToFetchHolders(bad_nodes) => {
                event_header = "FailedToFetchHolders";
                let network = self.network().clone();
                let pretty_log: Vec<_> = bad_nodes
                    .iter()
                    .map(|(peer_id, record_key)| {
                        let pretty_key = PrettyPrintRecordKey::from(record_key);
                        (peer_id, pretty_key)
                    })
                    .collect();
                // Note: this log will be checked in CI, and expecting `not appear`.
                //       any change to the keyword `failed to fetch` shall incur
                //       correspondent CI script change as well.
                debug!("Received notification from replication_fetcher, notifying {pretty_log:?} failed to fetch replication copies from.");
                let _handle = spawn(async move {
                    for (peer_id, record_key) in bad_nodes {
                        // Obsoleted fetch request (due to flooded in fresh replicates) could result
                        // in peer to be claimed as bad, as local copy blocks the entry to be cleared.
                        if let Ok(false) = network.is_record_key_present_locally(&record_key).await
                        {
                            error!(
                                "From peer {peer_id:?}, failed to fetch record {:?}",
                                PrettyPrintRecordKey::from(&record_key)
                            );
                            network.record_node_issues(peer_id, NodeIssue::ReplicationFailure);
                        }
                    }
                });
            }
            NetworkEvent::QuoteVerification { quotes } => {
                event_header = "QuoteVerification";
                let network = self.network().clone();

                let _handle = spawn(async move {
                    quotes_verification(&network, quotes).await;
                });
            }
            NetworkEvent::FreshReplicateToFetch { holder, keys } => {
                event_header = "FreshReplicateToFetch";
                self.fresh_replicate_to_fetch(holder, keys);
            }
            NetworkEvent::PeersForVersionQuery(peers) => {
                event_header = "PeersForVersionQuery";
                let network = self.network().clone();
                let _handle = spawn(async move {
                    Self::query_peers_version(network, peers).await;
                });
            }
        }

        trace!(
            "Network handling statistics, Event {event_header:?} handled in {:?} : {event_string:?}",
            start.elapsed()
        );
    }

    // Handle the response that was not awaited at the call site
    fn handle_response(&self, response: Response) -> Result<()> {
        match response {
            Response::Cmd(CmdResponse::Replicate(Ok(()))) => {
                // This should actually have been short-circuted when received
                warn!("Mishandled replicate response, should be handled earlier");
            }
            Response::Query(QueryResponse::GetReplicatedRecord(resp)) => {
                error!("Response to replication shall be handled by called not by common handler, {resp:?}");
            }
            Response::Cmd(CmdResponse::FreshReplicate(Ok(()))) => {
                // No need to handle
            }
            other => {
                warn!("handle_response not implemented for {other:?}");
            }
        };

        Ok(())
    }

    async fn handle_query(
        network: &Network,
        query: Query,
        payment_address: RewardsAddress,
    ) -> Response {
        let resp: QueryResponse = match query {
            Query::GetStoreQuote {
                key,
                data_type,
                data_size,
                nonce,
                difficulty,
            } => {
                let record_key = key.to_record_key();
                let self_id = network.peer_id();

                let maybe_quoting_metrics = network
                    .get_local_quoting_metrics(record_key.clone(), data_type, data_size)
                    .await;

                let storage_proofs = if let Some(nonce) = nonce {
                    Self::respond_x_closest_record_proof(
                        network,
                        key.clone(),
                        nonce,
                        difficulty,
                        false,
                    )
                    .await
                } else {
                    vec![]
                };

                match maybe_quoting_metrics {
                    Ok((quoting_metrics, is_already_stored)) => {
                        if is_already_stored {
                            QueryResponse::GetStoreQuote {
                                quote: Err(ProtocolError::RecordExists(
                                    PrettyPrintRecordKey::from(&record_key).into_owned(),
                                )),
                                peer_address: NetworkAddress::from(self_id),
                                storage_proofs,
                            }
                        } else {
                            QueryResponse::GetStoreQuote {
                                quote: Self::create_quote_for_storecost(
                                    network,
                                    &key,
                                    &quoting_metrics,
                                    &payment_address,
                                ),
                                peer_address: NetworkAddress::from(self_id),
                                storage_proofs,
                            }
                        }
                    }
                    Err(err) => {
                        warn!("GetStoreQuote failed for {key:?}: {err}");
                        QueryResponse::GetStoreQuote {
                            quote: Err(ProtocolError::GetStoreQuoteFailed),
                            peer_address: NetworkAddress::from(self_id),
                            storage_proofs,
                        }
                    }
                }
            }
            Query::GetReplicatedRecord { requester: _, key } => {
                let our_address = NetworkAddress::from(network.peer_id());
                let mut result = Err(ProtocolError::ReplicatedRecordNotFound {
                    holder: Box::new(our_address.clone()),
                    key: Box::new(key.clone()),
                });
                let record_key = key.as_record_key();

                if let Some(record_key) = record_key {
                    if let Ok(Some(record)) = network.get_local_record(&record_key).await {
                        result = Ok((our_address, Bytes::from(record.value)));
                    }
                }

                QueryResponse::GetReplicatedRecord(result)
            }
            Query::GetChunkExistenceProof {
                key,
                nonce,
                difficulty,
            } => QueryResponse::GetChunkExistenceProof(
                Self::respond_x_closest_record_proof(network, key, nonce, difficulty, true).await,
            ),
            Query::CheckNodeInProblem(target_address) => {
                debug!("Got CheckNodeInProblem for peer {target_address:?}");

                let is_in_trouble =
                    if let Ok(result) = network.is_peer_shunned(target_address.clone()).await {
                        result
                    } else {
                        debug!("Could not get status of {target_address:?}.");
                        false
                    };

                QueryResponse::CheckNodeInProblem {
                    reporter_address: NetworkAddress::from(network.peer_id()),
                    target_address,
                    is_in_trouble,
                }
            }
            Query::GetClosestPeers {
                key,
                num_of_peers,
                range,
                sign_result,
            } => {
                debug!(
                    "Got GetClosestPeers targeting {key:?} with {num_of_peers:?} peers or {range:?} range, signature {sign_result} required."
                );
                Self::respond_get_closest_peers(network, key, num_of_peers, range, sign_result)
                    .await
            }
            Query::GetVersion(_) => QueryResponse::GetVersion {
                peer: NetworkAddress::from(network.peer_id()),
                version: ant_build_info::package_version(),
            },
        };
        Response::Query(resp)
    }

    async fn respond_get_closest_peers(
        network: &Network,
        target: NetworkAddress,
        num_of_peers: Option<usize>,
        range: Option<[u8; 32]>,
        sign_result: bool,
    ) -> QueryResponse {
        let local_peers = network.get_local_peers_with_multiaddr().await;
        let peers: Vec<(NetworkAddress, Vec<Multiaddr>)> = if let Ok(local_peers) = local_peers {
            Self::calculate_get_closest_peers(local_peers, target.clone(), num_of_peers, range)
        } else {
            vec![]
        };

        let signature = if sign_result {
            let mut bytes = rmp_serde::to_vec(&target).unwrap_or_default();
            bytes.extend_from_slice(&rmp_serde::to_vec(&peers).unwrap_or_default());
            network.sign(&bytes).ok()
        } else {
            None
        };

        QueryResponse::GetClosestPeers {
            target,
            peers,
            signature,
        }
    }

    fn calculate_get_closest_peers(
        peer_addrs: Vec<(PeerId, Vec<Multiaddr>)>,
        target: NetworkAddress,
        num_of_peers: Option<usize>,
        range: Option<[u8; 32]>,
    ) -> Vec<(NetworkAddress, Vec<Multiaddr>)> {
        match (num_of_peers, range) {
            (_, Some(value)) => {
                let distance = U256::from_big_endian(&value);
                peer_addrs
                    .iter()
                    .filter_map(|(peer_id, multi_addrs)| {
                        let addr = NetworkAddress::from(*peer_id);
                        if target.distance(&addr).0 <= distance {
                            Some((addr, multi_addrs.clone()))
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            (Some(num_of_peers), _) => {
                let mut result: Vec<(NetworkAddress, Vec<Multiaddr>)> = peer_addrs
                    .iter()
                    .map(|(peer_id, multi_addrs)| {
                        let addr = NetworkAddress::from(*peer_id);
                        (addr, multi_addrs.clone())
                    })
                    .collect();
                result.sort_by_key(|(addr, _multi_addrs)| target.distance(addr));
                result.into_iter().take(num_of_peers).collect()
            }
            (None, None) => vec![],
        }
    }

    // Nodes only check ChunkProof each other, to avoid `multi-version` issue
    // Client check proof against all records, as have to fetch from network anyway.
    async fn respond_x_closest_record_proof(
        network: &Network,
        key: NetworkAddress,
        nonce: Nonce,
        difficulty: usize,
        chunk_only: bool,
    ) -> Vec<(NetworkAddress, Result<ChunkProof, ProtocolError>)> {
        let start = Instant::now();
        let mut results = vec![];
        if difficulty == 1 {
            // Client checking existence of published chunk.
            let mut result = Err(ProtocolError::ChunkDoesNotExist(key.clone()));
            if let Ok(Some(record)) = network.get_local_record(&key.to_record_key()).await {
                let proof = ChunkProof::new(&record.value, nonce);
                debug!("Chunk proof for {key:?} is {proof:?}");
                result = Ok(proof)
            } else {
                debug!("Could not get ChunkProof for {key:?} as we don't have the record locally.");
            }

            results.push((key.clone(), result));
        } else {
            let all_local_records = network.get_all_local_record_addresses().await;

            if let Ok(all_local_records) = all_local_records {
                let mut all_chunk_addrs: Vec<_> = if chunk_only {
                    all_local_records
                        .iter()
                        .filter_map(|(addr, record_type)| {
                            if *record_type == ValidationType::Chunk {
                                Some(addr.clone())
                            } else {
                                None
                            }
                        })
                        .collect()
                } else {
                    all_local_records.keys().cloned().collect()
                };

                // Sort by distance and only take first X closest entries
                all_chunk_addrs.sort_by_key(|addr| key.distance(addr));

                // TODO: this shall be deduced from resource usage dynamically
                let workload_factor = std::cmp::min(difficulty, CLOSE_GROUP_SIZE);

                for addr in all_chunk_addrs.iter().take(workload_factor) {
                    if let Ok(Some(record)) = network.get_local_record(&addr.to_record_key()).await
                    {
                        let proof = ChunkProof::new(&record.value, nonce);
                        debug!("Chunk proof for {key:?} is {proof:?}");
                        results.push((addr.clone(), Ok(proof)));
                    }
                }
            }

            info!(
                "Respond with {} answers to the StorageChallenge targeting {key:?} with {difficulty} difficulty, in {:?}",
                results.len(), start.elapsed()
            );
        }

        results
    }

    /// Check among all chunk type records that we have,
    /// and randomly pick one as the verification candidate.
    /// This will challenge all closest peers at once.
    async fn storage_challenge(network: Network) {
        let start = Instant::now();
        let closest_peers: Vec<(PeerId, Addresses)> =
            if let Ok(closest_peers) = network.get_closest_k_value_local_peers().await {
                closest_peers
                    .into_iter()
                    .take(CLOSE_GROUP_SIZE)
                    .collect_vec()
            } else {
                error!("Cannot get local neighbours");
                return;
            };
        if closest_peers.len() < CLOSE_GROUP_SIZE {
            debug!(
                "Not enough neighbours ({}/{}) to carry out storage challenge.",
                closest_peers.len(),
                CLOSE_GROUP_SIZE
            );
            return;
        }

        let mut verify_candidates: Vec<NetworkAddress> =
            if let Ok(all_keys) = network.get_all_local_record_addresses().await {
                all_keys
                    .iter()
                    .filter_map(|(addr, record_type)| {
                        if ValidationType::Chunk == *record_type {
                            Some(addr.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                error!("Failed to get local record addresses.");
                return;
            };
        let num_of_targets = verify_candidates.len();
        if num_of_targets < 50 {
            debug!("Not enough candidates({num_of_targets}/50) to be checked against neighbours.");
            return;
        }

        // To ensure the neighbours sharing same knowledge as to us,
        // The target is choosen to be not far from us.
        let self_addr = NetworkAddress::from(network.peer_id());
        verify_candidates.sort_by_key(|addr| self_addr.distance(addr));
        let index: usize = OsRng.gen_range(0..num_of_targets / 2);
        let target = verify_candidates[index].clone();
        // TODO: workload shall be dynamically deduced from resource usage
        let difficulty = CLOSE_GROUP_SIZE;
        verify_candidates.sort_by_key(|addr| target.distance(addr));
        let expected_targets = verify_candidates.into_iter().take(difficulty);
        let nonce: Nonce = thread_rng().gen::<u64>();
        let mut expected_proofs = HashMap::new();
        for addr in expected_targets {
            if let Ok(Some(record)) = network.get_local_record(&addr.to_record_key()).await {
                let expected_proof = ChunkProof::new(&record.value, nonce);
                let _ = expected_proofs.insert(addr, expected_proof);
            } else {
                error!("Local record {addr:?} cann't be loaded from disk.");
            }
        }
        let request = Request::Query(Query::GetChunkExistenceProof {
            key: target.clone(),
            nonce,
            difficulty,
        });

        let mut tasks = JoinSet::new();
        for (peer_id, addresses) in closest_peers {
            if peer_id == network.peer_id() {
                continue;
            }
            let network_clone = network.clone();
            let request_clone = request.clone();
            let expected_proofs_clone = expected_proofs.clone();
            let _ = tasks.spawn(async move {
                let res = scoring_peer(
                    network_clone,
                    (peer_id, addresses),
                    request_clone,
                    expected_proofs_clone,
                )
                .await;
                (peer_id, res)
            });
        }

        let mut peer_scores = vec![];
        while let Some(res) = tasks.join_next().await {
            match res {
                Ok((peer_id, score)) => {
                    let is_healthy = score > MIN_ACCEPTABLE_HEALTHY_SCORE;
                    if !is_healthy {
                        info!("Peer {peer_id:?} failed storage challenge with low score {score}/{MIN_ACCEPTABLE_HEALTHY_SCORE}.");
                        // TODO: shall the challenge failure immediately triggers the node to be removed?
                        network.record_node_issues(peer_id, NodeIssue::FailedChunkProofCheck);
                    }
                    peer_scores.push((peer_id, is_healthy));
                }
                Err(e) => {
                    info!("StorageChallenge task completed with error {e:?}");
                }
            }
        }
        if !peer_scores.is_empty() {
            network.notify_peer_scores(peer_scores);
        }

        info!(
            "Completed node StorageChallenge against neighbours in {:?}!",
            start.elapsed()
        );
    }

    /// Query peers' versions and update local knowledge.
    async fn query_peers_version(network: Network, peers: Vec<(PeerId, Addresses)>) {
        // To avoid choking, carry out the queries one by one
        for (peer_id, addrs) in peers {
            Self::try_query_peer_version(network.clone(), peer_id, addrs).await;
        }
    }

    /// Query peer's version and update local knowledge.
    async fn try_query_peer_version(network: Network, peer: PeerId, addrs: Addresses) {
        let request = Request::Query(Query::GetVersion(NetworkAddress::from(peer)));
        // We can skip passing `addrs` here as the new peer should be part of the kad::RT and swarm can get the addr.
        let version = match network.send_request(request, peer, addrs).await {
            Ok((Response::Query(QueryResponse::GetVersion { version, .. }), _conn_info)) => {
                info!("Fetched peer version {peer:?} as {version:?}");
                version
            }
            Ok(other) => {
                info!("Not a fetched peer version {peer:?}, {other:?}");
                "none".to_string()
            }
            Err(err) => {
                info!("Failed to fetch peer version {peer:?} with error {err:?}");
                // Failed version fetch (which contains dial then re-attempt by itself)
                // with error of `DialFailure` indicates the peer could be dead with high chance.
                // In that case, the peer shall be removed from the routing table.
                if let NetworkError::OutboundError(OutboundFailure::DialFailure) = err {
                    network.remove_peer(peer);
                    return;
                }
                "old".to_string()
            }
        };
        network.notify_node_version(peer, version);
    }

    #[allow(dead_code)]
    async fn network_density_sampling(network: Network) {
        for _ in 0..10 {
            let target = NetworkAddress::from(PeerId::random());
            // Result is sorted and only return CLOSE_GROUP_SIZE entries
            let peers = network.get_n_closest_peers(&target, CLOSE_GROUP_SIZE).await;
            if let Ok(peers) = peers {
                if peers.len() >= CLOSE_GROUP_SIZE {
                    // Calculate the distance to the farthest.
                    let distance =
                        target.distance(&NetworkAddress::from(peers[CLOSE_GROUP_SIZE - 1].0));
                    network.add_network_density_sample(distance);
                }
            }
            // Sleep a short while to avoid causing a spike on resource usage.
            sleep(std::time::Duration::from_secs(10)).await;
        }
    }
}

async fn scoring_peer(
    network: Network,
    peer: (PeerId, Addresses),
    request: Request,
    expected_proofs: HashMap<NetworkAddress, ChunkProof>,
) -> usize {
    let peer_id = peer.0;
    let start = Instant::now();
    let responses = network
        .send_and_get_responses(&[peer], &request, true)
        .await;

    if let Some(Ok((Response::Query(QueryResponse::GetChunkExistenceProof(answers)), _conn_info))) =
        responses.get(&peer_id)
    {
        if answers.is_empty() {
            info!("Peer {peer_id:?} didn't answer the ChunkProofChallenge.");
            return 0;
        }
        let elapsed = start.elapsed();

        let mut received_proofs = vec![];
        for (addr, proof) in answers {
            if let Ok(proof) = proof {
                received_proofs.push((addr.clone(), proof.clone()));
            }
        }

        let score = mark_peer(elapsed, received_proofs, &expected_proofs);
        info!(
            "Received {} answers from peer {peer_id:?} after {elapsed:?}, score it as {score}.",
            answers.len()
        );
        score
    } else {
        info!("Peer {peer_id:?} doesn't reply the ChunkProofChallenge, or replied with error.");
        0
    }
}

// Based on following metrics:
//   * the duration
//   * is there false answer
//   * percentage of correct answers among the expected closest
// The higher the score, the better confidence on the peer
fn mark_peer(
    duration: Duration,
    answers: Vec<(NetworkAddress, ChunkProof)>,
    expected_proofs: &HashMap<NetworkAddress, ChunkProof>,
) -> usize {
    let duration_score = duration_score_scheme(duration);
    let challenge_score = challenge_score_scheme(answers, expected_proofs);

    duration_score * challenge_score
}

// Less duration shall get higher score
fn duration_score_scheme(duration: Duration) -> usize {
    // So far just a simple stepped scheme, capped by HIGHEST_SCORE
    let in_ms = if let Some(value) = duration.as_millis().to_usize() {
        value
    } else {
        info!("Cannot get milli seconds from {duration:?}, using a default value of 1000ms.");
        1000
    };

    let step = std::cmp::min(HIGHEST_SCORE, in_ms / TIME_STEP);
    HIGHEST_SCORE - step
}

// Any false answer shall result in 0 score immediately
fn challenge_score_scheme(
    answers: Vec<(NetworkAddress, ChunkProof)>,
    expected_proofs: &HashMap<NetworkAddress, ChunkProof>,
) -> usize {
    let mut correct_answers = 0;
    for (addr, chunk_proof) in answers {
        if let Some(expected_proof) = expected_proofs.get(&addr) {
            if expected_proof.verify(&chunk_proof) {
                correct_answers += 1;
            } else {
                info!("Spot a false answer to the challenge regarding {addr:?}");
                // Any false answer shall result in 0 score immediately
                return 0;
            }
        }
    }
    // TODO: For those answers not among the expected_proofs,
    //       it could be due to having different knowledge of records to us.
    //       shall we:
    //         * set the target being close to us, so that neighbours sharing same knowledge in higher chance
    //         * fetch from local to testify
    //         * fetch from network to testify
    std::cmp::min(
        HIGHEST_SCORE,
        HIGHEST_SCORE * correct_answers / expected_proofs.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_no_local_peers() {
        let local_peers: Vec<(PeerId, Vec<Multiaddr>)> = vec![];
        let target = NetworkAddress::from(PeerId::random());
        let num_of_peers = Some(5);
        let range = None;
        let result = Node::calculate_get_closest_peers(local_peers, target, num_of_peers, range);

        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_fewer_local_peers_than_num_of_peers() {
        let local_peers: Vec<(PeerId, Vec<Multiaddr>)> = vec![
            (
                PeerId::random(),
                vec![Multiaddr::from_str("/ip4/192.168.1.1/tcp/8080").unwrap()],
            ),
            (
                PeerId::random(),
                vec![Multiaddr::from_str("/ip4/192.168.1.2/tcp/8080").unwrap()],
            ),
            (
                PeerId::random(),
                vec![Multiaddr::from_str("/ip4/192.168.1.2/tcp/8080").unwrap()],
            ),
        ];
        let target = NetworkAddress::from(PeerId::random());
        let num_of_peers = Some(2);
        let range = None;
        let result = Node::calculate_get_closest_peers(
            local_peers.clone(),
            target.clone(),
            num_of_peers,
            range,
        );

        // Result shall be sorted and truncated
        let mut expected_result: Vec<(NetworkAddress, Vec<Multiaddr>)> = local_peers
            .iter()
            .map(|(peer_id, multi_addrs)| {
                let addr = NetworkAddress::from(*peer_id);
                (addr, multi_addrs.clone())
            })
            .collect();
        expected_result.sort_by_key(|(addr, _multi_addrs)| target.distance(addr));
        let expected_result: Vec<_> = expected_result.into_iter().take(2).collect();

        assert_eq!(expected_result, result);
    }

    #[test]
    fn test_with_range_and_num_of_peers() {
        let local_peers: Vec<(PeerId, Vec<Multiaddr>)> = vec![
            (
                PeerId::random(),
                vec![Multiaddr::from_str("/ip4/192.168.1.1/tcp/8080").unwrap()],
            ),
            (
                PeerId::random(),
                vec![Multiaddr::from_str("/ip4/192.168.1.2/tcp/8080").unwrap()],
            ),
            (
                PeerId::random(),
                vec![Multiaddr::from_str("/ip4/192.168.1.2/tcp/8080").unwrap()],
            ),
        ];
        let target = NetworkAddress::from(PeerId::random());
        let num_of_peers = Some(0);
        let range_value = [128; 32];
        let range = Some(range_value);
        let result = Node::calculate_get_closest_peers(
            local_peers.clone(),
            target.clone(),
            num_of_peers,
            range,
        );

        // Range shall be preferred, i.e. the result peers shall all within the range
        let distance = U256::from_big_endian(&range_value);
        let expected_result: Vec<(NetworkAddress, Vec<Multiaddr>)> = local_peers
            .into_iter()
            .filter_map(|(peer_id, multi_addrs)| {
                let addr = NetworkAddress::from(peer_id);
                if target.distance(&addr).0 <= distance {
                    Some((addr, multi_addrs.clone()))
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(expected_result, result);
    }
}
