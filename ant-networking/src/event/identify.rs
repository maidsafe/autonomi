// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::relay_manager::is_a_relayed_peer;
use crate::{multiaddr_strip_p2p, Addresses, NetworkEvent, SwarmDriver};
use ant_protocol::version::IDENTIFY_PROTOCOL_STR;
use libp2p::identify::Info;
use libp2p::kad::K_VALUE;
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use std::collections::{hash_map, HashSet};
use std::time::{Duration, Instant};

/// The delay before we dial back a peer after receiving an identify event.
/// 180s will most likely remove the UDP tuple from the remote's NAT table.
/// This will make sure that the peer is reachable and that we can add it to the routing table.
const DIAL_BACK_DELAY: Duration = Duration::from_secs(180);

impl SwarmDriver {
    pub(super) fn handle_identify_event(&mut self, identify_event: libp2p::identify::Event) {
        match identify_event {
            libp2p::identify::Event::Received {
                peer_id,
                info,
                connection_id,
            } => {
                let start = Instant::now();
                self.handle_identify_received(peer_id, info, connection_id);
                trace!("SwarmEvent handled in {:?}: identify", start.elapsed());
            }
            // Log the other Identify events.
            libp2p::identify::Event::Sent { .. } => debug!("identify: {identify_event:?}"),
            libp2p::identify::Event::Pushed { .. } => debug!("identify: {identify_event:?}"),
            libp2p::identify::Event::Error { .. } => warn!("identify: {identify_event:?}"),
        }
    }

    fn handle_identify_received(
        &mut self,
        peer_id: libp2p::PeerId,
        info: Info,
        connection_id: libp2p::swarm::ConnectionId,
    ) {
        debug!("identify: received info from {peer_id:?} on {connection_id:?}. Info: {info:?}");
        // If the peer dials us with a different addr, we would add it to our RT via update_pre_existing_peer
        let Some((_, addr_fom_connection, _)) = self.live_connected_peers.get(&connection_id)
        else {
            warn!("identify: received info for peer {peer_id:?} on {connection_id:?} that is not in the live connected peers");
            return;
        };
        let mut addrs = vec![multiaddr_strip_p2p(addr_fom_connection)];

        let our_identify_protocol = IDENTIFY_PROTOCOL_STR.read().expect("IDENTIFY_PROTOCOL_STR has been locked to write. A call to set_network_id performed. This should not happen.").to_string();

        if info.protocol_version != our_identify_protocol {
            warn!("identify: {peer_id:?} does not have the same protocol. Our IDENTIFY_PROTOCOL_STR: {our_identify_protocol:?}. Their protocol version: {:?}",info.protocol_version);

            self.send_event(NetworkEvent::PeerWithUnsupportedProtocol {
                our_protocol: our_identify_protocol,
                their_protocol: info.protocol_version,
            });
            // Block the peer from any further communication.
            self.swarm.behaviour_mut().blocklist.block_peer(peer_id);
            if let Some(dead_peer) = self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id) {
                error!("Clearing out a protocol mismatch peer from RT. The peer pushed an incorrect identify info after being added: {peer_id:?}");
                self.update_on_peer_removal(*dead_peer.node.key.preimage());
            }

            return;
        }

        if info.agent_version.contains("client") {
            debug!("Peer {peer_id:?} is a client. Not dialing or adding to RT.");
            return;
        }

        let has_dialed = self.dialed_peers.contains(&peer_id);

        let is_relayed_peer = is_a_relayed_peer(addrs.iter());

        // Do not use an `already relayed` or a `bootstrap` peer as `potential relay candidate`.
        if !is_relayed_peer && !self.initial_bootstrap.is_bootstrap_peer(&peer_id) {
            if let Some(relay_manager) = self.relay_manager.as_mut() {
                debug!("Adding candidate relay server {peer_id:?}, it's not a bootstrap node");
                relay_manager.add_potential_candidates(&peer_id, &addrs, &info.protocols);
            }
        }

        if is_relayed_peer {
            let p2p_addrs = info
                .listen_addrs
                .iter()
                .filter(|addr| addr.iter().any(|seg| matches!(seg, Protocol::P2pCircuit)))
                .cloned()
                .collect::<Vec<_>>();
            debug!("Peer {peer_id:?} is a relayed peer. Not using {addrs:?} from connection info, rather using p2p addr from identify: {p2p_addrs:?}");
            addrs = p2p_addrs;
        }

        let (kbucket_full, already_present_in_rt, ilog2) =
            if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(peer_id) {
                let ilog2 = kbucket.range().0.ilog2();
                let num_peers = kbucket.num_entries();
                let is_bucket_full = num_peers >= K_VALUE.into();

                // check if peer_id is already a part of RT
                let already_present_in_rt = kbucket
                    .iter()
                    .any(|entry| entry.node.key.preimage() == &peer_id);

                (is_bucket_full, already_present_in_rt, ilog2)
            } else {
                return;
            };

        if already_present_in_rt {
            // If the peer is part already of the RT, try updating the addresses based on the new push info.
            // We don't have to dial it back.

            debug!("Received identify for {peer_id:?} that is already part of the RT. Checking if the addresses {addrs:?} are new.");
            self.update_pre_existing_peer(peer_id, &addrs);
        } else if !self.is_client && !self.local && !has_dialed {
            // When received an identify from un-dialed peer, try to dial it
            // The dial shall trigger the same identify to be sent again and confirm
            // peer is external accessible, hence safe to be added into RT.
            // Client doesn't need to dial back.

            let exists_in_dial_queue = self.dial_queue.contains_key(&peer_id);

            // Only need to dial back for not fulfilled kbucket
            if kbucket_full && !exists_in_dial_queue {
                debug!("received identify for a full bucket {ilog2:?}, not dialing {peer_id:?} on {addrs:?}");
                return;
            }

            info!("received identify info from undialed peer {peer_id:?} for not full kbucket {ilog2:?}, dialing back after {DIAL_BACK_DELAY:?}. Addrs: {addrs:?}");
            match self.dial_queue.entry(peer_id) {
                hash_map::Entry::Occupied(mut entry) => {
                    let (old_addrs, time) = entry.get_mut();

                    debug!("Resetting dial time for {peer_id:?}");
                    *time = Instant::now() + DIAL_BACK_DELAY;

                    for addr in addrs.iter() {
                        if !old_addrs.0.contains(addr) {
                            debug!("Adding new addr {addr:?} to dial queue for {peer_id:?}");
                            old_addrs.0.push(addr.clone());
                        } else {
                            debug!("Already have addr {addr:?} in dial queue for {peer_id:?}.");
                        }
                    }

                    if is_relayed_peer {
                        let mut removed = false;
                        old_addrs.0.retain(|addr| {
                            if addr.iter().any(|seg| matches!(seg, Protocol::P2pCircuit)) {
                                true
                            } else {
                                removed = true;
                                false
                            }
                        });
                        if removed {
                            debug!("Removed non-p2p addr from dial queue for {peer_id:?}. Remaining addrs: {old_addrs:?}");
                        }
                    }
                }
                hash_map::Entry::Vacant(entry) => {
                    debug!("Adding new addr {addrs:?} to dial queue for {peer_id:?}");
                    entry.insert((Addresses(addrs), Instant::now() + DIAL_BACK_DELAY));
                }
            }
        } else {
            // We care only for peers that we dialed and thus are reachable.
            // Or if we are local, we can add the peer directly.
            // A bad node cannot establish a connection with us. So we can add it to the RT directly.

            // With the new bootstrap cache, the workload is distributed,
            // hence no longer need to replace bootstrap nodes for workload share.
            // self.remove_bootstrap_from_full(peer_id);
            debug!("identify: attempting to add addresses to routing table for {peer_id:?}. Addrs: {addrs:?}");

            // Attempt to add the addresses to the routing table.
            for addr in addrs.into_iter() {
                let _routing_update = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, addr);
            }
        }
    }

    /// If the peer is part already of the RT, try updating the addresses based on the new push info.
    ///
    /// new_addrs contains only p2p addresses if the peer is a relayed peer.
    fn update_pre_existing_peer(&mut self, peer_id: libp2p::PeerId, new_addrs: &[Multiaddr]) {
        if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(peer_id) {
            let new_addrs = new_addrs.iter().cloned().collect::<HashSet<_>>();
            let mut addresses_to_add = Vec::new();
            let mut addresses_to_remove = Vec::new();

            let Some(entry) = kbucket
                .iter()
                .find(|entry| entry.node.key.preimage() == &peer_id)
            else {
                warn!("Peer {peer_id:?} is not part of the RT. Cannot update addresses.");
                return;
            };
            let existing_addrs = entry
                .node
                .value
                .iter()
                .map(multiaddr_strip_p2p)
                .collect::<HashSet<_>>();
            addresses_to_add.extend(new_addrs.difference(&existing_addrs));
            addresses_to_remove.extend(existing_addrs.difference(&new_addrs));

            if !addresses_to_remove.is_empty() {
                debug!("Removing addresses from RT for {peer_id:?} as the new identify does not contain them: {addresses_to_remove:?}");
                for multiaddr in addresses_to_remove {
                    let _routing_update = self
                        .swarm
                        .behaviour_mut()
                        .kademlia
                        .remove_address(&peer_id, multiaddr);
                }
            }

            if !addresses_to_add.is_empty() {
                debug!("Adding addresses to RT for {peer_id:?} as the new identify contains them: {addresses_to_add:?}");
                for multiaddr in addresses_to_add {
                    let _routing_update = self
                        .swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, multiaddr.clone());
                }
            }
        }
    }
}
