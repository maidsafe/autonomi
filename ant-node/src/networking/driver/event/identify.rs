// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::{
    Addresses, NetworkEvent, multiaddr_get_port,
    network::connection_action_logging,
    relay_manager::{RelayManager, is_a_relayed_peer},
};
use ant_protocol::version::IDENTIFY_PROTOCOL_STR;
use itertools::Itertools;
use libp2p::Multiaddr;
use libp2p::identify::Info;
use libp2p::kad::K_VALUE;
use libp2p::multiaddr::Protocol;
use std::collections::{HashSet, hash_map};
use std::time::{Duration, Instant};

/// The delay before we dial back a peer after receiving an identify event.
/// 180s will most likely remove the UDP tuple from the remote's NAT table.
/// This will make sure that the peer is reachable and that we can add it to the routing table.
pub(crate) const DIAL_BACK_DELAY: Duration = Duration::from_secs(180);

/// The list of protocols that are mandatory for a peer to support.
/// If a peer does not support these protocols, it will not be added to the routing table
const REQUIRED_PROTOCOLS: &[&str] = &["/autonomi/kad/"];

use super::SwarmDriver;

impl SwarmDriver {
    pub(super) fn handle_identify_event(&mut self, identify_event: libp2p::identify::Event) {
        match identify_event {
            libp2p::identify::Event::Received {
                peer_id,
                info,
                connection_id,
            } => {
                let start = Instant::now();
                // ELK logging. Do not update without proper testing.
                connection_action_logging(
                    &peer_id,
                    &self.self_peer_id,
                    &connection_id,
                    "Identify::Received",
                );

                self.handle_identify_received(peer_id, info, connection_id);
                trace!("SwarmEvent handled in {:?}: identify", start.elapsed());
            }
            // Log the other Identify events.
            libp2p::identify::Event::Sent { peer_id, .. } => {
                // ELK logging. Do not update without proper testing.
                connection_action_logging(
                    &peer_id,
                    &self.self_peer_id,
                    &identify_event.connection_id(),
                    "Identify::Sent",
                );
                debug!("identify: {identify_event:?}")
            }
            libp2p::identify::Event::Pushed { peer_id, .. } => {
                // ELK logging. Do not update without proper testing.
                connection_action_logging(
                    &peer_id,
                    &self.self_peer_id,
                    &identify_event.connection_id(),
                    "Identify::Pushed",
                );

                debug!("identify: {identify_event:?}")
            }
            libp2p::identify::Event::Error { peer_id, .. } => {
                // ELK logging. Do not update without proper testing.
                connection_action_logging(
                    &peer_id,
                    &self.self_peer_id,
                    &identify_event.connection_id(),
                    "Identify::Error",
                );
                warn!("identify: {identify_event:?}")
            }
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
        let Some((_, addr_fom_connection, _)) =
            self.live_connected_peers.get(&connection_id).cloned()
        else {
            warn!(
                "identify: received info for peer {peer_id:?} on {connection_id:?} that is not in the live connected peers"
            );
            return;
        };

        let our_identify_protocol = IDENTIFY_PROTOCOL_STR.read().expect("IDENTIFY_PROTOCOL_STR has been locked to write. A call to set_network_id performed. This should not happen.").to_string();

        if info.protocol_version != our_identify_protocol {
            warn!(
                "identify: {peer_id:?} does not have the same protocol. Our IDENTIFY_PROTOCOL_STR: {our_identify_protocol:?}. Their protocol version: {:?}",
                info.protocol_version
            );

            self.send_event(NetworkEvent::PeerWithUnsupportedProtocol {
                our_protocol: our_identify_protocol,
                their_protocol: info.protocol_version,
            });
            // Block the peer from any further communication.
            let _ = self.swarm.behaviour_mut().blocklist.block_peer(peer_id);
            if let Some(dead_peer) = self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id) {
                error!(
                    "Clearing out a protocol mismatch peer from RT. The peer pushed an incorrect identify info after being added: {peer_id:?}"
                );
                self.update_on_peer_removal(*dead_peer.node.key.preimage());
            }

            return;
        }

        let has_dialed = self.dialed_peers.contains(&peer_id);
        let is_relayed_peer = is_a_relayed_peer(info.listen_addrs.iter());
        let addrs = if !is_relayed_peer {
            let addr = craft_valid_multiaddr_without_p2p(&addr_fom_connection);
            let Some(addr) = addr else {
                warn!("identify: no valid multiaddr found for {peer_id:?} on {connection_id:?}");
                return;
            };
            debug!("Peer {peer_id:?} is a normal peer, crafted valid multiaddress : {addr:?}.");
            vec![addr]
        } else {
            let p2p_addrs = info
                .listen_addrs
                .iter()
                .filter_map(|addr| RelayManager::craft_relay_address(addr, None))
                .unique()
                .collect::<Vec<_>>();
            debug!(
                "Peer {peer_id:?} is a relayed peer. Using p2p addr from identify directly: {p2p_addrs:?}"
            );
            p2p_addrs
        };

        // return early for reachability-check-peer / clients
        if info.agent_version.contains("reachability-check-peer") {
            debug!(
                "Peer {peer_id:?} is requesting for a reachability check. Adding it to the dial queue. Not adding to RT."
            );
            let _ = self.dial_queue.insert(
                peer_id,
                (
                    Addresses(addrs.clone()),
                    Instant::now() + DIAL_BACK_DELAY,
                    1,
                ),
            );
            return;
        } else if info.agent_version.contains("client") {
            debug!("Peer {peer_id:?} is a client. Not dialing or adding to RT.");
            return;
        }

        // Check which required protocols are missing
        let missing_protocols: Vec<&&str> = REQUIRED_PROTOCOLS
            .iter()
            .filter(|required| {
                !info
                    .protocols
                    .iter()
                    .any(|protocol| protocol.as_ref().contains(*required))
            })
            .collect();

        if !missing_protocols.is_empty() {
            warn!(
                "Peer {peer_id:?} does not support some mandatory protocols from: {REQUIRED_PROTOCOLS:?}, not adding it to our RT. Missing protocols: {missing_protocols:?}"
            );
            self.send_event(NetworkEvent::PeerWithUnsupportedProtocol {
                our_protocol: format!("{REQUIRED_PROTOCOLS:?}"),
                their_protocol: format!(
                    "{:?}",
                    info.protocols
                        .iter()
                        .map(|protocol| protocol.to_string())
                        .collect::<Vec<_>>()
                ),
            });

            // Block the peer from any further communication.
            let _ = self.swarm.behaviour_mut().blocklist.block_peer(peer_id);
            if let Some(dead_peer) = self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id) {
                error!(
                    "Clearing out peer as it does not support some mandatory protocols. The peer pushed an incorrect identify info after being added: {peer_id:?}"
                );
                self.update_on_peer_removal(*dead_peer.node.key.preimage());
            }
            return;
        }

        // Do not use an `already relayed` or a `bootstrap` peer as `potential relay candidate`.
        if !is_relayed_peer && !self.initial_bootstrap.is_bootstrap_peer(&peer_id) {
            if let Some(relay_manager) = self.relay_manager.as_mut() {
                debug!("Adding candidate relay server {peer_id:?}, it's not a bootstrap node");
                relay_manager.add_potential_candidates(&peer_id, &addrs, &info.protocols);
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

            debug!(
                "Received identify for {peer_id:?} that is already part of the RT. Checking if the addresses {addrs:?} are new."
            );
            self.update_pre_existing_peer(peer_id, &addrs, is_relayed_peer);
        } else if !self.local && !has_dialed {
            // When received an identify from un-dialed peer, try to dial it
            // The dial shall trigger the same identify to be sent again and confirm
            // peer is external accessible, hence safe to be added into RT.
            // Client doesn't need to dial back.

            let exists_in_dial_queue = self.dial_queue.contains_key(&peer_id);

            // Only need to dial back for not fulfilled kbucket
            if kbucket_full && !exists_in_dial_queue {
                debug!(
                    "received identify for a full bucket {ilog2:?}, not dialing {peer_id:?} on {addrs:?}"
                );
                return;
            }

            info!(
                "received identify info from undialed peer {peer_id:?} for not full kbucket {ilog2:?}, dialing back after {DIAL_BACK_DELAY:?}. Addrs: {addrs:?}"
            );

            let support_dnd = does_the_peer_support_dnd(&info);
            let mut send_dnd = false;
            match self.dial_queue.entry(peer_id) {
                hash_map::Entry::Occupied(mut entry) => {
                    let (old_addrs, time, resets) = entry.get_mut();

                    *resets += 1;

                    // if the peer supports DND, reset the time always.
                    // if the peer does not support DND, do not reset the time if resets >= 3.

                    *time = Instant::now() + DIAL_BACK_DELAY;

                    if *resets >= 3 {
                        send_dnd = true;

                        if support_dnd {
                            debug!(
                                "Peer {peer_id:?} has been reset 3 times. Will now send a DoNotDisturb request."
                            );
                        } else {
                            *time = Instant::now();
                            warn!(
                                "Peer {peer_id:?} has been reset 3 times. It does not support DoNotDisturb. Dialing it back immediately."
                            );
                        }
                    } else {
                        debug!(
                            "Peer {peer_id:?} has been re-added to the dial queue; Reset the dial back time to {DIAL_BACK_DELAY:?} (resets: {resets})",
                        );
                    }

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
                            debug!(
                                "Removed non-p2p addr from dial queue for {peer_id:?}. Remaining addrs: {old_addrs:?}"
                            );
                        }
                    }
                }
                hash_map::Entry::Vacant(entry) => {
                    debug!("Adding new addr {addrs:?} to dial queue for {peer_id:?}");
                    let _ = entry.insert((Addresses(addrs), Instant::now() + DIAL_BACK_DELAY, 1));
                }
            }

            // If the peer does not support DoNotDisturb cmd, we dial it back immediately. Else there is a possibility
            // of this peer getting stuck in our dial queue forever.
            //
            // This is a backward compatibility change and can be removed once we are sure that all nodes
            // support the DoNotDisturb cmd.
            if send_dnd && support_dnd {
                self.swarm
                    .behaviour_mut()
                    .do_not_disturb
                    .send_do_not_disturb_request(peer_id, DIAL_BACK_DELAY.as_secs() + 20);
            }
        } else {
            // We care only for peers that we dialed and thus are reachable.
            // Or if we are local, we can add the peer directly.
            // A bad node cannot establish a connection with us. So we can add it to the RT directly.

            // With the new bootstrap cache, the workload is distributed,
            // hence no longer need to replace bootstrap nodes for workload share.
            // self.remove_bootstrap_from_full(peer_id);
            debug!(
                "identify: attempting to add addresses to routing table for {peer_id:?}. Addrs: {addrs:?}"
            );

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
    /// new_addrs will not contain non-p2p-circuit (non-relayed) addresses if the peer is a relayed peer.
    fn update_pre_existing_peer(
        &mut self,
        peer_id: libp2p::PeerId,
        new_addrs: &[Multiaddr],
        is_relayed_peer: bool,
    ) {
        if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(peer_id) {
            let new_addrs = new_addrs.iter().cloned().collect::<HashSet<_>>();
            let mut addresses_to_add = Vec::new();

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

            // remove addresses only for relayed peers.
            if is_relayed_peer {
                let mut addresses_to_remove = Vec::new();
                addresses_to_remove.extend(existing_addrs.difference(&new_addrs));

                if !addresses_to_remove.is_empty() {
                    debug!(
                        "Removing addresses from RT for {peer_id:?} (relayed) as the new identify does not contain them: {addresses_to_remove:?}"
                    );
                    for multiaddr in addresses_to_remove {
                        let _routing_update = self
                            .swarm
                            .behaviour_mut()
                            .kademlia
                            .remove_address(&peer_id, multiaddr);
                    }
                }
            }

            if !addresses_to_add.is_empty() {
                debug!(
                    "Adding addresses to RT for {peer_id:?} as the new identify contains them: {addresses_to_add:?}"
                );
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

fn does_the_peer_support_dnd(info: &Info) -> bool {
    for protocol in &info.protocols {
        if protocol.to_string().contains("autonomi/dnd") {
            return true;
        }
    }
    false
}

/// Craft valid multiaddr like /ip4/68.183.39.80/udp/31055/quic-v1
/// RelayManager::craft_relay_address for relayed addr. This is for non-relayed addr.
fn craft_valid_multiaddr_without_p2p(addr: &Multiaddr) -> Option<Multiaddr> {
    let mut new_multiaddr = Multiaddr::empty();
    let ip = addr.iter().find_map(|p| match p {
        Protocol::Ip4(addr) => Some(addr),
        _ => None,
    })?;
    let port = multiaddr_get_port(addr)?;

    new_multiaddr.push(Protocol::Ip4(ip));
    new_multiaddr.push(Protocol::Udp(port));
    new_multiaddr.push(Protocol::QuicV1);

    Some(new_multiaddr)
}

/// Build a `Multiaddr` with the p2p protocol filtered out.
/// If it is a relayed address, then the relay's P2P address is preserved.
fn multiaddr_strip_p2p(multiaddr: &Multiaddr) -> Multiaddr {
    let is_relayed = multiaddr.iter().any(|p| matches!(p, Protocol::P2pCircuit));

    if is_relayed {
        // Do not add any PeerId after we've found the P2PCircuit protocol. The prior one is the relay's PeerId which
        // we should preserve.
        let mut before_relay_protocol = true;
        let mut new_multi_addr = Multiaddr::empty();
        for p in multiaddr.iter() {
            if matches!(p, Protocol::P2pCircuit) {
                before_relay_protocol = false;
            }
            if matches!(p, Protocol::P2p(_)) && !before_relay_protocol {
                continue;
            }
            new_multi_addr.push(p);
        }
        new_multi_addr
    } else {
        multiaddr
            .iter()
            .filter(|p| !matches!(p, Protocol::P2p(_)))
            .collect()
    }
}
