// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{error::Result, event::NodeEventsChannel, Marker, NodeEvent};
#[cfg(feature = "open-metrics")]
use crate::metrics::NodeMetrics;
use crate::RunningNode;
use bls::{PublicKey, PK_SIZE};
use bytes::Bytes;
use libp2p::{autonat::NatStatus, identity::Keypair, Multiaddr};
#[cfg(feature = "open-metrics")]
use prometheus_client::registry::Registry;
use rand::{rngs::StdRng, Rng, SeedableRng};
use sn_networking::{Network, NetworkBuilder, NetworkEvent, SwarmDriver, CLOSE_GROUP_SIZE};
use sn_protocol::{
    error::Error as ProtocolError,
    messages::{Cmd, CmdResponse, Query, QueryResponse, Response},
    NetworkAddress, PrettyPrintRecordKey,
};
use sn_transfers::{CashNoteRedemption, LocalWallet, MainPubkey, MainSecretKey};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{broadcast, mpsc::Receiver},
    task::spawn,
};

/// Expected topic name where notifications of royalty transfers are sent on.
/// The notification msg is expected to contain the serialised public key, followed by the
/// serialised transfer info encrypted against the referenced public key.
pub const ROYALTY_TRANSFER_NOTIF_TOPIC: &str = "ROYALTY_TRANSFER_NOTIFICATION";

/// Defines the percentage (1/50) of node to act as royalty_transfer_notify forwarder.
const FORWARDER_CHOOSING_FACTOR: usize = 50;

/// Interval to trigger replication of all records to all peers.
/// This is the max time it should take. Minimum interval at any ndoe will be half this
pub const PERIODIC_REPLICATION_INTERVAL_MAX_S: u64 = 45;

/// Helper to build and run a Node
pub struct NodeBuilder {
    keypair: Keypair,
    addr: SocketAddr,
    initial_peers: Vec<Multiaddr>,
    local: bool,
    root_dir: PathBuf,
    #[cfg(feature = "open-metrics")]
    metrics_server_port: u16,
}

impl NodeBuilder {
    /// Instantiate the builder
    pub fn new(
        keypair: Keypair,
        addr: SocketAddr,
        initial_peers: Vec<Multiaddr>,
        local: bool,
        root_dir: PathBuf,
    ) -> Self {
        Self {
            keypair,
            addr,
            initial_peers,
            local,
            root_dir,
            #[cfg(feature = "open-metrics")]
            metrics_server_port: 0,
        }
    }

    #[cfg(feature = "open-metrics")]
    /// Set the port for the OpenMetrics server. Defaults to a random port if not set
    pub fn metrics_server_port(&mut self, port: u16) {
        self.metrics_server_port = port;
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
        // TODO: Make this key settable, and accessible via API
        let reward_key = MainSecretKey::random();
        let reward_address = reward_key.main_pubkey();

        let mut wallet = LocalWallet::load_from_main_key(&self.root_dir, reward_key)?;
        // store in case it's a fresh wallet created if none was found
        wallet.deposit_and_store_to_disk(&vec![])?;

        #[cfg(feature = "open-metrics")]
        let (metrics_registry, node_metrics) = {
            let mut metrics_registry = Registry::default();
            let node_metrics = NodeMetrics::new(&mut metrics_registry);
            (metrics_registry, node_metrics)
        };

        let mut network_builder = NetworkBuilder::new(self.keypair, self.local, self.root_dir);

        network_builder.enable_gossip();
        network_builder.listen_addr(self.addr);
        #[cfg(feature = "open-metrics")]
        network_builder.metrics_registry(metrics_registry);
        #[cfg(feature = "open-metrics")]
        network_builder.metrics_server_port(self.metrics_server_port);

        let (network, network_event_receiver, swarm_driver) = network_builder.build_node()?;
        let node_events_channel = NodeEventsChannel::default();
        let (node_cmds, _) = broadcast::channel(10);

        let node = Node {
            network: network.clone(),
            events_channel: node_events_channel.clone(),
            node_cmds: node_cmds.clone(),
            initial_peers: Arc::new(self.initial_peers),
            reward_address: Arc::new(reward_address),
            transfer_notifs_filter: None,
            #[cfg(feature = "open-metrics")]
            node_metrics,
        };
        let running_node = RunningNode {
            network,
            node_events_channel,
            node_cmds,
        };

        // Run the node
        node.run(swarm_driver, network_event_receiver);

        // Having a portion of nodes (1/50) subscribe to the ROYALTY_TRANSFER_NOTIF_TOPIC
        // Such nodes become `forwarder` to ensure the actual beneficary won't miss.
        let index: usize = StdRng::from_entropy().gen_range(0..FORWARDER_CHOOSING_FACTOR);
        if index == FORWARDER_CHOOSING_FACTOR / 2 {
            trace!("Picked as a forwarding node to subscribe to the {ROYALTY_TRANSFER_NOTIF_TOPIC} topic");
            // Forwarder only needs to forward topic msgs on libp2p level,
            // i.e. no need to handle topic msgs, hence not a `listener`.
            running_node
                .subscribe_to_topic(ROYALTY_TRANSFER_NOTIF_TOPIC.to_string())
                .map(|()| info!("Node has been subscribed to gossipsub topic '{ROYALTY_TRANSFER_NOTIF_TOPIC}' to receive network royalties payments notifications."))?;
        }

        Ok(running_node)
    }
}

/// Commands that can be sent by the user to the Node instance, e.g. to mutate some settings.
#[derive(Clone)]
pub enum NodeCmd {
    /// Set a PublicKey to start decoding and accepting Transfer notifications received over gossipsub.
    TransferNotifsFilter(Option<PublicKey>),
}

/// `Node` represents a single node in the distributed network. It handles
/// network events, processes incoming requests, interacts with the data
/// storage, and broadcasts node-related events.
#[derive(Clone)]
pub(crate) struct Node {
    pub(crate) network: Network,
    pub(crate) events_channel: NodeEventsChannel,
    // We keep a copy of the Sender which is clonable and we can obtain a receiver from.
    node_cmds: broadcast::Sender<NodeCmd>,
    // Peers that are dialed at startup of node.
    initial_peers: Arc<Vec<Multiaddr>>,
    reward_address: Arc<MainPubkey>,
    transfer_notifs_filter: Option<PublicKey>,
    #[cfg(feature = "open-metrics")]
    pub(crate) node_metrics: NodeMetrics,
}

impl Node {
    /// Runs the provided `SwarmDriver` and spawns a task to process for `NetworkEvents`
    fn run(
        mut self,
        swarm_driver: SwarmDriver,
        mut network_event_receiver: Receiver<NetworkEvent>,
    ) {
        let mut rng = StdRng::from_entropy();

        let peers_connected = Arc::new(AtomicUsize::new(0));
        let mut cmds_receiver = self.node_cmds.subscribe();

        let _handle = spawn(swarm_driver.run());
        let _handle = spawn(async move {
            // use a random inactivity timeout to ensure that the nodes do not sync when messages
            // are being transmitted.
            let replication_interval: u64 = rng.gen_range(
                PERIODIC_REPLICATION_INTERVAL_MAX_S / 2..PERIODIC_REPLICATION_INTERVAL_MAX_S,
            );
            let replication_interval_time = Duration::from_secs(replication_interval);
            debug!("Replication interval set to {replication_interval_time:?}");

            let mut replication_interval = tokio::time::interval(replication_interval_time);
            let _ = replication_interval.tick().await; // first tick completes immediately

            loop {
                let peers_connected = peers_connected.clone();

                tokio::select! {
                    net_event = network_event_receiver.recv() => {
                        match net_event {
                            Some(event) => {
                                let start = std::time::Instant::now();
                                let event_string = format!("{:?}", event);

                                self.handle_network_event(event, peers_connected).await ;
                                info!("Handled non-blocking network event in {:?}: {:?}", start.elapsed(), event_string);

                            }
                            None => {
                                error!("The `NetworkEvent` channel is closed");
                                self.events_channel.broadcast(NodeEvent::ChannelClosed);
                                break;
                            }
                        }
                    }
                    // runs every replication_interval time
                    _ = replication_interval.tick() => {
                        let start = std::time::Instant::now();
                        info!("Periodic replication triggered");
                        let network = self.network.clone();
                        self.record_metrics(Marker::IntervalReplicationTriggered);

                        let _handle = spawn(async move {
                            Marker::ForcedReplication.log();

                            if let Err(err) = Self::try_interval_replication(network)
                                .await
                            {
                                error!("Error while triggering replication {err:?}");
                            }

                            info!("Periodic replication took {:?}", start.elapsed());
                        });
                    }
                    node_cmd = cmds_receiver.recv() => {
                        match node_cmd {
                            Ok(NodeCmd::TransferNotifsFilter(filter)) => {
                                self.transfer_notifs_filter = filter;
                                let _ = self.network.start_handle_gossip();
                            }
                            Err(err) => error!("When trying to read from the NodeCmds channel/receiver: {err:?}")
                        }
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
        self.node_metrics.record(marker);
    }

    // **** Private helpers *****

    /// Handle a network event.
    /// Spawns a thread for any likely long running tasks
    async fn handle_network_event(&self, event: NetworkEvent, peers_connected: Arc<AtomicUsize>) {
        // when the node has not been connected to enough peers, it should not perform activities
        // that might require peers in the RT to succeed.
        let mut log_when_not_enough_peers = true;
        let start = std::time::Instant::now();
        loop {
            if peers_connected.load(Ordering::Relaxed) >= CLOSE_GROUP_SIZE {
                break;
            }
            match &event {
                // these activities requires the node to be connected to some peer to be able to carry
                // out get kad.get_record etc. This happens during replication/PUT. So we should wait
                // until we have enough nodes, else these might fail.
                NetworkEvent::CmdRequestReceived { .. }
                | NetworkEvent::QueryRequestReceived { .. }
                | NetworkEvent::UnverifiedRecord(_)
                | NetworkEvent::FailedToWrite(_)
                | NetworkEvent::ResponseReceived { .. }
                | NetworkEvent::KeysForReplication(_) => {
                    if log_when_not_enough_peers {
                        debug!("Waiting before processing certain NetworkEvent before reaching {CLOSE_GROUP_SIZE} peers");
                    }
                    log_when_not_enough_peers = false;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                // These events do not need to wait until there are enough peers
                NetworkEvent::PeerAdded(..)
                | NetworkEvent::PeerRemoved(..)
                | NetworkEvent::NewListenAddr(_)
                | NetworkEvent::NatStatusChanged(_)
                | NetworkEvent::GossipsubMsgReceived { .. }
                | NetworkEvent::GossipsubMsgPublished { .. } => break,
            }
        }
        let event_string = format!("{:?}", event);
        trace!("Handling NetworkEvent {event_string:?}");

        self.handle_sync_network_event(event, peers_connected);

        trace!(
            "NetworkEvent handled in {:?} : {event_string:?}",
            start.elapsed()
        );
    }

    /// Handle synchronous network events.
    fn handle_sync_network_event(&self, event: NetworkEvent, peers_connected: Arc<AtomicUsize>) {
        match event {
            NetworkEvent::PeerAdded(peer_id, connected_peers) => {
                // increment peers_connected and send ConnectedToNetwork event if have connected to K_VALUE peers
                let _ = peers_connected.fetch_add(1, Ordering::SeqCst);
                if peers_connected.load(Ordering::SeqCst) == CLOSE_GROUP_SIZE {
                    self.events_channel.broadcast(NodeEvent::ConnectedToNetwork);
                }

                self.record_metrics(Marker::PeersInRoutingTable(connected_peers));
                self.record_metrics(Marker::PeerAddedToRoutingTable(peer_id));

                // try replication here
                let net_clone = self.network.clone();
                self.record_metrics(Marker::IntervalReplicationTriggered);
                let _handle = spawn(async move {
                    if let Err(err) = Self::try_interval_replication(net_clone).await {
                        error!("Error while triggering replication {err:?}");
                    }
                });
            }
            NetworkEvent::PeerRemoved(peer_id, connected_peers) => {
                self.record_metrics(Marker::PeersInRoutingTable(connected_peers));
                self.record_metrics(Marker::PeerRemovedFromRoutingTable(peer_id));

                let net = self.network.clone();
                self.record_metrics(Marker::IntervalReplicationTriggered);
                let _handle = spawn(async move {
                    if let Err(e) = Self::try_interval_replication(net).await {
                        error!("Error while triggering replication {e:?}");
                    }
                });
            }
            NetworkEvent::NewListenAddr(_) => {
                if !cfg!(feature = "local-discovery") {
                    let network = self.network.clone();
                    let peers = self.initial_peers.clone();
                    let _handle = spawn(async move {
                        for addr in &*peers {
                            if let Err(err) = network.dial(addr.clone()).await {
                                tracing::error!("Failed to dial {addr}: {err:?}");
                            };
                        }
                    });
                }
            }
            NetworkEvent::NatStatusChanged(status) => {
                if matches!(status, NatStatus::Private) {
                    tracing::warn!("NAT status is determined to be private!");
                    self.events_channel.broadcast(NodeEvent::BehindNat);
                }
            }
            NetworkEvent::FailedToWrite(key) => {
                if let Err(e) = self.network.remove_failed_local_record(key) {
                    error!("Failed to remove local record: {e:?}");
                }
            }
            NetworkEvent::ResponseReceived { res } => {
                trace!("NetworkEvent::ResponseReceived {res:?}");
                if let Err(err) = self.handle_response(res) {
                    error!("Error while handling NetworkEvent::ResponseReceived {err:?}");
                }
            }
            NetworkEvent::KeysForReplication(keys) => {
                self.record_metrics(Marker::fetching_keys_for_replication(&keys));

                if let Err(err) = self.fetch_replication_keys_without_wait(keys) {
                    error!("Error while trying to fetch replicated data {err:?}");
                }
            }
            NetworkEvent::QueryRequestReceived { query, channel } => {
                let network = self.network.clone();
                let payment_address = *self.reward_address;

                let _handle = spawn(async move {
                    let res = Self::handle_query(&network, query, payment_address).await;

                    if let Err(error) = network.send_response(res, channel) {
                        error!("Error while sending response form query req: {error:?}");
                    }
                });
            }
            NetworkEvent::CmdRequestReceived { cmd } => {
                self.handle_node_cmd(cmd);
            }
            NetworkEvent::UnverifiedRecord(record) => {
                // queries can be long running and require validation, so we spawn a task to handle them
                let self_clone = self.clone();
                let _handle = spawn(async move {
                    let key = PrettyPrintRecordKey::from(&record.key).into_owned();
                    match self_clone.validate_and_store_record(record).await {
                        Ok(cmdok) => trace!("UnverifiedRecord {key} stored with {cmdok:?}."),
                        Err(err) => {
                            self_clone.record_metrics(Marker::RecordRejected(&key));
                            trace!("UnverifiedRecord {key} failed to be stored with error {err:?}.")
                        }
                    }
                });
            }
            NetworkEvent::GossipsubMsgReceived { topic, msg }
            | NetworkEvent::GossipsubMsgPublished { topic, msg } => {
                trace!("Received a gossip msg for the topic of {topic}");
                let events_channel = self.events_channel.clone();

                if events_channel.receiver_count() == 0 {
                    return;
                }
                if topic == ROYALTY_TRANSFER_NOTIF_TOPIC {
                    // this is expected to be a notification of a transfer which we treat specially,
                    // and we try to decode it only if it's referring to a PK the user is interested in
                    if let Some(filter_pk) = self.transfer_notifs_filter {
                        let _handle = spawn(async move {
                            match try_decode_transfer_notif(&msg, filter_pk) {
                                Ok(Some(notif_event)) => events_channel.broadcast(notif_event),
                                Ok(None) => { /* transfer notif filered out */ }
                                Err(err) => {
                                    warn!("GossipsubMsg matching the transfer notif. topic name, couldn't be decoded as such: {err:?}");
                                    events_channel
                                        .broadcast(NodeEvent::GossipsubMsg { topic, msg });
                                }
                            }
                        });
                    }
                } else {
                    events_channel.broadcast(NodeEvent::GossipsubMsg { topic, msg });
                }
            }
        }
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
            other => {
                warn!("handle_response not implemented for {other:?}");
            }
        };

        Ok(())
    }

    async fn handle_query(
        network: &Network,
        query: Query,
        payment_address: MainPubkey,
    ) -> Response {
        let resp: QueryResponse = match query {
            Query::GetStoreCost(address) => {
                trace!("Got GetStoreCost request for {address:?}");

                let record_exists = {
                    if let Some(key) = address.as_record_key() {
                        match network.is_record_key_present_locally(&key).await {
                            Ok(res) => res,
                            Err(error) => {
                                error!("Problem getting record key's existence: {error:?}");
                                false
                            }
                        }
                    } else {
                        false
                    }
                };

                if record_exists {
                    QueryResponse::GetStoreCost {
                        quote: Err(ProtocolError::RecordExists(
                            PrettyPrintRecordKey::from(&address.to_record_key()).into_owned(),
                        )),
                        payment_address,
                    }
                } else {
                    let store_cost = network
                        .get_local_storecost()
                        .await
                        .map_err(|_| ProtocolError::GetStoreCostFailed);

                    QueryResponse::GetStoreCost {
                        quote: Self::create_quote_for_storecost(network, store_cost, address),
                        payment_address,
                    }
                }
            }
            Query::GetReplicatedRecord { requester, key } => {
                trace!("Got GetReplicatedRecord from {requester:?} regarding {key:?}");

                let our_address = NetworkAddress::from_peer(network.peer_id);
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
        };
        Response::Query(resp)
    }

    fn handle_node_cmd(&self, cmd: Cmd) {
        Marker::NodeCmdReceived(&cmd).log();
        match cmd {
            Cmd::Replicate { holder, keys } => {
                let network = self.network.clone();

                let _handle = spawn(async move {
                    trace!(
                        "Received replication list from {holder:?} of {} keys",
                        keys.len()
                    );

                    if let Some(peer_id) = holder.as_peer_id() {
                        let local_peers: Vec<_> =
                            match network.get_closest_k_value_local_peers().await {
                                // accept replication requests from the close_group * 2 peers away, giving us some margin
                                // for replication on churn
                                Ok(peers) => peers.into_iter().take(CLOSE_GROUP_SIZE * 2).collect(),
                                Err(err) => {
                                    error!("Failed to get close group local peers: {err:?}");
                                    return;
                                }
                            };

                        // lets be sure we should handle this message
                        if local_peers.contains(&peer_id) {
                            // todo: error is not propagated to the caller here
                            let _ = network.add_keys_to_replication_fetcher(peer_id, keys);
                        } else {
                            warn!("Received replication list from {peer_id:?} which is not in our close group");
                        }
                    } else {
                        error!(
                            "Within the replication list, Can not parse peer_id from {holder:?}"
                        );
                    }
                });
            }
        };
    }
}

fn try_decode_transfer_notif(msg: &[u8], filter: PublicKey) -> eyre::Result<Option<NodeEvent>> {
    let mut key_bytes = [0u8; PK_SIZE];
    key_bytes.copy_from_slice(
        msg.get(0..PK_SIZE)
            .ok_or_else(|| eyre::eyre!("msg doesn't have enough bytes"))?,
    );
    let key = PublicKey::from_bytes(key_bytes)?;
    if key == filter {
        let cashnote_redemptions: Vec<CashNoteRedemption> = rmp_serde::from_slice(&msg[PK_SIZE..])?;
        Ok(Some(NodeEvent::TransferNotif {
            key,
            cashnote_redemptions,
        }))
    } else {
        Ok(None)
    }
}
