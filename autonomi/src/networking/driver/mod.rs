// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// private modules (the innards of the NetworkDriver)
mod swarm_events;
mod task_handler;

use std::{num::NonZeroUsize, time::Duration};

use crate::networking::interface::{Command, NetworkTask};
use crate::networking::NetworkError;
use ant_protocol::version::{IDENTIFY_CLIENT_VERSION_STR, IDENTIFY_PROTOCOL_STR};
use ant_protocol::PrettyPrintRecordKey;
use ant_protocol::{
    messages::{Query, Request, Response},
    version::REQ_RESPONSE_VERSION_STR,
};
use libp2p::kad::store::MemoryStoreConfig;
use libp2p::kad::NoKnownPeers;
use libp2p::{
    core::muxing::StreamMuxerBox,
    futures::StreamExt,
    identity::Keypair,
    kad::{self, store::MemoryStore},
    multiaddr::Protocol,
    quic::tokio::Transport as QuicTransport,
    request_response::{self, cbor::codec::Codec as CborCodec, ProtocolSupport},
    swarm::NetworkBehaviour,
    Multiaddr, PeerId, StreamProtocol, Swarm, Transport,
};
use task_handler::TaskHandler;
use tokio::sync::mpsc;

use ant_protocol::constants::{
    KAD_STREAM_PROTOCOL_ID, MAX_PACKET_SIZE, MAX_RECORD_SIZE, REPLICATION_FACTOR,
};

/// Libp2p defaults to 10s which is quite fast, we are more patient
pub const REQ_TIMEOUT: Duration = Duration::from_secs(30);
/// Libp2p defaults to 60s for kad queries, we are more patient
pub const KAD_QUERY_TIMEOUT: Duration = Duration::from_secs(120);
/// Libp2p defaults to 3, we are more aggressive
pub const KAD_ALPHA: NonZeroUsize = NonZeroUsize::new(3).expect("KAD_ALPHA must be > 0");
/// Interval of resending identify to connected peers.
/// Libp2p defaults to 5 minutes, we use 1 hour.
const RESEND_IDENTIFY_INVERVAL: Duration = Duration::from_secs(3600); // todo: taken over from ant-networking. Why 1 hour?
/// Size of the LRU cache for peers and their addresses.
/// Libp2p defaults to 100, we use 2k.
const PEER_CACHE_SIZE: usize = 2_000;

/// Driver for the Autonomi Client Network
///
/// Acts as the background runner and interface for the libp2p swarm which talks to nodes on the network
///
/// Do NOT add any fields unless absolutely necessary
/// Please see how long SwarmDriver ended up in ant-networking to understand why
///
/// Please read the doc comment above
pub(crate) struct NetworkDriver {
    /// libp2p interaction through the swarm and its events
    swarm: Swarm<AutonomiClientBehaviour>,
    /// can receive tasks from the [`crate::Network`]
    task_receiver: mpsc::Receiver<NetworkTask>,
    /// can receive commands from the [`crate::driver::task_handler::TaskHandler`]
    cmd_receiver: mpsc::Receiver<Command>,
    /// pending tasks currently awaiting swarm events to progress
    /// this is an opaque struct that can only be mutated by the module were [`crate::driver::task_handler::TaskHandler`] is defined
    pending_tasks: TaskHandler,
}

#[derive(NetworkBehaviour)]
pub(crate) struct AutonomiClientBehaviour {
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub identify: libp2p::identify::Behaviour,
    pub request_response: request_response::cbor::Behaviour<Request, Response>,
}

impl NetworkDriver {
    /// Create a new network runner
    pub fn new(task_receiver: mpsc::Receiver<NetworkTask>) -> Self {
        // random new client id
        let keypair = Keypair::generate_ed25519();
        let peer_id = PeerId::from(keypair.public());

        info!("Client Peer ID: {peer_id}");

        // set transport
        let quic_config = libp2p::quic::Config::new(&keypair);
        let transport_gen = QuicTransport::new(quic_config);
        let trans = transport_gen.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)));
        let transport = trans.boxed();

        // identify behaviour
        let identify = {
            let identify_protocol_str = IDENTIFY_PROTOCOL_STR
                .read()
                .expect("Could not get IDENTIFY_PROTOCOL_STR")
                .clone();
            let agent_version = IDENTIFY_CLIENT_VERSION_STR
                .read()
                .expect("Could not get IDENTIFY_CLIENT_VERSION_STR")
                .clone();
            let cfg = libp2p::identify::Config::new(identify_protocol_str, keypair.public())
                .with_agent_version(agent_version)
                .with_interval(RESEND_IDENTIFY_INVERVAL) // todo: find a way to disable this. Clients shouldn't need to
                .with_hide_listen_addrs(true)
                .with_cache_size(PEER_CACHE_SIZE);
            libp2p::identify::Behaviour::new(cfg)
        };

        // autonomi requests
        let request_response = {
            let cfg = request_response::Config::default().with_request_timeout(REQ_TIMEOUT);

            let req_res_version_str = REQ_RESPONSE_VERSION_STR
                .read()
                .expect("no protocol version")
                .clone();
            let stream = StreamProtocol::try_from_owned(req_res_version_str)
                .expect("StreamProtocol should start with a /");
            let proto = [(stream, ProtocolSupport::Outbound)];

            let codec = CborCodec::<Request, Response>::default()
                .set_request_size_maximum(2 * MAX_PACKET_SIZE as u64);

            request_response::Behaviour::with_codec(codec, proto, cfg)
        };

        // kademlia
        let store_cfg = MemoryStoreConfig {
            max_value_bytes: MAX_RECORD_SIZE,
            ..Default::default()
        };
        let store = MemoryStore::with_config(peer_id, store_cfg);
        let mut kad_cfg = libp2p::kad::Config::new(StreamProtocol::new(KAD_STREAM_PROTOCOL_ID));
        kad_cfg
            .set_kbucket_inserts(libp2p::kad::BucketInserts::OnConnected)
            .set_max_packet_size(MAX_PACKET_SIZE)
            .set_parallelism(KAD_ALPHA)
            .set_replication_factor(REPLICATION_FACTOR)
            .set_query_timeout(KAD_QUERY_TIMEOUT)
            .set_periodic_bootstrap_interval(None);

        // setup kad and autonomi requests as our behaviour
        let behaviour = AutonomiClientBehaviour {
            kademlia: libp2p::kad::Behaviour::with_config(peer_id, store, kad_cfg),
            identify,
            request_response,
        };

        // create swarm
        let swarm_config = libp2p::swarm::Config::with_tokio_executor();
        let swarm = Swarm::new(transport, behaviour, peer_id, swarm_config);

        let (cmd_sender, cmd_receiver) = mpsc::channel(1);

        let task_handler = TaskHandler::new(cmd_sender);

        Self {
            swarm,
            task_receiver,
            cmd_receiver,
            pending_tasks: task_handler,
        }
    }

    /// Run the network runner, loops forever waiting for tasks and processing them
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                // tasks sent by client
                task = self.task_receiver.recv() => {
                    match task {
                        Some(task) => self.process_task(task),
                        None => {
                            info!("Task receiver closed, exiting");
                            break;
                        }
                    }
                },
                cmd = self.cmd_receiver.recv() => {
                    match cmd {
                        Some(cmd) => self.process_cmd(cmd),
                        None => {
                            info!("Command receiver closed, exiting");
                            break;
                        }
                    }
                },
                // swarm events
                swarm_event = self.swarm.select_next_some() => {
                    if let Err(e) = self.process_swarm_event(swarm_event) {
                        error!("Error processing swarm event: {e}");
                    }
                }
            }
        }
    }

    /// Shorthand for kad behaviour mut
    fn kad(&mut self) -> &mut kad::Behaviour<MemoryStore> {
        &mut self.swarm.behaviour_mut().kademlia
    }

    /// Shorthand for request response behaviour mut
    fn req(&mut self) -> &mut request_response::cbor::Behaviour<Request, Response> {
        &mut self.swarm.behaviour_mut().request_response
    }

    /// Add peers to our routing table
    pub(crate) fn connect_to_peers(&mut self, peers: Vec<Multiaddr>) -> Result<(), NoKnownPeers> {
        for contact in peers {
            let contact_id = match contact.iter().find(|p| matches!(p, Protocol::P2p(_))) {
                Some(Protocol::P2p(id)) => id,
                _ => panic!("No peer id found in contact"),
            };

            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&contact_id, contact);
        }

        self.swarm.behaviour_mut().kademlia.bootstrap().map(|_| ())
    }

    /// Process a task sent by the client, start the query on kad and add it to the pending tasks
    /// Events from the swarm will help update the task, they are handled in [`crate::driver::NetworkDriver::process_swarm_event`]
    fn process_task(&mut self, task: NetworkTask) {
        match task {
            NetworkTask::GetClosestPeers { addr, resp, n } => {
                let query_id = self
                    .kad()
                    .get_n_closest_peers(addr.to_record_key().to_vec(), n);
                self.pending_tasks
                    .insert_task(query_id, NetworkTask::GetClosestPeers { addr, resp, n });
            }
            NetworkTask::GetRecord { addr, quorum, resp } => {
                let query_id = self.kad().get_record(addr.to_record_key());
                self.pending_tasks
                    .insert_task(query_id, NetworkTask::GetRecord { addr, quorum, resp });
            }
            NetworkTask::PutRecord {
                record,
                to,
                quorum,
                resp,
            } => {
                let query_id = if to.is_empty() {
                    let _pretty_key = PrettyPrintRecordKey::from(&record.key);
                    let error_str =
                        "Target holders of record {_pretty_key:?} shall be provided".to_string();
                    if let Err(e) = resp.send(Err(NetworkError::PutRecordError(error_str))) {
                        error!("Error sending put record response: {e:?}");
                    }
                    return;
                } else {
                    for peer_info in &to {
                        // Add the peer addresses to our cache before sending a query.
                        for addr in &peer_info.addrs {
                            self.swarm.add_peer_address(peer_info.peer_id, addr.clone());
                        }
                    }

                    let to = to.clone().into_iter().map(|p| p.peer_id);

                    self.kad().put_record_to(record.clone(), to, quorum)
                };

                self.pending_tasks.insert_task(
                    query_id,
                    NetworkTask::PutRecord {
                        record,
                        to,
                        quorum,
                        resp,
                    },
                );
            }
            NetworkTask::GetQuote {
                addr,
                peer,
                data_type,
                data_size,
                resp,
            } => {
                let req = Request::Query(Query::GetStoreQuote {
                    key: addr.clone(),
                    data_type,
                    data_size,
                    nonce: None,
                    difficulty: 0,
                });

                // Add the peer addresses to our cache before sending a request.
                for addr in &peer.addrs {
                    self.swarm.add_peer_address(peer.peer_id, addr.clone());
                }

                let req_id = self.req().send_request(&peer.peer_id, req);

                self.pending_tasks.insert_query(
                    req_id,
                    NetworkTask::GetQuote {
                        addr,
                        peer,
                        data_type,
                        data_size,
                        resp,
                    },
                );
            }
            NetworkTask::Request {
                peer_id,
                addresses,
                req,
                resp,
            } => {
                // Add the peer addresses to our cache before sending a request.
                for addr in &addresses.0 {
                    self.swarm.add_peer_address(peer_id, addr.clone());
                }

                let req_id = self.req().send_request(&peer_id, req.clone());

                self.pending_tasks.insert_query(
                    req_id,
                    NetworkTask::Request {
                        peer_id,
                        addresses,
                        req,
                        resp,
                    },
                );
            }
        }
    }

    /// Process commands.
    fn process_cmd(&mut self, cmd: Command) {
        match cmd {
            Command::TerminateQuery(query_id) => {
                if let Some(mut query) = self.kad().query_mut(&query_id) {
                    query.finish();
                }
            }
        }
    }
}
