// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    error::{Error, Result},
    msg::MsgCodec,
    SwarmDriver,
};
use crate::{
    domain::storage::DiskBackedRecordStore,
    network::{sort_peers_by_address, sort_peers_by_key, CLOSE_GROUP_SIZE, IDENTIFY_AGENT_STR},
    protocol::{
        messages::{Cmd, QueryResponse, ReplicatedData, Request, Response},
        storage::Chunk,
        NetworkAddress,
    },
};

#[cfg(feature = "local-discovery")]
use libp2p::mdns;

use libp2p::{
    kad::{kbucket::Key as KBucketKey, GetRecordOk, Kademlia, KademliaEvent, QueryResult, K_VALUE},
    multiaddr::Protocol,
    request_response::{self, ResponseChannel as PeerResponseChannel},
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId,
};
#[cfg(feature = "local-discovery")]
use std::collections::hash_map;
use std::collections::HashSet;
use tokio::sync::oneshot;
use tracing::{info, warn};

// Threshold of times of `OutgoingConnectionError` detected within the period.
// If higher than this number of times detected,
// the peer is counted as dropped out from the network.
const DEAD_PEER_DETECTION_THRESHOLD: usize = 3;

// Defines how close that a node will trigger repliation.
// That is, the node has to be among the REPLICATION_RANGE closest to data,
// to carry out the replication.
const REPLICATION_RANGE: usize = 2;

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NodeEvent")]
pub(super) struct NodeBehaviour {
    pub(super) request_response: request_response::Behaviour<MsgCodec>,
    pub(super) kademlia: Kademlia<DiskBackedRecordStore>,
    #[cfg(feature = "local-discovery")]
    pub(super) mdns: mdns::tokio::Behaviour,
    pub(super) identify: libp2p::identify::Behaviour,
}

#[derive(Debug)]
pub(super) enum NodeEvent {
    MsgReceived(request_response::Event<Request, Response>),
    Kademlia(KademliaEvent),
    #[cfg(feature = "local-discovery")]
    Mdns(Box<mdns::Event>),
    Identify(Box<libp2p::identify::Event>),
}

impl From<request_response::Event<Request, Response>> for NodeEvent {
    fn from(event: request_response::Event<Request, Response>) -> Self {
        NodeEvent::MsgReceived(event)
    }
}

impl From<KademliaEvent> for NodeEvent {
    fn from(event: KademliaEvent) -> Self {
        NodeEvent::Kademlia(event)
    }
}

#[cfg(feature = "local-discovery")]
impl From<mdns::Event> for NodeEvent {
    fn from(event: mdns::Event) -> Self {
        NodeEvent::Mdns(Box::new(event))
    }
}

impl From<libp2p::identify::Event> for NodeEvent {
    fn from(event: libp2p::identify::Event) -> Self {
        NodeEvent::Identify(Box::new(event))
    }
}

#[derive(Debug)]
/// Channel to send the `Response` through.
pub enum MsgResponder {
    /// Respond to a request from `self` through a simple one-shot channel.
    FromSelf(oneshot::Sender<Result<Response>>),
    /// Respond to a request from a peer in the network.
    FromPeer(PeerResponseChannel<Response>),
}

#[derive(Debug)]
/// Events forwarded by the underlying Network; to be used by the upper layers
pub enum NetworkEvent {
    /// Incoming `Request` from a peer
    RequestReceived {
        /// Request
        req: Request,
        /// The channel to send the `Response` through
        channel: MsgResponder,
    },
    /// Emitted when the DHT is updated
    PeerAdded(PeerId),
    /// Started listening on a new address
    NewListenAddr(Multiaddr),
}

impl SwarmDriver {
    // Handle `SwarmEvents`
    pub(super) async fn handle_swarm_events<EventError: std::error::Error>(
        &mut self,
        event: SwarmEvent<NodeEvent, EventError>,
    ) -> Result<()> {
        let span = info_span!("Handling a swarm event");
        let _ = span.enter();
        match event {
            SwarmEvent::Behaviour(NodeEvent::MsgReceived(event)) => {
                if let Err(e) = self.handle_msg(event).await {
                    warn!("MsgReceivedError: {e:?}");
                }
            }
            SwarmEvent::Behaviour(NodeEvent::Kademlia(ref event)) => match event {
                KademliaEvent::OutboundQueryProgressed {
                    id,
                    result: QueryResult::GetClosestPeers(Ok(closest_peers)),
                    stats,
                    step,
                } => {
                    trace!("Query task {id:?} returned with peers {closest_peers:?}, {stats:?} - {step:?}");

                    let (sender, mut current_closest) =
                        self.pending_get_closest_peers.remove(id).ok_or_else(|| {
                            trace!("Can't locate query task {id:?}, shall be completed already.");
                            Error::ReceivedKademliaEventDropped(event.clone())
                        })?;

                    // TODO: consider order the result and terminate when reach any of the
                    //       following creterias:
                    //   1, `stats.num_pending()` is 0
                    //   2, `stats.duration()` is longer than a defined period
                    let new_peers: HashSet<PeerId> =
                        closest_peers.peers.clone().into_iter().collect();
                    current_closest.extend(new_peers);
                    if current_closest.len() >= usize::from(K_VALUE) || step.last {
                        sender
                            .send(current_closest)
                            .map_err(|_| Error::InternalMsgChannelDropped)?;
                    } else {
                        let _ = self
                            .pending_get_closest_peers
                            .insert(*id, (sender, current_closest));
                    }
                }
                KademliaEvent::OutboundQueryProgressed {
                    id,
                    result: QueryResult::GetRecord(result),
                    stats,
                    step,
                } => {
                    trace!("Record query task {id:?} returned with result, {stats:?} - {step:?}");
                    if let Ok(GetRecordOk::FoundRecord(peer_record)) = result {
                        trace!(
                            "Query {id:?} returned with record {:?} from peer {:?}",
                            peer_record.record.key,
                            peer_record.peer
                        );
                        if let Some(sender) = self.pending_query.remove(id) {
                            sender
                                .send(Ok(QueryResponse::GetChunk(Ok(Chunk::new(
                                    peer_record.record.value.clone().into(),
                                )))))
                                .map_err(|_| Error::InternalMsgChannelDropped)?;
                        }
                    } else {
                        warn!("Query {id:?} failed to get record with result {result:?}");
                        if step.last {
                            // To avoid the caller wait forever on a non-existring entry
                            if let Some(sender) = self.pending_query.remove(id) {
                                sender
                                    .send(Err(Error::RecordNotFound))
                                    .map_err(|_| Error::InternalMsgChannelDropped)?;
                            }
                        }
                        // TODO: send an error response back?
                    }
                }
                KademliaEvent::RoutingUpdated {
                    peer, is_new_peer, ..
                } => {
                    if *is_new_peer {
                        self.event_sender
                            .send(NetworkEvent::PeerAdded(*peer))
                            .await?;
                        self.try_trigger_replication(peer, false);
                    }
                }
                KademliaEvent::InboundRequest { request } => {
                    info!("got inbound request: {request:?}");
                }
                todo => {
                    error!("KademliaEvent has not been implemented: {todo:?}");
                }
            },
            SwarmEvent::Behaviour(NodeEvent::Identify(iden)) => {
                info!("IdentifyEvent: {iden:?}");
                match *iden {
                    libp2p::identify::Event::Received { peer_id, info } => {
                        if info.agent_version.starts_with(IDENTIFY_AGENT_STR) {
                            info!("{:?} Adding peer to routing table, based on received identify info from {peer_id:?}: {info:?}", self.self_peer_id);
                            for multiaddr in info.listen_addrs {
                                let _routing_update = self
                                    .swarm
                                    .behaviour_mut()
                                    .kademlia
                                    .add_address(&peer_id, multiaddr);
                            }
                        }
                    }
                    libp2p::identify::Event::Sent { .. } => {}
                    libp2p::identify::Event::Pushed { .. } => {}
                    libp2p::identify::Event::Error { .. } => {}
                }
            }
            #[cfg(feature = "local-discovery")]
            SwarmEvent::Behaviour(NodeEvent::Mdns(mdns_event)) => match *mdns_event {
                mdns::Event::Discovered(list) => {
                    for (peer_id, multiaddr) in list {
                        info!("Node discovered and dialing: {multiaddr:?}");

                        let mut dial_failed = None;
                        // TODO: Deduplicate this functionality by calling in on SwarmCmd::Dial
                        if let hash_map::Entry::Vacant(dial_entry) =
                            self.pending_dial.entry(peer_id)
                        {
                            let (sender, _receiver) = oneshot::channel();
                            let _ = dial_entry.insert(sender);
                            // TODO: Dropping the receiver immediately might get logged as error later.
                            if let Err(error) = self
                                .swarm
                                .dial(multiaddr.with(Protocol::P2p(peer_id.into())))
                            {
                                dial_failed = Some(error);
                            }
                        }

                        // if we error'd out, send the error back
                        if let Some(error) = dial_failed {
                            if let Some(sender) = self.pending_dial.remove(&peer_id) {
                                let _ = sender.send(Err(error.into()));
                            }
                        }
                    }
                }
                mdns::Event::Expired(peer) => {
                    info!("mdns peer {peer:?} expired");
                }
            },
            SwarmEvent::NewListenAddr { address, .. } => {
                let local_peer_id = *self.swarm.local_peer_id();
                let address = address.with(Protocol::P2p(local_peer_id.into()));
                self.event_sender
                    .send(NetworkEvent::NewListenAddr(address.clone()))
                    .await?;
                info!("Local node is listening on {address:?}");
            }
            SwarmEvent::IncomingConnection { .. } => {}
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                if endpoint.is_dialer() {
                    info!("Connected with {peer_id:?}");
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Ok(()));
                    }
                }
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                endpoint,
                cause,
                ..
            } => {
                info!("Connection closed to Peer {peer_id} - {endpoint:?} - {cause:?}");
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                warn!("Having OutgoingConnectionError {peer_id:?} - {error:?}");
                if let Some(peer_id) = peer_id {
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Err(error.into()));
                    } else {
                        info!("OutgoingConnectionError is due to non pending_dial to {peer_id}");
                    }
                    // A dead peer will cause a bunch of `OutgoingConnectionError`s
                    // to be received within a short period.
                    if let Some(value) = self.potential_dead_peers.get_mut(&peer_id) {
                        *value += 1;
                        if *value > DEAD_PEER_DETECTION_THRESHOLD {
                            trace!("Detected dead peer {peer_id:?}");
                            self.try_trigger_replication(&peer_id, true);
                            let _ = self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                        }
                    } else {
                        let _ = self.potential_dead_peers.insert(peer_id, 1);
                    }
                }
            }
            SwarmEvent::IncomingConnectionError { .. } => {}
            SwarmEvent::Dialing(peer_id) => info!("Dialing {peer_id}"),
            todo => error!("SwarmEvent has not been implemented: {todo:?}"),
        }
        Ok(())
    }

    // Replication is triggered when the newly added peer or the dead peer was among our closest.
    fn try_trigger_replication(&mut self, peer: &PeerId, is_dead_peer: bool) {
        let our_address = NetworkAddress::from_peer(self.self_peer_id);
        // Fetch from local shall be enough.
        let closest_peers: Vec<_> = self
            .swarm
            .behaviour_mut()
            .kademlia
            .get_closest_local_peers(&our_address.as_kbucket_key())
            .collect();
        let target = NetworkAddress::from_peer(*peer).as_kbucket_key();
        if closest_peers.iter().any(|key| *key == target) {
            let mut all_peers: Vec<PeerId> = vec![];
            for kbucket in self.swarm.behaviour_mut().kademlia.kbuckets() {
                for entry in kbucket.iter() {
                    all_peers.push(entry.node.key.clone().into_preimage());
                }
            }
            let churned_peer_address = NetworkAddress::from_peer(*peer);
            // Only nearby peers (two times of the CLOSE_GROUP_SIZE) may affect the later on
            // calculation of `closest peers to each entry`.
            // Hecence to reduce the computation work, no need to take all peers.
            let sorted_peers: Vec<PeerId> =
                if let Ok(sorted_peers) = sort_peers_by_address(all_peers, &churned_peer_address) {
                    sorted_peers
                        .iter()
                        .take(2 * CLOSE_GROUP_SIZE)
                        .cloned()
                        .collect()
                } else {
                    return;
                };
            if sorted_peers.len() <= CLOSE_GROUP_SIZE {
                return;
            }

            let distance_bar = NetworkAddress::from_peer(sorted_peers[CLOSE_GROUP_SIZE])
                .distance(&churned_peer_address);

            // The fetched entries are records that supposed to be held by the churned_peer.
            let entries_to_be_replicated = self
                .swarm
                .behaviour_mut()
                .kademlia
                .store_mut()
                .entries_to_be_replicated(churned_peer_address.as_kbucket_key(), distance_bar);
            let storage_dir = self
                .swarm
                .behaviour_mut()
                .kademlia
                .store_mut()
                .storage_dir();

            for key in entries_to_be_replicated.iter() {
                let record_key = KBucketKey::from(key.to_vec());
                let closest_peers: Vec<_> = if let Ok(sorted_peers) =
                    sort_peers_by_key(sorted_peers.clone(), &record_key)
                {
                    sorted_peers
                        .iter()
                        .take(CLOSE_GROUP_SIZE + 1)
                        .cloned()
                        .collect()
                } else {
                    continue;
                };

                // Only carry out replication when self within REPLICATION_RANGE
                let replicate_range = NetworkAddress::from_peer(closest_peers[REPLICATION_RANGE]);
                if our_address.as_kbucket_key().distance(&record_key)
                    >= replicate_range.as_kbucket_key().distance(&record_key)
                {
                    continue;
                }

                let dst = if is_dead_peer {
                    // If the churned peer is a dead peer, then the replication target
                    // shall be: the `CLOSE_GROUP_SIZE`th closest node to each data entry.
                    if !closest_peers.iter().any(|p| *p == *peer) {
                        continue;
                    }
                    closest_peers[CLOSE_GROUP_SIZE]
                } else {
                    *peer
                };
                if let Some(record) = DiskBackedRecordStore::read_from_disk(key, &storage_dir) {
                    let chunk = Chunk::new(record.value.clone().into());
                    let request = Request::Cmd(Cmd::Replicate(ReplicatedData::Chunk(chunk)));
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&dst, request);
                }
            }
        }
    }
}
