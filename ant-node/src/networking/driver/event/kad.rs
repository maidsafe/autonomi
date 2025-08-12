// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::{
    Addresses, CLOSE_GROUP_SIZE, NetworkError, Result, driver::PendingGetClosestType,
};
use libp2p::kad::{self, GetClosestPeersError, InboundRequest, K_VALUE, QueryResult};
use std::collections::hash_map::Entry;
use std::time::Instant;

use super::SwarmDriver;

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
                                if let Err(e) = sender.send(current_closest) {
                                    warn!(
                                        "Failed to send closest peers response - receiver dropped: {e:?}"
                                    );
                                }
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
                        #[allow(clippy::let_underscore_future)]
                        let _ = tokio::spawn(async move {
                            let _ = sender.send(vec![]);
                        });
                    }
                }
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
                debug!(
                    "Kademlia Bootstrap with {id:?} progressed with {bootstrap_result:?} and step {step:?}"
                );
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
                        self.trigger_network_discovery(0);
                    }
                }

                info!(
                    "kad_event::RoutingUpdated {:?}: {peer:?}, is_new_peer: {is_new_peer:?} old_peer: {old_peer:?}",
                    self.peers_in_rt
                );
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
                    debug!(
                        "InboundRequest::GetRecord doesn't have local record, with {num_closer_peers:?} closer_peers"
                    );
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
                debug!("kad::Event ignored: {other:?}");
            }
        }

        self.log_handling(event_string.to_string(), start.elapsed());

        trace!(
            "kad::Event handled in {:?}: {event_string:?}",
            start.elapsed()
        );

        Ok(())
    }
}
