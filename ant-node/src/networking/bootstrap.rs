// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::driver::NodeBehaviour;
use crate::networking::multiaddr_get_p2p;
use libp2p::core::ConnectedPoint;
use libp2p::{Multiaddr, PeerId, Swarm};
use std::collections::{HashSet, VecDeque};

const CONCURRENT_DIALS: usize = 3;
const MAX_PEERS_BEFORE_TERMINATION: usize = 5;

/// Manages the initial bootstrap process for connecting to the network.
#[derive(Debug)]
pub(crate) struct InitialBootstrap {
    /// Queue of addresses to dial
    initial_addrs: VecDeque<Multiaddr>,
    /// Addresses currently being dialed
    ongoing_dials: HashSet<Multiaddr>,
    /// Whether bootstrap has completed
    bootstrap_completed: bool,
    /// PeerIds of initial bootstrap peers (for identification)
    initial_bootstrap_peer_ids: HashSet<PeerId>,
}

impl InitialBootstrap {
    pub(crate) fn new(initial_addrs: Vec<Multiaddr>) -> Self {
        // Extract peer IDs from addresses
        let initial_bootstrap_peer_ids: HashSet<PeerId> =
            initial_addrs.iter().filter_map(multiaddr_get_p2p).collect();

        Self {
            initial_addrs: initial_addrs.into_iter().collect(),
            ongoing_dials: HashSet::new(),
            bootstrap_completed: false,
            initial_bootstrap_peer_ids,
        }
    }

    /// Check if a peer is one of our initial bootstrap peers
    pub(crate) fn is_bootstrap_peer(&self, peer_id: &PeerId) -> bool {
        self.initial_bootstrap_peer_ids.contains(peer_id)
    }

    /// Check if the bootstrap process has terminated
    pub(crate) fn has_terminated(&self) -> bool {
        self.bootstrap_completed
    }

    /// Trigger the bootstrapping process by dialing initial peers
    pub(crate) fn trigger_bootstrapping_process(
        &mut self,
        swarm: &mut Swarm<NodeBehaviour>,
        peers_in_rt: usize,
    ) {
        if !self.should_we_continue_bootstrapping(peers_in_rt, true) {
            return;
        }

        self.dial_next_addresses(swarm);
    }

    /// Called when a connection is established
    pub(crate) fn on_connection_established(
        &mut self,
        endpoint: &ConnectedPoint,
        swarm: &mut Swarm<NodeBehaviour>,
        peers_in_rt: usize,
    ) {
        // Remove the address from ongoing dials
        if let ConnectedPoint::Dialer { address, .. } = endpoint {
            let _ = self.ongoing_dials.remove(address);
        }

        // Continue dialing if needed
        if self.should_we_continue_bootstrapping(peers_in_rt, false) {
            self.dial_next_addresses(swarm);
        }
    }

    /// Called when an outgoing connection fails
    pub(crate) fn on_outgoing_connection_error(
        &mut self,
        _peer_id: Option<PeerId>,
        swarm: &mut Swarm<NodeBehaviour>,
        peers_in_rt: usize,
    ) {
        // Note: We can't easily remove the failed address from ongoing_dials
        // without the address info. The dial will eventually timeout and be cleaned.

        // Continue dialing if needed
        if self.should_we_continue_bootstrapping(peers_in_rt, false) {
            self.dial_next_addresses(swarm);
        }
    }

    /// Check if we should continue the bootstrapping process
    fn should_we_continue_bootstrapping(&mut self, peers_in_rt: usize, verbose: bool) -> bool {
        // If we have enough peers, we're done
        if peers_in_rt >= MAX_PEERS_BEFORE_TERMINATION {
            if verbose {
                info!(
                    "Bootstrap complete: reached {} peers in routing table",
                    peers_in_rt
                );
            }
            self.bootstrap_completed = true;
            return false;
        }

        // If we've exhausted all addresses and have no ongoing dials, we're done
        if self.initial_addrs.is_empty() && self.ongoing_dials.is_empty() {
            if verbose {
                info!("Bootstrap complete: exhausted all initial addresses");
            }
            self.bootstrap_completed = true;
            return false;
        }

        true
    }

    /// Dial the next addresses from the queue
    fn dial_next_addresses(&mut self, swarm: &mut Swarm<NodeBehaviour>) {
        // Dial up to CONCURRENT_DIALS - ongoing_dials.len() addresses
        while self.ongoing_dials.len() < CONCURRENT_DIALS {
            if let Some(addr) = self.initial_addrs.pop_front() {
                info!("Dialing bootstrap peer: {}", addr);
                match swarm.dial(addr.clone()) {
                    Ok(_) => {
                        let _ = self.ongoing_dials.insert(addr);
                    }
                    Err(e) => {
                        warn!("Failed to dial bootstrap peer {}: {}", addr, e);
                    }
                }
            } else {
                break;
            }
        }
    }
}
