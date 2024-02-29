// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    driver::{PendingGetClosestType, SwarmDriver},
    error::{Error, Result},
    multiaddr_pop_p2p, GetRecordCfg, GetRecordError, MsgResponder, NetworkEvent, CLOSE_GROUP_SIZE,
    REPLICATE_RANGE,
};
use bytes::Bytes;
use libp2p::{
    kad::{store::RecordStore, Quorum, Record, RecordKey},
    swarm::dial_opts::DialOpts,
    Multiaddr, PeerId,
};
use sn_protocol::{
    messages::{Cmd, Request, Response},
    storage::{RecordHeader, RecordKind, RecordType},
    NetworkAddress, PrettyPrintRecordKey,
};
use sn_transfers::NanoTokens;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
};
use tokio::sync::oneshot;
use xor_name::XorName;

use crate::target_arch::Instant;

const MAX_CONTINUOUS_HDD_WRITE_ERROR: usize = 5;

/// Commands to send to the Swarm
#[allow(clippy::large_enum_variant)]
pub enum SwarmCmd {
    StartListening {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<()>>,
    },
    Dial {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<()>>,
    },
    DialWithOpts {
        opts: DialOpts,
        sender: oneshot::Sender<Result<()>>,
    },
    // Returns all the peers from all the k-buckets from the local Routing Table.
    // This includes our PeerId as well.
    GetAllLocalPeers {
        sender: oneshot::Sender<Vec<PeerId>>,
    },
    /// Get a map where each key is the ilog2 distance of that Kbucket and each value is a vector of peers in that
    /// bucket.
    GetKBuckets {
        sender: oneshot::Sender<BTreeMap<u32, Vec<PeerId>>>,
    },
    // Returns up to K_VALUE peers from all the k-buckets from the local Routing Table.
    // And our PeerId as well.
    GetClosestKLocalPeers {
        sender: oneshot::Sender<Vec<PeerId>>,
    },
    // Get closest peers from the network
    GetClosestPeersToAddressFromNetwork {
        key: NetworkAddress,
        sender: oneshot::Sender<Vec<PeerId>>,
    },
    // Get closest peers from the local RoutingTable
    GetCloseGroupLocalPeers {
        key: NetworkAddress,
        sender: oneshot::Sender<Vec<PeerId>>,
    },
    GetSwarmLocalState(oneshot::Sender<SwarmLocalState>),
    // Send Request to the PeerId.
    SendRequest {
        req: Request,
        peer: PeerId,

        // If a `sender` is provided, the requesting node will await for a `Response` from the
        // Peer. The result is then returned at the call site.
        //
        // If a `sender` is not provided, the requesting node will not wait for the Peer's
        // response. Instead we trigger a `NetworkEvent::ResponseReceived` which calls the common
        // `response_handler`
        sender: Option<oneshot::Sender<Result<Response>>>,
    },
    SendResponse {
        resp: Response,
        channel: MsgResponder,
    },
    /// Check if the local RecordStore contains the provided key
    RecordStoreHasKey {
        key: RecordKey,
        sender: oneshot::Sender<bool>,
    },
    /// Get the Addresses of all the Records held locally
    GetAllLocalRecordAddresses {
        sender: oneshot::Sender<HashMap<NetworkAddress, RecordType>>,
    },
    /// Get Record from the Kad network
    GetNetworkRecord {
        key: RecordKey,
        sender: oneshot::Sender<std::result::Result<Record, GetRecordError>>,
        cfg: GetRecordCfg,
    },
    /// GetLocalStoreCost for this node
    GetLocalStoreCost {
        key: RecordKey,
        sender: oneshot::Sender<NanoTokens>,
    },
    /// Notify the node received a payment.
    PaymentReceived,
    /// Get data from the local RecordStore
    GetLocalRecord {
        key: RecordKey,
        sender: oneshot::Sender<Option<Record>>,
    },
    /// Put record to network
    PutRecord {
        record: Record,
        sender: oneshot::Sender<Result<()>>,
        quorum: Quorum,
    },
    /// Put record to specific node
    PutRecordTo {
        peers: Vec<PeerId>,
        record: Record,
        sender: oneshot::Sender<Result<()>>,
        quorum: Quorum,
    },
    /// Put record to the local RecordStore
    PutLocalRecord {
        record: Record,
    },
    /// Remove a local record from the RecordStore
    /// Typically because the write failed
    RemoveFailedLocalRecord {
        key: RecordKey,
    },
    /// Add a local record to the RecordStore's HashSet of stored records
    /// This should be done after the record has been stored to disk
    AddLocalRecordAsStored {
        key: RecordKey,
        record_type: RecordType,
    },
    /// Triggers interval repliation
    TriggerIntervalReplication,
    /// Subscribe to a given Gossipsub topic
    GossipsubSubscribe(String),
    /// Unsubscribe from a given Gossipsub topic
    GossipsubUnsubscribe(String),
    /// Publish a message through Gossipsub protocol
    GossipsubPublish {
        /// Topic to publish on
        topic_id: String,
        /// Raw bytes of the message to publish
        msg: Bytes,
    },
    GossipHandler,
    /// Notify whether peer is in trouble
    SendNodeStatus {
        peer_id: PeerId,
        addrs: HashSet<Multiaddr>,
        is_bad: bool,
    },
    // Whether peer is considered as `in trouble` by self
    IsPeerInTrouble {
        target: NetworkAddress,
        sender: oneshot::Sender<bool>,
    },
}

/// Debug impl for SwarmCmd to avoid printing full Record, instead only RecodKey
/// and RecordKind are printed.
impl Debug for SwarmCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwarmCmd::StartListening { addr, .. } => {
                write!(f, "SwarmCmd::StartListening {{ addr: {addr:?} }}")
            }
            SwarmCmd::Dial { addr, .. } => {
                write!(f, "SwarmCmd::Dial {{ addr: {addr:?} }}")
            }
            SwarmCmd::GetNetworkRecord { key, cfg, .. } => {
                write!(
                    f,
                    "SwarmCmd::GetNetworkRecord {{ key: {:?}, cfg: {cfg:?}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            SwarmCmd::PutRecord { record, .. } => {
                write!(
                    f,
                    "SwarmCmd::PutRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(&record.key)
                )
            }
            SwarmCmd::PutRecordTo { peers, record, .. } => {
                write!(
                    f,
                    "SwarmCmd::PutRecordTo {{ peers: {peers:?}, key: {:?} }}",
                    PrettyPrintRecordKey::from(&record.key)
                )
            }
            SwarmCmd::PutLocalRecord { record } => {
                write!(
                    f,
                    "SwarmCmd::PutLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(&record.key)
                )
            }
            SwarmCmd::RemoveFailedLocalRecord { key } => {
                write!(
                    f,
                    "SwarmCmd::RemoveFailedLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            SwarmCmd::AddLocalRecordAsStored { key, record_type } => {
                write!(
                    f,
                    "SwarmCmd::AddLocalRecordAsStored {{ key: {:?}, record_type: {record_type:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            SwarmCmd::TriggerIntervalReplication => {
                write!(f, "SwarmCmd::TriggerIntervalReplication")
            }
            SwarmCmd::GossipsubSubscribe(topic) => {
                write!(f, "SwarmCmd::GossipsubSubscribe({topic:?})")
            }
            SwarmCmd::GossipsubUnsubscribe(topic) => {
                write!(f, "SwarmCmd::GossipsubUnsubscribe({topic:?})")
            }
            SwarmCmd::GossipsubPublish { topic_id, msg } => {
                write!(
                    f,
                    "SwarmCmd::GossipsubPublish {{ topic_id: {topic_id:?}, msg len: {:?} }}",
                    msg.len()
                )
            }
            SwarmCmd::DialWithOpts { opts, .. } => {
                write!(f, "SwarmCmd::DialWithOpts {{ opts: {opts:?} }}")
            }
            SwarmCmd::GetClosestPeersToAddressFromNetwork { key, .. } => {
                write!(f, "SwarmCmd::GetClosestPeers {{ key: {key:?} }}")
            }
            SwarmCmd::GetClosestKLocalPeers { .. } => {
                write!(f, "SwarmCmd::GetClosestKLocalPeers")
            }
            SwarmCmd::GetCloseGroupLocalPeers { key, .. } => {
                write!(f, "SwarmCmd::GetCloseGroupLocalPeers {{ key: {key:?} }}")
            }
            SwarmCmd::GetLocalStoreCost { .. } => {
                write!(f, "SwarmCmd::GetLocalStoreCost")
            }
            SwarmCmd::PaymentReceived => {
                write!(f, "SwarmCmd::PaymentReceived")
            }
            SwarmCmd::GetLocalRecord { key, .. } => {
                write!(
                    f,
                    "SwarmCmd::GetLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            SwarmCmd::GetAllLocalRecordAddresses { .. } => {
                write!(f, "SwarmCmd::GetAllLocalRecordAddresses")
            }
            SwarmCmd::GetAllLocalPeers { .. } => {
                write!(f, "SwarmCmd::GetAllLocalPeers")
            }
            SwarmCmd::GetKBuckets { .. } => {
                write!(f, "SwarmCmd::GetKBuckets")
            }
            SwarmCmd::GetSwarmLocalState { .. } => {
                write!(f, "SwarmCmd::GetSwarmLocalState")
            }
            SwarmCmd::RecordStoreHasKey { key, .. } => {
                write!(
                    f,
                    "SwarmCmd::RecordStoreHasKey {:?}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            SwarmCmd::SendResponse { resp, .. } => {
                write!(f, "SwarmCmd::SendResponse resp: {resp:?}")
            }
            SwarmCmd::SendRequest { req, peer, .. } => {
                write!(f, "SwarmCmd::SendRequest req: {req:?}, peer: {peer:?}")
            }
            SwarmCmd::GossipHandler => {
                write!(f, "SwarmCmd::GossipHandler")
            }
            SwarmCmd::SendNodeStatus {
                peer_id, is_bad, ..
            } => {
                write!(
                    f,
                    "SwarmCmd::SendNodeStatus peer {peer_id:?}, is_bad: {is_bad:?}"
                )
            }
            SwarmCmd::IsPeerInTrouble { target, .. } => {
                write!(f, "SwarmCmd::IsPeerInTrouble target: {target:?}")
            }
        }
    }
}
/// Snapshot of information kept in the Swarm's local state
#[derive(Debug, Clone)]
pub struct SwarmLocalState {
    /// List of currently connected peers
    pub connected_peers: Vec<PeerId>,
    /// List of addresses the node is currently listening on
    pub listeners: Vec<Multiaddr>,
}

impl SwarmDriver {
    pub(crate) fn handle_cmd(&mut self, cmd: SwarmCmd) -> Result<(), Error> {
        let start = Instant::now();
        let mut cmd_string = "";
        match cmd {
            SwarmCmd::TriggerIntervalReplication => {
                cmd_string = "TriggerIntervalReplication";
                self.try_interval_replication()?;
            }
            SwarmCmd::GetNetworkRecord { key, sender, cfg } => {
                cmd_string = "GetNetworkRecord";
                let query_id = self.swarm.behaviour_mut().kademlia.get_record(key.clone());

                debug!(
                    "Record {:?} with task {query_id:?} expected to be held by {:?}",
                    PrettyPrintRecordKey::from(&key),
                    cfg.expected_holders
                );

                if self
                    .pending_get_record
                    .insert(query_id, (sender, Default::default(), cfg))
                    .is_some()
                {
                    warn!("An existing get_record task {query_id:?} got replaced");
                }
                // Logging the status of the `pending_get_record`.
                // We also interested in the status of `result_map` (which contains record) inside.
                let total_records: usize = self
                    .pending_get_record
                    .iter()
                    .map(|(_, (_, result_map, _))| result_map.len())
                    .sum();
                info!("We now have {} pending get record attempts and cached {total_records} fetched copies",
                      self.pending_get_record.len());
            }
            SwarmCmd::GetLocalStoreCost { key, sender } => {
                cmd_string = "GetLocalStoreCost";
                let record_exists = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .contains(&key);
                let cost = if record_exists {
                    NanoTokens::zero()
                } else {
                    self.swarm.behaviour_mut().kademlia.store_mut().store_cost()
                };

                let _res = sender.send(cost);
            }
            SwarmCmd::PaymentReceived => {
                cmd_string = "PaymentReceived";
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .payment_received();
            }
            SwarmCmd::GetLocalRecord { key, sender } => {
                cmd_string = "GetLocalRecord";
                let record = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .get(&key)
                    .map(|rec| rec.into_owned());
                let _ = sender.send(record);
            }
            SwarmCmd::PutRecord {
                record,
                sender,
                quorum,
            } => {
                cmd_string = "PutRecord";
                let record_key = PrettyPrintRecordKey::from(&record.key).into_owned();
                trace!(
                    "Putting record sized: {:?} to network {:?}",
                    record.value.len(),
                    record_key
                );
                let res = match self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .put_record(record, quorum)
                {
                    Ok(request_id) => {
                        trace!("Sent record {record_key:?} to network. Request id: {request_id:?} to network");
                        Ok(())
                    }
                    Err(error) => {
                        error!("Error sending record {record_key:?} to network");
                        Err(Error::from(error))
                    }
                };

                if let Err(err) = sender.send(res) {
                    error!("Could not send response to PutRecord cmd: {:?}", err);
                }
            }
            SwarmCmd::PutRecordTo {
                peers,
                record,
                sender,
                quorum,
            } => {
                cmd_string = "PutRecordTo";
                let record_key = PrettyPrintRecordKey::from(&record.key).into_owned();
                trace!(
                    "Putting record {record_key:?} sized: {:?} to {peers:?}",
                    record.value.len(),
                );
                let peers_count = peers.len();
                let request_id = self.swarm.behaviour_mut().kademlia.put_record_to(
                    record,
                    peers.into_iter(),
                    quorum,
                );
                trace!("Sent record {record_key:?} to {peers_count:?} peers. Request id: {request_id:?}");

                if let Err(err) = sender.send(Ok(())) {
                    error!("Could not send response to PutRecordTo cmd: {:?}", err);
                }
            }
            SwarmCmd::PutLocalRecord { record } => {
                cmd_string = "PutLocalRecord";
                let key = record.key.clone();
                let record_key = PrettyPrintRecordKey::from(&key);

                let record_type = match RecordHeader::from_record(&record) {
                    Ok(record_header) => {
                        match record_header.kind {
                            RecordKind::Chunk => RecordType::Chunk,
                            RecordKind::Spend | RecordKind::Register => {
                                let content_hash = XorName::from_content(&record.value);
                                RecordType::NonChunk(content_hash)
                            }
                            RecordKind::ChunkWithPayment | RecordKind::RegisterWithPayment => {
                                error!("Record {record_key:?} with payment shall not be stored locally.");
                                return Err(Error::InCorrectRecordHeader);
                            }
                        }
                    }
                    Err(err) => {
                        error!("For record {record_key:?}, failed to parse record_header {err:?}");
                        return Err(Error::InCorrectRecordHeader);
                    }
                };

                let result = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .put_verified(record, record_type.clone());
                // No matter storing the record succeeded or not,
                // the entry shall be removed from the `replication_fetcher`.
                // In case of local store error, re-attempt will be carried out
                // within the next replication round.
                let new_keys_to_fetch = self
                    .replication_fetcher
                    .notify_about_new_put(key.clone(), record_type);
                if !new_keys_to_fetch.is_empty() {
                    self.send_event(NetworkEvent::KeysToFetchForReplication(new_keys_to_fetch));
                }

                // The record_store will prune far records and setup a `distance range`,
                // once reached the `max_records` cap.
                if let Some(distance) = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .get_distance_range()
                {
                    self.replication_fetcher.set_distance_range(distance);
                }

                if let Err(err) = result {
                    error!("Cann't store verified record {record_key:?} locally: {err:?}");
                    cmd_string = "PutLocalRecord error";
                    self.log_handling(cmd_string.to_string(), start.elapsed());
                    return Err(err.into());
                };
            }
            SwarmCmd::AddLocalRecordAsStored { key, record_type } => {
                trace!("Adding Record locally, for {key:?} and {record_type:?}");
                cmd_string = "AddLocalRecordAsStored";
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .mark_as_stored(key, record_type);
                // Reset counter on any success HDD write.
                self.hard_disk_write_error = 0;
            }
            SwarmCmd::RemoveFailedLocalRecord { key } => {
                info!("Removing Record locally, for {key:?}");
                cmd_string = "RemoveFailedLocalRecord";
                self.swarm.behaviour_mut().kademlia.store_mut().remove(&key);
                self.hard_disk_write_error = self.hard_disk_write_error.saturating_add(1);
                // When there is certain amount of continuous HDD write error,
                // the hard disk is considered as full, and the node shall be terminated.
                if self.hard_disk_write_error > MAX_CONTINUOUS_HDD_WRITE_ERROR {
                    self.send_event(NetworkEvent::TerminateNode);
                }
            }
            SwarmCmd::RecordStoreHasKey { key, sender } => {
                cmd_string = "RecordStoreHasKey";
                let has_key = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .contains(&key);
                let _ = sender.send(has_key);
            }
            SwarmCmd::GetAllLocalRecordAddresses { sender } => {
                cmd_string = "GetAllLocalRecordAddresses";
                #[allow(clippy::mutable_key_type)] // for the Bytes in NetworkAddress
                let addresses = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .record_addresses();
                let _ = sender.send(addresses);
            }

            SwarmCmd::StartListening { addr, sender } => {
                cmd_string = "StartListening";
                let _ = match self.swarm.listen_on(addr) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(e.into())),
                };
            }
            SwarmCmd::Dial { addr, sender } => {
                cmd_string = "Dial";

                let mut addr_copy = addr.clone();
                if let Some(peer_id) = multiaddr_pop_p2p(&mut addr_copy) {
                    // Only consider the dial peer is bootstrap node when proper PeerId is provided.
                    if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(peer_id) {
                        let ilog2 = kbucket.range().0.ilog2();
                        let peers = self.bootstrap_peers.entry(ilog2).or_default();
                        peers.insert(peer_id);
                    }
                }
                let _ = match self.dial(addr) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(e.into())),
                };
            }
            SwarmCmd::DialWithOpts { opts, sender } => {
                cmd_string = "DialWithOpts";
                let _ = match self.dial_with_opts(opts) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(e.into())),
                };
            }
            SwarmCmd::GetClosestPeersToAddressFromNetwork { key, sender } => {
                cmd_string = "GetClosestPeersToAddressFromNetwork";
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_closest_peers(key.as_bytes());
                let _ = self.pending_get_closest_peers.insert(
                    query_id,
                    (
                        PendingGetClosestType::FunctionCall(sender),
                        Default::default(),
                    ),
                );
            }
            SwarmCmd::GetAllLocalPeers { sender } => {
                cmd_string = "GetAllLocalPeers";
                let _ = sender.send(self.get_all_local_peers());
            }
            SwarmCmd::GetKBuckets { sender } => {
                cmd_string = "GetKBuckets";
                let mut ilog2_kbuckets = BTreeMap::new();
                for kbucket in self.swarm.behaviour_mut().kademlia.kbuckets() {
                    let range = kbucket.range();
                    if let Some(distance) = range.0.ilog2() {
                        let peers_in_kbucket = kbucket
                            .iter()
                            .map(|peer_entry| peer_entry.node.key.clone().into_preimage())
                            .collect::<Vec<PeerId>>();
                        let _ = ilog2_kbuckets.insert(distance, peers_in_kbucket);
                    } else {
                        // This shall never happen.
                        error!("bucket is ourself ???!!!");
                    }
                }
                let _ = sender.send(ilog2_kbuckets);
            }
            SwarmCmd::GetCloseGroupLocalPeers { key, sender } => {
                cmd_string = "GetCloseGroupLocalPeers";
                let key = key.as_kbucket_key();
                // calls `kbuckets.closest_keys(key)` internally, which orders the peers by
                // increasing distance
                // Note it will return all peers, heance a chop down is required.
                let closest_peers = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_closest_local_peers(&key)
                    .map(|peer| peer.into_preimage())
                    .take(CLOSE_GROUP_SIZE)
                    .collect();

                let _ = sender.send(closest_peers);
            }
            SwarmCmd::GetClosestKLocalPeers { sender } => {
                cmd_string = "GetClosestKLocalPeers";
                let _ = sender.send(self.get_closest_k_value_local_peers());
            }
            SwarmCmd::SendRequest { req, peer, sender } => {
                cmd_string = "SendRequest";
                // If `self` is the recipient, forward the request directly to our upper layer to
                // be handled.
                // `self` then handles the request and sends a response back again to itself.
                if peer == *self.swarm.local_peer_id() {
                    trace!("Sending query request to self");
                    if let Request::Query(query) = req {
                        self.send_event(NetworkEvent::QueryRequestReceived {
                            query,
                            channel: MsgResponder::FromSelf(sender),
                        });
                    } else {
                        // We should never receive a Replicate request from ourselves.
                        // we already hold this data if we do... so we can ignore
                        trace!("Replicate cmd to self received, ignoring");
                    }
                } else {
                    let request_id = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&peer, req);
                    trace!("Sending request {request_id:?} to peer {peer:?}");
                    let _ = self.pending_requests.insert(request_id, sender);

                    trace!("Pending Requests now: {:?}", self.pending_requests.len());
                }
            }
            SwarmCmd::SendResponse { resp, channel } => {
                cmd_string = "SendResponse";
                match channel {
                    // If the response is for `self`, send it directly through the oneshot channel.
                    MsgResponder::FromSelf(channel) => {
                        trace!("Sending response to self");
                        match channel {
                            Some(channel) => {
                                channel
                                    .send(Ok(resp))
                                    .map_err(|_| Error::InternalMsgChannelDropped)?;
                            }
                            None => {
                                // responses that are not awaited at the call site must be handled
                                // separately
                                self.send_event(NetworkEvent::ResponseReceived { res: resp });
                            }
                        }
                    }
                    MsgResponder::FromPeer(channel) => {
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(channel, resp)
                            .map_err(Error::OutgoingResponseDropped)?;
                    }
                }
            }
            SwarmCmd::GetSwarmLocalState(sender) => {
                cmd_string = "GetSwarmLocalState";
                let current_state = SwarmLocalState {
                    connected_peers: self.swarm.connected_peers().cloned().collect(),
                    listeners: self.swarm.listeners().cloned().collect(),
                };

                sender
                    .send(current_state)
                    .map_err(|_| Error::InternalMsgChannelDropped)?;
            }
            SwarmCmd::GossipsubSubscribe(topic_id) => {
                let topic_id = libp2p::gossipsub::IdentTopic::new(topic_id);
                if let Some(gossip) = self.swarm.behaviour_mut().gossipsub.as_mut() {
                    gossip.subscribe(&topic_id)?;
                }
            }
            SwarmCmd::GossipsubUnsubscribe(topic_id) => {
                let topic_id = libp2p::gossipsub::IdentTopic::new(topic_id);

                if let Some(gossip) = self.swarm.behaviour_mut().gossipsub.as_mut() {
                    gossip.unsubscribe(&topic_id)?;
                }
            }
            SwarmCmd::GossipsubPublish { topic_id, msg } => {
                cmd_string = "GossipsubPublish";
                // If we publish a Gossipsub message, we might not receive the same message on our side.
                // Hence push an event to notify that we've published a message
                if self.is_gossip_handler {
                    self.send_event(NetworkEvent::GossipsubMsgPublished {
                        topic: topic_id.clone(),
                        msg: msg.clone(),
                    });
                }
                let topic_id = libp2p::gossipsub::IdentTopic::new(topic_id);
                if let Some(gossip) = self.swarm.behaviour_mut().gossipsub.as_mut() {
                    gossip.publish(topic_id, msg)?;
                }
            }
            SwarmCmd::GossipHandler => {
                self.is_gossip_handler = true;
            }
            SwarmCmd::SendNodeStatus {
                peer_id,
                addrs,
                is_bad,
            } => {
                cmd_string = "SendNodeStatus";
                let _ = self.bad_nodes_ongoing_verifications.remove(&peer_id);
                if is_bad {
                    info!("Peer {peer_id:?} is considered as bad");
                    let _ = self.bad_nodes.insert(peer_id);
                } else {
                    trace!(%peer_id, ?addrs, "peer considered as active, attempting to add addresses to routing table");

                    for multiaddr in &addrs {
                        let _routing_update = self
                            .swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, multiaddr.clone());
                    }
                }
            }
            SwarmCmd::IsPeerInTrouble { target, sender } => {
                cmd_string = "IsPeerInTrouble";
                if let Some(peer_id) = target.as_peer_id() {
                    let _ = sender.send(self.bad_nodes.contains(&peer_id));
                } else {
                    let _ = sender.send(false);
                }
            }
        }

        self.log_handling(cmd_string.to_string(), start.elapsed());

        Ok(())
    }

    fn try_interval_replication(&mut self) -> Result<()> {
        // get closest peers from buckets, sorted by increasing distance to us
        let our_peer_id = self.self_peer_id.into();
        let closest_k_peers = self
            .swarm
            .behaviour_mut()
            .kademlia
            .get_closest_local_peers(&our_peer_id)
            // Map KBucketKey<PeerId> to PeerId.
            .map(|key| key.into_preimage());

        // Only grab the closest nodes within the REPLICATE_RANGE
        let replicate_targets = closest_k_peers
            .into_iter()
            // add some leeway to allow for divergent knowledge
            .take(REPLICATE_RANGE)
            .collect::<Vec<_>>();

        let all_records: Vec<_> = self
            .swarm
            .behaviour_mut()
            .kademlia
            .store_mut()
            .record_addresses_ref()
            .values()
            .cloned()
            .collect();

        if !all_records.is_empty() {
            trace!(
                "Sending a replication list of {} keys to {replicate_targets:?} ",
                all_records.len()
            );
            let request = Request::Cmd(Cmd::Replicate {
                holder: NetworkAddress::from_peer(self.self_peer_id),
                keys: all_records,
            });
            for peer_id in replicate_targets {
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer_id, request.clone());
                trace!("Sending request {request_id:?} to peer {peer_id:?}");
                let _ = self.pending_requests.insert(request_id, None);
            }
            trace!("Pending Requests now: {:?}", self.pending_requests.len());
        }

        Ok(())
    }
}
