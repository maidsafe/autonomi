// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::driver::{BadNodes, NodeBehaviour};
use ant_protocol::NetworkAddress;
use itertools::Itertools;
use libp2p::{
    core::transport::ListenerId, multiaddr::Protocol, Multiaddr, PeerId, StreamProtocol, Swarm,
};
use rand::Rng;
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_CONCURRENT_RELAY_CONNECTIONS: usize = 4;
const MAX_POTENTIAL_CANDIDATES: usize = 1000;

pub(crate) fn is_a_relayed_peer(addrs: &HashSet<Multiaddr>) -> bool {
    addrs
        .iter()
        .any(|multiaddr| multiaddr.iter().any(|p| matches!(p, Protocol::P2pCircuit)))
}

/// Manage the relay servers that we are connected to.
/// This is the client side of the relay server protocol.
#[derive(Debug)]
pub(crate) struct RelayManager {
    self_peer_id: PeerId,
    candidates: Vec<(PeerId, Multiaddr)>,
    waiting_for_reservation: BTreeMap<PeerId, Multiaddr>,
    connected_relays: BTreeMap<PeerId, Multiaddr>,

    /// Tracker for the relayed listen addresses.
    relayed_listener_id_map: HashMap<ListenerId, PeerId>,
}

impl RelayManager {
    pub(crate) fn new(self_peer_id: PeerId) -> Self {
        Self {
            self_peer_id,
            connected_relays: Default::default(),
            waiting_for_reservation: Default::default(),
            candidates: Default::default(),
            relayed_listener_id_map: Default::default(),
        }
    }

    /// Should we keep this peer alive? Closing a connection to that peer would remove that server from the listen addr.
    pub(crate) fn keep_alive_peer(&self, peer_id: &PeerId) -> bool {
        self.connected_relays.contains_key(peer_id)
            || self.waiting_for_reservation.contains_key(peer_id)
    }

    /// Add a potential candidate to the list if it satisfies all the identify checks and also supports the relay server
    /// protocol.
    pub(crate) fn add_potential_candidates(
        &mut self,
        peer_id: &PeerId,
        addrs: &HashSet<Multiaddr>,
        stream_protocols: &Vec<StreamProtocol>,
    ) {
        if self.candidates.len() >= MAX_POTENTIAL_CANDIDATES {
            return;
        }

        if Self::does_it_support_relay_server_protocol(stream_protocols) {
            // todo: collect and manage multiple addrs
            if let Some(addr) = addrs.iter().next() {
                // The calling place shall already checked whether the peer is `relayed`.
                // Hence here can add the addr directly.
                if let Some(relay_addr) = Self::craft_relay_address(addr, Some(*peer_id)) {
                    debug!("Adding {peer_id:?} with {relay_addr:?} as a potential relay candidate");
                    self.candidates.push((*peer_id, relay_addr));
                }
            }
        } else {
            debug!("Peer {peer_id:?} does not support relay server protocol");
        }
    }

    // todo: how do we know if a reservation has been revoked / if the peer has gone offline?
    /// Try connecting to candidate relays if we are below the threshold connections.
    /// This is run periodically on a loop.
    pub(crate) fn try_connecting_to_relay(
        &mut self,
        swarm: &mut Swarm<NodeBehaviour>,
        bad_nodes: &BadNodes,
    ) {
        if self.connected_relays.len() >= MAX_CONCURRENT_RELAY_CONNECTIONS
            || self.candidates.is_empty()
        {
            return;
        }

        let reservations_to_make = MAX_CONCURRENT_RELAY_CONNECTIONS - self.connected_relays.len();
        let mut n_reservations = 0;

        while n_reservations < reservations_to_make {
            // todo: should we remove all our other `listen_addr`? And should we block from adding `add_external_address` if
            // we're behind nat?

            // Pick a random closest candidate from the vector. Check if empty, or `gen_range` panics for empty range.
            let self_peer_id = NetworkAddress::from_peer(self.self_peer_id);
            self.candidates.sort_by_key(|(peer_id, _)| {
                self_peer_id.distance(&NetworkAddress::from_peer(*peer_id))
            });
            let index = if self.candidates.is_empty() {
                debug!("No more relay candidates.");
                break;
            } else {
                let max_range = std::cmp::min(self.candidates.len(), 5);
                rand::thread_rng().gen_range(0..max_range)
            };

            let (peer_id, relay_addr) = self.candidates.swap_remove(index);
            // skip if detected as a bad node
            if let Some((_, is_bad)) = bad_nodes.get(&peer_id) {
                if *is_bad {
                    debug!("Peer {peer_id:?} is considered as a bad node. Skipping it.");
                    continue;
                }
            }

            if self.connected_relays.contains_key(&peer_id)
                || self.waiting_for_reservation.contains_key(&peer_id)
            {
                debug!("We are already using {peer_id:?} as a relay server. Skipping.");
                continue;
            }

            match swarm.listen_on(relay_addr.clone()) {
                Ok(id) => {
                    info!("Sending reservation to relay {peer_id:?} on {relay_addr:?}");
                    self.waiting_for_reservation.insert(peer_id, relay_addr);
                    self.relayed_listener_id_map.insert(id, peer_id);
                    n_reservations += 1;
                }
                Err(err) => {
                    error!(
                        "Error while trying to listen on the relay addr: {err:?} on {relay_addr:?}"
                    );
                }
            }
        }
    }

    /// Update client state after we've successfully made reservation with a relay.
    pub(crate) fn on_successful_reservation_by_client(
        &mut self,
        peer_id: &PeerId,
        swarm: &mut Swarm<NodeBehaviour>,
    ) {
        if tracing::level_enabled!(tracing::Level::DEBUG) {
            let all_external_addresses = swarm.external_addresses().collect_vec();
            let all_listeners = swarm.listeners().collect_vec();
            debug!("All our listeners: {all_listeners:?}");
            debug!("All our external addresses: {all_external_addresses:?}");
        }

        match self.waiting_for_reservation.remove(peer_id) {
            Some(addr) => {
                info!("Successfully made reservation with {peer_id:?} on {addr:?}. Adding the addr to external address.");
                swarm.add_external_address(addr.clone());
                self.connected_relays.insert(*peer_id, addr);
            }
            None => {
                debug!("Made a reservation with a peer that we had not requested to");
            }
        }
    }

    /// Update client state if the reservation has been cancelled or if the relay has closed.
    pub(crate) fn on_listener_closed(
        &mut self,
        listener_id: &ListenerId,
        swarm: &mut Swarm<NodeBehaviour>,
    ) {
        let Some(peer_id) = self.relayed_listener_id_map.remove(listener_id) else {
            return;
        };

        if let Some(addr) = self.connected_relays.remove(&peer_id) {
            info!("Removing connected relay server as the listener has been closed: {peer_id:?}");
            info!("Removing external addr: {addr:?}");
            swarm.remove_external_address(&addr);

            // Even though we craft and store addrs in this format /ip4/198.51.100.0/tcp/55555/p2p/QmRelay/p2p-circuit/,
            // sometimes our PeerId is added at the end by the swarm?, which we want to remove as well i.e.,
            // /ip4/198.51.100.0/tcp/55555/p2p/QmRelay/p2p-circuit/p2p/QmSelf
            let Ok(addr_with_self_peer_id) = addr.with_p2p(self.self_peer_id) else {
                return;
            };
            info!("Removing external addr: {addr_with_self_peer_id:?}");
            swarm.remove_external_address(&addr_with_self_peer_id);
        }
        if let Some(addr) = self.waiting_for_reservation.remove(&peer_id) {
            info!("Removed peer form waiting_for_reservation as the listener has been closed {peer_id:?}: {addr:?}");
            debug!(
                "waiting_for_reservation len: {:?}",
                self.waiting_for_reservation.len()
            )
        }
    }

    fn does_it_support_relay_server_protocol(protocols: &Vec<StreamProtocol>) -> bool {
        for stream_protocol in protocols {
            if *stream_protocol == "/libp2p/circuit/relay/0.2.0/stop" {
                return true;
            }
        }
        false
    }

    /// The listen addr should be something like /ip4/198.51.100.0/tcp/55555/p2p/QmRelay/p2p-circuit/
    fn craft_relay_address(addr: &Multiaddr, peer_id: Option<PeerId>) -> Option<Multiaddr> {
        let mut output_addr = Multiaddr::empty();

        let ip = addr
            .iter()
            .find(|protocol| matches!(protocol, Protocol::Ip4(_)))?;
        output_addr.push(ip);
        let port = addr
            .iter()
            .find(|protocol| matches!(protocol, Protocol::Udp(_)))?;
        output_addr.push(port);
        output_addr.push(Protocol::QuicV1);

        let peer_id = {
            if let Some(peer_id) = peer_id {
                Protocol::P2p(peer_id)
            } else {
                addr.iter()
                    .find(|protocol| matches!(protocol, Protocol::P2p(_)))?
            }
        };
        output_addr.push(peer_id);
        output_addr.push(Protocol::P2pCircuit);

        debug!("Crafted p2p relay address: {output_addr:?}");
        Some(output_addr)
    }
}
