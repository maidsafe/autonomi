// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::MAX_DIAL_ATTEMPTS;
use crate::networking::{driver::event::DIAL_BACK_DELAY, multiaddr_get_p2p};
use libp2p::{Multiaddr, PeerId, multiaddr::Protocol, swarm::ConnectionId};
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt,
    net::SocketAddr,
    time::{Duration, Instant},
};

const TIMEOUT_ON_INITIATED_STATE: Duration = Duration::from_secs(30);
const TIMEOUT_ON_CONNECTED_STATE: Duration = Duration::from_secs(20 + DIAL_BACK_DELAY.as_secs());

/// Higher level struct that manages everything that is related to dialing.
#[derive(Debug)]
pub(crate) struct DialManager {
    // The number of attempts/retries we have made with the entire Dialer workflow.
    pub(crate) current_workflow_attempt: usize,
    pub(crate) dialer: Dialer,
    pub(crate) all_dial_attempts: HashMap<PeerId, DialResult>,
    pub(crate) initial_contacts_manager: InitialContactsManager,
}

/// A struct that can be re initialized to start a new reachability check attempt.
#[derive(Debug, Clone, Default)]
pub(crate) struct Dialer {
    // Critical field, should only be managed by the DialManager. Don't try to access it directly.
    ongoing_dial_attempts: HashMap<PeerId, DialState>,
    pub(super) identify_observed_external_addr: HashMap<PeerId, Vec<(SocketAddr, ConnectionId)>>,
    pub(super) incoming_connection_ids: HashSet<ConnectionId>,
    pub(super) incoming_connection_local_adapter_map: HashMap<ConnectionId, SocketAddr>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InitialContactsManager {
    pub(super) initial_contacts: Vec<Multiaddr>,
    pub(super) attempted_indices: HashSet<usize>,
}

/// The final result of a dial attempt.
#[derive(Debug, Clone)]
pub(crate) enum DialResult {
    /// We did not receive any response from the remote peer after dialing.
    TimedOutOnInitiated,

    /// We did not get a dialback in time.
    TimedOutAfterConnecting,

    /// We have received an error from the remote peer.
    ErrorDuringDial,

    /// The dial attempt was successful with the peer.
    SuccessfulDialBack,
}

#[derive(Clone)]
/// The state of a dial attempt that we initiated with a remote peer.
///
/// The state can only be transitioned to Connected or DialBackReceived.
pub(super) enum DialState {
    /// We have initiated a dial attempt.
    Initiated { at: Instant },
    /// We got a successful response from the remote peer. We can now wait for them to contact us back after the
    /// DIAL_BACK_DELAY.
    Connected { at: Instant },
    /// We have received a response from the remote peer after the DIAL_BACK_DELAY.
    DialBackReceived { at: Instant },
}

impl fmt::Debug for DialState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialState::Initiated { at } => write!(
                f,
                "DialState::Initiated at {at:?} elapsed {:?}",
                at.elapsed()
            ),
            DialState::Connected { at } => write!(
                f,
                "DialState::Connected at {at:?} elapsed {:?}",
                at.elapsed()
            ),
            DialState::DialBackReceived { at } => write!(
                f,
                "DialState::DialBackReceived at {at:?} elapsed {:?}",
                at.elapsed()
            ),
        }
    }
}

impl DialState {
    fn elapsed(&self) -> Duration {
        match self {
            DialState::Initiated { at } => at.elapsed(),
            DialState::Connected { at } => at.elapsed(),
            DialState::DialBackReceived { at } => at.elapsed(),
        }
    }

    fn transition_to_connected(&mut self, peer_id: &PeerId) {
        match self {
            DialState::Initiated { .. } => {
                *self = DialState::Connected { at: Instant::now() };
            }
            _ => {
                warn!(
                    "DialState for {peer_id:?} cannot be transitioned to Connected. Current state: {self:?}"
                );
            }
        }
    }

    fn transition_to_dial_back_received(&mut self, peer_id: &PeerId) {
        match self {
            DialState::Connected { at } => {
                if at.elapsed() > DIAL_BACK_DELAY {
                    info!("DialState for {peer_id:?} has been updated to DialBackReceived");
                    *self = DialState::DialBackReceived { at: Instant::now() };
                } else {
                    warn!(
                        "DialState for {peer_id:?} has not been updated to DialBackReceived. We got the response too early."
                    );
                }
            }
            _ => {
                warn!(
                    "DialState for {peer_id:?} cannot be transitioned to DialBackReceived. Current state: {self:?}"
                );
            }
        }
    }
}

impl InitialContactsManager {
    pub(crate) fn new(initial_contacts: Vec<Multiaddr>) -> Self {
        let len = initial_contacts.len();
        let initial_contacts: Vec<Multiaddr> = initial_contacts
            .into_iter()
            .filter(|addr| {
                !addr
                    .iter()
                    .any(|protocol| matches!(protocol, Protocol::P2pCircuit))
            })
            .filter(|addr| {
                addr.iter()
                    .any(|protocol| matches!(protocol, Protocol::P2p(_)))
            })
            .collect();
        info!(
            "Initial contacts len after filtering out circuit addresses and ones without peer ids: {:?}. original len: {len:?}",
            initial_contacts.len()
        );
        Self {
            initial_contacts,
            attempted_indices: HashSet::new(),
        }
    }

    /// Return a random contact from the initial contacts list that we haven't attempted to dial yet.
    pub(crate) fn get_next_contact(&mut self) -> Option<Multiaddr> {
        if self.attempted_indices.len() >= self.initial_contacts.len() {
            return None;
        }

        let mut rng = rand::thread_rng();
        let mut index = rand::Rng::gen_range(&mut rng, 0..self.initial_contacts.len());

        while self.attempted_indices.contains(&index) {
            index = rand::Rng::gen_range(&mut rng, 0..self.initial_contacts.len());
        }

        let _ = self.attempted_indices.insert(index);
        Some(self.initial_contacts[index].clone())
    }

    pub(crate) fn reset(&mut self) {
        self.attempted_indices.clear();
    }
}

impl DialManager {
    pub(crate) fn new(initial_contacts: Vec<Multiaddr>) -> Self {
        Self {
            current_workflow_attempt: 1,
            dialer: Dialer::default(),
            all_dial_attempts: HashMap::new(),
            initial_contacts_manager: InitialContactsManager::new(initial_contacts),
        }
    }

    pub(crate) fn reattempt_workflow(&mut self) {
        self.current_workflow_attempt += 1;
        self.dialer = Dialer::default();
        self.initial_contacts_manager.reset();
    }

    pub(crate) fn get_next_contact(&mut self) -> Option<Multiaddr> {
        self.initial_contacts_manager.get_next_contact()
    }

    /// Check if we can perform a new dial attempt.
    pub(crate) fn can_we_perform_new_dial(&self) -> bool {
        self.dialer.ongoing_dial_attempts.len() < MAX_DIAL_ATTEMPTS
    }

    /// Dialing has completed if:
    /// 1. We still have peers that we haven't successfully connected to yet.
    /// 2. We are still waiting for DIAL_BACK_DELAY on peers whom we have successfully connected to, but not yet received a response from.
    pub(crate) fn has_dialing_completed(&self) -> bool {
        let mut still_waiting_for_dial_back = false;
        debug!(
            "Checking if dialing has completed. Ongoing dial attempts: {:?}",
            self.dialer.ongoing_dial_attempts
        );
        for state in self.dialer.ongoing_dial_attempts.values() {
            match state {
                DialState::Initiated { .. } => {
                    // this state should eventually be cleaned up by `cleanup_dial_attempts`
                    still_waiting_for_dial_back = true;
                }
                DialState::Connected { .. } => {
                    if state.elapsed().as_secs() < TIMEOUT_ON_CONNECTED_STATE.as_secs() {
                        still_waiting_for_dial_back = true;
                    }
                }
                DialState::DialBackReceived { .. } => {}
            }
        }

        !still_waiting_for_dial_back
    }

    /// Check if we are faulty.
    pub(crate) fn are_we_faulty(&self) -> bool {
        if !self.has_dialing_completed() {
            warn!("Dialing has not completed yet. We are not faulty.");
            return false;
        }

        for state in self.dialer.ongoing_dial_attempts.values() {
            match state {
                DialState::DialBackReceived { .. } | DialState::Connected { .. } => {
                    return false;
                }
                _ => {}
            }
        }

        // not faulty if atleast one dial attempt was successful. (i.e, connection established or dial back received)
        let mut faulty = true;
        for dial_result in self.all_dial_attempts.values() {
            match dial_result {
                DialResult::TimedOutAfterConnecting => {
                    faulty = false;
                }
                DialResult::SuccessfulDialBack => {
                    faulty = false;
                }
                _ => {}
            }
        }

        faulty
    }

    pub(crate) fn on_successful_dial(&mut self, peer_id: &PeerId, address: &Multiaddr) {
        let _ = self
            .dialer
            .ongoing_dial_attempts
            .insert(*peer_id, DialState::Initiated { at: Instant::now() });
        info!(
            "Dial attempt initiated for peer with address: {address}. Ongoing dial attempts: {}",
            self.dialer.ongoing_dial_attempts.len()
        );
    }

    pub(crate) fn on_error_during_dial_attempt(&mut self, peer_id: &PeerId) {
        // Any successful/timeout result should be preferred over a dial error.
        if self.all_dial_attempts.contains_key(peer_id) {
            debug!(
                "Not tracking dial attempt error result for {peer_id:?} as we already have better results for it."
            );
            return;
        }

        let _ = self
            .all_dial_attempts
            .insert(*peer_id, DialResult::ErrorDuringDial);
    }

    pub(crate) fn on_connection_established_as_dialer(&mut self, address: &Multiaddr) {
        if let Some(peer_id) = multiaddr_get_p2p(address) {
            let entry = self.dialer.ongoing_dial_attempts
                .entry(peer_id)
                .and_modify(|state| {
                    let old_state = state.clone();
                    state.transition_to_connected(&peer_id);
                    info!("Connection established for {peer_id:?} that we had dialed. We'll wait for dial back now. Transition from {old_state:?} To {state:?}. Elapsed: {:?} seconds", state.elapsed().as_secs());
                });
            if let Entry::Vacant(_) = entry {
                info!(
                    "We have dialed {peer_id:?} that was not in our ongoing dial attempts. This is unexpected. Not tracking it."
                );
            }
        } else {
            warn!("Dialer address does not contain peer id: {address:?}");
        }
    }

    pub(crate) fn on_successful_dial_back_identify(&mut self, peer_id: &PeerId) {
        let entry = self.dialer.ongoing_dial_attempts
            .entry(*peer_id)
            .and_modify(|state| {
                let old_state = state.clone();
                state.transition_to_dial_back_received(peer_id);
                info!("Identify received for for {peer_id:?} that we had dialed! Transition from {old_state:?} To {state:?}. Elapsed: {:?} seconds", state.elapsed().as_secs());
            });

        if let Entry::Vacant(_) = entry {
            info!(
                "We received identify from {peer_id:?} that was not in our ongoing dial attempts. This is unexpected. Not tracking it."
            );
        }
    }

    pub(crate) fn on_outgoing_connection_error(&mut self, peer_id: PeerId) {
        warn!(
            "Dial attempt for peer {peer_id:?} has failed. Removing it from ongoing_dial_attempts."
        );
        let _ = self.dialer.ongoing_dial_attempts.remove(&peer_id);
    }

    // cleanup dial attempts if we're stuck in Attempted state for too long
    pub(crate) fn cleanup_dial_attempts(&mut self) {
        let mut to_remove_peers = Vec::new();
        for (peer, state) in self.dialer.ongoing_dial_attempts.iter() {
            let tracked_peer = self.all_dial_attempts.get(peer);

            match state {
                DialState::Initiated { .. } => {
                    if state.elapsed().as_secs() > TIMEOUT_ON_INITIATED_STATE.as_secs() {
                        info!(
                            "Dial attempt for {peer:?} with state {state:?} has timed out (timeout: {TIMEOUT_ON_INITIATED_STATE:?}). Cleaning up."
                        );
                        to_remove_peers.push(*peer);
                        if tracked_peer.is_some() {
                            // only override dial errors (which are low priority, if we have established a connection on a different address)
                            if let Some(DialResult::ErrorDuringDial) = tracked_peer {
                                let _ = self
                                    .all_dial_attempts
                                    .insert(*peer, DialResult::TimedOutOnInitiated);
                            }
                        } else {
                            let _ = self
                                .all_dial_attempts
                                .insert(*peer, DialResult::TimedOutOnInitiated);
                        }
                    }
                }
                DialState::Connected { .. } => {
                    // Don't cleanup this state. If we did not receive a dial back, then it means that the peer is not reachable.
                    if state.elapsed().as_secs() > TIMEOUT_ON_CONNECTED_STATE.as_secs() {
                        if tracked_peer.is_some() {
                            // only override dial errors (which are low priority, if we have established a connection on a different address)
                            if let Some(DialResult::ErrorDuringDial) = tracked_peer {
                                let _ = self
                                    .all_dial_attempts
                                    .insert(*peer, DialResult::TimedOutAfterConnecting);
                            }
                        } else {
                            let _ = self
                                .all_dial_attempts
                                .insert(*peer, DialResult::TimedOutAfterConnecting);
                        }
                    }
                }
                DialState::DialBackReceived { .. } => {
                    // override if not already successful
                    if !matches!(tracked_peer, Some(DialResult::SuccessfulDialBack)) {
                        let _ = self
                            .all_dial_attempts
                            .insert(*peer, DialResult::SuccessfulDialBack);
                    }
                }
            }
        }

        for peer in to_remove_peers {
            let _ = self.dialer.ongoing_dial_attempts.remove(&peer);
        }
    }
}
