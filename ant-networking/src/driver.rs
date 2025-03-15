// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    bootstrap::{InitialBootstrap, InitialBootstrapTrigger, INITIAL_BOOTSTRAP_CHECK_INTERVAL},
    circular_vec::CircularVec,
    cmd::{LocalSwarmCmd, NetworkSwarmCmd},
    config::GetRecordCfg,
    driver::kad::U256,
    error::{NetworkError, Result},
    event::{NetworkEvent, NodeEvent},
    external_address::ExternalAddressManager,
    fifo_register::FifoRegister,
    log_markers::Marker,
    network_discovery::{NetworkDiscovery, NETWORK_DISCOVER_INTERVAL},
    record_store::{ClientRecordStore, NodeRecordStore, NodeRecordStoreConfig},
    record_store_api::UnifiedRecordStore,
    relay_manager::RelayManager,
    replication_fetcher::ReplicationFetcher,
    time::{interval, spawn, Instant, Interval},
    transport, Addresses, GetRecordError, Network, NodeIssue, CLOSE_GROUP_SIZE,
};
#[cfg(feature = "open-metrics")]
use crate::{
    metrics::service::run_metrics_server, metrics::NetworkMetricsRecorder, MetricsRegistries,
};
use ant_bootstrap::BootstrapCacheStore;
use ant_evm::PaymentQuote;
use ant_protocol::{
    messages::{Request, Response},
    version::{
        get_network_id, IDENTIFY_CLIENT_VERSION_STR, IDENTIFY_NODE_VERSION_STR,
        IDENTIFY_PROTOCOL_STR, REQ_RESPONSE_VERSION_STR,
    },
    NetworkAddress, PrettyPrintKBucketKey,
};
use futures::future::Either;
use futures::StreamExt;
use libp2p::{core::muxing::StreamMuxerBox, relay, swarm::behaviour::toggle::Toggle};
use libp2p::{
    identity::Keypair,
    kad::{self, KBucketDistance as Distance, QueryId, Record, RecordKey, K_VALUE},
    multiaddr::Protocol,
    request_response::{self, Config as RequestResponseConfig, OutboundRequestId, ProtocolSupport},
    swarm::{ConnectionId, NetworkBehaviour, StreamProtocol, Swarm},
    Multiaddr, PeerId,
};
use libp2p::{swarm::SwarmEvent, Transport as _};
#[cfg(feature = "open-metrics")]
use prometheus_client::metrics::info::Info;
use rand::Rng;
use std::{
    collections::{btree_map::Entry, BTreeMap, HashMap, HashSet},
    convert::TryInto,
    fmt::Debug,
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    path::PathBuf,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Duration;
use tracing::warn;
use xor_name::XorName;

/// Interval over which we check for the farthest record we _should_ be holding
/// based upon our knowledge of the CLOSE_GROUP
pub(crate) const CLOSET_RECORD_CHECK_INTERVAL: Duration = Duration::from_secs(15);

/// Interval over which we query relay manager to check if we can make any more reservations.
pub(crate) const RELAY_MANAGER_RESERVATION_INTERVAL: Duration = Duration::from_secs(30);

const KAD_STREAM_PROTOCOL_ID: StreamProtocol = StreamProtocol::new("/autonomi/kad/1.0.0");

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

/// 10 is the max number of issues per node we track to avoid mem leaks
/// The boolean flag to indicate whether the node is considered as bad or not
pub(crate) type BadNodes = BTreeMap<PeerId, (Vec<(NodeIssue, Instant)>, bool)>;

/// What is the largest packet to send over the network.
/// Records larger than this will be rejected.
pub const MAX_PACKET_SIZE: usize = 1024 * 1024 * 5; // the chunk size is 1mb, so should be higher than that to prevent failures

// Timeout for requests sent/received through the request_response behaviour.
const REQUEST_TIMEOUT_DEFAULT_S: Duration = Duration::from_secs(30);
// Sets the keep-alive timeout of idle connections.
const CONNECTION_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(10);

// Inverval of resending identify to connected peers.
const RESEND_IDENTIFY_INVERVAL: Duration = Duration::from_secs(3600);

const NETWORKING_CHANNEL_SIZE: usize = 10_000;

/// Time before a Kad query times out if no response is received
const KAD_QUERY_TIMEOUT_S: Duration = Duration::from_secs(10);

/// Client requires a super long time when have get_closest query against production network
const CLIENT_KAD_QUERY_TIMEOUT_S: Duration = Duration::from_secs(60);

/// Interval to trigger native libp2p::kad bootstrap.
/// This is the max time it should take. Minimum interval at any node will be half this
const PERIODIC_KAD_BOOTSTRAP_INTERVAL_MAX_S: u64 = 21600;

// Init during compilation, instead of runtime error that should never happen
// Option<T>::expect will be stabilised as const in the future (https://github.com/rust-lang/rust/issues/67441)
const REPLICATION_FACTOR: NonZeroUsize = match NonZeroUsize::new(CLOSE_GROUP_SIZE + 2) {
    Some(v) => v,
    None => panic!("CLOSE_GROUP_SIZE should not be zero"),
};

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
    pub(super) relay_server: libp2p::relay::Behaviour,
    pub(super) kademlia: kad::Behaviour<UnifiedRecordStore>,
    pub(super) request_response: request_response::cbor::Behaviour<Request, Response>,
}

#[derive(Debug)]
pub struct NetworkBuilder {
    bootstrap_cache: Option<BootstrapCacheStore>,
    concurrency_limit: Option<usize>,
    initial_contacts: Vec<Multiaddr>,
    is_behind_home_network: bool,
    keypair: Keypair,
    listen_addr: Option<SocketAddr>,
    local: bool,
    #[cfg(feature = "open-metrics")]
    metrics_registries: Option<MetricsRegistries>,
    #[cfg(feature = "open-metrics")]
    metrics_server_port: Option<u16>,
    request_timeout: Option<Duration>,
    upnp: bool,
}

impl NetworkBuilder {
    pub fn new(keypair: Keypair, local: bool, initial_contacts: Vec<Multiaddr>) -> Self {
        Self {
            bootstrap_cache: None,
            concurrency_limit: None,
            initial_contacts,
            is_behind_home_network: false,
            keypair,
            listen_addr: None,
            local,
            #[cfg(feature = "open-metrics")]
            metrics_registries: None,
            #[cfg(feature = "open-metrics")]
            metrics_server_port: None,
            request_timeout: None,
            upnp: false,
        }
    }

    pub fn bootstrap_cache(&mut self, bootstrap_cache: BootstrapCacheStore) {
        self.bootstrap_cache = Some(bootstrap_cache);
    }

    pub fn is_behind_home_network(&mut self, enable: bool) {
        self.is_behind_home_network = enable;
    }

    pub fn listen_addr(&mut self, listen_addr: SocketAddr) {
        self.listen_addr = Some(listen_addr);
    }

    pub fn request_timeout(&mut self, request_timeout: Duration) {
        self.request_timeout = Some(request_timeout);
    }

    pub fn concurrency_limit(&mut self, concurrency_limit: usize) {
        self.concurrency_limit = Some(concurrency_limit);
    }

    /// Set the registries used inside the metrics server.
    /// Configure the `metrics_server_port` to enable the metrics server.
    #[cfg(feature = "open-metrics")]
    pub fn metrics_registries(&mut self, registries: MetricsRegistries) {
        self.metrics_registries = Some(registries);
    }

    #[cfg(feature = "open-metrics")]
    /// The metrics server is enabled only if the port is provided.
    pub fn metrics_server_port(&mut self, port: Option<u16>) {
        self.metrics_server_port = port;
    }

    pub fn upnp(&mut self, upnp: bool) {
        self.upnp = upnp;
    }

    /// Creates a new `SwarmDriver` instance, along with a `Network` handle
    /// for sending commands and an `mpsc::Receiver<NetworkEvent>` for receiving
    /// network events. It initializes the swarm, sets up the transport, and
    /// configures the Kademlia and mDNS behaviour for peer discovery.
    ///
    /// # Returns
    ///
    /// A tuple containing a `Network` handle, an `mpsc::Receiver<NetworkEvent>`,
    /// and a `SwarmDriver` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if there is a problem initializing the mDNS behaviour.
    pub fn build_node(
        self,
        root_dir: PathBuf,
    ) -> Result<(Network, mpsc::Receiver<NetworkEvent>, SwarmDriver)> {
        let bootstrap_interval = rand::thread_rng().gen_range(
            PERIODIC_KAD_BOOTSTRAP_INTERVAL_MAX_S / 2..PERIODIC_KAD_BOOTSTRAP_INTERVAL_MAX_S,
        );

        let mut kad_cfg = kad::Config::new(KAD_STREAM_PROTOCOL_ID);
        let _ = kad_cfg
            .set_kbucket_inserts(libp2p::kad::BucketInserts::Manual)
            // how often a node will replicate records that it has stored, aka copying the key-value pair to other nodes
            // this is a heavier operation than publication, so it is done less frequently
            // Set to `None` to ensure periodic replication disabled.
            .set_replication_interval(None)
            // how often a node will publish a record key, aka telling the others it exists
            // Set to `None` to ensure periodic publish disabled.
            .set_publication_interval(None)
            // 1mb packet size
            .set_max_packet_size(MAX_PACKET_SIZE)
            // How many nodes _should_ store data.
            .set_replication_factor(REPLICATION_FACTOR)
            .set_query_timeout(KAD_QUERY_TIMEOUT_S)
            // Require iterative queries to use disjoint paths for increased resiliency in the presence of potentially adversarial nodes.
            .disjoint_query_paths(true)
            // Records never expire
            .set_record_ttl(None)
            .set_replication_factor(REPLICATION_FACTOR)
            .set_periodic_bootstrap_interval(Some(Duration::from_secs(bootstrap_interval)))
            // Emit PUT events for validation prior to insertion into the RecordStore.
            // This is no longer needed as the record_storage::put now can carry out validation.
            // .set_record_filtering(KademliaStoreInserts::FilterBoth)
            // Disable provider records publication job
            .set_provider_publication_interval(None);

        let store_cfg = {
            let storage_dir_path = root_dir.join("record_store");
            // In case the node instanace is restarted for a different version of network,
            // the previous storage folder shall be wiped out,
            // to avoid bring old data into new network.
            check_and_wipe_storage_dir_if_necessary(
                root_dir.clone(),
                storage_dir_path.clone(),
                get_network_id(),
            )?;

            // Configures the disk_store to store records under the provided path and increase the max record size
            // The storage dir is appendixed with key_version str to avoid bringing records from old network into new

            if let Err(error) = std::fs::create_dir_all(&storage_dir_path) {
                return Err(NetworkError::FailedToCreateRecordStoreDir {
                    path: storage_dir_path,
                    source: error,
                });
            }
            let peer_id = PeerId::from(self.keypair.public());
            let encryption_seed: [u8; 16] = peer_id
                .to_bytes()
                .get(..16)
                .expect("Cann't get encryption_seed from keypair")
                .try_into()
                .expect("Cann't get 16 bytes from serialised key_pair");
            NodeRecordStoreConfig {
                max_value_bytes: MAX_PACKET_SIZE, // TODO, does this need to be _less_ than MAX_PACKET_SIZE
                storage_dir: storage_dir_path,
                historic_quote_dir: root_dir.clone(),
                encryption_seed,
                ..Default::default()
            }
        };

        let listen_addr = self.listen_addr;
        let upnp = self.upnp;

        let (network, events_receiver, mut swarm_driver) =
            self.build(kad_cfg, Some(store_cfg), false, ProtocolSupport::Full, upnp);

        // Listen on the provided address
        let listen_socket_addr = listen_addr.ok_or(NetworkError::ListenAddressNotProvided)?;

        // Listen on QUIC
        let addr_quic = Multiaddr::from(listen_socket_addr.ip())
            .with(Protocol::Udp(listen_socket_addr.port()))
            .with(Protocol::QuicV1);
        swarm_driver
            .listen_on(addr_quic)
            .expect("Multiaddr should be supported by our configured transports");

        Ok((network, events_receiver, swarm_driver))
    }

    /// Same as `build_node` API but creates the network components in client mode
    pub fn build_client(self) -> (Network, mpsc::Receiver<NetworkEvent>, SwarmDriver) {
        // Create a Kademlia behaviour for client mode, i.e. set req/resp protocol
        // to outbound-only mode and don't listen on any address
        let mut kad_cfg = kad::Config::new(KAD_STREAM_PROTOCOL_ID); // default query timeout is 60 secs

        // 1mb packet size
        let _ = kad_cfg
            .set_kbucket_inserts(libp2p::kad::BucketInserts::Manual)
            .set_max_packet_size(MAX_PACKET_SIZE)
            .set_replication_factor(REPLICATION_FACTOR)
            .set_query_timeout(CLIENT_KAD_QUERY_TIMEOUT_S)
            // Require iterative queries to use disjoint paths for increased resiliency in the presence of potentially adversarial nodes.
            .disjoint_query_paths(true)
            // How many nodes _should_ store data.
            .set_replication_factor(REPLICATION_FACTOR);

        let (network, net_event_recv, driver) =
            self.build(kad_cfg, None, true, ProtocolSupport::Outbound, false);

        (network, net_event_recv, driver)
    }

    /// Private helper to create the network components with the provided config and req/res behaviour
    fn build(
        self,
        kad_cfg: kad::Config,
        record_store_cfg: Option<NodeRecordStoreConfig>,
        is_client: bool,
        req_res_protocol: ProtocolSupport,
        upnp: bool,
    ) -> (Network, mpsc::Receiver<NetworkEvent>, SwarmDriver) {
        let identify_protocol_str = IDENTIFY_PROTOCOL_STR
            .read()
            .expect("Failed to obtain read lock for IDENTIFY_PROTOCOL_STR")
            .clone();

        let peer_id = PeerId::from(self.keypair.public());
        // vdash metric (if modified please notify at https://github.com/happybeing/vdash/issues):
        info!(
            "Process (PID: {}) with PeerId: {peer_id}",
            std::process::id()
        );
        info!(
            "Self PeerID {peer_id} is represented as kbucket_key {:?}",
            PrettyPrintKBucketKey(NetworkAddress::from_peer(peer_id).as_kbucket_key())
        );

        #[cfg(feature = "open-metrics")]
        let mut metrics_registries = self.metrics_registries.unwrap_or_default();

        // ==== Transport ====
        #[cfg(feature = "open-metrics")]
        let main_transport = transport::build_transport(&self.keypair, &mut metrics_registries);
        #[cfg(not(feature = "open-metrics"))]
        let main_transport = transport::build_transport(&self.keypair);
        let transport = if !self.local {
            debug!("Preventing non-global dials");
            // Wrap upper in a transport that prevents dialing local addresses.
            libp2p::core::transport::global_only::Transport::new(main_transport).boxed()
        } else {
            main_transport
        };

        let (relay_transport, relay_behaviour) =
            libp2p::relay::client::new(self.keypair.public().to_peer_id());
        let relay_transport = relay_transport
            .upgrade(libp2p::core::upgrade::Version::V1Lazy)
            .authenticate(
                libp2p::noise::Config::new(&self.keypair)
                    .expect("Signing libp2p-noise static DH keypair failed."),
            )
            .multiplex(libp2p::yamux::Config::default())
            .or_transport(transport);

        let transport = relay_transport
            .map(|either_output, _| match either_output {
                Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
                Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            })
            .boxed();

        #[cfg(feature = "open-metrics")]
        let metrics_recorder = if let Some(port) = self.metrics_server_port {
            let metrics_recorder = NetworkMetricsRecorder::new(&mut metrics_registries);
            let metadata_sub_reg = metrics_registries
                .metadata
                .sub_registry_with_prefix("ant_networking");

            metadata_sub_reg.register(
                "peer_id",
                "Identifier of a peer of the network",
                Info::new(vec![("peer_id".to_string(), peer_id.to_string())]),
            );
            metadata_sub_reg.register(
                "identify_protocol_str",
                "The protocol version string that is used to connect to the correct network",
                Info::new(vec![(
                    "identify_protocol_str".to_string(),
                    identify_protocol_str.clone(),
                )]),
            );

            run_metrics_server(metrics_registries, port);
            Some(metrics_recorder)
        } else {
            None
        };

        // RequestResponse Behaviour
        let request_response = {
            let cfg = RequestResponseConfig::default()
                .with_request_timeout(self.request_timeout.unwrap_or(REQUEST_TIMEOUT_DEFAULT_S));
            let req_res_version_str = REQ_RESPONSE_VERSION_STR
                .read()
                .expect("Failed to obtain read lock for REQ_RESPONSE_VERSION_STR")
                .clone();

            info!("Building request response with {req_res_version_str:?}",);
            request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::try_from_owned(req_res_version_str)
                        .expect("StreamProtocol should start with a /"),
                    req_res_protocol,
                )],
                cfg,
            )
        };

        let (network_event_sender, network_event_receiver) = mpsc::channel(NETWORKING_CHANNEL_SIZE);
        let (network_swarm_cmd_sender, network_swarm_cmd_receiver) =
            mpsc::channel(NETWORKING_CHANNEL_SIZE);
        let (local_swarm_cmd_sender, local_swarm_cmd_receiver) =
            mpsc::channel(NETWORKING_CHANNEL_SIZE);

        // Kademlia Behaviour
        let kademlia = {
            match record_store_cfg {
                Some(store_cfg) => {
                    #[cfg(feature = "open-metrics")]
                    let record_stored_metrics =
                        metrics_recorder.as_ref().map(|r| r.records_stored.clone());
                    let node_record_store = NodeRecordStore::with_config(
                        peer_id,
                        store_cfg,
                        network_event_sender.clone(),
                        local_swarm_cmd_sender.clone(),
                        #[cfg(feature = "open-metrics")]
                        record_stored_metrics,
                    );

                    let store = UnifiedRecordStore::Node(node_record_store);
                    debug!("Using Kademlia with NodeRecordStore!");
                    kad::Behaviour::with_config(peer_id, store, kad_cfg)
                }
                // no cfg provided for client
                None => {
                    let store = UnifiedRecordStore::Client(ClientRecordStore::default());
                    debug!("Using Kademlia with ClientRecordStore!");
                    kad::Behaviour::with_config(peer_id, store, kad_cfg)
                }
            }
        };

        let agent_version = if is_client {
            IDENTIFY_CLIENT_VERSION_STR
                .read()
                .expect("Failed to obtain read lock for IDENTIFY_CLIENT_VERSION_STR")
                .clone()
        } else {
            IDENTIFY_NODE_VERSION_STR
                .read()
                .expect("Failed to obtain read lock for IDENTIFY_NODE_VERSION_STR")
                .clone()
        };
        // Identify Behaviour
        info!("Building Identify with identify_protocol_str: {identify_protocol_str:?} and identify_protocol_str: {identify_protocol_str:?}");
        let identify = {
            let cfg = libp2p::identify::Config::new(identify_protocol_str, self.keypair.public())
                .with_agent_version(agent_version)
                // Enlength the identify interval from default 5 mins to 1 hour.
                .with_interval(RESEND_IDENTIFY_INVERVAL)
                .with_hide_listen_addrs(true);
            libp2p::identify::Behaviour::new(cfg)
        };

        let upnp = if !self.local && !is_client && upnp {
            debug!("Enabling UPnP port opening behavior");
            Some(libp2p::upnp::tokio::Behaviour::default())
        } else {
            None
        }
        .into(); // Into `Toggle<T>`

        let relay_server = {
            let relay_server_cfg = relay::Config {
                max_reservations: 128,             // Amount of peers we are relaying for
                max_circuits: 1024, // The total amount of relayed connections at any given moment.
                max_circuits_per_peer: 256, // Amount of relayed connections per peer (both dst and src)
                circuit_src_rate_limiters: vec![], // No extra rate limiting for now
                // We should at least be able to relay packets with chunks etc.
                max_circuit_bytes: MAX_PACKET_SIZE as u64,
                ..Default::default()
            };
            libp2p::relay::Behaviour::new(peer_id, relay_server_cfg)
        };

        let behaviour = NodeBehaviour {
            blocklist: libp2p::allow_block_list::Behaviour::default(),
            relay_client: relay_behaviour,
            relay_server,
            upnp,
            request_response,
            kademlia,
            identify,
        };

        let swarm_config = libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(CONNECTION_KEEP_ALIVE_TIMEOUT);

        let swarm = Swarm::new(transport, behaviour, peer_id, swarm_config);

        let replication_fetcher = ReplicationFetcher::new(peer_id, network_event_sender.clone());

        // Enable relay manager for nodes behind home network
        let relay_manager = if !is_client && self.is_behind_home_network {
            let relay_manager = RelayManager::new(peer_id);
            #[cfg(feature = "open-metrics")]
            let mut relay_manager = relay_manager;
            #[cfg(feature = "open-metrics")]
            if let Some(metrics_recorder) = &metrics_recorder {
                relay_manager.set_reservation_health_metrics(
                    metrics_recorder.relay_reservation_health.clone(),
                );
            }
            Some(relay_manager)
        } else {
            info!("Relay manager is disabled for this node.");
            None
        };
        // Enable external address manager for public nodes and not behind nat
        let external_address_manager = if !is_client && !self.local && !self.is_behind_home_network
        {
            Some(ExternalAddressManager::new(peer_id))
        } else {
            info!("External address manager is disabled for this node.");
            None
        };

        let swarm_driver = SwarmDriver {
            swarm,
            self_peer_id: peer_id,
            local: self.local,
            is_client,
            is_behind_home_network: self.is_behind_home_network,
            #[cfg(feature = "open-metrics")]
            close_group: Vec::with_capacity(CLOSE_GROUP_SIZE),
            peers_in_rt: 0,
            initial_bootstrap: InitialBootstrap::new(self.initial_contacts),
            initial_bootstrap_trigger: InitialBootstrapTrigger::new(self.upnp, is_client),
            bootstrap_cache: self.bootstrap_cache,
            relay_manager,
            connected_relay_clients: Default::default(),
            external_address_manager,
            replication_fetcher,
            #[cfg(feature = "open-metrics")]
            metrics_recorder,
            // kept here to ensure we can push messages to the channel
            // and not block the processing thread unintentionally
            network_cmd_sender: network_swarm_cmd_sender.clone(),
            network_cmd_receiver: network_swarm_cmd_receiver,
            local_cmd_sender: local_swarm_cmd_sender.clone(),
            local_cmd_receiver: local_swarm_cmd_receiver,
            event_sender: network_event_sender,
            pending_get_closest_peers: Default::default(),
            pending_requests: Default::default(),
            pending_get_record: Default::default(),
            dialed_peers: CircularVec::new(1024),
            network_discovery: NetworkDiscovery::new(&peer_id),
            live_connected_peers: Default::default(),
            latest_established_connection_ids: Default::default(),
            handling_statistics: Default::default(),
            handled_times: 0,
            hard_disk_write_error: 0,
            bad_nodes: Default::default(),
            quotes_history: Default::default(),
            replication_targets: Default::default(),
            last_replication: None,
            last_connection_pruning_time: Instant::now(),
            network_density_samples: FifoRegister::new(100),
            peers_version: Default::default(),
        };

        let network = Network::new(
            network_swarm_cmd_sender,
            local_swarm_cmd_sender,
            peer_id,
            self.keypair,
        );

        (network, network_event_receiver, swarm_driver)
    }
}

fn check_and_wipe_storage_dir_if_necessary(
    root_dir: PathBuf,
    storage_dir_path: PathBuf,
    cur_version_str: String,
) -> Result<()> {
    let mut prev_version_str = String::new();
    let version_file = root_dir.join("network_key_version");
    {
        match fs::File::open(version_file.clone()) {
            Ok(mut file) => {
                file.read_to_string(&mut prev_version_str)?;
            }
            Err(err) => {
                warn!("Failed in accessing version file {version_file:?}: {err:?}");
                // Assuming file was not created yet
                info!("Creating a new version file at {version_file:?}");
                fs::File::create(version_file.clone())?;
            }
        }
    }

    // In case of version mismatch:
    //   * the storage_dir shall be wiped out
    //   * the version file shall be updated
    if cur_version_str != prev_version_str {
        warn!("Trying to wipe out storage dir {storage_dir_path:?}, as cur_version {cur_version_str:?} doesn't match prev_version {prev_version_str:?}");
        let _ = fs::remove_dir_all(storage_dir_path);

        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(version_file.clone())?;
        info!("Writing cur_version {cur_version_str:?} into version file at {version_file:?}");
        file.write_all(cur_version_str.as_bytes())?;
    }

    Ok(())
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

    network_cmd_sender: mpsc::Sender<NetworkSwarmCmd>,
    pub(crate) local_cmd_sender: mpsc::Sender<LocalSwarmCmd>,
    local_cmd_receiver: mpsc::Receiver<LocalSwarmCmd>,
    network_cmd_receiver: mpsc::Receiver<NetworkSwarmCmd>,
    event_sender: mpsc::Sender<NetworkEvent>, // Use `self.send_event()` to send a NetworkEvent.

    /// Trackers for underlying behaviour related events
    pub(crate) pending_get_closest_peers: PendingGetClosest,
    pub(crate) pending_requests:
        HashMap<OutboundRequestId, Option<oneshot::Sender<Result<Response>>>>,
    pub(crate) pending_get_record: PendingGetRecord,
    /// A list of the most recent peers we have dialed ourselves. Old dialed peers are evicted once the vec fills up.
    pub(crate) dialed_peers: CircularVec<PeerId>,
    // Peers that having live connection to. Any peer got contacted during kad network query
    // will have live connection established. And they may not appear in the RT.
    pub(crate) live_connected_peers: BTreeMap<ConnectionId, (PeerId, Multiaddr, Instant)>,
    /// The list of recently established connections ids.
    /// This is used to prevent log spamming.
    pub(crate) latest_established_connection_ids: HashMap<usize, (IpAddr, Instant)>,
    // Record the handling time of the recent 10 for each handling kind.
    handling_statistics: BTreeMap<String, Vec<Duration>>,
    handled_times: usize,
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
                    if let Some(new_interval) = self.run_network_discover_continuously(network_discover_interval.period()).await {
                        network_discover_interval = new_interval;
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
                        let self_addr = NetworkAddress::from_peer(self.self_peer_id);
                        let close_peers_distance = self_addr.distance(&NetworkAddress::from_peer(closest_k_peers[CLOSE_GROUP_SIZE + 1].0));

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
                        //     let self_addr = NetworkAddress::from_peer(self.self_peer_id);
                        //     self_addr.distance(&NetworkAddress::from_peer(closest_k_peers[CLOSE_GROUP_SIZE]))
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
            &NetworkAddress::from_peer(self.self_peer_id),
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
    use super::check_and_wipe_storage_dir_if_necessary;
    use std::{fs, io::Read, time::Duration};

    #[tokio::test]
    async fn version_file_update() {
        let temp_dir = std::env::temp_dir();
        let unique_dir_name = uuid::Uuid::new_v4().to_string();
        let root_dir = temp_dir.join(unique_dir_name);
        fs::create_dir_all(&root_dir).expect("Failed to create root directory");

        let version_file = root_dir.join("network_key_version");
        let storage_dir = root_dir.join("record_store");

        let cur_version = uuid::Uuid::new_v4().to_string();
        assert!(check_and_wipe_storage_dir_if_necessary(
            root_dir.clone(),
            storage_dir.clone(),
            cur_version.clone()
        )
        .is_ok());
        {
            let mut content_str = String::new();
            let mut file = fs::OpenOptions::new()
                .read(true)
                .open(version_file.clone())
                .expect("Failed to open version file");
            file.read_to_string(&mut content_str)
                .expect("Failed to read from version file");
            assert_eq!(content_str, cur_version);

            drop(file);
        }

        fs::create_dir_all(&storage_dir).expect("Failed to create storage directory");
        assert!(fs::metadata(storage_dir.clone()).is_ok());

        let cur_version = uuid::Uuid::new_v4().to_string();
        assert!(check_and_wipe_storage_dir_if_necessary(
            root_dir.clone(),
            storage_dir.clone(),
            cur_version.clone()
        )
        .is_ok());
        {
            let mut content_str = String::new();
            let mut file = fs::OpenOptions::new()
                .read(true)
                .open(version_file.clone())
                .expect("Failed to open version file");
            file.read_to_string(&mut content_str)
                .expect("Failed to read from version file");
            assert_eq!(content_str, cur_version);

            drop(file);
        }
        // The storage_dir shall be removed as version_key changed
        assert!(fs::metadata(storage_dir.clone()).is_err());
    }

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
