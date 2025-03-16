// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    driver::PendingGetClosestType, get_graph_entry_from_record, time::Instant, Addresses,
    GetRecordCfg, GetRecordError, NetworkError, Result, SwarmDriver, CLOSE_GROUP_SIZE,
};
use ant_protocol::{
    storage::{try_serialize_record, DataTypes, GraphEntry, RecordKind},
    NetworkAddress, PrettyPrintRecordKey,
};
use itertools::Itertools;
use libp2p::kad::{
    self, GetClosestPeersError, InboundRequest, PeerRecord, ProgressStep, QueryId, QueryResult,
    QueryStats, Record, K_VALUE,
};
use std::collections::{hash_map::Entry, BTreeSet, HashSet};
use tokio::sync::oneshot;
use xor_name::XorName;

impl SwarmDriver {
    pub(super) fn handle_kad_event(&mut self, kad_event: libp2p::kad::Event) -> Result<()> {
        let start = Instant::now();
        let event_string;

        match kad_event {
            kad::Event::OutboundQueryProgressed {
                id,
                result: QueryResult::GetClosestPeers(Ok(closest_peers)),
                ref stats,
                ref step,
            } => {
                event_string = "kad_event::get_closest_peers";
                debug!(
                    "Query task {id:?} of key {:?} returned with peers {:?}, {stats:?} - {step:?}",
                    hex::encode(closest_peers.key.clone()),
                    closest_peers.peers,
                );

                if let Entry::Occupied(mut entry) = self.pending_get_closest_peers.entry(id) {
                    let (_, current_closest) = entry.get_mut();

                    // TODO: consider order the result and terminate when reach any of the
                    //       following criteria:
                    //   1, `stats.num_pending()` is 0
                    //   2, `stats.duration()` is longer than a defined period
                    current_closest.extend(
                        closest_peers
                            .peers
                            .into_iter()
                            .map(|i| (i.peer_id, Addresses(i.addrs))),
                    );
                    if current_closest.len() >= usize::from(K_VALUE) || step.last {
                        let (get_closest_type, current_closest) = entry.remove();
                        match get_closest_type {
                            PendingGetClosestType::NetworkDiscovery => self
                                .network_discovery
                                .handle_get_closest_query(current_closest),
                            PendingGetClosestType::FunctionCall(sender) => {
                                sender
                                    .send(current_closest)
                                    .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                            }
                        }
                    }
                } else {
                    debug!("Can't locate query task {id:?}, it has likely been completed already.");
                    return Err(NetworkError::ReceivedKademliaEventDropped {
                        query_id: id,
                        event: "GetClosestPeers Ok".to_string(),
                    });
                }
            }
            // Handle GetClosestPeers timeouts
            kad::Event::OutboundQueryProgressed {
                id,
                result: QueryResult::GetClosestPeers(Err(err)),
                ref stats,
                ref step,
            } => {
                event_string = "kad_event::get_closest_peers_err";
                error!("GetClosest Query task {id:?} errored with {err:?}, {stats:?} - {step:?}");

                let (get_closest_type, mut current_closest) =
                    self.pending_get_closest_peers.remove(&id).ok_or_else(|| {
                        debug!(
                            "Can't locate query task {id:?}, it has likely been completed already."
                        );
                        NetworkError::ReceivedKademliaEventDropped {
                            query_id: id,
                            event: "Get ClosestPeers error".to_string(),
                        }
                    })?;

                // We have `current_closest` from previous progress,
                // and `peers` from `GetClosestPeersError`.
                // Trust them and leave for the caller to check whether they are enough.
                match err {
                    GetClosestPeersError::Timeout { peers, .. } => {
                        current_closest
                            .extend(peers.into_iter().map(|i| (i.peer_id, Addresses(i.addrs))));
                    }
                }

                match get_closest_type {
                    PendingGetClosestType::NetworkDiscovery => self
                        .network_discovery
                        .handle_get_closest_query(current_closest),
                    PendingGetClosestType::FunctionCall(sender) => {
                        tokio::spawn(async move {
                            let _ = sender.send(current_closest);
                        });
                    }
                }
            }

            kad::Event::OutboundQueryProgressed {
                id,
                result: QueryResult::GetRecord(Ok(kad::GetRecordOk::FoundRecord(peer_record))),
                stats,
                step,
            } => {
                event_string = "kad_event::get_record::found";
                debug!(
                    "Query task {id:?} returned with record {:?} from peer {:?}, {stats:?} - {step:?}",
                    PrettyPrintRecordKey::from(&peer_record.record.key),
                    peer_record.peer
                );
                self.accumulate_get_record_found(id, peer_record, stats, step)?;
            }
            kad::Event::OutboundQueryProgressed {
                id,
                result:
                    QueryResult::GetRecord(Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord {
                        cache_candidates,
                    })),
                stats,
                step,
            } => {
                event_string = "kad_event::get_record::finished_no_additional";
                debug!("Query task {id:?} of get_record completed with {stats:?} - {step:?} - {cache_candidates:?}");
                self.handle_get_record_finished(id, step)?;
            }
            kad::Event::OutboundQueryProgressed {
                id,
                result: QueryResult::GetRecord(Err(get_record_err)),
                stats,
                step,
            } => {
                // log the errors
                match &get_record_err {
                    kad::GetRecordError::NotFound { key, closest_peers } => {
                        event_string = "kad_event::GetRecordError::NotFound";
                        info!("Query task {id:?} NotFound record {:?} among peers {closest_peers:?}, {stats:?} - {step:?}",
                        PrettyPrintRecordKey::from(key));
                    }
                    kad::GetRecordError::QuorumFailed {
                        key,
                        records,
                        quorum,
                    } => {
                        event_string = "kad_event::GetRecordError::QuorumFailed";
                        let pretty_key = PrettyPrintRecordKey::from(key);
                        let peers = records
                            .iter()
                            .map(|peer_record| peer_record.peer)
                            .collect_vec();
                        info!("Query task {id:?} QuorumFailed record {pretty_key:?} among peers {peers:?} with quorum {quorum:?}, {stats:?} - {step:?}");
                    }
                    kad::GetRecordError::Timeout { key } => {
                        event_string = "kad_event::GetRecordError::Timeout";
                        let pretty_key = PrettyPrintRecordKey::from(key);

                        debug!(
                            "Query task {id:?} timed out when looking for record {pretty_key:?}"
                        );
                    }
                }
                self.handle_get_record_error(id, get_record_err, stats, step)?;
            }
            kad::Event::OutboundQueryProgressed {
                id,
                result: QueryResult::PutRecord(Err(put_record_err)),
                stats,
                step,
            } => {
                // Currently, only `client` calls `put_record_to` to upload data.
                // The result of such operation is not critical to client in general.
                // However, if client keeps receiving error responses, it may indicating:
                //   1, Client itself is with slow connection
                //   OR
                //   2, The payee node selected could be in trouble
                //
                // TODO: Figure out which payee node the error response is related to,
                //       and may exclude that node from later on payee selection.
                let (key, success, quorum) = match &put_record_err {
                    kad::PutRecordError::QuorumFailed {
                        key,
                        success,
                        quorum,
                    } => {
                        event_string = "kad_event::PutRecordError::QuorumFailed";
                        (key, success, quorum)
                    }
                    kad::PutRecordError::Timeout {
                        key,
                        success,
                        quorum,
                    } => {
                        event_string = "kad_event::PutRecordError::Timeout";
                        (key, success, quorum)
                    }
                };
                error!("Query task {id:?} failed put record {:?} {:?}, required quorum {quorum}, stored on {success:?}, {stats:?} - {step:?}",
                       PrettyPrintRecordKey::from(key), event_string);
            }
            kad::Event::OutboundQueryProgressed {
                id,
                result: QueryResult::PutRecord(Ok(put_record_ok)),
                stats,
                step,
            } => {
                event_string = "kad_event::PutRecordOk";
                debug!(
                    "Query task {id:?} put record {:?} ok, {stats:?} - {step:?}",
                    PrettyPrintRecordKey::from(&put_record_ok.key)
                );
            }
            // Shall no longer receive this event
            kad::Event::OutboundQueryProgressed {
                id,
                result: QueryResult::Bootstrap(bootstrap_result),
                step,
                ..
            } => {
                event_string = "kad_event::OutboundQueryProgressed::Bootstrap";
                // here BootstrapOk::num_remaining refers to the remaining random peer IDs to query, one per
                // bucket that still needs refreshing.
                debug!("Kademlia Bootstrap with {id:?} progressed with {bootstrap_result:?} and step {step:?}");
            }
            kad::Event::RoutingUpdated {
                peer,
                is_new_peer,
                old_peer,
                addresses,
                ..
            } => {
                event_string = "kad_event::RoutingUpdated";
                if is_new_peer {
                    self.update_on_peer_addition(peer, Addresses(addresses.into_vec()));

                    // This should only happen once
                    if self.network_discovery.notify_new_peer() {
                        info!("Performing the first bootstrap");
                        self.trigger_network_discovery();
                    }
                }

                info!("kad_event::RoutingUpdated {:?}: {peer:?}, is_new_peer: {is_new_peer:?} old_peer: {old_peer:?}", self.peers_in_rt);
                if let Some(old_peer) = old_peer {
                    info!("Evicted old peer on new peer join: {old_peer:?}");
                    self.update_on_peer_removal(old_peer);
                }
            }
            kad::Event::InboundRequest {
                request: InboundRequest::PutRecord { .. },
            } => {
                event_string = "kad_event::InboundRequest::PutRecord";
                // Ignored to reduce logging. When `Record filtering` is enabled,
                // the `record` variable will contain the content for further validation before put.
            }
            kad::Event::InboundRequest {
                request: InboundRequest::FindNode { .. },
            } => {
                event_string = "kad_event::InboundRequest::FindNode";
                // Ignored to reduce logging. With continuous bootstrap, this is triggered often.
            }
            kad::Event::InboundRequest {
                request:
                    InboundRequest::GetRecord {
                        num_closer_peers,
                        present_locally,
                    },
            } => {
                event_string = "kad_event::InboundRequest::GetRecord";
                if !present_locally && num_closer_peers < CLOSE_GROUP_SIZE {
                    debug!("InboundRequest::GetRecord doesn't have local record, with {num_closer_peers:?} closer_peers");
                }
            }
            kad::Event::UnroutablePeer { peer } => {
                event_string = "kad_event::UnroutablePeer";
                debug!(peer_id = %peer, "kad::Event: UnroutablePeer");
            }
            kad::Event::RoutablePeer { peer, .. } => {
                // We get this when we don't add a peer via the identify step.
                // And we don't want to add these as they were rejected by identify for some reason.
                event_string = "kad_event::RoutablePeer";
                debug!(peer_id = %peer, "kad::Event: RoutablePeer");
            }
            other => {
                event_string = "kad_event::Other";
                info!("kad::Event ignored: {other:?}");
            }
        }

        self.log_handling(event_string.to_string(), start.elapsed());

        trace!(
            "kad::Event handled in {:?}: {event_string:?}",
            start.elapsed()
        );

        Ok(())
    }

    // For `get_record` returning behaviour:
    //   1, targeting a non-existing entry
    //     there will only be one event of `kad::Event::OutboundQueryProgressed`
    //     with `ProgressStep::last` to be `true`
    //          `QueryStats::requests` to be 20 (K-Value)
    //          `QueryStats::success` to be over majority of the requests
    //          `err::NotFound::closest_peers` contains a list of CLOSE_GROUP_SIZE peers
    //   2, targeting an existing entry
    //     there will a sequence of (at least CLOSE_GROUP_SIZE) events of
    //     `kad::Event::OutboundQueryProgressed` to be received
    //     with `QueryStats::end` always being `None`
    //          `ProgressStep::last` all to be `false`
    //          `ProgressStep::count` to be increased with step of 1
    //             capped and stopped at CLOSE_GROUP_SIZE, may have duplicated counts
    //          `PeerRecord::peer` could be None to indicate from self
    //             in which case it always use a duplicated `ProgressStep::count`
    //     the sequence will be completed with `FinishedWithNoAdditionalRecord`
    //     where: `cache_candidates`: being the peers supposed to hold the record but not
    //            `ProgressStep::count`: to be `number of received copies plus one`
    //            `ProgressStep::last` to be `true`

    /// Accumulates the GetRecord query results
    /// If we get enough responses (quorum) for a record with the same content hash:
    /// - we return the Record after comparing with the target record. This might return RecordDoesNotMatch if the
    ///   check fails.
    /// - if multiple content hashes are found, we return a SplitRecord Error
    ///   And then we stop the kad query as we are done here.
    fn accumulate_get_record_found(
        &mut self,
        query_id: QueryId,
        peer_record: PeerRecord,
        _stats: QueryStats,
        step: ProgressStep,
    ) -> Result<()> {
        let peer_id = if let Some(peer_id) = peer_record.peer {
            peer_id
        } else {
            self.self_peer_id
        };
        let pretty_key = PrettyPrintRecordKey::from(&peer_record.record.key).into_owned();

        if let Entry::Occupied(mut entry) = self.pending_get_record.entry(query_id) {
            let (_key, _senders, result_map, cfg) = entry.get_mut();

            if !cfg.expected_holders.is_empty() {
                if cfg.expected_holders.remove(&peer_id) {
                    debug!("For record {pretty_key:?} task {query_id:?}, received a copy from an expected holder {peer_id:?}");
                } else {
                    debug!("For record {pretty_key:?} task {query_id:?}, received a copy from an unexpected holder {peer_id:?}");
                }
            }

            // Insert the record and the peer into the result_map.
            let record_content_hash = XorName::from_content(&peer_record.record.value);
            debug!("For record {pretty_key:?} task {query_id:?}, received a copy {peer_id:?} with content hash {record_content_hash:?}");

            let responded_peers =
                if let Entry::Occupied(mut entry) = result_map.entry(record_content_hash) {
                    let (_, peer_list) = entry.get_mut();
                    let _ = peer_list.insert(peer_id);
                    peer_list.len()
                } else {
                    let mut peer_list = HashSet::new();
                    let _ = peer_list.insert(peer_id);
                    result_map.insert(record_content_hash, (peer_record.record.clone(), peer_list));
                    1
                };

            let expected_answers = cfg.get_quorum.get_value();
            debug!("Expecting {expected_answers:?} answers for record {pretty_key:?} task {query_id:?}, received {responded_peers} so far");

            if responded_peers >= expected_answers {
                if !cfg.expected_holders.is_empty() {
                    debug!("For record {pretty_key:?} task {query_id:?}, fetch completed with non-responded expected holders {:?}", cfg.expected_holders);
                }
                let cfg = cfg.clone();

                // Remove the query task and consume the variables.
                let (_key, senders, result_map, _) = entry.remove();

                if result_map.len() == 1 {
                    Self::send_record_after_checking_target(senders, peer_record.record, &cfg)?;
                } else {
                    debug!("For record {pretty_key:?} task {query_id:?}, fetch completed with split record");
                    let mut accumulated_graph_entries = BTreeSet::new();
                    for (record, _) in result_map.values() {
                        match get_graph_entry_from_record(record) {
                            Ok(graph_entries) => {
                                accumulated_graph_entries.extend(graph_entries);
                            }
                            Err(_) => {
                                continue;
                            }
                        }
                    }
                    if !accumulated_graph_entries.is_empty() {
                        info!("For record {pretty_key:?} task {query_id:?}, found split record for a GraphEntry, accumulated and sending them as a single record");
                        let accumulated_graph_entries = accumulated_graph_entries
                            .into_iter()
                            .collect::<Vec<GraphEntry>>();

                        let bytes = try_serialize_record(
                            &accumulated_graph_entries,
                            RecordKind::DataOnly(DataTypes::GraphEntry),
                        )?;

                        let new_accumulated_record = Record {
                            key: peer_record.record.key,
                            value: bytes.to_vec(),
                            publisher: None,
                            expires: None,
                        };
                        for sender in senders {
                            let new_accumulated_record = new_accumulated_record.clone();

                            sender
                                .send(Ok(new_accumulated_record))
                                .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                        }
                    } else {
                        for sender in senders {
                            let result_map = result_map.clone();
                            sender
                                .send(Err(GetRecordError::SplitRecord { result_map }))
                                .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                        }
                    }
                }

                // Stop the query; possibly stops more nodes from being queried.
                if let Some(mut query) = self.swarm.behaviour_mut().kademlia.query_mut(&query_id) {
                    query.finish();
                }
            } else if usize::from(step.count) >= CLOSE_GROUP_SIZE {
                debug!("For record {pretty_key:?} task {query_id:?}, got {:?} with {} versions so far.",
                   step.count, result_map.len());
            }
        } else {
            // return error if the entry cannot be found
            return Err(NetworkError::ReceivedKademliaEventDropped {
                query_id,
                event: format!("Accumulate Get Record of {pretty_key:?}"),
            });
        }
        Ok(())
    }

    /// Handles the possible cases when a GetRecord Query completes.
    /// The accumulate_get_record_found returns the record if the quorum is satisfied
    ///
    /// If we have reached this point but did not got enough records,
    /// or got split records (which prevented the quorum to pass),
    /// returns the following errors:
    ///     RecordNotFound if the result_map is empty.
    ///     NotEnoughCopies if there is only a single content hash version.
    ///     SplitRecord if there are multiple content hash versions.
    fn handle_get_record_finished(&mut self, query_id: QueryId, step: ProgressStep) -> Result<()> {
        // return error if the entry cannot be found
        if let Some((r_key, senders, result_map, cfg)) = self.pending_get_record.remove(&query_id) {
            let num_of_versions = result_map.len();
            let data_key_address = NetworkAddress::from_record_key(&r_key);

            // we have a split record, return it
            if num_of_versions > 1 {
                warn!(
                    "Multiple versions ({num_of_versions}) found for record {data_key_address:?}!"
                );
                for sender in senders {
                    sender
                        .send(Err(GetRecordError::SplitRecord {
                            result_map: result_map.clone(),
                        }))
                        .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                }

                return Ok(());
            }

            // we have no results, bail
            if num_of_versions == 0 {
                debug!("No versions found for record {data_key_address:?}!");
                for sender in senders {
                    sender
                        .send(Err(GetRecordError::RecordNotFound))
                        .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                }
                return Ok(());
            }

            // if we have searched thoroughly, we can return the record
            if num_of_versions == 1 {
                let result = if let Some((record, peers)) = result_map.values().next() {
                    trace!("one version found for record {data_key_address:?}!");

                    if peers.len() >= cfg.get_quorum.get_value() {
                        Ok(record.clone())
                    } else {
                        Err(GetRecordError::NotEnoughCopies {
                            record: record.clone(),
                            expected: cfg.get_quorum.get_value(),
                            got: peers.len(),
                        })
                    }
                } else {
                    debug!("Getting record task {query_id:?} completed with step count {:?}, but no copy found.", step.count);
                    Err(GetRecordError::RecordNotFound)
                };
                for sender in senders {
                    sender
                        .send(result.clone())
                        .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                }
            }
        } else {
            debug!("Can't locate query task {query_id:?} during GetRecord finished. We might have already returned the result to the sender.");
        }
        Ok(())
    }

    /// Handles the possible cases when a kad GetRecord returns an error.
    /// If we get NotFound/QuorumFailed, we return a RecordNotFound error. Kad currently does not enforce any quorum.
    /// If we get a Timeout:
    /// - return a QueryTimeout if we get a split record (?) if we have multiple content hashes.
    /// - if the quorum is satisfied, we return the record after comparing it with the target record. This might return
    ///   RecordDoesNotMatch if the check fails.
    /// - else we return q QueryTimeout error.
    fn handle_get_record_error(
        &mut self,
        query_id: QueryId,
        get_record_err: kad::GetRecordError,
        _stats: QueryStats,
        _step: ProgressStep,
    ) -> Result<()> {
        match &get_record_err {
            kad::GetRecordError::NotFound { .. } | kad::GetRecordError::QuorumFailed { .. } => {
                // return error if the entry cannot be found
                let (_key, senders, _, cfg) =
                self.pending_get_record.remove(&query_id).ok_or_else(|| {
                    debug!("Can't locate query task {query_id:?}, it has likely been completed already.");
                    NetworkError::ReceivedKademliaEventDropped {
                            query_id,
                            event: "GetRecordError NotFound or QuorumFailed".to_string(),
                        }
                })?;

                if cfg.expected_holders.is_empty() {
                    info!("Get record task {query_id:?} failed with error {get_record_err:?}");
                } else {
                    debug!("Get record task {query_id:?} failed with {:?} expected holders not responded, error {get_record_err:?}", cfg.expected_holders);
                }
                for sender in senders {
                    sender
                        .send(Err(GetRecordError::RecordNotFound))
                        .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                }
            }
            kad::GetRecordError::Timeout { key } => {
                // return error if the entry cannot be found
                let pretty_key = PrettyPrintRecordKey::from(key);
                let (_key, senders, result_map, cfg) =
                    self.pending_get_record.remove(&query_id).ok_or_else(|| {
                        debug!(
                            "Can't locate query task {query_id:?} for {pretty_key:?}, it has likely been completed already."
                        );
                        NetworkError::ReceivedKademliaEventDropped {
                            query_id,
                            event: format!("GetRecordError Timeout {pretty_key:?}"),
                        }
                    })?;

                let required_response_count = cfg.get_quorum.get_value();

                // if we've a split over the result xorname, then we don't attempt to resolve this here.
                // Retry and resolve through normal flows without a timeout.
                // todo: is the above still the case? Why don't we return a split record error.
                if result_map.len() > 1 {
                    warn!(
                        "Get record task {query_id:?} for {pretty_key:?} timed out with split result map"
                    );
                    for sender in senders {
                        sender
                            .send(Err(GetRecordError::QueryTimeout))
                            .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                    }

                    return Ok(());
                }

                // if we have enough responses here, we can return the record
                if let Some((record, peers)) = result_map.values().next() {
                    if peers.len() >= required_response_count {
                        Self::send_record_after_checking_target(senders, record.clone(), &cfg)?;
                        return Ok(());
                    }
                }

                warn!("Get record task {query_id:?} for {pretty_key:?} returned insufficient responses. {:?} did not return record", cfg.expected_holders);
                for sender in senders {
                    // Otherwise report the timeout
                    sender
                        .send(Err(GetRecordError::QueryTimeout))
                        .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
                }
            }
        }

        Ok(())
    }

    fn send_record_after_checking_target(
        senders: Vec<oneshot::Sender<std::result::Result<Record, GetRecordError>>>,
        record: Record,
        cfg: &GetRecordCfg,
    ) -> Result<()> {
        let res = if cfg.does_target_match(&record) {
            Ok(record)
        } else {
            Err(GetRecordError::RecordDoesNotMatch(record))
        };

        for sender in senders {
            sender
                .send(res.clone())
                .map_err(|_| NetworkError::InternalMsgChannelDropped)?;
        }

        Ok(())
    }
}
