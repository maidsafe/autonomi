// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(feature = "open-metrics")]
use crate::networking::{
    MetricsRegistries, metrics::MetadataExtendedRecorder, metrics::MetadataRecorder,
    metrics::NetworkMetricsRecorder, metrics::service::run_metrics_server,
};

use crate::{
    ReachabilityStatus,
    networking::{
        CLOSE_GROUP_SIZE, NetworkEvent,
        bootstrap::{InitialBootstrap, InitialBootstrapTrigger},
        circular_vec::CircularVec,
        driver::{NodeBehaviour, SwarmDriver, network_discovery::NetworkDiscovery},
        error::{NetworkError, Result},
        external_address::ExternalAddressManager,
        reachability_check::{ReachabilityCheckBehaviour, ReachabilityCheckSwarmDriver},
        record_store::{NodeRecordStore, NodeRecordStoreConfig},
        relay_manager::RelayManager,
        replication_fetcher::ReplicationFetcher,
        transport,
    },
};
use ant_bootstrap::BootstrapCacheStore;
use ant_protocol::constants::{KAD_STREAM_PROTOCOL_ID, MAX_PACKET_SIZE, REPLICATION_FACTOR};
use ant_protocol::messages::{Request, Response};
use ant_protocol::version::IDENTIFY_REACHABILITY_CHECK_CLIENT_VERSION_STR;
use ant_protocol::{
    NetworkAddress, PrettyPrintKBucketKey,
    version::{IDENTIFY_PROTOCOL_STR, REQ_RESPONSE_VERSION_STR, get_network_id_str},
};
use futures::future::Either;
use libp2p::{
    Multiaddr, PeerId,
    identity::Keypair,
    kad,
    multiaddr::Protocol,
    request_response::{
        self, Config as RequestResponseConfig, ProtocolSupport, cbor::codec::Codec as CborCodec,
    },
    swarm::{StreamProtocol, Swarm},
};
use libp2p::{Transport as _, core::transport::ListenerId};
use libp2p::{core::muxing::StreamMuxerBox, relay};
use std::time::Instant;
use std::{
    convert::TryInto,
    fmt::Debug,
    fs,
    io::{Read, Write},
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
};
use tokio::sync::{mpsc, watch};

// Timeout for requests sent/received through the request_response behaviour.
const REQUEST_TIMEOUT_DEFAULT_S: Duration = Duration::from_secs(30);
// Sets the keep-alive timeout of idle connections.
const CONNECTION_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(10);

// Inverval of resending identify to connected peers.
const RESEND_IDENTIFY_INVERVAL: Duration = Duration::from_secs(3600);

const NETWORKING_CHANNEL_SIZE: usize = 10_000;

/// Time before a Kad query times out if no response is received
const KAD_QUERY_TIMEOUT_S: Duration = Duration::from_secs(10);

/// Timeout for listening on a local address
const LISTEN_TIMEOUT_S: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub(crate) struct NetworkConfig {
    pub keypair: Keypair,
    pub local: bool,
    pub initial_contacts: Vec<Multiaddr>,
    pub listen_addr: SocketAddr,
    pub root_dir: PathBuf,
    pub shutdown_rx: tokio::sync::watch::Receiver<bool>,
    pub bootstrap_cache: Option<BootstrapCacheStore>,
    pub no_upnp: bool,
    /// The reachability status found using the reachability swarm
    pub reachability_status: Option<ReachabilityStatus>,
    pub relay_client: bool,
    pub custom_request_timeout: Option<Duration>,
    #[cfg(feature = "open-metrics")]
    pub metrics_registries: MetricsRegistries,
    #[cfg(feature = "open-metrics")]
    pub metrics_server_port: Option<u16>,
}

/// Creates a new `SwarmDriver` instance, along with a `Network` handle
/// for sending commands and an `mpsc::Receiver<NetworkEvent>` for receiving
/// network events. It initializes the swarm, sets up the transport, and
/// configures the Kademlia and mDNS behaviour for peer discovery.
///
/// # Returns
///
/// A tuple containing a `Network` handle, an `mpsc::Receiver<NetworkEvent>`,
/// a `SwarmDriver` instance, and an optional metrics server shutdown sender.
///
/// # Errors
///
/// Returns an error if there is a problem initializing the mDNS behaviour.
pub(super) fn init_driver(
    config: NetworkConfig,
) -> Result<(
    SwarmDriver,
    mpsc::Receiver<NetworkEvent>,
    Option<watch::Sender<bool>>,
)> {
    let mut kad_cfg = kad::Config::new(StreamProtocol::new(KAD_STREAM_PROTOCOL_ID));
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
        // may consider to use disjoint paths for increased resiliency in the presence of potentially adversarial nodes.
        // however, this has the risk of libp2p report back partial-correct result in case of high peer query failure rate.
        // .disjoint_query_paths(true)
        // Records never expire
        .set_record_ttl(None)
        // Disable node side libp2p periodic bootstrap
        .set_periodic_bootstrap_interval(None)
        // Emit PUT events for validation prior to insertion into the RecordStore.
        // This is no longer needed as the record_storage::put now can carry out validation.
        // .set_record_filtering(KademliaStoreInserts::FilterBoth)
        // Disable provider records publication job
        .set_provider_publication_interval(None);

    let store_cfg = {
        let storage_dir_path = config.root_dir.join("record_store");
        // In case the node instanace is restarted for a different version of network,
        // the previous storage folder shall be wiped out,
        // to avoid bring old data into new network.
        check_and_wipe_storage_dir_if_necessary(
            config.root_dir.clone(),
            storage_dir_path.clone(),
            get_network_id_str(),
        )?;

        // Configures the disk_store to store records under the provided path and increase the max record size
        // The storage dir is appendixed with key_version str to avoid bringing records from old network into new

        if let Err(error) = std::fs::create_dir_all(&storage_dir_path) {
            return Err(NetworkError::FailedToCreateRecordStoreDir {
                path: storage_dir_path,
                source: error,
            });
        }
        let peer_id = PeerId::from(config.keypair.public());
        let encryption_seed: [u8; 16] = peer_id
            .to_bytes()
            .get(..16)
            .expect("Cann't get encryption_seed from keypair")
            .try_into()
            .expect("Cann't get 16 bytes from serialised key_pair");
        NodeRecordStoreConfig {
            max_value_bytes: MAX_PACKET_SIZE, // TODO, does this need to be _less_ than MAX_PACKET_SIZE
            storage_dir: storage_dir_path,
            historic_quote_dir: config.root_dir.clone(),
            encryption_seed,
            ..Default::default()
        }
    };

    // Listen on the provided address
    let listen_socket_addr = config.listen_addr;

    let (events_receiver, mut swarm_driver, metrics_shutdown_tx) =
        init_swarm_driver(kad_cfg, store_cfg, config);

    // Listen on QUIC
    let addr_quic = Multiaddr::from(listen_socket_addr.ip())
        .with(Protocol::Udp(listen_socket_addr.port()))
        .with(Protocol::QuicV1);
    let _listener_id = listen_on_with_retry(&mut swarm_driver.swarm, addr_quic.clone())?;
    info!("Listening on QUIC address: {addr_quic:?}");
    Ok((swarm_driver, events_receiver, metrics_shutdown_tx))
}

/// Private helper to create the network components with the provided config and req/res behaviour
fn init_swarm_driver(
    kad_cfg: kad::Config,
    record_store_cfg: NodeRecordStoreConfig,
    config: NetworkConfig,
) -> (
    mpsc::Receiver<NetworkEvent>,
    SwarmDriver,
    Option<watch::Sender<bool>>,
) {
    let identify_protocol_str = IDENTIFY_PROTOCOL_STR
        .read()
        .expect("Failed to obtain read lock for IDENTIFY_PROTOCOL_STR")
        .clone();

    let peer_id = PeerId::from(config.keypair.public());
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
    let mut metrics_registries = config.metrics_registries;

    // ==== Transport ====
    #[cfg(feature = "open-metrics")]
    let transport = transport::build_transport(&config.keypair, &mut metrics_registries);
    #[cfg(not(feature = "open-metrics"))]
    let transport = transport::build_transport(&config.keypair);
    let transport = if !config.local {
        debug!("Preventing non-global dials");
        // Wrap upper in a transport that prevents dialing local addresses.
        libp2p::core::transport::global_only::Transport::new(transport).boxed()
    } else {
        transport
    };

    let (relay_transport, relay_client) =
        libp2p::relay::client::new(config.keypair.public().to_peer_id());
    let relay_transport = relay_transport
        .upgrade(libp2p::core::upgrade::Version::V1Lazy)
        .authenticate(
            libp2p::noise::Config::new(&config.keypair)
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
    let (metrics_recorder, metrics_shutdown_tx) = if let Some(port) = config.metrics_server_port {
        let metrics_recorder =
            NetworkMetricsRecorder::new(&mut metrics_registries, &config.reachability_status);
        let mut metadata_recorder = MetadataRecorder::new(&mut metrics_registries);
        metadata_recorder.register_peer_id(&peer_id);
        metadata_recorder.register_identify_protocol_string(identify_protocol_str.clone());
        let mut metadata_extended_recorder = MetadataExtendedRecorder::new(&mut metrics_registries);
        metadata_extended_recorder.register_peer_id(&peer_id);
        metadata_extended_recorder.register_pid();
        metadata_extended_recorder.register_bin_version();
        metadata_extended_recorder.register_root_dir(&config.root_dir);
        metadata_extended_recorder.register_log_dir(&config.root_dir.join("logs"));

        let shutdown_tx = run_metrics_server(metrics_registries, port);
        (Some(metrics_recorder), Some(shutdown_tx))
    } else {
        (None, None)
    };

    #[cfg(not(feature = "open-metrics"))]
    let metrics_shutdown_tx = None;

    let request_response = {
        let cfg = RequestResponseConfig::default().with_request_timeout(
            config
                .custom_request_timeout
                .unwrap_or(REQUEST_TIMEOUT_DEFAULT_S),
        );
        let req_res_version_str = REQ_RESPONSE_VERSION_STR
            .read()
            .expect("Failed to obtain read lock for REQ_RESPONSE_VERSION_STR")
            .clone();
        info!("Building request response with {req_res_version_str:?}");
        let codec = CborCodec::<Request, Response>::default()
            .set_request_size_maximum(2 * MAX_PACKET_SIZE as u64);
        request_response::Behaviour::with_codec(
            codec,
            [(
                StreamProtocol::try_from_owned(req_res_version_str)
                    .expect("StreamProtocol should start with a /"),
                ProtocolSupport::Full,
            )],
            cfg,
        )
    };

    let (network_event_sender, network_event_receiver) = mpsc::channel(NETWORKING_CHANNEL_SIZE);
    let (network_swarm_cmd_sender, network_swarm_cmd_receiver) =
        mpsc::channel(NETWORKING_CHANNEL_SIZE);
    let (local_swarm_cmd_sender, local_swarm_cmd_receiver) = mpsc::channel(NETWORKING_CHANNEL_SIZE);

    // Kademlia Behaviour
    let kademlia = {
        #[cfg(feature = "open-metrics")]
        let record_stored_metrics = metrics_recorder.as_ref().map(|r| r.records_stored.clone());
        let node_record_store = NodeRecordStore::with_config(
            peer_id,
            record_store_cfg,
            network_event_sender.clone(),
            local_swarm_cmd_sender.clone(),
            #[cfg(feature = "open-metrics")]
            record_stored_metrics,
        );

        let store = node_record_store;
        debug!("Using Kademlia with NodeRecordStore!");
        kad::Behaviour::with_config(peer_id, store, kad_cfg)
    };

    let agent_version =
        ant_protocol::version::construct_node_user_agent(env!("CARGO_PKG_VERSION").to_string());

    // Identify Behaviour
    info!(
        "Building Identify with identify_protocol_str: {identify_protocol_str:?} and identify_protocol_str: {identify_protocol_str:?}"
    );
    let identify = {
        let cfg = libp2p::identify::Config::new(identify_protocol_str, config.keypair.public())
            .with_agent_version(agent_version)
            // Enlength the identify interval from default 5 mins to 1 hour.
            .with_interval(RESEND_IDENTIFY_INVERVAL)
            .with_hide_listen_addrs(true);
        libp2p::identify::Behaviour::new(cfg)
    };

    let upnp = if !config.local && !config.no_upnp && !config.relay_client {
        debug!("Enabling UPnP port opening behavior");
        Some(crate::networking::driver::behaviour::upnp::behaviour::Behaviour::default())
    } else {
        None
    }
    .into(); // Into `Toggle<T>`

    let relay_server = if !config.relay_client {
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
        do_not_disturb: crate::networking::driver::behaviour::do_not_disturb::Behaviour::default(),
        // `Relay client Behaviour` is enabled for all nodes. This is required for normal nodes to connect to relay
        // clients.
        relay_client,
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

    // Enable relay manager to allow the node to act as a relay client and connect via relay servers to the network
    let relay_manager = if config.relay_client {
        let relay_manager = RelayManager::new(peer_id);
        #[cfg(feature = "open-metrics")]
        let mut relay_manager = relay_manager;
        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = &metrics_recorder {
            relay_manager
                .set_reservation_health_metrics(metrics_recorder.relay_reservation_health.clone());
        }
        Some(relay_manager)
    } else {
        info!("Relay manager is disabled for this node.");
        None
    };
    // Enable external address manager for public nodes and not behind nat
    let external_address_manager = if !config.local && !config.relay_client {
        Some(ExternalAddressManager::new(peer_id))
    } else {
        info!("External address manager is disabled for this node.");
        None
    };

    let is_upnp_enabled = swarm.behaviour().upnp.is_enabled();
    let swarm_driver = SwarmDriver {
        swarm,
        self_peer_id: peer_id,
        local: config.local,
        is_relay_client: config.relay_client,
        #[cfg(feature = "open-metrics")]
        close_group: Vec::with_capacity(CLOSE_GROUP_SIZE),
        peers_in_rt: 0,
        initial_bootstrap: InitialBootstrap::new(config.initial_contacts),
        initial_bootstrap_trigger: InitialBootstrapTrigger::new(is_upnp_enabled),
        bootstrap_cache: config.bootstrap_cache,
        relay_manager,
        connected_relay_clients: Default::default(),
        external_address_manager,
        replication_fetcher,
        #[cfg(feature = "open-metrics")]
        metrics_recorder,
        // kept here to ensure we can push messages to the channel
        // and not block the processing thread unintentionally
        network_cmd_sender: network_swarm_cmd_sender,
        network_cmd_receiver: network_swarm_cmd_receiver,
        local_cmd_sender: local_swarm_cmd_sender,
        local_cmd_receiver: local_swarm_cmd_receiver,
        event_sender: network_event_sender,
        pending_get_closest_peers: Default::default(),
        pending_requests: Default::default(),
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
        peers_version: Default::default(),
        dial_queue: Default::default(),
    };

    (network_event_receiver, swarm_driver, metrics_shutdown_tx)
}

/// Creates a new `ReachabilityCheckSwarmDriver` instance to perform reachability checks.
pub(crate) async fn init_reachability_check_swarm(
    config: NetworkConfig,
) -> Result<(ReachabilityCheckSwarmDriver, Option<watch::Sender<bool>>)> {
    let identify_protocol_str = IDENTIFY_PROTOCOL_STR
        .read()
        .expect("Failed to obtain read lock for IDENTIFY_PROTOCOL_STR")
        .clone();

    let peer_id = PeerId::from(config.keypair.public());
    info!(
        "Self PeerID {peer_id} is represented as kbucket_key {:?}",
        PrettyPrintKBucketKey(NetworkAddress::from(peer_id).as_kbucket_key())
    );

    #[cfg(feature = "open-metrics")]
    let mut metrics_registries = config.metrics_registries;

    // ==== Transport ====
    #[cfg(feature = "open-metrics")]
    let transport = transport::build_transport(&config.keypair, &mut metrics_registries);
    #[cfg(not(feature = "open-metrics"))]
    let transport = transport::build_transport(&self.keypair);
    let transport = if !config.local {
        debug!("Preventing non-global dials");
        // Wrap upper in a transport that prevents dialing local addresses.
        libp2p::core::transport::global_only::Transport::new(transport).boxed()
    } else {
        transport
    };

    let (relay_transport, _relay_client) =
        libp2p::relay::client::new(config.keypair.public().to_peer_id());
    let transport = relay_transport
        .upgrade(libp2p::core::upgrade::Version::V1Lazy)
        .authenticate(
            libp2p::noise::Config::new(&config.keypair)
                .expect("Signing libp2p-noise static DH keypair failed."),
        )
        .multiplex(libp2p::yamux::Config::default())
        .or_transport(transport);

    let transport = transport
        .map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed();

    #[cfg(feature = "open-metrics")]
    let (metrics_recorder, metrics_shutdown_tx) = if let Some(port) = config.metrics_server_port {
        let metrics_recorder =
            NetworkMetricsRecorder::new(&mut metrics_registries, &config.reachability_status);
        // set the reachability_check_progress gauge to 1 to denote in progress
        let _ = metrics_recorder.reachability_check_progress.set(1.0);

        let mut metadata_recorder = MetadataRecorder::new(&mut metrics_registries);
        metadata_recorder.register_peer_id(&peer_id);
        metadata_recorder.register_identify_protocol_string(identify_protocol_str.clone());
        let mut metadata_extended_recorder = MetadataExtendedRecorder::new(&mut metrics_registries);
        metadata_extended_recorder.register_peer_id(&peer_id);
        metadata_extended_recorder.register_pid();
        metadata_extended_recorder.register_bin_version();
        metadata_extended_recorder.register_root_dir(&config.root_dir);
        metadata_extended_recorder.register_log_dir(&config.root_dir.join("logs"));

        let shutdown_tx = run_metrics_server(metrics_registries, port);
        (Some(metrics_recorder), Some(shutdown_tx))
    } else {
        (None, None)
    };

    #[cfg(not(feature = "open-metrics"))]
    let metrics_shutdown_tx = None;

    // Identify Behaviour
    let agent_version = IDENTIFY_REACHABILITY_CHECK_CLIENT_VERSION_STR
        .read()
        .expect("Failed to obtain read lock for IDENTIFY_REACHABILITY_CHECK_CLIENT_VERSION_STR")
        .clone();
    info!(
        "Building Identify with identify_protocol_str: {identify_protocol_str:?} and identify_protocol_str: {identify_protocol_str:?}"
    );
    let identify = {
        let cfg = libp2p::identify::Config::new(identify_protocol_str, config.keypair.public())
            .with_agent_version(agent_version)
            // Enlength the identify interval from default 5 mins to 1 hour.
            .with_interval(RESEND_IDENTIFY_INVERVAL)
            .with_hide_listen_addrs(true);
        libp2p::identify::Behaviour::new(cfg)
    };

    let behaviour = ReachabilityCheckBehaviour { identify };

    let swarm_config = libp2p::swarm::Config::with_tokio_executor()
        .with_idle_connection_timeout(CONNECTION_KEEP_ALIVE_TIMEOUT);

    let swarm = Swarm::new(transport, behaviour, peer_id, swarm_config);

    let swarm_driver = ReachabilityCheckSwarmDriver::new(
        swarm,
        &config.keypair,
        config.local,
        config.listen_addr,
        config.initial_contacts,
        config.no_upnp,
        #[cfg(feature = "open-metrics")]
        metrics_recorder,
    )
    .await?;

    Ok((swarm_driver, metrics_shutdown_tx))
}

pub(crate) fn listen_on_with_retry<TBehaviour: libp2p::swarm::NetworkBehaviour>(
    swarm: &mut Swarm<TBehaviour>,
    addr: libp2p::core::multiaddr::Multiaddr,
) -> Result<ListenerId> {
    let start_time = std::time::Instant::now();
    loop {
        match swarm.listen_on(addr.clone()) {
            Ok(listener_id) => {
                return Ok(listener_id);
            }
            Err(err) => {
                error!("Failed to listen on QUIC address {addr:?}: {err}");

                if start_time.elapsed() > LISTEN_TIMEOUT_S {
                    error!(
                        "Failed to listen on QUIC address {addr:?} after {} seconds",
                        LISTEN_TIMEOUT_S.as_secs()
                    );
                    return Err(NetworkError::ListenFailed(addr));
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
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
                let _ = file.read_to_string(&mut prev_version_str)?;
            }
            Err(err) => {
                warn!("Failed in accessing version file {version_file:?}: {err:?}");
                // Assuming file was not created yet
                info!("Creating a new version file at {version_file:?}");
                let _ = fs::File::create(version_file.clone())?;
            }
        }
    }

    // In case of version mismatch:
    //   * the storage_dir shall be wiped out
    //   * the version file shall be updated
    if cur_version_str != prev_version_str {
        warn!(
            "Trying to wipe out storage dir {storage_dir_path:?}, as cur_version {cur_version_str:?} doesn't match prev_version {prev_version_str:?}"
        );
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
        assert!(
            check_and_wipe_storage_dir_if_necessary(
                root_dir.clone(),
                storage_dir.clone(),
                cur_version.clone()
            )
            .is_ok()
        );
        {
            let mut content_str = String::new();
            let mut file = fs::OpenOptions::new()
                .read(true)
                .open(version_file.clone())
                .expect("Failed to open version file");
            let _ = file
                .read_to_string(&mut content_str)
                .expect("Failed to read from version file");
            assert_eq!(content_str, cur_version);

            drop(file);
        }

        fs::create_dir_all(&storage_dir).expect("Failed to create storage directory");
        assert!(fs::metadata(storage_dir.clone()).is_ok());

        let cur_version = uuid::Uuid::new_v4().to_string();
        assert!(
            check_and_wipe_storage_dir_if_necessary(
                root_dir.clone(),
                storage_dir.clone(),
                cur_version.clone()
            )
            .is_ok()
        );
        {
            let mut content_str = String::new();
            let mut file = fs::OpenOptions::new()
                .read(true)
                .open(version_file.clone())
                .expect("Failed to open version file");
            let _ = file
                .read_to_string(&mut content_str)
                .expect("Failed to read from version file");
            assert_eq!(content_str, cur_version);

            drop(file);
        }
        // The storage_dir shall be removed as version_key changed
        assert!(fs::metadata(storage_dir.clone()).is_err());
    }
}
