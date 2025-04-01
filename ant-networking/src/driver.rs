// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(feature = "open-metrics")]
use crate::metrics::NetworkMetricsRecorder;
use crate::{
    bootstrap::{InitialBootstrap, InitialBootstrapTrigger, INITIAL_BOOTSTRAP_CHECK_INTERVAL},
    circular_vec::CircularVec,
    cmd::{LocalSwarmCmd, NetworkSwarmCmd},
    config::GetRecordCfg,
    driver::kad::U256,
    error::Result,
    event::{NetworkEvent, NodeEvent},
    external_address::ExternalAddressManager,
    fifo_register::FifoRegister,
    log_markers::Marker,
    network_discovery::{NetworkDiscovery, NETWORK_DISCOVER_INTERVAL},
    record_store_api::UnifiedRecordStore,
    relay_manager::RelayManager,
    replication_fetcher::ReplicationFetcher,
    time::{interval, spawn, Instant, Interval},
    Addresses, GetRecordError, NodeIssue, CLOSE_GROUP_SIZE,
};
use ant_bootstrap::BootstrapCacheStore;
use ant_evm::PaymentQuote;
use ant_protocol::messages::ConnectionInfo;
use ant_protocol::{
    messages::{Request, Response},
    NetworkAddress,
};
use futures::StreamExt;
use libp2p::{
    kad::{self, KBucketDistance as Distance, QueryId, Record, RecordKey, K_VALUE},
    request_response::OutboundRequestId,
    swarm::{
        dial_opts::{DialOpts, PeerCondition},
        ConnectionId, Swarm,
    },
    Multiaddr, PeerId,
};
use libp2p::{
    request_response,
    swarm::{behaviour::toggle::Toggle, NetworkBehaviour, SwarmEvent},
};
use rand::Rng;
use std::{
    collections::{btree_map::Entry, BTreeMap, HashMap, HashSet, VecDeque},
    net::IpAddr,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Duration;
use tracing::warn;
use xor_name::XorName;

/// 10 is the max number of issues per node we track to avoid mem leaks
/// The boolean flag to indicate whether the node is considered as bad or not
pub(crate) type BadNodes = BTreeMap<PeerId, (Vec<(NodeIssue, Instant)>, bool)>;

/// Interval over which we check for the farthest record we _should_ be holding
/// based upon our knowledge of the CLOSE_GROUP
pub(crate) const CLOSET_RECORD_CHECK_INTERVAL: Duration = Duration::from_secs(15);

/// Interval over which we query relay manager to check if we can make any more reservations.
pub(crate) const RELAY_MANAGER_RESERVATION_INTERVAL: Duration = Duration::from_secs(30);

/// Interval over which we check if we could dial any peer in the dial queue.
const DIAL_QUEUE_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// The ways in which the Get Closest queries are used.
pub(crate) enum PendingGetClosestType {
    /// The network discovery method is present at the networking layer
    /// Thus we can just process the queries made by NetworkDiscovery without using any channels
    NetworkDiscovery,
    /// These are queries made by a function at the upper layers and contains a channel to send the result back.
    FunctionCall(oneshot::Sender<Vec<(PeerId, Addresses)>>),
}
type PendingGetClosest = HashMap<QueryId, (PendingGetClosestType, Vec<(PeerId, Addresses)>)>;

/// Using XorName to differentiate different record content under the same key.
type GetRecordResultMap = HashMap<XorName, (Record, HashSet<PeerId>)>;
pub(crate) type PendingGetRecord = HashMap<
    QueryId,
    (
        RecordKey, // record we're fetching, to dedupe repeat requests
        Vec<oneshot::Sender<std::result::Result<Record, GetRecordError>>>, // vec of senders waiting for this record
        GetRecordResultMap,
        GetRecordCfg,
    ),
>;

impl From<std::convert::Infallible> for NodeEvent {
    fn from(_: std::convert::Infallible) -> Self {
        panic!("NodeBehaviour is not Infallible!")
    }
}

/// The behaviors are polled in the order they are defined.
/// The first struct member is polled until it returns Poll::Pending before moving on to later members.
/// Prioritize the behaviors related to connection handling.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "NodeEvent")]
pub(super) struct NodeBehaviour {
    pub(super) blocklist:
        libp2p::allow_block_list::Behaviour<libp2p::allow_block_list::BlockedPeers>,
    pub(super) identify: libp2p::identify::Behaviour,
    pub(super) upnp: Toggle<libp2p::upnp::tokio::Behaviour>,
    pub(super) relay_client: libp2p::relay::client::Behaviour,
    pub(super) relay_server: Toggle<libp2p::relay::Behaviour>,
    pub(super) kademlia: kad::Behaviour<UnifiedRecordStore>,
    pub(super) request_response: request_response::cbor::Behaviour<Request, Response>,
}

pub struct SwarmDriver {
    pub(crate) swarm: Swarm<NodeBehaviour>,
    pub(crate) self_peer_id: PeerId,
    /// When true, we don't filter our local addresses
    pub(crate) local: bool,
    pub(crate) is_client: bool,
    pub(crate) is_behind_home_network: bool,
    #[cfg(feature = "open-metrics")]
    pub(crate) close_group: Vec<PeerId>,
    pub(crate) peers_in_rt: usize,
    pub(crate) initial_bootstrap: InitialBootstrap,
    pub(crate) initial_bootstrap_trigger: InitialBootstrapTrigger,
    pub(crate) network_discovery: NetworkDiscovery,
    pub(crate) bootstrap_cache: Option<BootstrapCacheStore>,
    pub(crate) external_address_manager: Option<ExternalAddressManager>,
    pub(crate) relay_manager: Option<RelayManager>,
    /// The peers that are using our relay service.
    pub(crate) connected_relay_clients: HashSet<PeerId>,
    /// The peers that are closer to our PeerId. Includes self.
    pub(crate) replication_fetcher: ReplicationFetcher,
    #[cfg(feature = "open-metrics")]
    pub(crate) metrics_recorder: Option<NetworkMetricsRecorder>,

    pub(crate) network_cmd_sender: mpsc::Sender<NetworkSwarmCmd>,
    pub(crate) local_cmd_sender: mpsc::Sender<LocalSwarmCmd>,
    pub(crate) local_cmd_receiver: mpsc::Receiver<LocalSwarmCmd>,
    pub(crate) network_cmd_receiver: mpsc::Receiver<NetworkSwarmCmd>,
    pub(crate) event_sender: mpsc::Sender<NetworkEvent>, // Use `self.send_event()` to send a NetworkEvent.

    /// Trackers for underlying behaviour related events
    pub(crate) pending_get_closest_peers: PendingGetClosest,
    #[allow(clippy::type_complexity)]
    pub(crate) pending_requests: HashMap<
        OutboundRequestId,
        Option<oneshot::Sender<Result<(Response, Option<ConnectionInfo>)>>>,
    >,
    pub(crate) pending_get_record: PendingGetRecord,
    /// A list of the most recent peers we have dialed ourselves. Old dialed peers are evicted once the vec fills up.
    pub(crate) dialed_peers: CircularVec<PeerId>,
    pub(crate) dial_queue: VecDeque<(PeerId, Addresses, Instant)>,
    // Peers that having live connection to. Any peer got contacted during kad network query
    // will have live connection established. And they may not appear in the RT.
    pub(crate) live_connected_peers: BTreeMap<ConnectionId, (PeerId, Multiaddr, Instant)>,
    /// The list of recently established connections ids.
    /// This is used to prevent log spamming.
    pub(crate) latest_established_connection_ids: HashMap<usize, (IpAddr, Instant)>,
    // Record the handling time of the recent 10 for each handling kind.
    pub(crate) handling_statistics: BTreeMap<String, Vec<Duration>>,
    pub(crate) handled_times: usize,
    pub(crate) hard_disk_write_error: usize,
    pub(crate) bad_nodes: BadNodes,
    pub(crate) quotes_history: BTreeMap<PeerId, PaymentQuote>,
    pub(crate) replication_targets: BTreeMap<PeerId, Instant>,
    /// when was the last replication event
    /// This allows us to throttle replication no matter how it is triggered
    pub(crate) last_replication: Option<Instant>,
    /// when was the last outdated connection prunning undertaken.
    pub(crate) last_connection_pruning_time: Instant,
    /// FIFO cache for the network density samples
    pub(crate) network_density_samples: FifoRegister,
    /// record versions of those peers that in the non-full-kbuckets.
    pub(crate) peers_version: HashMap<PeerId, String>,
}

impl SwarmDriver {
    /// Asynchronously drives the swarm event loop, handling events from both
    /// the swarm and command receiver. This function will run indefinitely,
    /// until the command channel is closed.
    ///
    /// The `tokio::select` macro is used to concurrently process swarm events
    /// and command receiver messages, ensuring efficient handling of multiple
    /// asynchronous tasks.
    pub async fn run(mut self, mut shutdown_rx: watch::Receiver<bool>) {
        let mut network_discover_interval = interval(NETWORK_DISCOVER_INTERVAL);
        let mut set_farthest_record_interval = interval(CLOSET_RECORD_CHECK_INTERVAL);
        let mut relay_manager_reservation_interval = interval(RELAY_MANAGER_RESERVATION_INTERVAL);
        let mut initial_bootstrap_trigger_check_interval =
            Some(interval(INITIAL_BOOTSTRAP_CHECK_INTERVAL));
        let mut dial_queue_check_interval = interval(DIAL_QUEUE_CHECK_INTERVAL);
        dial_queue_check_interval.tick().await; // first tick completes immediately

        let mut bootstrap_cache_save_interval = self.bootstrap_cache.as_ref().and_then(|cache| {
            if cache.config().disable_cache_writing {
                None
            } else {
                // add a variance of 10% to the interval, to avoid all nodes writing to disk at the same time.
                let duration =
                    Self::duration_with_variance(cache.config().min_cache_save_duration, 10);
                Some(interval(duration))
            }
        });
        if let Some(interval) = bootstrap_cache_save_interval.as_mut() {
            interval.tick().await; // first tick completes immediately
            info!(
                "Bootstrap cache save interval is set to {:?}",
                interval.period()
            );
        }

        let mut round_robin_index = 0;
        // temporarily skip processing IncomingConnectionError swarm event to avoid log spamming
        let mut previous_incoming_connection_error_event = None;
        loop {
            tokio::select! {
                // polls futures in order they appear here (as opposed to random)
                biased;

                // Prioritise any local cmds pending.
                // https://github.com/libp2p/rust-libp2p/blob/master/docs/coding-guidelines.md#prioritize-local-work-over-new-work-from-a-remote
                local_cmd = self.local_cmd_receiver.recv() => match local_cmd {
                    Some(cmd) => {
                        let start = Instant::now();
                        let cmd_string = format!("{cmd:?}");
                        if let Err(err) = self.handle_local_cmd(cmd) {
                            warn!("Error while handling local cmd: {err}");
                        }
                        trace!("LocalCmd handled in {:?}: {cmd_string:?}", start.elapsed());
                    },
                    None =>  continue,
                },
                // next check if we have locally generated network cmds
                some_cmd = self.network_cmd_receiver.recv() => match some_cmd {
                    Some(cmd) => {
                        let start = Instant::now();
                        let cmd_string = format!("{cmd:?}");
                        if let Err(err) = self.handle_network_cmd(cmd) {
                            warn!("Error while handling cmd: {err}");
                        }
                        trace!("SwarmCmd handled in {:?}: {cmd_string:?}", start.elapsed());
                    },
                    None =>  continue,
                },
                // Check for a shutdown command.
                result = shutdown_rx.changed() => {
                    if result.is_ok() && *shutdown_rx.borrow() || result.is_err() {
                        info!("Shutdown signal received or sender dropped. Exiting swarm driver loop.");
                        break;
                    }
                },
                // next take and react to external swarm events
                swarm_event = self.swarm.select_next_some() => {
                    // Refer to the handle_swarm_events::IncomingConnectionError for more info on why we skip
                    // processing the event for one round.
                    if let Some(previous_event) = previous_incoming_connection_error_event.take() {
                        if let Err(err) = self.handle_swarm_events(swarm_event) {
                            warn!("Error while handling swarm event: {err}");
                        }
                        if let Err(err) = self.handle_swarm_events(previous_event) {
                            warn!("Error while handling swarm event: {err}");
                        }
                        continue;
                    }
                    if matches!(swarm_event, SwarmEvent::IncomingConnectionError {..}) {
                        previous_incoming_connection_error_event = Some(swarm_event);
                        continue;
                    }

                    // logging for handling events happens inside handle_swarm_events
                    // otherwise we're rewriting match statements etc around this anwyay
                    if let Err(err) = self.handle_swarm_events(swarm_event) {
                        warn!("Error while handling swarm event: {err}");
                    }
                },
                // thereafter we can check our intervals

                _ = dial_queue_check_interval.tick() => {
                    let now = Instant::now();
                    let mut to_remove = vec![];
                    // check if we can dial any peer in the dial queue
                    // if we have no peers in the dial queue, skip this check
                    for (idx, (peer_id, addrs, wait_time)) in self.dial_queue.iter().enumerate() {
                        if now > *wait_time {
                            info!("Dialing peer {peer_id:?} from dial queue with addresses {addrs:?}");
                            to_remove.push(idx);
                            if let Err(err) = self.swarm.dial(
                                DialOpts::peer_id(*peer_id)
                                    .condition(PeerCondition::NotDialing)
                                    .addresses(addrs.0.clone())
                                    .build(),
                            ) {
                                warn!(%peer_id, ?addrs, "dialing error: {err:?}");
                            }
                        }
                    }

                    for idx in to_remove.iter().rev() {
                        self.dial_queue.remove(*idx);
                    }
                },

                // check if we can trigger the initial bootstrap process
                // once it is triggered, we don't re-trigger it
                Some(()) = Self::conditional_interval(&mut initial_bootstrap_trigger_check_interval) => {
                    if self.initial_bootstrap_trigger.should_trigger_initial_bootstrap() {
                        info!("Triggering initial bootstrap process. This is a one-time operation.");
                        self.initial_bootstrap.trigger_bootstrapping_process(&mut self.swarm, self.peers_in_rt);
                        // we will not call this loop anymore, once the initial bootstrap is triggered.
                        // It should run on its own and complete.
                        initial_bootstrap_trigger_check_interval = None;
                    }
                }

                // runs every bootstrap_interval time
                _ = network_discover_interval.tick() => {
                    round_robin_index += 1;
                    if round_robin_index > 255 {
                        round_robin_index = 0;
                    }

                    if let Some(new_interval) = self.run_network_discover_continuously(network_discover_interval.period(), round_robin_index).await {
                        network_discover_interval = new_interval;
                    }

                    // Collect all peers_in_non_full_buckets
                    let mut peers_in_non_full_buckets = vec![];
                    for kbucket in self.swarm.behaviour_mut().kademlia.kbuckets() {
                        let num_entires = kbucket.num_entries();
                        if num_entires >= K_VALUE.get() {
                            continue;
                        } else {
                            let peers_in_kbucket = kbucket
                                .iter()
                                .map(|peer_entry| peer_entry.node.key.into_preimage())
                                .collect::<Vec<PeerId>>();
                            peers_in_non_full_buckets.extend(peers_in_kbucket);
                        }
                    }

                    // Ensure all existing node_version records are for those peers_in_non_full_buckets
                    self.peers_version
                        .retain(|peer_id, _version| peers_in_non_full_buckets.contains(peer_id));

                    #[cfg(feature = "open-metrics")]
                    if let Some(metrics_recorder) = &self.metrics_recorder {
                        metrics_recorder.update_node_versions(&self.peers_version);
                    }
                }
                _ = set_farthest_record_interval.tick() => {
                    if !self.is_client {
                        let kbucket_status = self.get_kbuckets_status();
                        self.update_on_kbucket_status(&kbucket_status);
                        if kbucket_status.estimated_network_size <= CLOSE_GROUP_SIZE {
                            info!("Not enough estimated network size {}, with {} peers_in_non_full_buckets and {} num_of_full_buckets.",
                            kbucket_status.estimated_network_size,
                            kbucket_status.peers_in_non_full_buckets,
                            kbucket_status.num_of_full_buckets);
                            continue;
                        }
                        // The entire Distance space is U256
                        // (U256::MAX is 115792089237316195423570985008687907853269984665640564039457584007913129639935)
                        // The network density (average distance among nodes) can be estimated as:
                        //     network_density = entire_U256_space / estimated_network_size
                        let density = U256::MAX / U256::from(kbucket_status.estimated_network_size);
                        let density_distance = density * U256::from(CLOSE_GROUP_SIZE);

                        // Use distance to close peer to avoid the situation that
                        // the estimated density_distance is too narrow.
                        let closest_k_peers = self.get_closest_k_value_local_peers();
                        if closest_k_peers.len() <= CLOSE_GROUP_SIZE + 2 {
                            continue;
                        }
                        // Results are sorted, hence can calculate distance directly
                        // Note: self is included
                        let self_addr = NetworkAddress::from(self.self_peer_id);
                        let close_peers_distance = self_addr.distance(&NetworkAddress::from(closest_k_peers[CLOSE_GROUP_SIZE + 1].0));

                        let distance = std::cmp::max(Distance(density_distance), close_peers_distance);

                        // The sampling approach has severe impact to the node side performance
                        // Hence suggested to be only used by client side.
                        // let distance = if let Some(distance) = self.network_density_samples.get_median() {
                        //     distance
                        // } else {
                        //     // In case sampling not triggered or yet,
                        //     // fall back to use the distance to CLOSE_GROUP_SIZEth closest
                        //     let closest_k_peers = self.get_closest_k_value_local_peers();
                        //     if closest_k_peers.len() <= CLOSE_GROUP_SIZE + 1 {
                        //         continue;
                        //     }
                        //     // Results are sorted, hence can calculate distance directly
                        //     // Note: self is included
                        //     let self_addr = NetworkAddress::from(self.self_peer_id);
                        //     self_addr.distance(&NetworkAddress::from(closest_k_peers[CLOSE_GROUP_SIZE]))
                        // };

                        info!("Set responsible range to {distance:?}({:?})", distance.ilog2());

                        // set any new distance to farthest record in the store
                        self.swarm.behaviour_mut().kademlia.store_mut().set_distance_range(distance);
                        // the distance range within the replication_fetcher shall be in sync as well
                        self.replication_fetcher.set_replication_distance_range(distance);
                    }
                }
                _ = relay_manager_reservation_interval.tick() => {
                    if let Some(relay_manager) = &mut self.relay_manager {
                        relay_manager.try_connecting_to_relay(&mut self.swarm, &self.bad_nodes)
                    }
                },
                Some(()) = Self::conditional_interval(&mut bootstrap_cache_save_interval) => {
                    let Some(bootstrap_cache) = self.bootstrap_cache.as_mut() else {
                        continue;
                    };
                    let Some(current_interval) = bootstrap_cache_save_interval.as_mut() else {
                        continue;
                    };
                    let start = Instant::now();

                    let config = bootstrap_cache.config().clone();
                    let mut old_cache = bootstrap_cache.clone();

                    let new = match BootstrapCacheStore::new(config) {
                        Ok(new) => new,
                        Err(err) => {
                            error!("Failed to create a new empty cache: {err}");
                            continue;
                        }
                    };
                    *bootstrap_cache = new;

                    // save the cache to disk
                    spawn(async move {
                        if let Err(err) = old_cache.sync_and_flush_to_disk(true) {
                            error!("Failed to save bootstrap cache: {err}");
                        }
                    });

                    if current_interval.period() >= bootstrap_cache.config().max_cache_save_duration {
                        continue;
                    }

                    // add a variance of 1% to the max interval to avoid all nodes writing to disk at the same time.
                    let max_cache_save_duration =
                        Self::duration_with_variance(bootstrap_cache.config().max_cache_save_duration, 1);

                    // scale up the interval until we reach the max
                    let scaled = current_interval.period().as_secs().saturating_mul(bootstrap_cache.config().cache_save_scaling_factor);
                    let new_duration = Duration::from_secs(std::cmp::min(scaled, max_cache_save_duration.as_secs()));
                    info!("Scaling up the bootstrap cache save interval to {new_duration:?}");

                    *current_interval = interval(new_duration);
                    current_interval.tick().await;

                    trace!("Bootstrap cache synced in {:?}", start.elapsed());

                },
            }
        }
    }

    // --------------------------------------------
    // ---------- Crate helpers -------------------
    // --------------------------------------------

    /// Pushes NetworkSwarmCmd off thread so as to be non-blocking
    /// this is a wrapper around the `mpsc::Sender::send` call
    pub(crate) fn queue_network_swarm_cmd(&self, event: NetworkSwarmCmd) {
        let event_sender = self.network_cmd_sender.clone();
        let capacity = event_sender.capacity();

        // push the event off thread so as to be non-blocking
        let _handle = spawn(async move {
            if capacity == 0 {
                warn!(
                    "NetworkSwarmCmd channel is full. Await capacity to send: {:?}",
                    event
                );
            }
            if let Err(error) = event_sender.send(event).await {
                error!("SwarmDriver failed to send event: {}", error);
            }
        });
    }

    /// Sends an event after pushing it off thread so as to be non-blocking
    /// this is a wrapper around the `mpsc::Sender::send` call
    pub(crate) fn send_event(&self, event: NetworkEvent) {
        let event_sender = self.event_sender.clone();
        let capacity = event_sender.capacity();

        // push the event off thread so as to be non-blocking
        let _handle = spawn(async move {
            if capacity == 0 {
                warn!(
                    "NetworkEvent channel is full. Await capacity to send: {:?}",
                    event
                );
            }
            if let Err(error) = event_sender.send(event).await {
                error!("SwarmDriver failed to send event: {}", error);
            }
        });
    }

    /// Get closest K_VALUE peers from our local RoutingTable. Contains self.
    /// Is sorted for closeness to self.
    pub(crate) fn get_closest_k_value_local_peers(&mut self) -> Vec<(PeerId, Addresses)> {
        // Limit ourselves to K_VALUE (20) peers.
        let peers: Vec<_> = self.get_closest_local_peers_to_target(
            &NetworkAddress::from(self.self_peer_id),
            K_VALUE.get() - 1,
        );

        // Start with our own PeerID and chain the closest.
        std::iter::once((self.self_peer_id, Default::default()))
            .chain(peers)
            .collect()
    }

    /// Get closest X peers to the target. Not containing self.
    /// Is sorted for closeness to the target.
    pub(crate) fn get_closest_local_peers_to_target(
        &mut self,
        target: &NetworkAddress,
        num_of_peers: usize,
    ) -> Vec<(PeerId, Addresses)> {
        let peer_ids = self
            .swarm
            .behaviour_mut()
            .kademlia
            .get_closest_local_peers(&target.as_kbucket_key())
            // Map KBucketKey<PeerId> to PeerId.
            .map(|key| key.into_preimage())
            .take(num_of_peers)
            .collect();
        self.collect_peers_info(peer_ids)
    }

    /// Collect peers' address info
    fn collect_peers_info(&mut self, peers: Vec<PeerId>) -> Vec<(PeerId, Addresses)> {
        let mut peers_info = vec![];
        for peer_id in peers {
            if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(peer_id) {
                if let Some(entry) = kbucket
                    .iter()
                    .find(|entry| entry.node.key.preimage() == &peer_id)
                {
                    peers_info.push((peer_id, Addresses(entry.node.value.clone().into_vec())));
                }
            }
        }

        peers_info
    }

    /// Record one handling time.
    /// Log for every 100 received.
    pub(crate) fn log_handling(&mut self, handle_string: String, handle_time: Duration) {
        if handle_string.is_empty() {
            return;
        }

        match self.handling_statistics.entry(handle_string) {
            Entry::Occupied(mut entry) => {
                let records = entry.get_mut();
                records.push(handle_time);
            }
            Entry::Vacant(entry) => {
                entry.insert(vec![handle_time]);
            }
        }

        self.handled_times += 1;

        if self.handled_times >= 100 {
            self.handled_times = 0;

            let mut stats: Vec<(String, usize, Duration)> = self
                .handling_statistics
                .iter()
                .map(|(kind, durations)| {
                    let count = durations.len();
                    let avg_time = durations.iter().sum::<Duration>() / count as u32;
                    (kind.clone(), count, avg_time)
                })
                .collect();

            stats.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by count in descending order

            trace!("SwarmDriver Handling Statistics: {:?}", stats);
            // now we've logged, lets clear the stats from the btreemap
            self.handling_statistics.clear();
        }
    }

    /// Calls Marker::log() to insert the marker into the log files.
    /// Also calls NodeMetrics::record() to record the metric if the `open-metrics` feature flag is enabled.
    pub(crate) fn record_metrics(&self, marker: Marker) {
        marker.log();
        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = self.metrics_recorder.as_ref() {
            metrics_recorder.record_from_marker(marker)
        }
    }
    #[cfg(feature = "open-metrics")]
    /// Updates metrics that rely on our current close group.
    pub(crate) fn record_change_in_close_group(&self, new_close_group: Vec<PeerId>) {
        if let Some(metrics_recorder) = self.metrics_recorder.as_ref() {
            metrics_recorder.record_change_in_close_group(new_close_group);
        }
    }

    /// Listen on the provided address. Also records it within RelayManager
    pub(crate) fn listen_on(&mut self, addr: Multiaddr) -> Result<()> {
        let id = self.swarm.listen_on(addr.clone())?;
        info!("Listening on {id:?} with addr: {addr:?}");
        Ok(())
    }

    /// Returns a new duration that is within +/- variance of the provided duration.
    fn duration_with_variance(duration: Duration, variance: u32) -> Duration {
        let actual_variance = duration / variance;
        let random_adjustment =
            Duration::from_secs(rand::thread_rng().gen_range(0..actual_variance.as_secs()));
        if random_adjustment.as_secs() % 2 == 0 {
            duration - random_adjustment
        } else {
            duration + random_adjustment
        }
    }

    /// To tick an optional interval inside tokio::select! without looping forever.
    async fn conditional_interval(i: &mut Option<Interval>) -> Option<()> {
        match i {
            Some(i) => {
                i.tick().await;
                Some(())
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[tokio::test]
    async fn test_duration_variance_fn() {
        let duration = Duration::from_secs(100);
        let variance = 10;
        for _ in 0..10000 {
            let new_duration = crate::SwarmDriver::duration_with_variance(duration, variance);
            if new_duration < duration - duration / variance
                || new_duration > duration + duration / variance
            {
                panic!("new_duration: {new_duration:?} is not within the expected range",);
            }
        }
    }
}
