// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::NatDetectionError;
use crate::event::DIAL_BACK_DELAY;
use crate::{endpoint_str, multiaddr_get_p2p, multiaddr_get_socket_addr, multiaddr_pop_p2p};
use custom_debug::Debug as CustomDebug;
use futures::StreamExt;
use libp2p::core::transport::ListenerId;
use libp2p::core::ConnectedPoint;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{ConnectionId, DialError};
use libp2p::{identify, Multiaddr, PeerId};
use libp2p::{
    swarm::{NetworkBehaviour, SwarmEvent},
    Swarm,
};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;
pub(crate) const MAX_DIAL_ATTEMPTS: usize = 5;
const MAX_WORKFLOW_ATTEMPTS: usize = 3;

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
pub enum ReachabilityStatus {
    Upnp,
    Reachable { local_adapter: SocketAddr },
    Unreachable { retry: bool },
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum NatDetectionState {
    WaitingForUpnp,
    WaitingForExternalAddr {
        // The number of attempts/retries we have made using this state.
        current_workflow_attempt: usize,
        ongoing_dial_attempts: HashMap<PeerId, (DialAttemptState, Instant)>,

        listeners: HashMap<ListenerId, Vec<SocketAddr>>,
        identify_observed_external_addr: HashMap<PeerId, Vec<(SocketAddr, ConnectionId)>>,
        incoming_connection_ids: HashSet<ConnectionId>,
        incoming_connection_local_adapter_map: HashMap<ConnectionId, SocketAddr>,
    },
}

#[derive(Debug, Clone)]
pub enum DialAttemptState {
    /// We have initiated a dial attempt.
    InitialDialAttempted,
    /// We got a successful response from the remote peer. We can now wait for them to contact us back after the
    /// DIAL_BACK_DELAY.
    InitialSuccessfulResponseReceived,
    /// We have received a response from the remote peer after the DIAL_BACK_DELAY.
    DialedBackAfterWait,
}

impl NatDetectionSwarmDriver {
    pub fn new(swarm: Swarm<NatDetectionBehaviour>, initial_contacts: Vec<Multiaddr>) -> Self {
        let swarm = swarm;

        Self {
            swarm,
            state: NatDetectionState::WaitingForExternalAddr {
                current_workflow_attempt: 1,
                ongoing_dial_attempts: Default::default(),

                listeners: Default::default(),
                incoming_connection_ids: Default::default(),
                identify_observed_external_addr: Default::default(),
                incoming_connection_local_adapter_map: Default::default(),
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
    pub async fn detect(mut self) -> Result<ReachabilityStatus, crate::NetworkError> {
        let mut dial_check_interval = tokio::time::interval(std::time::Duration::from_secs(5));
        dial_check_interval.tick().await; // first tick is immediate
        loop {
            tokio::select! {
                // next take and react to external swarm events
                swarm_event = self.swarm.select_next_some() => {
                    // logging for handling events happens inside handle_swarm_events
                    // otherwise we're rewriting match statements etc around this anwyay
                    match self.handle_swarm_events(swarm_event) {
                        Ok(Some(status)) => {
                            info!("Reachability status has been found to be: {status:?}");
                            return Ok(status);
                        }
                        Ok(None) => {}
                        Err(err) => {
                            warn!("Error while handling swarm event: {err}");
                        }
                    }
                }
                _ = dial_check_interval.tick() => {
                    // check if we have any ongoing dial attempts
                    match &mut self.state {
                        NatDetectionState::WaitingForUpnp => {}
                        NatDetectionState::WaitingForExternalAddr {
                            ongoing_dial_attempts,
                            identify_observed_external_addr,
                            incoming_connection_local_adapter_map,
                            listeners,
                            ..
                        } => {
                            Self::cleanup_dial_attempts(ongoing_dial_attempts);
                            if let Err(err) = Self::trigger_dial(&mut self.initial_contacts, ongoing_dial_attempts, &mut self.swarm) {
                                warn!("Error while triggering dial: {err}");
                            }

                            if Self::has_dialing_completed(ongoing_dial_attempts) {
                                info!("Dialing completed. We have received enough observed addresses.");
                                match Self::get_reachability_status(
                                    identify_observed_external_addr,
                                    incoming_connection_local_adapter_map,
                                    listeners,
                                ) {
                                    Ok(status) => {
                                        info!("Reachability status has been found to be: {status:?}");
                                        return Ok(status);
                                    }
                                    Err(err) => {
                                        warn!("Error while getting reachability status: {err}");
                                    }
                                }
                            }
                        }
                    }

                }
            }
        }
    }

    fn handle_swarm_events(
        &mut self,
        event: SwarmEvent<NatDetectionEvent>,
    ) -> Result<Option<ReachabilityStatus>, NatDetectionError> {
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
                                self.on_upnp_result()?;
                            }
                            libp2p::upnp::Event::NewExternalAddr(addr) => {
                                info!("UPnP: New external address: {addr:?}");
                                return Ok(Some(ReachabilityStatus::Upnp));
                            }
                            libp2p::upnp::Event::NonRoutableGateway => {
                                warn!("UPnP gateway is not routable. Switching state to WaitingForExternalAddr");
                                self.on_upnp_result()?;
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
                current_workflow_attempt,
                ongoing_dial_attempts,
                incoming_connection_ids,
                identify_observed_external_addr,
                incoming_connection_local_adapter_map,
                listeners,
            } => match event {
                SwarmEvent::NewListenAddr {
                    mut address,
                    listener_id,
                } => {
                    event_string = "new listen addr";

                    let socket_addr = multiaddr_get_socket_addr(&address);
                    if let Some(socket_addr) = socket_addr {
                        listeners.entry(listener_id).or_default().push(socket_addr);
                    } else {
                        warn!("Unable to get socket address from: {address:?}");
                    }

                    let local_peer_id = *self.swarm.local_peer_id();
                    if address.iter().last() != Some(Protocol::P2p(local_peer_id)) {
                        address.push(Protocol::P2p(local_peer_id));
                    }

                    info!("Local node is listening {listener_id:?} on {address:?}. Adding it as an external address.");
                    self.swarm.add_external_address(address.clone());
                }
                SwarmEvent::IncomingConnection {
                    connection_id,
                    local_addr,
                    send_back_addr,
                } => {
                    event_string = "incoming";
                    debug!("IncomingConnection ({connection_id:?}) with local_addr: {local_addr:?} send_back_addr: {send_back_addr:?}");

                    let socket_addr = multiaddr_get_socket_addr(&local_addr);

                    match socket_addr {
                        Some(socket_addr) => {
                            incoming_connection_local_adapter_map
                                .insert(connection_id, socket_addr);
                        }
                        _ => {
                            warn!(
                                "Unable to get socket_addr from local_addr address: {local_addr:?}"
                            );
                        }
                    }
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

                    if let ConnectedPoint::Dialer { address, .. } = endpoint {
                        if let Some(peer_id) = multiaddr_get_p2p(&address) {
                            ongoing_dial_attempts
                                .entry(peer_id)
                                .and_modify(|(state, time)| {
                                    *state = DialAttemptState::InitialSuccessfulResponseReceived;
                                    *time = Instant::now();
                                })
                                .or_insert_with(|| {
                                    (
                                        DialAttemptState::InitialSuccessfulResponseReceived,
                                        Instant::now(),
                                    )
                                });
                        } else {
                            warn!("Dialer address does not contain peer id: {address:?}");
                        }
                    } else {
                        incoming_connection_ids.insert(connection_id);
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

                    // drop the state for the peer
                    if let Some(peer_id) = peer_id {
                        ongoing_dial_attempts.remove(&peer_id);
                        Self::trigger_dial(
                            &mut self.initial_contacts,
                            ongoing_dial_attempts,
                            &mut self.swarm,
                        )?;
                    } else {
                        warn!("OutgoingConnectionError: Peer ID not found");
                    };

                    if incoming_connection_ids.remove(&connection_id) {
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

                    if incoming_connection_ids.remove(&connection_id) {
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
                            debug!(%peer_id, ?info, "identify: received info on {connection_id:?}");
                            if incoming_connection_ids.contains(&connection_id) {
                                debug!("Received identify info from incoming connection {connection_id:?}. Adding observed address to our list.");
                                Self::insert_observed_address(
                                    identify_observed_external_addr,
                                    peer_id,
                                    info.observed_addr,
                                    connection_id,
                                );

                                ongoing_dial_attempts
                                    .entry(peer_id)
                                    .and_modify(|(state, time)| {
                                        if matches!(
                                            state,
                                            DialAttemptState::InitialSuccessfulResponseReceived
                                        ) && time.elapsed() > DIAL_BACK_DELAY
                                        {
                                            info!("State for peer {peer_id:?} has been updated to DialedBackAfterWait");
                                            *state = DialAttemptState::DialedBackAfterWait;
                                            *time = Instant::now();
                                        }
                                    });
                            }
                            if Self::has_dialing_completed(ongoing_dial_attempts) {
                                info!("Dialing completed. We have received enough observed addresses.");
                                return Ok(Some(Self::get_reachability_status(
                                    identify_observed_external_addr,
                                    incoming_connection_local_adapter_map,
                                    listeners,
                                )?));
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

    fn trigger_dial(
        initial_contacts: &mut Vec<Multiaddr>,
        ongoing_dial_attempts: &mut HashMap<PeerId, (DialAttemptState, Instant)>,
        swarm: &mut Swarm<NatDetectionBehaviour>,
    ) -> Result<(), NatDetectionError> {
        while ongoing_dial_attempts.len() < MAX_DIAL_ATTEMPTS {
            // get the first contact with peer id present in it and remove it
            let index = initial_contacts
                .iter()
                .position(|addr| matches!(addr.iter().last(), Some(Protocol::P2p(_))));

            if let Some(index) = index {
                let mut addr = initial_contacts.remove(index);
                let addr_clone = addr.clone();
                let peer_id = multiaddr_pop_p2p(&mut addr).ok_or(
                    NatDetectionError::InvalidState("PeerId should always be present".to_string()),
                )?;

                let opts = DialOpts::peer_id(peer_id)
                    // If we have a peer ID, we can prevent simultaneous dials.
                    .condition(PeerCondition::NotDialing)
                    .addresses(vec![addr])
                    .build();

                info!("Trying to dial peer with address: {addr_clone}",);

                match swarm.dial(opts) {
                    Ok(()) => {
                        ongoing_dial_attempts.insert(
                            peer_id,
                            (DialAttemptState::InitialDialAttempted, Instant::now()),
                        );
                        info!("Dial attempt initiated for peer with address: {addr_clone}. Ongoing dial attempts: {}", ongoing_dial_attempts.len());
                    }
                    Err(err) => match err {
                        DialError::LocalPeerId { .. } => {
                            warn!("Failed to dial peer with address: {addr_clone}. This is our own peer ID. Dialing the next peer");
                        }
                        DialError::NoAddresses => {
                            error!("Failed to dial peer with address: {addr_clone}. No addresses found. Dialing the next peer");
                        }
                        DialError::DialPeerConditionFalse(_) => {
                            warn!("We are already dialing the peer with address: {addr_clone}. Dialing the next peer. This error is harmless.");
                        }
                        DialError::Aborted => {
                            error!(" Pending connection attempt has been aborted for {addr_clone}. Dialing the next peer.");
                        }
                        DialError::WrongPeerId { obtained, .. } => {
                            error!("The peer identity obtained on the connection did not match the one that was expected. Expected: {peer_id:?}, obtained: {obtained}. Dialing the next peer.");
                        }
                        DialError::Denied { cause } => {
                            error!("The dialing attempt was denied by the remote peer. Cause: {cause}. Dialing the next peer.");
                        }
                        DialError::Transport(items) => {
                            error!("Failed to dial peer with address: {addr_clone}. Transport error: {items:?}. Dialing the next peer.");
                        }
                    },
                }
            } else {
                // todo: go for contact without peer id
                break;
            }
        }

        Ok(())
    }

    // cleanup dial attempts if we're stuck in InitialDialAttempted state for too long
    fn cleanup_dial_attempts(
        ongoing_dial_attempts: &mut HashMap<PeerId, (DialAttemptState, Instant)>,
    ) {
        ongoing_dial_attempts.retain(|peer, (state, time)| {
            if matches!(state, DialAttemptState::InitialDialAttempted) {
                let elapsed = time.elapsed();
                if elapsed.as_secs() > 30 {
                    info!("Dial attempt for {peer:?} with state {state:?} has timed out. Cleaning up.");
                    false
                } else {
                    true
                }
            } else {
               true
            }
        });
    }

    /// Dialing has completed if:
    /// 1. We still have peers that we haven't successfully connected to yet.
    /// 2. We are still waiting for DIAL_BACK_DELAY on peers whom we have successfully connected to, but not yet received a response from.
    fn has_dialing_completed(
        ongoing_dial_attempts: &HashMap<PeerId, (DialAttemptState, Instant)>,
    ) -> bool {
        let mut still_waiting_for_dial_back = false;
        for (state, instant) in ongoing_dial_attempts.values() {
            match state {
                DialAttemptState::InitialDialAttempted => {
                    // this state should eventually be cleaned up by `cleanup_dial_attempts`
                    still_waiting_for_dial_back = true;
                }
                DialAttemptState::InitialSuccessfulResponseReceived => {
                    if instant.elapsed() < DIAL_BACK_DELAY {
                        still_waiting_for_dial_back = true;
                    }
                }
                DialAttemptState::DialedBackAfterWait => {}
            }
        }
        !still_waiting_for_dial_back
    }

    fn on_upnp_result(&mut self) -> Result<(), NatDetectionError> {
        let mut ongoing_dial_attempts = HashMap::new();
        Self::trigger_dial(
            &mut self.initial_contacts,
            &mut ongoing_dial_attempts,
            &mut self.swarm,
        )?;

        self.state = NatDetectionState::WaitingForExternalAddr {
            current_workflow_attempt: 1,
            ongoing_dial_attempts,
            listeners: Default::default(),
            identify_observed_external_addr: Default::default(),
            incoming_connection_ids: Default::default(),
            incoming_connection_local_adapter_map: Default::default(),
        };

        Ok(())
    }

    fn insert_observed_address(
        identify_observed_external_addr: &mut HashMap<PeerId, Vec<(SocketAddr, ConnectionId)>>,
        src_peer: PeerId,
        address: Multiaddr,
        connection_id: ConnectionId,
    ) {
        let Some(socket_addr) = multiaddr_get_socket_addr(&address) else {
            warn!("Unable to get socket address from: {address:?}");
            return;
        };

        match identify_observed_external_addr.entry(src_peer) {
            Entry::Occupied(mut entry) => {
                let addresses = entry.get_mut();

                info!("Observed Address: Peer {src_peer:?} has observed us at: {address:?}");
                addresses.push((socket_addr, connection_id));
            }
            Entry::Vacant(entry) => {
                info!("Observed Address: Peer {src_peer:?} has observed us at: {address:?}");
                entry.insert(vec![(socket_addr, connection_id)]);
            }
        }
    }

    /// First we try to determine if we are reachable or not.
    ///
    /// And then we map the external address to the local adapter address.
    /// If the local adapter is unspecified, we can use any address from the same ListenerId.
    fn get_reachability_status(
        identify_observed_external_addr: &HashMap<PeerId, Vec<(SocketAddr, ConnectionId)>>,
        incoming_connection_local_adapter_map: &HashMap<ConnectionId, SocketAddr>,
        listeners: &HashMap<ListenerId, Vec<SocketAddr>>,
    ) -> Result<ReachabilityStatus, NatDetectionError> {
        let (reachable_addresses, retry) =
            Self::determine_reachability_via_external_addr(identify_observed_external_addr)?;
        if reachable_addresses.is_empty() {
            debug!("No reachable addresses found. We are unreachable.");
            return Ok(ReachabilityStatus::Unreachable { retry });
        }

        // find all connection ids for the reachable addresses
        let mut reachable_connection_ids = HashMap::new();
        for reachable_addr in &reachable_addresses {
            for addrs in identify_observed_external_addr.values() {
                for (addr, connection_id) in addrs {
                    if addr == reachable_addr {
                        reachable_connection_ids
                            .entry(*reachable_addr)
                            .or_insert(vec![])
                            .push(*connection_id);
                    }
                }
            }
        }

        info!("Reachable addresses: {reachable_addresses:?}");
        info!("Reachable connection ids: {reachable_connection_ids:?}");

        let mut external_to_local_addr_map: HashMap<SocketAddr, HashSet<SocketAddr>> =
            HashMap::new();
        for (reachable_addr, connection_ids) in reachable_connection_ids {
            for connection_id in connection_ids {
                if let Some(local_adapter_addr) =
                    incoming_connection_local_adapter_map.get(&connection_id)
                {
                    if let IpAddr::V4(ip) = local_adapter_addr.ip() {
                        if ip.is_unspecified() || ip.is_documentation() || ip.is_broadcast() {
                            info!("Local adapter address {local_adapter_addr:?} is unspecified. Fetching another local adapter address from the listener.");
                            for (listener_id, listener_addrs) in listeners {
                                if listener_addrs.iter().any(|addr| addr == local_adapter_addr) {
                                    info!("Listener {listener_id:?} has local adapter address {local_adapter_addr:?} in it's list {listener_addrs:?}");
                                    // get a listener that is not the current local_adapter_addr
                                    if let Some(another_listener_address) = listener_addrs
                                        .iter()
                                        .find(|&addr| addr != local_adapter_addr)
                                    {
                                        info!("Using {another_listener_address:?} as the local adapter address, instead of {local_adapter_addr:?} for the reachable address {reachable_addr:?}");
                                        external_to_local_addr_map
                                            .entry(reachable_addr)
                                            .or_default()
                                            .insert(*another_listener_address);
                                    }
                                    break;
                                }
                            }
                        } else {
                            external_to_local_addr_map
                                .entry(reachable_addr)
                                .or_default()
                                .insert(*local_adapter_addr);
                        }
                    } else {
                        continue;
                    }
                }
            }
        }

        info!("External address to local adapter map: {external_to_local_addr_map:?}");

        // pop first one
        let (reachable_addr, local_adapter_addrs) =
            external_to_local_addr_map.into_iter().next().ok_or(
                NatDetectionError::InvalidState("No reachable addresses found".to_string()),
            )?;

        info!("Reachable address: {reachable_addr:?} and corresponding local adapter: {local_adapter_addrs:?}");

        Ok(ReachabilityStatus::Reachable {
            local_adapter: *local_adapter_addrs.iter().next().ok_or(
                NatDetectionError::InvalidState("No local adapter addresses found".to_string()),
            )?,
        })
    }

    /// We received our observed addrs via identify. We would now determine if we're reachable/unreachable via the
    /// identify external addr.
    ///
    /// Returns a vector of addresses that are reachable and a boolean to retry the entire process.
    fn determine_reachability_via_external_addr(
        identify_observed_external_addr: &HashMap<PeerId, Vec<(SocketAddr, ConnectionId)>>,
    ) -> Result<(Vec<SocketAddr>, bool), NatDetectionError> {
        info!("Determining NAT status based on observed addresses: {identify_observed_external_addr:?}");

        if identify_observed_external_addr.is_empty() {
            info!("No observed addresses found. We're unreachable.");
            return Ok((vec![], false));
        } else if identify_observed_external_addr.len() < 3 {
            info!("Not enough observed addresses found. Retrying.");
            return Ok((vec![], true));
        }

        let mut ports = HashSet::new();
        let mut ips = HashSet::new();
        for addresses in identify_observed_external_addr.values() {
            for (addr, _id) in addresses {
                ports.insert(addr.port());
                if let IpAddr::V4(ip) = addr.ip() {
                    ips.insert(ip);
                }
            }
        }

        if ports.len() != 1 {
            info!("Multiple ports observed. Reachability status is Unreachable. This should not happen as symmetric NATs should not get a response back.");
            return Ok((vec![], false));
        }

        let port = *ports.iter().next().ok_or(NatDetectionError::InvalidState(
            "Ports should not be empty".to_string(),
        ))?;
        if port == 0 {
            info!("Observed port is 0. Reachability status is Unreachable.");
            return Ok((vec![], false));
        }

        #[allow(clippy::comparison_chain)]
        if ips.len() == 1 {
            let ip = ips.iter().next().ok_or(NatDetectionError::InvalidState(
                "IPs should not be empty".to_string(),
            ))?;
            if ip.is_unspecified() || ip.is_documentation() || ip.is_broadcast() {
                info!(
                    "Observed address {ip:?} is unspecified. Reachability status is Unreachable."
                );
                Ok((vec![], false))
            } else if ip.is_private() {
                let addr = SocketAddr::new(IpAddr::V4(*ip), port);
                info!(
                    "Observed IP address {addr:?} is non-global. Reachability status is Reachable."
                );
                Ok((vec![addr], false))
            } else {
                let addr = SocketAddr::new(IpAddr::V4(*ip), port);
                info!("Observed IP address {addr:?} is global. Reachability status is Reachable.");
                Ok((vec![addr], false))
            }
        } else if ips.len() > 1 {
            // if mix of private and public IPs, pick the private one (i.e,. on a local testnet on a public machine)
            // if all are private, prioritize localhost first
            let public_ip = ips
                .iter()
                .filter(|ip| {
                    !ip.is_private()
                        && !ip.is_unspecified()
                        && !ip.is_documentation()
                        && !ip.is_loopback()
                })
                .collect::<Vec<_>>();

            let private_ip = ips
                .iter()
                .filter(|ip| ip.is_private() || ip.is_loopback())
                .collect::<Vec<_>>();

            if !private_ip.is_empty() {
                // try to pick localhost
                if private_ip.iter().any(|ip| ip.is_loopback()) {
                    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
                    info!("We have multiple private IP addresses, picking localhost: {addr:?}. Reachability status is Reachable.");
                    return Ok((vec![addr], false));
                }

                let addrs = private_ip
                    .iter()
                    .map(|ip| SocketAddr::new(IpAddr::V4(**ip), port))
                    .collect::<Vec<_>>();
                info!("We have multiple private IP addresses {addrs:?}. Reachability status is Reachable.");
                return Ok((addrs, false));
            }

            if !public_ip.is_empty() {
                let addrs = public_ip
                    .iter()
                    .map(|ip| SocketAddr::new(IpAddr::V4(**ip), port))
                    .collect::<Vec<_>>();
                info!("We have multiple public IP addresses {addrs:?}. Reachability status is Reachable.");
                return Ok((addrs, false));
            }

            error!("We have multiple IP addresses, but none are private or public. Reachability status is Unreachable.");
            return Ok((vec![], false));
        } else {
            error!("We have no IP addresses. NAT status is NonPublic.");
            return Ok((vec![], false));
        }
    }
}

fn should_we_retry(status: &ReachabilityStatus, state: &NatDetectionState) -> bool {
    let current_workflow_attempt = match state {
        NatDetectionState::WaitingForExternalAddr {
            current_workflow_attempt,
            ..
        } => *current_workflow_attempt,
        NatDetectionState::WaitingForUpnp => return false,
    };

    match status {
        ReachabilityStatus::Unreachable { retry } => {
            if *retry {
                if current_workflow_attempt <= MAX_WORKFLOW_ATTEMPTS {
                    info!(
                        "Retrying entire workflow. Current workflow attempt: {} of {MAX_WORKFLOW_ATTEMPTS}",
                        current_workflow_attempt + 1
                    );
                    true
                } else {
                    info!("Max workflow attempts reached. Not retrying.");
                    false
                }
            } else {
                info!("No retry needed.");
                false
            }
        }
        ReachabilityStatus::Reachable { .. } => false,
        ReachabilityStatus::Upnp => false,
    }
}
