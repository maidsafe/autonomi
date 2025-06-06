// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{driver::NodeBehaviour, multiaddr_get_ip, multiaddr_get_port, multiaddr_is_global};
use itertools::Itertools;
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId, Swarm};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};

/// The maximum number of reports before an candidate address is confirmed
const MAX_REPORTS_BEFORE_CONFIRMATION: u8 = 3;
/// The maximum number of reports for a confirmed address before switching to a new IP address
const MAX_REPORTS_BEFORE_SWITCHING_IP: u8 = 10;
/// The maximum number of confirmed addresses needed before switching to a new IP address
const MAX_CONFIRMED_ADDRESSES_BEFORE_SWITCHING_IP: u8 = 5;
/// The maximum number of candidates to store
const MAX_CANDIDATES: usize = 50;

/// Manages the external addresses of a Public node. For a relayed node, the RelayManager should deal with
/// adding and removing external addresses. Also, we don't manage "local" addresses here.
// TODO:
// 1. if the max candidate is reached, kick out the oldest candidate sorted by # of reports
#[derive(Debug)]
pub struct ExternalAddressManager {
    /// All the external addresses of the node
    address_states: Vec<ExternalAddressState>,
    /// The current IP address of all the external addresses.
    current_ip_address: Option<IpAddr>,
    /// The peer id of the node
    peer_id: PeerId,
    // Port -> (ok, error) count
    connection_stats: HashMap<u16, PortStats>,
    // Bad ports
    bad_ports: HashSet<u16>,
}

#[derive(Debug, Default)]
struct PortStats {
    ok: usize,
    error: usize,
}

impl PortStats {
    fn success_rate(&self) -> f64 {
        if self.ok + self.error == 0 {
            0.0
        } else {
            self.ok as f64 / (self.ok + self.error) as f64
        }
    }

    fn is_faulty(&self) -> bool {
        // Give the address a chance to prove itself
        if self.ok + self.error < 10 {
            return false;
        }

        // Still give the address a chance to prove itself
        if self.ok + self.error < 100 {
            return self.success_rate() < 0.5;
        }

        self.success_rate() < 0.9
    }
}

impl ExternalAddressManager {
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            address_states: Vec::new(),
            current_ip_address: None,
            peer_id,
            connection_stats: HashMap::new(),
            bad_ports: HashSet::new(),
        }
    }

    /// Get the list of candidate addresses
    pub fn candidate_addresses(&self) -> Vec<&Multiaddr> {
        self.address_states
            .iter()
            .filter_map(|state| {
                if let ExternalAddressState::Candidate { address, .. } = state {
                    Some(address)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Add an external address candidate to the manager.
    /// If the address has been reported often enough, it is confirmed and added to the swarm.
    /// If a new IP address has been reported often enough, then we switch to the new IP address and discard the old
    /// external addresses.
    pub fn add_external_address_candidate(
        &mut self,
        address: Multiaddr,
        swarm: &mut Swarm<NodeBehaviour>,
    ) {
        if !multiaddr_is_global(&address) {
            debug!("Address is not global, ignoring: {address:?}");
            return;
        }

        let Some(address) = self.craft_external_address(&address) else {
            debug!("Address is ill formed, not added to manager: {address:?}");
            return;
        };

        let Some(port) = multiaddr_get_port(&address) else {
            return;
        };

        if self.bad_ports.contains(&port) {
            debug!("External address had problem earlier, ignoring: {address:?}");
            return;
        }

        if let Some(state) = self
            .address_states
            .iter_mut()
            .find(|state| state.multiaddr() == &address)
        {
            state.increment_reports();

            if state.is_candidate() {
                if state.num_reports() >= MAX_REPORTS_BEFORE_CONFIRMATION {
                    // if the IP address of our confirmed address is the same as the new address, then add it
                    let confirmed = if let Some(current_ip_address) = self.current_ip_address {
                        current_ip_address == *state.ip_address()
                    } else {
                        true
                    };

                    if confirmed {
                        info!("External address confirmed, adding it to swarm: {address:?}");
                        swarm.add_external_address(address.clone());
                        *state = ExternalAddressState::Confirmed {
                            address: address.clone(),
                            num_reports: state.num_reports(),
                            ip_address: *state.ip_address(),
                        };

                        Self::print_swarm_state(swarm);
                        return;
                    } else {
                        debug!(
                            "External address {address:?} is not confirmed due to mismatched IP address. Checking if we can switch to new IP."
                        );
                    }
                }
            } else {
                debug!(
                    "External address: {address:?} is already confirmed or a listener. Do nothing"
                );
                return;
            }
        }
        // check if we need to update to new ip.
        // TODO: Need to observe this
        if let Some(current_ip_address) = self.current_ip_address {
            let mut new_ip_map = HashMap::new();

            for state in &self.address_states {
                if let ExternalAddressState::Candidate {
                    ip_address,
                    num_reports,
                    ..
                } = state
                {
                    if current_ip_address != *ip_address
                        && *num_reports >= MAX_REPORTS_BEFORE_SWITCHING_IP
                    {
                        *new_ip_map.entry(ip_address).or_insert(0) += 1;
                    }
                }
            }

            if let Some((&&new_ip, count)) = new_ip_map
                .iter()
                .sorted_by_key(|(_, count)| *count)
                .next_back()
            {
                if *count >= MAX_CONFIRMED_ADDRESSES_BEFORE_SWITCHING_IP {
                    info!("New IP map as count>= {MAX_CONFIRMED_ADDRESSES_BEFORE_SWITCHING_IP}: {new_ip_map:?}");
                    self.switch_to_new_ip(new_ip, swarm);
                    return;
                }
            }
        }

        if self.candidate_addresses().len() >= MAX_CANDIDATES {
            debug!("Max candidates reached, not adding new candidate external address {address:?}");
            return;
        }

        if self
            .address_states
            .iter()
            .any(|state| state.multiaddr() == &address)
        {
            // incremented in the previous find().
            debug!(
                "External address {address:?} already exists in manager. Report count incremented."
            );
            return;
        }

        let Some(ip_address) = multiaddr_get_ip(&address) else {
            return;
        };
        debug!("Added external address to manager: {address:?}");
        self.address_states.push(ExternalAddressState::Candidate {
            address,
            num_reports: 0,
            ip_address,
        });
    }

    /// Adds a non-local listen-addr to the swarm and the manager.
    /// If the IP address of the listen-addr is different from the current IP address, then we directly
    /// switch to the new IP address.
    pub fn on_new_listen_addr(&mut self, listen_addr: Multiaddr, swarm: &mut Swarm<NodeBehaviour>) {
        // only add our global addresses
        let address = if multiaddr_is_global(&listen_addr) {
            let Some(address) = self.craft_external_address(&listen_addr) else {
                error!("Listen address is ill formed, not added to manager: {listen_addr:?}");
                return;
            };
            address
        } else {
            debug!("Listen address is not global, ignoring: {listen_addr:?}");
            return;
        };
        let Some(ip_address) = multiaddr_get_ip(&address) else {
            return;
        };

        // set the current IP address if it is not set
        if self.current_ip_address.is_none() {
            self.current_ip_address = Some(ip_address);
        }

        // Switch to new IP early.
        if let Some(current_ip_address) = self.current_ip_address {
            if current_ip_address != ip_address {
                self.address_states.push(ExternalAddressState::Listener {
                    address: address.clone(),
                    ip_address,
                });
                // this will add it as external addr
                self.switch_to_new_ip(ip_address, swarm);
                return;
            }
        }

        if let Some(state) = self
            .address_states
            .iter_mut()
            .find(|state| state.multiaddr() == &address)
        {
            match state {
                ExternalAddressState::Candidate { ip_address, .. } => {
                    info!("Listen Addr was found as a candidate. Adding it as external to the swarm {address:?}");

                    swarm.add_external_address(address.clone());
                    *state = ExternalAddressState::Listener {
                        address: address.clone(),
                        ip_address: *ip_address,
                    };

                    Self::print_swarm_state(swarm);
                    return;
                }
                ExternalAddressState::Confirmed { ip_address, .. } => {
                    debug!("Listen address was found as confirmed. Changing it to Listener {address:?}.");
                    *state = ExternalAddressState::Listener {
                        address: address.clone(),
                        ip_address: *ip_address,
                    };
                    return;
                }
                ExternalAddressState::Listener { .. } => {
                    debug!("Listen address is already a listener {address:?}. Do nothing");
                    return;
                }
            }
        }

        // if it is a new one, add it as a Listener
        info!("Listen Addr was not found in the manager. Adding it as external to the swarm {address:?}");
        self.address_states.push(ExternalAddressState::Listener {
            address: address.clone(),
            ip_address,
        });
        swarm.add_external_address(address);
    }

    /// Remove a listen-addr from the manager if expired.
    pub fn on_expired_listen_addr(&mut self, listen_addr: Multiaddr, swarm: &Swarm<NodeBehaviour>) {
        let address = if multiaddr_is_global(&listen_addr) {
            let Some(address) = self.craft_external_address(&listen_addr) else {
                error!("Listen address is ill formed, ignoring {listen_addr:?}");
                return;
            };
            address
        } else {
            debug!("Listen address is not global, ignoring: {listen_addr:?}");
            return;
        };

        self.address_states.retain(|state| {
            if state.multiaddr() == &address {
                debug!("Removing listen address from manager: {address:?}");
                // Todo: should we call swarm.remove_listener()? or is it already removed? Confirm with the below debug.
                Self::print_swarm_state(swarm);
                false
            } else {
                true
            }
        });
    }

    pub fn on_incoming_connection_error(
        &mut self,
        on_address: Multiaddr,
        swarm: &mut Swarm<NodeBehaviour>,
    ) {
        let Some(port) = multiaddr_get_port(&on_address) else {
            return;
        };

        let stats = self.connection_stats.entry(port).or_default();
        stats.error = stats.error.saturating_add(1);

        if stats.is_faulty() {
            // remove all the addresses with this port
            let mut removed_confirmed = Vec::new();
            let mut removed_candidates = Vec::new();
            let mut to_remove_indices = Vec::new();

            for (idx, state) in &mut self.address_states.iter().enumerate() {
                if state.is_confirmed() || state.is_candidate() {
                    let Some(state_port) = multiaddr_get_port(state.multiaddr()) else {
                        continue;
                    };

                    if state_port == port {
                        if state.is_confirmed() {
                            removed_confirmed.push(state.multiaddr().clone());
                        } else {
                            removed_candidates.push(state.multiaddr().clone());
                        }
                        to_remove_indices.push(idx);
                    }
                }
            }
            for idx in to_remove_indices.iter().rev() {
                swarm.remove_external_address(self.address_states[*idx].multiaddr());
                self.address_states.remove(*idx);
            }
            if !removed_candidates.is_empty() {
                debug!("Removed external candidates due to connection errors on port {port}: {removed_candidates:?}");
            }
            if !removed_confirmed.is_empty() {
                info!("Removed external addresses due to connection errors on port {port}: {removed_confirmed:?}");
            }
            if !removed_candidates.is_empty() || !removed_confirmed.is_empty() {
                Self::print_swarm_state(swarm);
            }
        }
    }

    /// Reset the incoming connection errors for a port
    pub fn on_established_incoming_connection(&mut self, on_address: Multiaddr) {
        let Some(port) = multiaddr_get_port(&on_address) else {
            return;
        };

        let stats = self.connection_stats.entry(port).or_default();
        stats.ok = stats.ok.saturating_add(1);
    }

    /// Switch to a new IP address. The old external addresses are removed and the new ones are added.
    /// The new IP address is set as the current IP address.
    fn switch_to_new_ip(&mut self, new_ip: IpAddr, swarm: &mut Swarm<NodeBehaviour>) {
        info!("Switching to new IpAddr: {new_ip}");
        self.current_ip_address = Some(new_ip);

        // remove all the old confirmed addresses with different ip
        let mut removed_addresses = Vec::new();
        let mut to_remove_indices = Vec::new();
        for (idx, state) in &mut self.address_states.iter().enumerate() {
            if state.is_candidate() {
                continue;
            }

            if state.ip_address() != &new_ip {
                // todo: should we remove listener from swarm?
                swarm.remove_external_address(state.multiaddr());
                removed_addresses.push(state.multiaddr().clone());
                to_remove_indices.push(idx);
            }
        }
        for idx in to_remove_indices.iter().rev() {
            self.address_states.remove(*idx);
        }
        info!("Removed addresses due to change of IP: {removed_addresses:?}");

        // add the new confirmed addresses with new ip
        for state in &mut self.address_states {
            if state.ip_address() == &new_ip {
                match state {
                    ExternalAddressState::Candidate {
                        address,
                        num_reports,
                        ip_address,
                    } => {
                        if *num_reports >= MAX_REPORTS_BEFORE_SWITCHING_IP {
                            info!("Switching to new IP, adding confirmed address: {address:?}");
                            swarm.add_external_address(address.clone());
                            *state = ExternalAddressState::Confirmed {
                                address: address.clone(),
                                num_reports: *num_reports,
                                ip_address: *ip_address,
                            };
                        }
                    }

                    ExternalAddressState::Listener { address, .. } => {
                        info!("Switching to new IP, adding listen address as external address {address:?}");
                        swarm.add_external_address(address.clone());
                    }
                    _ => {}
                }
            }
        }
        Self::print_swarm_state(swarm);
    }

    /// Craft a proper address Ws or Quic address to avoid any ill formed addresses
    /// Example:
    /// /ip4/131.131.131.131/tcp/53620/ws/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5
    /// /ip4/131.131.131.131/udp/53620/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5
    fn craft_external_address(&self, given_address: &Multiaddr) -> Option<Multiaddr> {
        let mut output_address = Multiaddr::empty();

        let ip = given_address
            .iter()
            .find(|protocol| matches!(protocol, Protocol::Ip4(_)))?;
        output_address.push(ip);

        if let Some(ws_protocol) = given_address
            .iter()
            .find(|protocol| matches!(protocol, Protocol::Ws(_)))
        {
            let port = given_address
                .iter()
                .find(|protocol| matches!(protocol, Protocol::Tcp(_)))?;
            output_address.push(port);
            output_address.push(ws_protocol);
        } else if given_address
            .iter()
            .any(|protocol| matches!(protocol, Protocol::QuicV1))
        {
            let port = given_address
                .iter()
                .find(|protocol| matches!(protocol, Protocol::Udp(_)))?;
            output_address.push(port);
            output_address.push(Protocol::QuicV1);
        } else {
            return None;
        }

        output_address.push(Protocol::P2p(self.peer_id));
        Some(output_address)
    }

    fn print_swarm_state(swarm: &Swarm<NodeBehaviour>) {
        let listen_addr = swarm.listeners().collect::<Vec<_>>();
        let external_addr = swarm.external_addresses().collect::<Vec<_>>();
        // Combined output to reduce cpu/disk usage.
        info!("All External addresses: {external_addr:?}, and listen addresses: {listen_addr:?}");
    }
}

#[derive(Debug)]
enum ExternalAddressState {
    Candidate {
        address: Multiaddr,
        num_reports: u8,
        ip_address: IpAddr,
    },
    Confirmed {
        address: Multiaddr,
        num_reports: u8,
        ip_address: IpAddr,
    },
    Listener {
        address: Multiaddr,
        ip_address: IpAddr,
    },
}

impl ExternalAddressState {
    fn multiaddr(&self) -> &Multiaddr {
        match self {
            Self::Candidate { address, .. } => address,
            Self::Confirmed { address, .. } => address,
            Self::Listener { address, .. } => address,
        }
    }

    fn ip_address(&self) -> &IpAddr {
        match self {
            Self::Candidate { ip_address, .. } => ip_address,
            Self::Confirmed { ip_address, .. } => ip_address,
            Self::Listener { ip_address, .. } => ip_address,
        }
    }

    fn increment_reports(&mut self) {
        debug!(
            "Incrementing reports for address: {}, current reports: {}",
            self.multiaddr(),
            self.num_reports(),
        );
        match self {
            Self::Candidate { num_reports, .. } => *num_reports = num_reports.saturating_add(1),
            Self::Confirmed { num_reports, .. } => *num_reports = num_reports.saturating_add(1),
            Self::Listener { .. } => {}
        }
    }

    fn num_reports(&self) -> u8 {
        match self {
            Self::Candidate { num_reports, .. } => *num_reports,
            Self::Confirmed { num_reports, .. } => *num_reports,
            Self::Listener { .. } => u8::MAX,
        }
    }

    fn is_candidate(&self) -> bool {
        matches!(self, Self::Candidate { .. })
    }

    fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed { .. })
    }
}
