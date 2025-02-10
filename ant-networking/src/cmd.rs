// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    config::GetRecordCfg,
    driver::{PendingGetClosestType, SwarmDriver},
    error::{NetworkError, Result},
    event::TerminateNodeReason,
    log_markers::Marker,
    multiaddr_pop_p2p, GetRecordError, MsgResponder, NetworkEvent, ResponseQuorum,
    CLOSE_GROUP_SIZE,
};
use ant_evm::{PaymentQuote, QuotingMetrics};
use ant_protocol::{
    messages::{Cmd, Request, Response},
    storage::{DataTypes, RecordHeader, RecordKind, ValidationType},
    NetworkAddress, PrettyPrintRecordKey,
};
use libp2p::{
    kad::{
        store::{Error as StoreError, RecordStore},
        KBucketDistance as Distance, Record, RecordKey, K_VALUE,
    },
    Multiaddr, PeerId,
};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    time::Duration,
};
use tokio::sync::oneshot;
use xor_name::XorName;

use crate::time::Instant;

const MAX_CONTINUOUS_HDD_WRITE_ERROR: usize = 5;

// Shall be synced with `ant_node::PERIODIC_REPLICATION_INTERVAL_MAX_S`
const REPLICATION_TIMEOUT: Duration = Duration::from_secs(45);

// Throttles replication to at most once every 30 seconds
const MIN_REPLICATION_INTERVAL_S: Duration = Duration::from_secs(30);

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum NodeIssue {
    /// Some connections might be considered to be critical and should be tracked.
    ConnectionIssue,
    /// Data Replication failed
    ReplicationFailure,
    /// Close nodes have reported this peer as bad
    CloseNodesShunning,
    /// Provided a bad quote
    BadQuoting,
    /// Peer failed to pass the chunk proof verification
    FailedChunkProofCheck,
}

impl std::fmt::Display for NodeIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NodeIssue::ConnectionIssue => write!(f, "CriticalConnectionIssue"),
            NodeIssue::ReplicationFailure => write!(f, "ReplicationFailure"),
            NodeIssue::CloseNodesShunning => write!(f, "CloseNodesShunning"),
            NodeIssue::BadQuoting => write!(f, "BadQuoting"),
            NodeIssue::FailedChunkProofCheck => write!(f, "FailedChunkProofCheck"),
        }
    }
}

/// Commands to send to the Swarm
pub enum LocalSwarmCmd {
    /// Get a list of all peers in local RT, with correspondent Multiaddr info attached as well.
    GetPeersWithMultiaddr {
        sender: oneshot::Sender<Vec<(PeerId, Vec<Multiaddr>)>>,
    },
    /// Get a map where each key is the ilog2 distance of that Kbucket
    /// and each value is a vector of peers in that bucket.
    GetKBuckets {
        sender: oneshot::Sender<BTreeMap<u32, Vec<PeerId>>>,
    },
    /// Returns the replicate candidates in range.
    /// In case the range is too narrow, returns at lease CLOSE_GROUP_SIZE peers.
    GetReplicateCandidates {
        data_addr: NetworkAddress,
        sender: oneshot::Sender<Result<Vec<PeerId>>>,
    },
    // Returns up to K_VALUE peers from all the k-buckets from the local Routing Table.
    // And our PeerId as well.
    GetClosestKLocalPeers {
        sender: oneshot::Sender<Vec<PeerId>>,
    },
    // Get closest peers from the local RoutingTable
    GetCloseGroupLocalPeers {
        key: NetworkAddress,
        sender: oneshot::Sender<Vec<PeerId>>,
    },
    GetSwarmLocalState(oneshot::Sender<SwarmLocalState>),
    /// Check if the local RecordStore contains the provided key
    RecordStoreHasKey {
        key: RecordKey,
        sender: oneshot::Sender<Result<bool>>,
    },
    /// Get the Addresses of all the Records held locally
    GetAllLocalRecordAddresses {
        sender: oneshot::Sender<Result<HashMap<NetworkAddress, ValidationType>>>,
    },
    /// Get data from the local RecordStore
    GetLocalRecord {
        key: RecordKey,
        sender: oneshot::Sender<Option<Record>>,
    },
    /// GetLocalQuotingMetrics for this node
    /// Returns the quoting metrics and whether the record at `key` is already stored locally
    GetLocalQuotingMetrics {
        key: RecordKey,
        data_type: u32,
        data_size: usize,
        sender: oneshot::Sender<Result<(QuotingMetrics, bool)>>,
    },
    /// Notify the node received a payment.
    PaymentReceived,
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
        record_type: ValidationType,
        data_type: DataTypes,
    },
    /// Add a peer to the blocklist
    AddPeerToBlockList {
        peer_id: PeerId,
    },
    /// Notify whether peer is in trouble
    RecordNodeIssue {
        peer_id: PeerId,
        issue: NodeIssue,
    },
    // Whether peer is considered as `in trouble` by self
    IsPeerShunned {
        target: NetworkAddress,
        sender: oneshot::Sender<bool>,
    },
    // Quote verification agaisnt historical collected quotes
    QuoteVerification {
        quotes: Vec<(PeerId, PaymentQuote)>,
    },
    // Notify a fetch completion
    FetchCompleted((RecordKey, ValidationType)),
    /// Triggers interval repliation
    /// NOTE: This does result in outgoing messages, but is produced locally
    TriggerIntervalReplication,
    /// Triggers unrelevant record cleanup
    TriggerIrrelevantRecordCleanup,
    /// Add a network density sample
    AddNetworkDensitySample {
        distance: Distance,
    },
    /// Send peer scores (collected from storage challenge) to replication_fetcher
    NotifyPeerScores {
        peer_scores: Vec<(PeerId, bool)>,
    },
    /// Add fresh replicate records into replication_fetcher
    AddFreshReplicateRecords {
        holder: NetworkAddress,
        keys: Vec<(NetworkAddress, ValidationType)>,
    },
}

/// Commands to send to the Swarm
pub enum NetworkSwarmCmd {
    Dial {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<()>>,
    },
    // Get closest peers from the network
    GetClosestPeersToAddressFromNetwork {
        key: NetworkAddress,
        sender: oneshot::Sender<Vec<PeerId>>,
    },

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

    /// Get Record from the Kad network
    GetNetworkRecord {
        key: RecordKey,
        sender: oneshot::Sender<std::result::Result<Record, GetRecordError>>,
        cfg: GetRecordCfg,
    },

    /// Put record to network
    PutRecord {
        record: Record,
        sender: oneshot::Sender<Result<()>>,
        quorum: ResponseQuorum,
    },
    /// Put record to specific node
    PutRecordTo {
        peers: Vec<PeerId>,
        record: Record,
        sender: oneshot::Sender<Result<()>>,
        quorum: ResponseQuorum,
    },
}

/// Debug impl for LocalSwarmCmd to avoid printing full Record, instead only RecodKey
/// and RecordKind are printed.
impl Debug for LocalSwarmCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocalSwarmCmd::PutLocalRecord { record } => {
                write!(
                    f,
                    "LocalSwarmCmd::PutLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(&record.key)
                )
            }
            LocalSwarmCmd::RemoveFailedLocalRecord { key } => {
                write!(
                    f,
                    "LocalSwarmCmd::RemoveFailedLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::AddLocalRecordAsStored {
                key,
                record_type,
                data_type,
            } => {
                write!(
                    f,
                    "LocalSwarmCmd::AddLocalRecordAsStored {{ key: {:?}, record_type: {record_type:?}, data_type: {data_type:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::GetReplicateCandidates { .. } => {
                write!(f, "LocalSwarmCmd::GetReplicateCandidates")
            }
            LocalSwarmCmd::GetClosestKLocalPeers { .. } => {
                write!(f, "LocalSwarmCmd::GetClosestKLocalPeers")
            }
            LocalSwarmCmd::GetCloseGroupLocalPeers { key, .. } => {
                write!(
                    f,
                    "LocalSwarmCmd::GetCloseGroupLocalPeers {{ key: {key:?} }}"
                )
            }
            LocalSwarmCmd::GetLocalQuotingMetrics { .. } => {
                write!(f, "LocalSwarmCmd::GetLocalQuotingMetrics")
            }
            LocalSwarmCmd::PaymentReceived => {
                write!(f, "LocalSwarmCmd::PaymentReceived")
            }
            LocalSwarmCmd::GetLocalRecord { key, .. } => {
                write!(
                    f,
                    "LocalSwarmCmd::GetLocalRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::GetAllLocalRecordAddresses { .. } => {
                write!(f, "LocalSwarmCmd::GetAllLocalRecordAddresses")
            }
            LocalSwarmCmd::GetPeersWithMultiaddr { .. } => {
                write!(f, "LocalSwarmCmd::GetPeersWithMultiaddr")
            }
            LocalSwarmCmd::GetKBuckets { .. } => {
                write!(f, "LocalSwarmCmd::GetKBuckets")
            }
            LocalSwarmCmd::GetSwarmLocalState { .. } => {
                write!(f, "LocalSwarmCmd::GetSwarmLocalState")
            }
            LocalSwarmCmd::RecordStoreHasKey { key, .. } => {
                write!(
                    f,
                    "LocalSwarmCmd::RecordStoreHasKey {:?}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::AddPeerToBlockList { peer_id } => {
                write!(f, "LocalSwarmCmd::AddPeerToBlockList {peer_id:?}")
            }
            LocalSwarmCmd::RecordNodeIssue { peer_id, issue } => {
                write!(
                    f,
                    "LocalSwarmCmd::SendNodeStatus peer {peer_id:?}, issue: {issue:?}"
                )
            }
            LocalSwarmCmd::IsPeerShunned { target, .. } => {
                write!(f, "LocalSwarmCmd::IsPeerInTrouble target: {target:?}")
            }
            LocalSwarmCmd::QuoteVerification { quotes } => {
                write!(
                    f,
                    "LocalSwarmCmd::QuoteVerification of {} quotes",
                    quotes.len()
                )
            }
            LocalSwarmCmd::FetchCompleted((key, record_type)) => {
                write!(
                    f,
                    "LocalSwarmCmd::FetchCompleted({record_type:?} : {:?})",
                    PrettyPrintRecordKey::from(key)
                )
            }
            LocalSwarmCmd::TriggerIntervalReplication => {
                write!(f, "LocalSwarmCmd::TriggerIntervalReplication")
            }
            LocalSwarmCmd::TriggerIrrelevantRecordCleanup => {
                write!(f, "LocalSwarmCmd::TriggerUnrelevantRecordCleanup")
            }
            LocalSwarmCmd::AddNetworkDensitySample { distance } => {
                write!(f, "LocalSwarmCmd::AddNetworkDensitySample({distance:?})")
            }
            LocalSwarmCmd::NotifyPeerScores { peer_scores } => {
                write!(f, "LocalSwarmCmd::NotifyPeerScores({peer_scores:?})")
            }
            LocalSwarmCmd::AddFreshReplicateRecords { holder, keys } => {
                write!(
                    f,
                    "LocalSwarmCmd::AddFreshReplicateRecords({holder:?}, {keys:?})"
                )
            }
        }
    }
}

/// Debug impl for NetworkSwarmCmd to avoid printing full Record, instead only RecodKey
/// and RecordKind are printed.
impl Debug for NetworkSwarmCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkSwarmCmd::Dial { addr, .. } => {
                write!(f, "NetworkSwarmCmd::Dial {{ addr: {addr:?} }}")
            }
            NetworkSwarmCmd::GetNetworkRecord { key, cfg, .. } => {
                write!(
                    f,
                    "NetworkSwarmCmd::GetNetworkRecord {{ key: {:?}, cfg: {cfg:?}",
                    PrettyPrintRecordKey::from(key)
                )
            }
            NetworkSwarmCmd::PutRecord { record, .. } => {
                write!(
                    f,
                    "NetworkSwarmCmd::PutRecord {{ key: {:?} }}",
                    PrettyPrintRecordKey::from(&record.key)
                )
            }
            NetworkSwarmCmd::PutRecordTo { peers, record, .. } => {
                write!(
                    f,
                    "NetworkSwarmCmd::PutRecordTo {{ peers: {peers:?}, key: {:?} }}",
                    PrettyPrintRecordKey::from(&record.key)
                )
            }
            NetworkSwarmCmd::GetClosestPeersToAddressFromNetwork { key, .. } => {
                write!(f, "NetworkSwarmCmd::GetClosestPeers {{ key: {key:?} }}")
            }
            NetworkSwarmCmd::SendResponse { resp, .. } => {
                write!(f, "NetworkSwarmCmd::SendResponse resp: {resp:?}")
            }
            NetworkSwarmCmd::SendRequest { req, peer, .. } => {
                write!(
                    f,
                    "NetworkSwarmCmd::SendRequest req: {req:?}, peer: {peer:?}"
                )
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
    pub(crate) fn handle_network_cmd(&mut self, cmd: NetworkSwarmCmd) -> Result<(), NetworkError> {
        let start = Instant::now();
        let cmd_string;
        match cmd {
            NetworkSwarmCmd::GetNetworkRecord { key, sender, cfg } => {
                cmd_string = "GetNetworkRecord";

                for (pending_query, (inflight_record_query_key, senders, _, _)) in
                    self.pending_get_record.iter_mut()
                {
                    if *inflight_record_query_key == key {
                        debug!(
                            "GetNetworkRecord for {:?} is already in progress. Adding sender to {pending_query:?}",
                            PrettyPrintRecordKey::from(&key)
                        );
                        senders.push(sender);

                        // early exit as we're already processing this query
                        return Ok(());
                    }
                }

                let query_id = self.swarm.behaviour_mut().kademlia.get_record(key.clone());

                debug!(
                    "Record {:?} with task {query_id:?} expected to be held by {:?}",
                    PrettyPrintRecordKey::from(&key),
                    cfg.expected_holders
                );

                if self
                    .pending_get_record
                    .insert(query_id, (key, vec![sender], Default::default(), cfg))
                    .is_some()
                {
                    warn!("An existing get_record task {query_id:?} got replaced");
                }
                // Logging the status of the `pending_get_record`.
                // We also interested in the status of `result_map` (which contains record) inside.
                let total_records: usize = self
                    .pending_get_record
                    .iter()
                    .map(|(_, (_, _, result_map, _))| result_map.len())
                    .sum();
                info!("We now have {} pending get record attempts and cached {total_records} fetched copies",
                      self.pending_get_record.len());
            }
            NetworkSwarmCmd::PutRecord {
                record,
                sender,
                quorum,
            } => {
                cmd_string = "PutRecord";
                let record_key = PrettyPrintRecordKey::from(&record.key).into_owned();
                debug!(
                    "Putting record sized: {:?} to network {:?}",
                    record.value.len(),
                    record_key
                );
                let res = match self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .put_record(record, quorum.get_kad_quorum())
                {
                    Ok(request_id) => {
                        debug!("Sent record {record_key:?} to network. Request id: {request_id:?} to network");
                        Ok(())
                    }
                    Err(error) => {
                        error!("Error sending record {record_key:?} to network");
                        Err(NetworkError::from(error))
                    }
                };

                if let Err(err) = sender.send(res) {
                    error!("Could not send response to PutRecord cmd: {:?}", err);
                }
            }
            NetworkSwarmCmd::PutRecordTo {
                peers,
                record,
                sender,
                quorum,
            } => {
                cmd_string = "PutRecordTo";
                let record_key = PrettyPrintRecordKey::from(&record.key).into_owned();
                debug!(
                    "Putting record {record_key:?} sized: {:?} to {peers:?}",
                    record.value.len(),
                );
                let peers_count = peers.len();
                let request_id = self.swarm.behaviour_mut().kademlia.put_record_to(
                    record,
                    peers.into_iter(),
                    quorum.get_kad_quorum(),
                );
                debug!("Sent record {record_key:?} to {peers_count:?} peers. Request id: {request_id:?}");

                if let Err(err) = sender.send(Ok(())) {
                    error!("Could not send response to PutRecordTo cmd: {:?}", err);
                }
            }

            NetworkSwarmCmd::Dial { addr, sender } => {
                cmd_string = "Dial";

                if let Some(peer_id) = multiaddr_pop_p2p(&mut addr.clone()) {
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
            NetworkSwarmCmd::GetClosestPeersToAddressFromNetwork { key, sender } => {
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

            NetworkSwarmCmd::SendRequest { req, peer, sender } => {
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
            NetworkSwarmCmd::SendResponse { resp, channel } => {
                cmd_string = "SendResponse";
                match channel {
                    // If the response is for `self`, send it directly through the oneshot channel.
                    MsgResponder::FromSelf(channel) => {
                        trace!("Sending response to self");
                        match channel {
                            Some(channel) => {
                                channel
                                    .send(Ok(resp))
                                    .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
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
                            .map_err(NetworkError::OutgoingResponseDropped)?;
                    }
                }
            }
        }

        self.log_handling(cmd_string.to_string(), start.elapsed());

        Ok(())
    }
    pub(crate) fn handle_local_cmd(&mut self, cmd: LocalSwarmCmd) -> Result<(), NetworkError> {
        let start = Instant::now();
        let mut cmd_string;
        match cmd {
            LocalSwarmCmd::TriggerIntervalReplication => {
                cmd_string = "TriggerIntervalReplication";
                self.try_interval_replication()?;
            }
            LocalSwarmCmd::GetLocalQuotingMetrics {
                key,
                data_type,
                data_size,
                sender,
            } => {
                cmd_string = "GetLocalQuotingMetrics";
                let (
                    _index,
                    _total_peers,
                    peers_in_non_full_buckets,
                    num_of_full_buckets,
                    _kbucket_table_stats,
                ) = self.kbuckets_status();
                let estimated_network_size =
                    Self::estimate_network_size(peers_in_non_full_buckets, num_of_full_buckets);
                let (quoting_metrics, is_already_stored) = match self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .quoting_metrics(
                        &key,
                        data_type,
                        data_size,
                        Some(estimated_network_size as u64),
                    ) {
                    Ok(res) => res,
                    Err(err) => {
                        let _res = sender.send(Err(err));
                        return Ok(());
                    }
                };
                self.record_metrics(Marker::QuotingMetrics {
                    quoting_metrics: &quoting_metrics,
                });

                // To avoid sending entire list to client, sending those that:
                //     closer than the CLOSE_GROUP_SIZEth closest node to the target
                let mut bad_nodes: Vec<_> = self
                    .bad_nodes
                    .iter()
                    .filter_map(|(peer_id, (_issue_list, is_bad))| {
                        if *is_bad {
                            Some(NetworkAddress::from_peer(*peer_id))
                        } else {
                            None
                        }
                    })
                    .collect();

                // List is ordered already, hence the last one is always the one wanted
                let kbucket_key = NetworkAddress::from_record_key(&key).as_kbucket_key();
                let closest_peers: Vec<_> = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_closest_local_peers(&kbucket_key)
                    .map(|peer| peer.into_preimage())
                    .take(CLOSE_GROUP_SIZE)
                    .collect();
                // In case of not enough clsest_peers, send the entire list
                if closest_peers.len() >= CLOSE_GROUP_SIZE {
                    let boundary_peer = closest_peers[CLOSE_GROUP_SIZE - 1];
                    let key_address = NetworkAddress::from_record_key(&key);
                    let boundary_distance =
                        key_address.distance(&NetworkAddress::from_peer(boundary_peer));
                    bad_nodes
                        .retain(|peer_addr| key_address.distance(peer_addr) < boundary_distance);
                }

                let _res = sender.send(Ok((quoting_metrics, is_already_stored)));
            }
            LocalSwarmCmd::PaymentReceived => {
                cmd_string = "PaymentReceived";
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .payment_received();
            }
            LocalSwarmCmd::GetLocalRecord { key, sender } => {
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

            LocalSwarmCmd::PutLocalRecord { record } => {
                cmd_string = "PutLocalRecord";
                let key = record.key.clone();
                let record_key = PrettyPrintRecordKey::from(&key);

                let record_type = match RecordHeader::from_record(&record) {
                    Ok(record_header) => {
                        match record_header.kind {
                            RecordKind::DataOnly(DataTypes::Chunk) => ValidationType::Chunk,
                            RecordKind::DataOnly(_) => {
                                let content_hash = XorName::from_content(&record.value);
                                ValidationType::NonChunk(content_hash)
                            }
                            RecordKind::DataWithPayment(_) => {
                                error!("Record {record_key:?} with payment shall not be stored locally.");
                                return Err(NetworkError::InCorrectRecordHeader);
                            }
                        }
                    }
                    Err(err) => {
                        error!("For record {record_key:?}, failed to parse record_header {err:?}");
                        return Err(NetworkError::InCorrectRecordHeader);
                    }
                };

                let result = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .put_verified(record, record_type.clone());

                match result {
                    Ok(_) => {
                        // `replication_fetcher.farthest_acceptable_distance` shall only get
                        // shrinked, instead of expanding, even with more nodes joined to share
                        // the responsibility. Hence no need to reset it.
                        // Also, as `record_store` is `prune 1 on 1 success put`, which means
                        // once capacity reached max_records, there is only chance of rising slowly.
                        // Due to the async/parrellel handling in replication_fetcher & record_store.
                    }
                    Err(StoreError::MaxRecords) => {
                        // In case the capacity reaches full, restrict replication_fetcher to
                        // only fetch entries not farther than the current farthest record
                        let farthest = self
                            .swarm
                            .behaviour_mut()
                            .kademlia
                            .store_mut()
                            .get_farthest()?;
                        self.replication_fetcher.set_farthest_on_full(farthest);
                    }
                    Err(_) => {
                        // Nothing special to do for these errors,
                        // All error cases are further logged and bubbled up below
                    }
                }

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
                    .get_farthest_replication_distance()?
                {
                    self.replication_fetcher
                        .set_replication_distance_range(distance);
                }

                if let Err(err) = result {
                    error!("Can't store verified record {record_key:?} locally: {err:?}");
                    cmd_string = "PutLocalRecord error";
                    self.log_handling(cmd_string.to_string(), start.elapsed());
                    return Err(err.into());
                };
            }
            LocalSwarmCmd::AddLocalRecordAsStored {
                key,
                record_type,
                data_type,
            } => {
                cmd_string = "AddLocalRecordAsStored";
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .mark_as_stored(key, record_type, data_type);
                // Reset counter on any success HDD write.
                self.hard_disk_write_error = 0;
            }
            LocalSwarmCmd::RemoveFailedLocalRecord { key } => {
                info!("Removing Record locally, for {key:?}");
                cmd_string = "RemoveFailedLocalRecord";
                self.swarm.behaviour_mut().kademlia.store_mut().remove(&key);
                self.hard_disk_write_error = self.hard_disk_write_error.saturating_add(1);
                // When there is certain amount of continuous HDD write error,
                // the hard disk is considered as full, and the node shall be terminated.
                if self.hard_disk_write_error > MAX_CONTINUOUS_HDD_WRITE_ERROR {
                    self.send_event(NetworkEvent::TerminateNode {
                        reason: TerminateNodeReason::HardDiskWriteError,
                    });
                }
            }
            LocalSwarmCmd::RecordStoreHasKey { key, sender } => {
                cmd_string = "RecordStoreHasKey";
                let has_key = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .contains(&key);
                let _ = sender.send(has_key);
            }
            LocalSwarmCmd::GetAllLocalRecordAddresses { sender } => {
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
            LocalSwarmCmd::GetKBuckets { sender } => {
                cmd_string = "GetKBuckets";
                let mut ilog2_kbuckets = BTreeMap::new();
                for kbucket in self.swarm.behaviour_mut().kademlia.kbuckets() {
                    let range = kbucket.range();
                    if let Some(distance) = range.0.ilog2() {
                        let peers_in_kbucket = kbucket
                            .iter()
                            .map(|peer_entry| peer_entry.node.key.into_preimage())
                            .collect::<Vec<PeerId>>();
                        let _ = ilog2_kbuckets.insert(distance, peers_in_kbucket);
                    } else {
                        // This shall never happen.
                        error!("bucket is ourself ???!!!");
                    }
                }
                let _ = sender.send(ilog2_kbuckets);
            }
            LocalSwarmCmd::GetPeersWithMultiaddr { sender } => {
                cmd_string = "GetPeersWithMultiAddr";
                let mut result: Vec<(PeerId, Vec<Multiaddr>)> = vec![];
                for kbucket in self.swarm.behaviour_mut().kademlia.kbuckets() {
                    let peers_in_kbucket = kbucket
                        .iter()
                        .map(|peer_entry| {
                            (
                                peer_entry.node.key.into_preimage(),
                                peer_entry.node.value.clone().into_vec(),
                            )
                        })
                        .collect::<Vec<(PeerId, Vec<Multiaddr>)>>();
                    result.extend(peers_in_kbucket);
                }
                let _ = sender.send(result);
            }
            LocalSwarmCmd::GetCloseGroupLocalPeers { key, sender } => {
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
            LocalSwarmCmd::GetClosestKLocalPeers { sender } => {
                cmd_string = "GetClosestKLocalPeers";
                let _ = sender.send(self.get_closest_k_value_local_peers());
            }
            LocalSwarmCmd::GetReplicateCandidates { data_addr, sender } => {
                cmd_string = "GetReplicateCandidates";
                let _ = sender.send(self.get_replicate_candidates(&data_addr));
            }
            LocalSwarmCmd::GetSwarmLocalState(sender) => {
                cmd_string = "GetSwarmLocalState";
                let current_state = SwarmLocalState {
                    connected_peers: self.swarm.connected_peers().cloned().collect(),
                    listeners: self.swarm.listeners().cloned().collect(),
                };

                sender
                    .send(current_state)
                    .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
            }
            LocalSwarmCmd::AddPeerToBlockList { peer_id } => {
                cmd_string = "AddPeerToBlockList";
                self.swarm.behaviour_mut().blocklist.block_peer(peer_id);
            }
            LocalSwarmCmd::RecordNodeIssue { peer_id, issue } => {
                cmd_string = "RecordNodeIssues";
                self.record_node_issue(peer_id, issue);
            }
            LocalSwarmCmd::IsPeerShunned { target, sender } => {
                cmd_string = "IsPeerInTrouble";
                let is_bad = if let Some(peer_id) = target.as_peer_id() {
                    if let Some((_issues, is_bad)) = self.bad_nodes.get(&peer_id) {
                        *is_bad
                    } else {
                        false
                    }
                } else {
                    false
                };
                let _ = sender.send(is_bad);
            }
            LocalSwarmCmd::QuoteVerification { quotes } => {
                cmd_string = "QuoteVerification";
                for (peer_id, quote) in quotes {
                    // Do nothing if already being bad
                    if let Some((_issues, is_bad)) = self.bad_nodes.get(&peer_id) {
                        if *is_bad {
                            continue;
                        }
                    }
                    self.verify_peer_quote(peer_id, quote);
                }
            }
            LocalSwarmCmd::FetchCompleted((key, record_type)) => {
                info!(
                    "Fetch of {record_type:?} {:?} early completed, may have fetched an old version of the record.",
                    PrettyPrintRecordKey::from(&key)
                );
                cmd_string = "FetchCompleted";
                let new_keys_to_fetch = self
                    .replication_fetcher
                    .notify_fetch_early_completed(key, record_type);
                if !new_keys_to_fetch.is_empty() {
                    self.send_event(NetworkEvent::KeysToFetchForReplication(new_keys_to_fetch));
                }
            }
            LocalSwarmCmd::TriggerIrrelevantRecordCleanup => {
                cmd_string = "TriggerIrrelevantRecordCleanup";
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .store_mut()
                    .cleanup_irrelevant_records();
            }
            LocalSwarmCmd::AddNetworkDensitySample { distance } => {
                cmd_string = "AddNetworkDensitySample";
                self.network_density_samples.add(distance);
            }
            LocalSwarmCmd::NotifyPeerScores { peer_scores } => {
                cmd_string = "NotifyPeerScores";
                self.replication_fetcher.add_peer_scores(peer_scores);
            }
            LocalSwarmCmd::AddFreshReplicateRecords { holder, keys } => {
                cmd_string = "AddFreshReplicateRecords";
                let _ = self.add_keys_to_replication_fetcher(holder, keys, true);
            }
        }

        self.log_handling(cmd_string.to_string(), start.elapsed());

        Ok(())
    }

    pub(crate) fn record_node_issue(&mut self, peer_id: PeerId, issue: NodeIssue) {
        info!("Peer {peer_id:?} is reported as having issue {issue:?}");
        let (issue_vec, is_bad) = self.bad_nodes.entry(peer_id).or_default();
        let mut _new_bad_behaviour = None;
        let mut is_connection_issue = false;

        // If being considered as bad already, skip certain operations
        if !(*is_bad) {
            // Remove outdated entries
            issue_vec.retain(|(_, timestamp)| timestamp.elapsed().as_secs() < 300);

            // check if vec is already 10 long, if so, remove the oldest issue
            // we only track 10 issues to avoid mem leaks
            if issue_vec.len() == 10 {
                issue_vec.remove(0);
            }

            // To avoid being too sensitive, only consider as a new issue
            // when after certain while since the last one
            let is_new_issue = if let Some((_issue, timestamp)) = issue_vec.last() {
                timestamp.elapsed().as_secs() > 10
            } else {
                true
            };

            if is_new_issue {
                issue_vec.push((issue, Instant::now()));
            }

            // Only consider candidate as a bad node when:
            //   accumulated THREE same kind issues within certain period
            for (issue, _timestamp) in issue_vec.iter() {
                let issue_counts = issue_vec
                    .iter()
                    .filter(|(i, _timestamp)| *issue == *i)
                    .count();
                if issue_counts >= 3 {
                    // If it is a connection issue, we don't need to consider it as a bad node
                    if matches!(issue, NodeIssue::ConnectionIssue) {
                        is_connection_issue = true;
                    } else {
                        *is_bad = true;
                    }
                    _new_bad_behaviour = Some(issue.clone());
                    info!("Peer {peer_id:?} accumulated {issue_counts} times of issue {issue:?}. Consider it as a bad node now.");
                    // Once a bad behaviour detected, no point to continue
                    break;
                }
            }
        }

        // Give the faulty connection node more chances by removing the issue from the list. It is still evicted from
        // the routing table.
        if is_connection_issue {
            issue_vec.retain(|(issue, _timestamp)| !matches!(issue, NodeIssue::ConnectionIssue));
            info!("Evicting bad peer {peer_id:?} due to connection issue from RT.");
            if let Some(dead_peer) = self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id) {
                self.update_on_peer_removal(*dead_peer.node.key.preimage());
            }
            // TODO: re-enable once confirmed with upscale test
            // return;
        }

        // Disable disconnection and blocking which are not because of the connection issue.
        // TODO: re-enable once confirmed with upscale test
        // if *is_bad {
        //     info!("Evicting bad peer {peer_id:?} from RT.");
        //     if let Some(dead_peer) = self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id) {
        //         self.update_on_peer_removal(*dead_peer.node.key.preimage());
        //     }

        //     if let Some(bad_behaviour) = new_bad_behaviour {
        //         // inform the bad node about it and add to the blocklist after that (not for connection issues)
        //         self.record_metrics(Marker::PeerConsideredAsBad { bad_peer: &peer_id });

        //         warn!("Peer {peer_id:?} is considered as bad due to {bad_behaviour:?}. Informing the peer and adding to blocklist.");
        //         // response handling
        //         let (tx, rx) = oneshot::channel();
        //         let local_swarm_cmd_sender = self.local_cmd_sender.clone();
        //         tokio::spawn(async move {
        //             match rx.await {
        //                 Ok(result) => {
        //                     debug!("Got response for Cmd::PeerConsideredAsBad from {peer_id:?} {result:?}");
        //                     if let Err(err) = local_swarm_cmd_sender
        //                         .send(LocalSwarmCmd::AddPeerToBlockList { peer_id })
        //                         .await
        //                     {
        //                         error!("SwarmDriver failed to send LocalSwarmCmd: {err}");
        //                     }
        //                 }
        //                 Err(err) => {
        //                     error!("Failed to get response from one shot channel for Cmd::PeerConsideredAsBad : {err:?}");
        //                 }
        //             }
        //         });

        //         // request
        //         let request = Request::Cmd(Cmd::PeerConsideredAsBad {
        //             detected_by: NetworkAddress::from_peer(self.self_peer_id),
        //             bad_peer: NetworkAddress::from_peer(peer_id),
        //             bad_behaviour: bad_behaviour.to_string(),
        //         });
        //         self.queue_network_swarm_cmd(NetworkSwarmCmd::SendRequest {
        //             req: request,
        //             peer: peer_id,
        //             sender: Some(tx),
        //         });
        //     }
        // }
    }

    fn verify_peer_quote(&mut self, peer_id: PeerId, quote: PaymentQuote) {
        if let Some(history_quote) = self.quotes_history.get(&peer_id) {
            if !history_quote.historical_verify(&quote) {
                info!("From {peer_id:?}, detected a bad quote {quote:?} against history_quote {history_quote:?}");
                self.record_node_issue(peer_id, NodeIssue::BadQuoting);
                return;
            }

            if history_quote.is_newer_than(&quote) {
                return;
            }
        }

        let _ = self.quotes_history.insert(peer_id, quote);
    }

    fn try_interval_replication(&mut self) -> Result<()> {
        // Add a last_replication field to track the last time replication was performed
        if let Some(last_replication) = self.last_replication {
            if last_replication.elapsed() < MIN_REPLICATION_INTERVAL_S {
                info!("Skipping replication as minimum interval hasn't elapsed");
                return Ok(());
            }
        }
        // Store the current time as the last replication time
        self.last_replication = Some(Instant::now());

        let self_addr = NetworkAddress::from_peer(self.self_peer_id);
        let mut replicate_targets = self.get_replicate_candidates(&self_addr)?;

        let now = Instant::now();
        self.replication_targets
            .retain(|_peer_id, timestamp| *timestamp > now);
        // Only carry out replication to peer that not replicated to it recently
        replicate_targets.retain(|peer_id| !self.replication_targets.contains_key(peer_id));
        if replicate_targets.is_empty() {
            return Ok(());
        }

        let all_records: Vec<_> = self
            .swarm
            .behaviour_mut()
            .kademlia
            .store_mut()
            .record_addresses_ref()?
            .values()
            .cloned()
            .collect();

        if !all_records.is_empty() {
            debug!(
                "Sending a replication list of {} keys to {replicate_targets:?} ",
                all_records.len()
            );
            let request = Request::Cmd(Cmd::Replicate {
                holder: NetworkAddress::from_peer(self.self_peer_id),
                keys: all_records
                    .into_iter()
                    .map(|(addr, val_type, _data_type)| (addr, val_type))
                    .collect(),
            });
            for peer_id in replicate_targets {
                self.queue_network_swarm_cmd(NetworkSwarmCmd::SendRequest {
                    req: request.clone(),
                    peer: peer_id,
                    sender: None,
                });

                let _ = self
                    .replication_targets
                    .insert(peer_id, now + REPLICATION_TIMEOUT);
            }
        }

        Ok(())
    }

    // Replies with in-range replicate candidates
    // Fall back to expected_candidates peers if range is too narrow.
    // Note that:
    //   * For general replication, replicate candidates shall be closest to self, but with wider range
    //   * For replicate fresh records, the replicate candidates shall be the closest to data

    pub(crate) fn get_replicate_candidates(
        &mut self,
        target: &NetworkAddress,
    ) -> Result<Vec<PeerId>> {
        let is_periodic_replicate = target.as_peer_id().is_some();
        let expected_candidates = if is_periodic_replicate {
            CLOSE_GROUP_SIZE * 2
        } else {
            CLOSE_GROUP_SIZE
        };

        // get closest peers from buckets, sorted by increasing distance to the target
        let kbucket_key = target.as_kbucket_key();
        let closest_k_peers: Vec<PeerId> = self
            .swarm
            .behaviour_mut()
            .kademlia
            .get_closest_local_peers(&kbucket_key)
            // Map KBucketKey<PeerId> to PeerId.
            .map(|key| key.into_preimage())
            .take(K_VALUE.get())
            .collect();

        if let Some(responsible_range) = self
            .swarm
            .behaviour_mut()
            .kademlia
            .store_mut()
            .get_farthest_replication_distance()?
        {
            let peers_in_range = get_peers_in_range(&closest_k_peers, target, responsible_range);

            if peers_in_range.len() >= expected_candidates {
                return Ok(peers_in_range);
            }
        }

        // In case the range is too narrow, fall back to at least expected_candidates peers.
        Ok(closest_k_peers
            .iter()
            .take(expected_candidates)
            .cloned()
            .collect())
    }
}

/// Returns the nodes that within the defined distance.
fn get_peers_in_range(peers: &[PeerId], address: &NetworkAddress, range: Distance) -> Vec<PeerId> {
    peers
        .iter()
        .filter_map(|peer_id| {
            if address.distance(&NetworkAddress::from_peer(*peer_id)) <= range {
                Some(*peer_id)
            } else {
                None
            }
        })
        .collect()
}
