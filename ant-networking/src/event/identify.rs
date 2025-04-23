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
use libp2p::Multiaddr;
use std::collections::HashSet;
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
        debug!(conn_id=%connection_id, %peer_id, ?info, "identify: received info");
        // If the peer dials us with a different addr, we would add it to our RT via update_pre_existing_peer
        let Some((_, addr, _)) = self.live_connected_peers.get(&connection_id) else {
            warn!(conn_id=%connection_id, %peer_id, "identify: received info for a connection that is not in the live connected peers");
            return;
        };
        let addr = multiaddr_strip_p2p(addr);

        let our_identify_protocol = IDENTIFY_PROTOCOL_STR.read().expect("IDENTIFY_PROTOCOL_STR has been locked to write. A call to set_network_id performed. This should not happen.").to_string();

        if info.protocol_version != our_identify_protocol {
            warn!(?info.protocol_version, "identify: {peer_id:?} does not have the same protocol. Our IDENTIFY_PROTOCOL_STR: {our_identify_protocol:?}");

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

        let is_relayed_peer = is_a_relayed_peer([addr.clone()].iter());

        // Do not use an `already relayed` or a `bootstrap` peer as `potential relay candidate`.
        if !is_relayed_peer && !self.initial_bootstrap.is_bootstrap_peer(&peer_id) {
            if let Some(relay_manager) = self.relay_manager.as_mut() {
                debug!("Adding candidate relay server {peer_id:?}, it's not a bootstrap node");
                relay_manager.add_potential_candidates(&peer_id, &addr, &info.protocols);
            }
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

            debug!("Received identify for {peer_id:?} that is already part of the RT. Checking if the addresses {addr:?} are new.");

            self.update_pre_existing_peer(peer_id, &addr);
        } else if !self.is_client && !self.local && !has_dialed {
            // When received an identify from un-dialed peer, try to dial it
            // The dial shall trigger the same identify to be sent again and confirm
            // peer is external accessible, hence safe to be added into RT.
            // Client doesn't need to dial back.

            // Only need to dial back for not fulfilled kbucket
            if kbucket_full {
                debug!("received identify for a full bucket {ilog2:?}, not dialing {peer_id:?} on {addr:?}");
                return;
            }

            info!(%peer_id, ?addr, "received identify info from undialed peer for not full kbucket {ilog2:?}, dialing back after {DIAL_BACK_DELAY:?}");
            match self.dial_queue.entry(peer_id) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let (addrs, time) = entry.get_mut();
                    if !addrs.0.contains(&addr) {
                        debug!("Adding new addr {addr:?} to dial queue for {peer_id:?}. Resetting dial time.");
                        addrs.0.push(addr.clone());
                        *time = Instant::now() + DIAL_BACK_DELAY;
                    } else {
                        debug!("Already have addr {addr:?} in dial queue for {peer_id:?}. Not adding again.");
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    debug!("Adding new addr {addr:?} to dial queue for {peer_id:?}");
                    entry.insert((
                        Addresses(vec![addr.clone()]),
                        Instant::now() + DIAL_BACK_DELAY,
                    ));
                }
            }
        } else {
            // We care only for peers that we dialed and thus are reachable.
            // Or if we are local, we can add the peer directly.
            // A bad node cannot establish a connection with us. So we can add it to the RT directly.

            // With the new bootstrap cache, the workload is distributed,
            // hence no longer need to replace bootstrap nodes for workload share.
            // self.remove_bootstrap_from_full(peer_id);
            debug!(%peer_id, ?addr, "identify: attempting to add addresses to routing table");

            // Attempt to add the addresses to the routing table.
            let _routing_update = self
                .swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, addr);
        }
    }

    /// If the peer is part already of the RT, try updating the addresses based on the new push info.
    fn update_pre_existing_peer(&mut self, peer_id: libp2p::PeerId, new_addr: &Multiaddr) {
        if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(peer_id) {
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

            if !existing_addrs.contains(new_addr) {
                debug!(
                    "Adding new addr {new_addr:?} to RT for {peer_id:?} that already exists in RT"
                );

                let _routing_update = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, new_addr.clone());
            }
        }
    }
}
