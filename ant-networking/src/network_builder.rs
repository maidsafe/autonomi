// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    bootstrap::{InitialBootstrap, InitialBootstrapTrigger},
    circular_vec::CircularVec,
    driver::NodeBehaviour,
    error::{NetworkError, Result},
    event::NetworkEvent,
    external_address::ExternalAddressManager,
    fifo_register::FifoRegister,
    network_discovery::NetworkDiscovery,
    record_store::{ClientRecordStore, NodeRecordStore, NodeRecordStoreConfig},
    record_store_api::UnifiedRecordStore,
    relay_manager::RelayManager,
    replication_fetcher::ReplicationFetcher,
    time::Instant,
    transport, Network, SwarmDriver, CLOSE_GROUP_SIZE,
};
#[cfg(feature = "open-metrics")]
use crate::{
    metrics::service::run_metrics_server, metrics::NetworkMetricsRecorder, MetricsRegistries,
};
use ant_bootstrap::BootstrapCacheStore;
use ant_protocol::{
    version::{
        get_network_id, IDENTIFY_CLIENT_VERSION_STR, IDENTIFY_NODE_VERSION_STR,
        IDENTIFY_PROTOCOL_STR, REQ_RESPONSE_VERSION_STR,
    },
    NetworkAddress, PrettyPrintKBucketKey,
};
use futures::future::Either;
use libp2p::Transport as _;
use libp2p::{core::muxing::StreamMuxerBox, relay};
use libp2p::{
    identity::Keypair,
    kad,
    multiaddr::Protocol,
    request_response::{self, Config as RequestResponseConfig, ProtocolSupport},
    swarm::{StreamProtocol, Swarm},
    Multiaddr, PeerId,
};
#[cfg(feature = "open-metrics")]
use prometheus_client::metrics::info::Info;
use rand::Rng;
use std::{
    convert::TryInto,
    fmt::Debug,
    fs,
    io::{Read, Write},
    net::SocketAddr,
    num::NonZeroUsize,
    path::PathBuf,
    time::Duration,
};
use tokio::sync::mpsc;

// Timeout for requests sent/received through the request_response behaviour.
const REQUEST_TIMEOUT_DEFAULT_S: Duration = Duration::from_secs(30);
// Sets the keep-alive timeout of idle connections.
const CONNECTION_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(10);

// Inverval of resending identify to connected peers.
const RESEND_IDENTIFY_INVERVAL: Duration = Duration::from_secs(3600);

const NETWORKING_CHANNEL_SIZE: usize = 10_000;

/// Time before a Kad query times out if no response is received
const KAD_QUERY_TIMEOUT_S: Duration = Duration::from_secs(10);

// Init during compilation, instead of runtime error that should never happen
// Option<T>::expect will be stabilised as const in the future (https://github.com/rust-lang/rust/issues/67441)
const REPLICATION_FACTOR: NonZeroUsize = match NonZeroUsize::new(CLOSE_GROUP_SIZE + 2) {
    Some(v) => v,
    None => panic!("CLOSE_GROUP_SIZE should not be zero"),
};

const KAD_STREAM_PROTOCOL_ID: StreamProtocol = StreamProtocol::new("/autonomi/kad/1.0.0");

/// What is the largest packet to send over the network.
/// Records larger than this will be rejected.
pub const MAX_PACKET_SIZE: usize = 1024 * 1024 * 5; // the chunk size is 1mb, so should be higher than that to prevent failures

/// Interval to trigger native libp2p::kad bootstrap.
/// This is the max time it should take. Minimum interval at any node will be half this
const PERIODIC_KAD_BOOTSTRAP_INTERVAL_MAX_S: u64 = 21600;

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
            PrettyPrintKBucketKey(NetworkAddress::from(peer_id).as_kbucket_key())
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

        let relay_server = if !is_client && !self.is_behind_home_network {
            let relay_server_cfg = relay::Config {
                max_reservations: 128,             // Amount of peers we are relaying for
                max_circuits: 1024, // The total amount of relayed connections at any given moment.
                max_circuits_per_peer: 256, // Amount of relayed connections per peer (both dst and src)
                circuit_src_rate_limiters: vec![], // No extra rate limiting for now
                // We should at least be able to relay packets with chunks etc.
                max_circuit_bytes: MAX_PACKET_SIZE as u64,
                ..Default::default()
            };
            Some(libp2p::relay::Behaviour::new(peer_id, relay_server_cfg))
        } else {
            None
        }
        .into();

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
            // We use 255 here which allows covering a network larger than 64k without any rotating.
            // This is based on the libp2p kad::kBuckets peers distribution.
            dialed_peers: CircularVec::new(255),
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

#[cfg(test)]
mod tests {
    use super::check_and_wipe_storage_dir_if_necessary;
    use std::{fs, io::Read};

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
}
