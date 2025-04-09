// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::{driver::NodeBehaviour, multiaddr_get_p2p, multiaddr_pop_p2p};
use libp2p::{
    Multiaddr, PeerId, Swarm,
    core::ConnectedPoint,
    swarm::{
        DialError,
        dial_opts::{DialOpts, PeerCondition},
    },
};
use rand::seq::SliceRandom;
use std::collections::{HashSet, VecDeque};

/// Periodically check if the initial bootstrap process should be triggered.
/// This happens only once after the conditions for triggering the initial bootstrap process are met.
pub(crate) const INITIAL_BOOTSTRAP_CHECK_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(1);

/// The max number of concurrent dials to be made during the initial bootstrap process.
const CONCURRENT_DIALS: usize = 3;

/// The max number of peers to be added to the routing table before stopping the initial bootstrap process.
const MAX_PEERS_BEFORE_TERMINATION: usize = 5;

/// This is used to track the conditions that are required to trigger the initial bootstrap process once.
pub(crate) struct InitialBootstrapTrigger {
    pub(crate) upnp: bool,
    pub(crate) upnp_gateway_result_obtained: bool,
    pub(crate) listen_addr_obtained: bool,
}

impl InitialBootstrapTrigger {
    pub(crate) fn new(upnp: bool) -> Self {
        Self {
            upnp,
            upnp_gateway_result_obtained: false,
            listen_addr_obtained: false,
        }
    }

    /// Used to check if we can trigger the initial bootstrap process.
    ///
    /// - If we are a client, we should trigger the initial bootstrap process immediately.
    /// - If we have set upnp flag and if we have obtained the upnp gateway result, we should trigger the initial bootstrap process.
    /// - If we don't have upnp enabled, then we should trigger the initial bootstrap process only if we have a listen address available.
    pub(crate) fn should_trigger_initial_bootstrap(&self) -> bool {
        if self.upnp {
            return self.upnp_gateway_result_obtained;
        }

        if self.listen_addr_obtained {
            return true;
        }

        false
    }
}

pub(crate) struct InitialBootstrap {
    initial_addrs: VecDeque<Multiaddr>,
    ongoing_dials: HashSet<Multiaddr>,
    bootstrap_completed: bool,
    /// This tracker is used by other components to avoid overloading the initial peers.
    initial_bootstrap_peer_ids: HashSet<PeerId>,
}

impl InitialBootstrap {
    pub(crate) fn new(mut initial_addrs: Vec<Multiaddr>) -> Self {
        let bootstrap_completed = if initial_addrs.is_empty() {
            info!(
                "No initial addresses provided for bootstrap. Initial bootstrap process will not be triggered."
            );
            true
        } else {
            let mut rng = rand::thread_rng();
            initial_addrs.shuffle(&mut rng);
            false
        };

        let initial_bootstrap_peer_ids =
            initial_addrs.iter().filter_map(multiaddr_get_p2p).collect();

        Self {
            initial_addrs: initial_addrs.into(),
            ongoing_dials: Default::default(),
            bootstrap_completed,
            initial_bootstrap_peer_ids,
        }
    }

    /// Returns true if the peer is one of the initial bootstrap peers.
    pub(crate) fn is_bootstrap_peer(&self, peer_id: &PeerId) -> bool {
        self.initial_bootstrap_peer_ids.contains(peer_id)
    }

    /// Has the bootstrap process finished.
    pub(crate) fn has_terminated(&self) -> bool {
        self.bootstrap_completed
    }

    /// Trigger the initial bootstrap process.
    ///
    /// This will start dialing CONCURRENT_DIALS peers at a time from the initial addresses. If we have a successful
    /// dial and if a few peer are added to the routing table, we stop the initial bootstrap process.
    ///
    /// This should be called only ONCE and then the `on_connection_established` and `on_outgoing_connection_error`
    /// should be used to continue the process.
    /// Once the process is completed, the `bootstrap_completed` flag will be set to true, and this becomes a no-op.
    pub(super) fn trigger_bootstrapping_process(
        &mut self,
        swarm: &mut Swarm<NodeBehaviour>,
        peers_in_rt: usize,
    ) {
        if !self.should_we_continue_bootstrapping(peers_in_rt, true) {
            return;
        }

        while self.ongoing_dials.len() < CONCURRENT_DIALS && !self.initial_addrs.is_empty() {
            let Some(mut addr) = self.initial_addrs.pop_front() else {
                continue;
            };

            let addr_clone = addr.clone();
            let peer_id = multiaddr_pop_p2p(&mut addr);

            let opts = match peer_id {
                Some(peer_id) => DialOpts::peer_id(peer_id)
                    // If we have a peer ID, we can prevent simultaneous dials.
                    .condition(PeerCondition::NotDialing)
                    .addresses(vec![addr])
                    .build(),
                None => DialOpts::unknown_peer_id().address(addr).build(),
            };

            info!("Trying to dial peer with address: {addr_clone}",);

            match swarm.dial(opts) {
                Ok(()) => {
                    info!(
                        "Dial attempt initiated for peer with address: {addr_clone}. Ongoing dial attempts: {}",
                        self.ongoing_dials.len()
                    );
                    let _ = self.ongoing_dials.insert(addr_clone);
                }
                Err(err) => match err {
                    DialError::LocalPeerId { .. } => {
                        warn!(
                            "Failed to dial peer with address: {addr_clone}. This is our own peer ID. Dialing the next peer"
                        );
                    }
                    DialError::NoAddresses => {
                        error!(
                            "Failed to dial peer with address: {addr_clone}. No addresses found. Dialing the next peer"
                        );
                    }
                    DialError::DialPeerConditionFalse(_) => {
                        warn!(
                            "We are already dialing the peer with address: {addr_clone}. Dialing the next peer. This error is harmless."
                        );
                    }
                    DialError::Aborted => {
                        error!(
                            " Pending connection attempt has been aborted for {addr_clone}. Dialing the next peer."
                        );
                    }
                    DialError::WrongPeerId { obtained, .. } => {
                        error!(
                            "The peer identity obtained on the connection did not match the one that was expected. Expected: {peer_id:?}, obtained: {obtained}. Dialing the next peer."
                        );
                    }
                    DialError::Denied { cause } => {
                        error!(
                            "The dialing attempt was denied by the remote peer. Cause: {cause}. Dialing the next peer."
                        );
                    }
                    DialError::Transport(items) => {
                        error!(
                            "Failed to dial peer with address: {addr_clone}. Transport error: {items:?}. Dialing the next peer."
                        );
                    }
                },
            }
        }
    }

    /// Check if the initial bootstrap process should be triggered.
    /// Also update bootstrap_completed flag if the process is completed.
    fn should_we_continue_bootstrapping(&mut self, peers_in_rt: usize, verbose: bool) -> bool {
        if self.bootstrap_completed {
            if verbose {
                info!("Initial bootstrap process has already completed successfully.");
            } else {
                trace!("Initial bootstrap process has already completed successfully.");
            }
            return false;
        }

        if peers_in_rt >= MAX_PEERS_BEFORE_TERMINATION {
            // This will terminate the loop
            self.bootstrap_completed = true;
            self.initial_addrs.clear();
            self.ongoing_dials.clear();

            if verbose {
                info!(
                    "Initial bootstrap process completed successfully. We have {peers_in_rt} peers in the routing table."
                );
            } else {
                trace!(
                    "Initial bootstrap process completed successfully. We have {peers_in_rt} peers in the routing table."
                );
            }
            return false;
        }

        if self.ongoing_dials.len() >= CONCURRENT_DIALS {
            if verbose {
                info!(
                    "Initial bootstrap has {} ongoing dials. Not dialing anymore.",
                    self.ongoing_dials.len()
                );
            } else {
                debug!(
                    "Initial bootstrap has {} ongoing dials. Not dialing anymore.",
                    self.ongoing_dials.len()
                );
            }
            return false;
        }

        if peers_in_rt < MAX_PEERS_BEFORE_TERMINATION && self.initial_addrs.is_empty() {
            if verbose {
                info!(
                    "We have {peers_in_rt} peers in RT, but no more addresses to dial. Stopping initial bootstrap."
                );
            } else {
                debug!(
                    "We have {peers_in_rt} peers in RT, but no more addresses to dial. Stopping initial bootstrap."
                );
            }
            return false;
        }

        if self.initial_addrs.is_empty() {
            if verbose {
                warn!("Initial bootstrap has no more addresses to dial.");
            } else {
                debug!("Initial bootstrap has no more addresses to dial.");
            }
            return false;
        }

        true
    }

    pub(super) fn on_connection_established(
        &mut self,
        endpoint: &ConnectedPoint,
        swarm: &mut Swarm<NodeBehaviour>,
        peers_in_rt: usize,
    ) {
        if self.bootstrap_completed {
            return;
        }

        if let ConnectedPoint::Dialer { address, .. } = endpoint {
            if !self.ongoing_dials.remove(address) {
                // try to remove via peer Id, to not block the bootstrap process.
                // The only concern with the following removal is that we might increase the number of
                // dials/concurrent dials, which is fine.
                if let Some(peer_id) = multiaddr_get_p2p(address) {
                    self.ongoing_dials.retain(|addr| {
                        if let Some(id) = multiaddr_get_p2p(addr) {
                            id != peer_id
                        } else {
                            true
                        }
                    });
                }
            }
        }

        self.trigger_bootstrapping_process(swarm, peers_in_rt);
    }

    pub(super) fn on_outgoing_connection_error(
        &mut self,
        peer_id: Option<PeerId>,
        swarm: &mut Swarm<NodeBehaviour>,
        peers_in_rt: usize,
    ) {
        if self.bootstrap_completed {
            return;
        }

        match peer_id {
            Some(peer_id) => {
                self.ongoing_dials.retain(|addr| {
                    if let Some(id) = multiaddr_get_p2p(addr) {
                        id != peer_id
                    } else {
                        true
                    }
                });
            }
            // we are left with no option but to remove all the addresses from the ongoing dials that
            // do not have a peer ID.
            None => {
                self.ongoing_dials
                    .retain(|addr| multiaddr_get_p2p(addr).is_some());
            }
        }

        self.trigger_bootstrapping_process(swarm, peers_in_rt);
    }
}
