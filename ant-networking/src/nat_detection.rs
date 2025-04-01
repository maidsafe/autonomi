// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::bootstrap::InitialBootstrap;
use crate::error::Result;
use crate::{endpoint_str, multiaddr_get_ip, multiaddr_get_port};
use custom_debug::Debug as CustomDebug;
use futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::ConnectionId;
use libp2p::{identify, Multiaddr, PeerId};
use libp2p::{
    swarm::{NetworkBehaviour, SwarmEvent},
    Swarm,
};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;
pub(crate) const NAT_DETECTION_MAX_SUCCESSFUL_DIALS: usize = 5;

/// The behaviors are polled in the order they are defined.
/// The first struct member is polled until it returns Poll::Pending before moving on to later members.
/// Prioritize the behaviors related to connection handling.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "NatDetectionEvent")]
pub struct NatDetectionBehaviour {
    pub(super) upnp: libp2p::upnp::tokio::Behaviour,
    pub(super) identify: libp2p::identify::Behaviour,
}

/// NatDetectionEvent enum
#[derive(CustomDebug)]
pub enum NatDetectionEvent {
    Upnp(libp2p::upnp::Event),
    Identify(Box<libp2p::identify::Event>),
}

impl From<libp2p::upnp::Event> for NatDetectionEvent {
    fn from(event: libp2p::upnp::Event) -> Self {
        NatDetectionEvent::Upnp(event)
    }
}

impl From<libp2p::identify::Event> for NatDetectionEvent {
    fn from(event: libp2p::identify::Event) -> Self {
        NatDetectionEvent::Identify(Box::new(event))
    }
}

pub struct NatDetectionSwarmDriver {
    pub(crate) swarm: Swarm<NatDetectionBehaviour>,
    pub(crate) state: NatDetectionState,
    pub(crate) initial_contacts: Vec<Multiaddr>,
}

#[derive(Debug, Clone)]
pub enum NatStatus {
    Upnp,
    Public(SocketAddr),
    NonPublic,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum NatDetectionState {
    WaitingForUpnp,
    WaitingForExternalAddr {
        /// List of our observed (IP, port) by various peers
        our_observed_addresses: HashMap<PeerId, Vec<(Ipv4Addr, u16)>>,
        incoming_connections: HashSet<ConnectionId>,
        initial_bootstrap: InitialBootstrap,
    },
}

impl NatDetectionSwarmDriver {
    pub fn new(swarm: Swarm<NatDetectionBehaviour>, initial_contacts: Vec<Multiaddr>) -> Self {
        let swarm = swarm;
        let initial_bootstrap = InitialBootstrap::new(initial_contacts.clone());

        Self {
            swarm,
            state: NatDetectionState::WaitingForExternalAddr {
                our_observed_addresses: Default::default(),
                incoming_connections: Default::default(),
                initial_bootstrap,
            },
            initial_contacts,
        }
    }

    /// Asynchronously drives the swarm event loop. This function will run indefinitely,
    /// until the command channel is closed.
    ///
    /// The `tokio::select` macro is used to concurrently process swarm events
    /// and command receiver messages, ensuring efficient handling of multiple
    /// asynchronous tasks.
    pub async fn detect(mut self) -> Result<NatStatus> {
        loop {
            tokio::select! {
                // next take and react to external swarm events
                swarm_event = self.swarm.select_next_some() => {
                    // logging for handling events happens inside handle_swarm_events
                    // otherwise we're rewriting match statements etc around this anwyay
                    match self.handle_swarm_events(swarm_event) {
                        Ok(Some(status)) => {
                            info!("NAT status has been found to be: {status:?}");
                            return Ok(status);
                        }
                        Ok(None) => {}
                        Err(err) => {
                            warn!("Error while handling swarm event: {err}");
                        }
                    }

            }   }
        }
    }

    fn handle_swarm_events(
        &mut self,
        event: SwarmEvent<NatDetectionEvent>,
    ) -> Result<Option<NatStatus>> {
        let start = Instant::now();
        let event_string;

        match &mut self.state {
            NatDetectionState::WaitingForUpnp => {
                match event {
                    SwarmEvent::Behaviour(NatDetectionEvent::Upnp(upnp_event)) => {
                        event_string = "upnp_event";
                        info!(?upnp_event, "UPnP event");
                        match upnp_event {
                            libp2p::upnp::Event::GatewayNotFound => {
                                info!("UPnP gateway not found. Switching state to WaitingForExternalAddr");
                                self.on_upnp_result();
                            }
                            libp2p::upnp::Event::NewExternalAddr(addr) => {
                                info!("UPnP: New external address: {addr:?}");
                                return Ok(Some(NatStatus::Upnp));
                            }
                            libp2p::upnp::Event::NonRoutableGateway => {
                                warn!("UPnP gateway is not routable. Switching state to WaitingForExternalAddr");
                                self.on_upnp_result();
                            }
                            _ => {
                                info!("UPnP event (ignored): {upnp_event:?}");
                            }
                        }
                    }
                    other => {
                        event_string = "Other";

                        debug!("SwarmEvent has been ignored, we are waiting for UPnP result: {other:?}")
                    }
                }
            }
            NatDetectionState::WaitingForExternalAddr {
                our_observed_addresses,
                incoming_connections: incomming_connections,
                initial_bootstrap,
            } => match event {
                SwarmEvent::NewListenAddr {
                    mut address,
                    listener_id,
                } => {
                    event_string = "new listen addr";

                    let local_peer_id = *self.swarm.local_peer_id();
                    if address.iter().last() != Some(Protocol::P2p(local_peer_id)) {
                        address.push(Protocol::P2p(local_peer_id));
                    }

                    initial_bootstrap.trigger_bootstrapping_process(&mut self.swarm, 0);

                    info!("Local node is listening {listener_id:?} on {address:?}. Adding it as an external address.");
                    self.swarm.add_external_address(address.clone());
                }
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    connection_id,
                    endpoint,
                    num_established,
                    concurrent_dial_errors,
                    established_in,
                } => {
                    event_string = "ConnectionEstablished";
                    debug!(%peer_id, num_established, ?concurrent_dial_errors, "ConnectionEstablished ({connection_id:?}) in {established_in:?}: {}", endpoint_str(&endpoint));

                    initial_bootstrap.on_connection_established(
                        &endpoint,
                        &mut self.swarm,
                        our_observed_addresses.len(),
                    );

                    if endpoint.is_listener() {
                        incomming_connections.insert(connection_id);
                    }
                }

                SwarmEvent::OutgoingConnectionError {
                    connection_id,
                    peer_id,
                    error,
                } => {
                    event_string = "OutgoingConnErr";
                    warn!(
                        "OutgoingConnectionError on {connection_id:?} for {peer_id:?} - {error:?}"
                    );

                    initial_bootstrap.on_outgoing_connection_error(
                        None,
                        &mut self.swarm,
                        our_observed_addresses.len(),
                    );

                    if incomming_connections.remove(&connection_id) {
                        debug!("Removed connection {connection_id:?} from incomming_connections");
                    }
                }
                SwarmEvent::ConnectionClosed {
                    peer_id,
                    connection_id,
                    endpoint,
                    num_established,
                    cause,
                } => {
                    event_string = "ConnectionClosed";
                    debug!(%peer_id, num_established, ?cause, "ConnectionClosed ({connection_id:?}) in {endpoint:?}");

                    if incomming_connections.remove(&connection_id) {
                        debug!("Removed connection {connection_id:?} from incomming_connections");
                    }
                }
                SwarmEvent::Behaviour(NatDetectionEvent::Identify(identify_event)) => {
                    event_string = "Identify";
                    match *identify_event {
                        identify::Event::Received {
                            peer_id,
                            info,
                            connection_id,
                        } => {
                            debug!(conn_id=%connection_id, %peer_id, ?info, "identify: received info");
                            if incomming_connections.contains(&connection_id) {
                                debug!("Received identify info from incoming connection {connection_id:?}. Adding observed address to our list.");
                                Self::insert_observed_address(
                                    our_observed_addresses,
                                    peer_id,
                                    info.observed_addr,
                                );
                            }
                            if our_observed_addresses.len() > NAT_DETECTION_MAX_SUCCESSFUL_DIALS {
                                info!(
                                    "Received enough observed addresses. Determining NAT status."
                                );
                                return Ok(Some(Self::determine_nat_status_on_external_addr(
                                    our_observed_addresses,
                                )));
                            }
                        }
                        libp2p::identify::Event::Sent { .. } => {
                            debug!("identify: {identify_event:?}")
                        }
                        libp2p::identify::Event::Pushed { .. } => {
                            debug!("identify: {identify_event:?}")
                        }
                        libp2p::identify::Event::Error { .. } => {
                            warn!("identify: {identify_event:?}")
                        }
                    }
                }

                other => {
                    event_string = "Other";

                    debug!("SwarmEvent has been ignored: {other:?}")
                }
            },
        }

        trace!(
            "SwarmEvent handled in {:?}: {event_string:?}",
            start.elapsed()
        );

        Ok(None)
    }

    fn on_upnp_result(&mut self) {
        let mut initial_bootstrap = InitialBootstrap::new(self.initial_contacts.clone());

        initial_bootstrap.trigger_bootstrapping_process(&mut self.swarm, 0);

        self.state = NatDetectionState::WaitingForExternalAddr {
            our_observed_addresses: Default::default(),
            incoming_connections: Default::default(),
            initial_bootstrap,
        };
    }

    fn insert_observed_address(
        our_observed_addresses: &mut HashMap<PeerId, Vec<(Ipv4Addr, u16)>>,
        src_peer: PeerId,
        address: Multiaddr,
    ) {
        let ip = multiaddr_get_ip(&address);
        let port = multiaddr_get_port(&address);

        match (ip, port) {
            (Some(IpAddr::V4(ip)), Some(port)) => {
                let address = (ip, port);

                match our_observed_addresses.entry(src_peer) {
                    Entry::Occupied(mut entry) => {
                        let addresses = entry.get_mut();
                        if addresses.contains(&address) {
                            info!(
                                "Observed Address: Peer {src_peer:?} has already observed us at: {address:?}, skipping."
                            );
                        } else {
                            info!("Observed Address: Peer {src_peer:?} has observed us at: {address:?}");
                            addresses.push(address);
                        }
                    }
                    Entry::Vacant(entry) => {
                        info!(
                            "Observed Address: Peer {src_peer:?} has observed us at: {address:?}"
                        );
                        entry.insert(vec![address]);
                    }
                }
            }
            _ => {
                warn!("Unable to parse observed address: {address:?}");
            }
        }
    }

    fn determine_nat_status_on_external_addr(
        our_observed_addresses: &HashMap<PeerId, Vec<(Ipv4Addr, u16)>>,
    ) -> NatStatus {
        info!("Determining NAT status based on observed addresses: {our_observed_addresses:?}");
        let mut ports = HashSet::new();
        let mut ips = HashSet::new();
        for addresses in our_observed_addresses.values() {
            for (ip, port) in addresses {
                ports.insert(*port);
                ips.insert(*ip);
            }
        }

        if ports.len() != 1 {
            info!("Multiple ports observed. NAT status is NonPublic.");
            return NatStatus::NonPublic;
        }

        let port = *ports.iter().next().expect("ports should not be empty");
        if port == 0 {
            info!("Observed port is 0. NAT status is NonPublic.");
            return NatStatus::NonPublic;
        }

        #[allow(clippy::comparison_chain)]
        if ips.len() == 1 {
            let ip = ips.iter().next().expect("ips should not be empty");
            if ip.is_unspecified() || ip.is_documentation() || ip.is_broadcast() {
                info!("Observed address {ip:?} is unspecified. NAT status is NonPublic.");
                NatStatus::NonPublic
            } else if ip.is_private() {
                info!("Observed IP address {ip:?} is non-global. NAT status is Pubic.");
                NatStatus::Public(SocketAddr::new(IpAddr::V4(*ip), port))
            } else {
                info!("Observed IP address {ip:?} is global. NAT status is Public.");
                NatStatus::Public(SocketAddr::new(IpAddr::V4(*ip), port))
            }
        } else if ips.len() > 1 {
            // if mix of private and public IPs, pick the public one.
            // if all are private, prioritize localhost first
            let public_ip = ips
                .iter()
                .filter(|ip| !ip.is_private() || !ip.is_unspecified() || !ip.is_documentation())
                .collect::<Vec<_>>();

            let private_ip = ips.iter().filter(|ip| ip.is_private()).collect::<Vec<_>>();

            if !public_ip.is_empty() {
                info!("We have multiple public IP addresses {public_ip:?}. NAT status is Public.");
                // todo: return all?
                return NatStatus::Public(SocketAddr::new(IpAddr::V4(*public_ip[0]), port));
            }

            if !private_ip.is_empty() {
                // try to pick localhost
                if private_ip.iter().any(|ip| ip.is_loopback()) {
                    info!("We have multiple private IP addresses, picking localhost. NAT status is Public.");
                    return NatStatus::Public(SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::LOCALHOST),
                        port,
                    ));
                }

                info!("We have multiple private IP addresses. NAT status is Public.");
                return NatStatus::Public(SocketAddr::new(IpAddr::V4(*private_ip[0]), port));
            }

            error!("We have multiple IP addresses, but none are private or public. NAT status is NonPublic.");
            NatStatus::NonPublic
        } else {
            error!("We have no IP addresses. NAT status is NonPublic.");
            NatStatus::NonPublic
        }
    }
}
