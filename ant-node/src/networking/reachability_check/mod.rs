// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod dialer;
mod listener;

use custom_debug::Debug as CustomDebug;
use dialer::DialManager;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{ConnectionId, DialError};
use libp2p::{Multiaddr, PeerId, identify};
use libp2p::{
    Swarm,
    swarm::{NetworkBehaviour, SwarmEvent},
};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;

use crate::networking::error::ReachabilityCheckError;
#[cfg(feature = "open-metrics")]
use crate::networking::metrics::NetworkMetricsRecorder;
use crate::networking::network::endpoint_str;
use crate::networking::reachability_check::listener::get_all_listeners;
use crate::networking::{NetworkError, multiaddr_get_socket_addr, multiaddr_pop_p2p};

pub(crate) const MAX_DIAL_ATTEMPTS: usize = 5;
const MAX_WORKFLOW_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
/// The reachability status of the node.
pub enum ReachabilityStatus {
    /// We are reachable and have an external address.
    Reachable {
        /// The external address we are reachable at.
        addr: SocketAddr,
        /// Whether UPnP is supported or not.
        upnp: bool,
    },
    /// We are not externally reachable.
    NotRoutable {
        /// Whether UPnP is supported or not.
        upnp: bool,
    },
}

impl ReachabilityStatus {
    pub(crate) fn upnp_supported(&self) -> bool {
        match self {
            ReachabilityStatus::Reachable { upnp, .. } => *upnp,
            ReachabilityStatus::NotRoutable { upnp } => *upnp,
        }
    }

    pub(crate) fn is_reachable(&self) -> bool {
        matches!(self, ReachabilityStatus::Reachable { .. })
    }

    pub(crate) fn is_not_routable(&self) -> bool {
        matches!(self, ReachabilityStatus::NotRoutable { .. })
    }
}

/// The behaviors are polled in the order they are defined.
/// The first struct member is polled until it returns Poll::Pending before moving on to later members.
/// Prioritize the behaviors related to connection handling.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "ReachabilityCheckEvent")]
pub(crate) struct ReachabilityCheckBehaviour {
    pub(super) upnp: libp2p::upnp::tokio::Behaviour,
    pub(super) identify: libp2p::identify::Behaviour,
}

/// ReachabilityCheckEvent enum
#[derive(CustomDebug)]
pub(crate) enum ReachabilityCheckEvent {
    Upnp(libp2p::upnp::Event),
    Identify(Box<libp2p::identify::Event>),
}

impl From<libp2p::upnp::Event> for ReachabilityCheckEvent {
    fn from(event: libp2p::upnp::Event) -> Self {
        ReachabilityCheckEvent::Upnp(event)
    }
}

impl From<libp2p::identify::Event> for ReachabilityCheckEvent {
    fn from(event: libp2p::identify::Event) -> Self {
        ReachabilityCheckEvent::Identify(Box::new(event))
    }
}

pub(crate) struct ReachabilityCheckSwarmDriver {
    pub(crate) swarm: Swarm<ReachabilityCheckBehaviour>,
    pub(crate) upnp_supported: bool,
    pub(crate) dial_manager: DialManager,
    #[cfg(feature = "open-metrics")]
    pub(crate) metrics_recorder: Option<NetworkMetricsRecorder>,
}

impl ReachabilityCheckSwarmDriver {
    /// Create a new instance of the reachability check driver.
    ///
    /// We need atleast 5 working, non-circuit contacts, with PeerId on them to be able to determine
    /// the reachability status.
    pub(crate) async fn new(
        swarm: Swarm<ReachabilityCheckBehaviour>,
        keypair: &Keypair,
        local: bool,
        listen_addr: SocketAddr,
        initial_contacts: Vec<Multiaddr>,
        #[cfg(feature = "open-metrics")] metrics_recorder: Option<NetworkMetricsRecorder>,
    ) -> Result<Self, NetworkError> {
        let mut swarm = swarm;

        let observed_listeners = get_all_listeners(keypair, local, listen_addr).await?;

        if observed_listeners.is_empty() {
            error!("No listen addresses found. Cannot start reachability check.");
            return Err(NetworkError::NoListenAddressesFound);
        } else if observed_listeners.len() == 1 {
            if let Some(listen_socket_addr) = observed_listeners.iter().next() {
                if listen_socket_addr.ip().is_unspecified() {
                    error!(
                        "The only listen address found is unspecified. Cannot start reachability check."
                    );
                    return Err(NetworkError::NoListenAddressesFound);
                }
            }
        }

        let mut listeners: HashMap<ListenerId, HashSet<IpAddr>> = HashMap::new();
        for listen_addr in observed_listeners {
            // Listen on QUIC
            let addr_quic = Multiaddr::from(listen_addr.ip())
                .with(Protocol::Udp(listen_addr.port()))
                .with(Protocol::QuicV1);

            let listen_id = swarm
                .listen_on(addr_quic.clone())
                .expect("Multiaddr should be supported by our configured transports");

            info!("Listening on {listen_id:?} with addr: {addr_quic:?}");

            let ip_addr = listen_addr.ip();
            let _ = listeners.entry(listen_id).or_default().insert(ip_addr);
        }

        Ok(Self {
            swarm,
            dial_manager: DialManager::new(initial_contacts),
            upnp_supported: false,
            #[cfg(feature = "open-metrics")]
            metrics_recorder,
        })
    }
    /// Runs the reachability check workflow.
    pub(crate) async fn detect(mut self) -> Result<ReachabilityStatus, NetworkError> {
        info!("Starting reachability check workflow.");
        println!(
            "Reachability check workflow started. Current workflow attempt: 1 of {MAX_WORKFLOW_ATTEMPTS}"
        );
        let mut dial_check_interval = tokio::time::interval(std::time::Duration::from_secs(5));
        let _ = dial_check_interval.tick().await; // first tick is immediate
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
                            error!("Error while handling swarm event: {err}");
                            return Err(err.into());
                        }
                    }
                }
                _ = dial_check_interval.tick() => {
                    if let Some(status) = self.handle_dial_check_interval()? {
                        return Ok(status)
                    }
                }
            }
        }
    }

    fn handle_swarm_events(
        &mut self,
        event: SwarmEvent<ReachabilityCheckEvent>,
    ) -> Result<Option<ReachabilityStatus>, ReachabilityCheckError> {
        let start = Instant::now();
        let event_string;

        match event {
            SwarmEvent::NewListenAddr {
                mut address,
                listener_id,
            } => {
                event_string = "new listen addr";

                let local_peer_id = *self.swarm.local_peer_id();
                if address.iter().last() != Some(Protocol::P2p(local_peer_id)) {
                    address.push(Protocol::P2p(local_peer_id));
                }

                info!(
                    "Local node is listening {listener_id:?} on {address:?}. Adding it as an external address."
                );
                self.swarm.add_external_address(address.clone());
            }
            SwarmEvent::Behaviour(ReachabilityCheckEvent::Upnp(upnp_event)) => {
                event_string = "upnp_event";
                info!(?upnp_event, "UPnP event");
                let mut upnp_result_obtained = false;
                match upnp_event {
                    libp2p::upnp::Event::GatewayNotFound => {
                        info!("UPnP gateway not found. Trying to dial peers.");
                        self.upnp_supported = false;
                        upnp_result_obtained = true;
                    }
                    libp2p::upnp::Event::NewExternalAddr(addr) => {
                        info!(
                            "UPnP: New external address: {addr:?}. Trying to dial peers to confirm reachability."
                        );
                        self.upnp_supported = true;
                        upnp_result_obtained = false;
                    }
                    libp2p::upnp::Event::NonRoutableGateway => {
                        warn!("UPnP gateway is not routable. Trying to dial peers.");
                        self.upnp_supported = false;
                        upnp_result_obtained = true;
                    }
                    _ => {
                        info!("UPnP event (ignored): {upnp_event:?}");
                    }
                }

                if upnp_result_obtained {
                    self.trigger_dial()?;
                }
            }
            SwarmEvent::IncomingConnection {
                connection_id,
                local_addr,
                send_back_addr,
            } => {
                event_string = "incoming";
                debug!(
                    "IncomingConnection ({connection_id:?}) with local_addr: {local_addr:?} send_back_addr: {send_back_addr:?}"
                );

                let socket_addr = multiaddr_get_socket_addr(&local_addr);

                match socket_addr {
                    Some(socket_addr) => {
                        let _ = self
                            .dial_manager
                            .dialer
                            .incoming_connection_local_adapter_map
                            .insert(connection_id, socket_addr);
                    }
                    _ => {
                        warn!("Unable to get socket_addr from local_addr address: {local_addr:?}");
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

                // If we have dialed, then transition to connected state
                if let ConnectedPoint::Dialer { address, .. } = endpoint {
                    self.dial_manager
                        .on_connection_established_as_dialer(&address);
                } else {
                    let _ = self
                        .dial_manager
                        .dialer
                        .incoming_connection_ids
                        .insert(connection_id);
                }
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                peer_id,
                error,
            } => {
                event_string = "OutgoingConnErr";
                warn!("OutgoingConnectionError on {connection_id:?} for {peer_id:?} - {error:?}");

                // drop the state for the peer
                if let Some(peer_id) = peer_id {
                    self.dial_manager.on_outgoing_connection_error(peer_id);

                    self.trigger_dial()?;
                } else {
                    warn!("OutgoingConnectionError: Peer ID not found");
                };

                if self
                    .dial_manager
                    .dialer
                    .incoming_connection_ids
                    .remove(&connection_id)
                {
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

                if self
                    .dial_manager
                    .dialer
                    .incoming_connection_ids
                    .remove(&connection_id)
                {
                    debug!("Removed connection {connection_id:?} from incomming_connections");
                }
            }
            SwarmEvent::Behaviour(ReachabilityCheckEvent::Identify(identify_event)) => {
                event_string = "Identify";
                match *identify_event {
                    identify::Event::Received {
                        peer_id,
                        info,
                        connection_id,
                    } => {
                        debug!(%peer_id, ?info, "identify: received info on {connection_id:?}");
                        if self
                            .dial_manager
                            .dialer
                            .incoming_connection_ids
                            .contains(&connection_id)
                        {
                            debug!(
                                "Received identify info from incoming connection {connection_id:?}. Adding observed address to our list."
                            );
                            self.insert_observed_address(
                                peer_id,
                                info.observed_addr,
                                connection_id,
                            );
                            self.dial_manager.on_successful_dial_back_identify(&peer_id);
                        }
                        if self.dial_manager.has_dialing_completed() {
                            info!("Dialing completed. Checking the reachability status now.");
                            return self.get_reachability_status();
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
        }

        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = &self.metrics_recorder {
            let _ = metrics_recorder
                .connected_peers
                .set(self.swarm.connected_peers().count() as i64);
        }

        trace!(
            "SwarmEvent handled in {:?}: {event_string:?}",
            start.elapsed()
        );

        Ok(None)
    }

    fn handle_dial_check_interval(
        &mut self,
    ) -> Result<Option<ReachabilityStatus>, ReachabilityCheckError> {
        // check if we have any ongoing dial attempts
        self.dial_manager.cleanup_dial_attempts();
        self.trigger_dial()?;

        if self.dial_manager.has_dialing_completed() {
            info!("Dialing completed. Checking the reachability status now.");
            match self.get_reachability_status() {
                Ok(Some(status)) => {
                    info!("Reachability status has been found to be: {status:?}");
                    Ok(Some(status))
                }
                Ok(None) => {
                    info!("Reachability status is not yet determined.");
                    Ok(None)
                }
                Err(err) => {
                    warn!("Error while getting reachability status: {err}");
                    Err(err)
                }
            }
        } else {
            Ok(None)
        }
    }

    fn trigger_dial(&mut self) -> Result<(), ReachabilityCheckError> {
        while self.dial_manager.can_we_perform_new_dial() {
            let Some(mut addr) = self.dial_manager.get_next_contact() else {
                info!(
                    "Dialer has no more contacts to dial. The get_reachability_status method will now calculate the reachability status."
                );
                return Ok(());
            };

            let addr_clone = addr.clone();
            let peer_id =
                multiaddr_pop_p2p(&mut addr).ok_or(ReachabilityCheckError::EmptyPeerId)?;

            let opts = DialOpts::peer_id(peer_id)
                // If we have a peer ID, we can prevent simultaneous dials.
                .condition(PeerCondition::NotDialing)
                .addresses(vec![addr])
                .build();

            info!("Trying to dial peer with address: {addr_clone}",);

            match self.swarm.dial(opts) {
                Ok(()) => {
                    self.dial_manager.on_successful_dial(&peer_id, &addr_clone);
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
                        // only track error that occured due to io
                        self.dial_manager.on_error_during_dial_attempt(&peer_id);
                    }
                },
            }
        }

        Ok(())
    }

    fn insert_observed_address(
        &mut self,
        src_peer: PeerId,
        address: Multiaddr,
        connection_id: ConnectionId,
    ) {
        let Some(socket_addr) = multiaddr_get_socket_addr(&address) else {
            warn!("Unable to get socket address from: {address:?}");
            return;
        };

        match self
            .dial_manager
            .dialer
            .identify_observed_external_addr
            .entry(src_peer)
        {
            Entry::Occupied(mut entry) => {
                let addresses = entry.get_mut();

                info!("Observed Address: Peer {src_peer:?} has observed us at: {address:?}");
                addresses.push((socket_addr, connection_id));
            }
            Entry::Vacant(entry) => {
                info!("Observed Address: Peer {src_peer:?} has observed us at: {address:?}");
                let _ = entry.insert(vec![(socket_addr, connection_id)]);
            }
        }
    }

    /// First we try to determine if we are reachable or not.
    ///
    /// And then we map the external address to the local adapter address.
    /// If the local adapter is unspecified, we can use any address from the same ListenerId.
    fn get_reachability_status(
        &mut self,
    ) -> Result<Option<ReachabilityStatus>, ReachabilityCheckError> {
        let external_addr_result = self.determine_reachability_via_external_addr()?;

        if external_addr_result.retry {
            if self.do_we_have_retries_left() {
                println!(
                    "Retrying reachability check workflow. Current workflow attempt: {} of {MAX_WORKFLOW_ATTEMPTS}",
                    self.dial_manager.current_workflow_attempt + 1
                );
                info!(
                    "Retrying reachability check workflow. Current workflow attempt: {} of {MAX_WORKFLOW_ATTEMPTS}",
                    self.dial_manager.current_workflow_attempt + 1
                );
                self.dial_manager.reattempt_workflow();
                self.trigger_dial()?;
                return Ok(None);
            } else {
                info!("Max reachability check workflow attempts reached. Not retrying.");
            }
        }
        if external_addr_result.terminate {
            info!("Terminating the node as we are not routable.");
            return Ok(Some(ReachabilityStatus::NotRoutable {
                upnp: self.upnp_supported,
            }));
        }

        if external_addr_result.reachable_addresses.is_empty() {
            debug!(
                "No reachable addresses found. This should not happen. Terminating the node as we are not routable."
            );
            return Ok(Some(ReachabilityStatus::NotRoutable {
                upnp: self.upnp_supported,
            }));
        }

        // find all connection ids for the reachable addresses
        let mut reachable_connection_ids = HashMap::new();
        for reachable_addr in &external_addr_result.reachable_addresses {
            for addrs in self
                .dial_manager
                .dialer
                .identify_observed_external_addr
                .values()
            {
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

        info!(
            "Reachable addresses: {:?}",
            external_addr_result.reachable_addresses
        );
        info!("Reachable connection ids: {reachable_connection_ids:?}");
        info!(
            "Incoming connection local adapter map: {:?}",
            self.dial_manager
                .dialer
                .incoming_connection_local_adapter_map
        );

        let mut external_to_local_addr_map: HashMap<SocketAddr, HashSet<SocketAddr>> =
            HashMap::new();
        for (reachable_addr, connection_ids) in reachable_connection_ids {
            for connection_id in connection_ids {
                let Some(local_adapter_addr) = self
                    .dial_manager
                    .dialer
                    .incoming_connection_local_adapter_map
                    .get(&connection_id)
                else {
                    warn!(
                        "Unable to get local adapter address for connection id {connection_id:?}"
                    );
                    continue;
                };
                info!(
                    "Local adapter address for connection id {connection_id:?} is {local_adapter_addr:?}"
                );

                let IpAddr::V4(local_adapter_ip) = local_adapter_addr.ip() else {
                    warn!(
                        "Local adapter address {local_adapter_addr:?} is not an IPv4 address. Skipping."
                    );
                    continue;
                };

                if local_adapter_ip.is_unspecified() // 0.0.0.0
                    || local_adapter_ip.is_documentation()
                    || local_adapter_ip.is_broadcast()
                {
                    warn!(
                        "Local adapter address {local_adapter_ip:?} is unspecified, documentation or broadcast. Skipping."
                    );
                } else {
                    info!(
                        "Local adapter address {local_adapter_ip:?} is valid. Adding it to the external to local address map."
                    );
                    let _ = external_to_local_addr_map
                        .entry(reachable_addr)
                        .or_default()
                        .insert(*local_adapter_addr);
                }
            }
        }

        if external_to_local_addr_map.is_empty() {
            info!(
                "No local adapter mapping found for the reachable addresses. Returning the first external address instead."
            );
            let addr = external_addr_result
                .reachable_addresses
                .first()
                .ok_or(ReachabilityCheckError::ExternalAddrsShouldNotBeEmpty)?;
            return Ok(Some(ReachabilityStatus::Reachable {
                addr: *addr,
                upnp: self.upnp_supported,
            }));
        }

        info!("External address to local adapter map exists: {external_to_local_addr_map:?}");

        // prioritize the case where reachable address is the same as local adapter address
        // if not, pick the first one
        if let Some((reachable_addr, _)) =
            external_to_local_addr_map
                .iter()
                .find(|(reachable_addr, local_adapter_addrs)| {
                    local_adapter_addrs
                        .iter()
                        .any(|addr| addr.ip() == reachable_addr.ip())
                })
        {
            info!(
                "Found a reachable address {reachable_addr:?} that is the same as the local adapter address."
            );
            return Ok(Some(ReachabilityStatus::Reachable {
                addr: *reachable_addr,
                upnp: self.upnp_supported,
            }));
        }

        info!(
            "No reachable address found that is the same as the local adapter address. Picking the first external address & its first local adapter address."
        );

        let (reachable_addr, local_adapter_addrs) =
            external_to_local_addr_map
                .into_iter()
                .next()
                .ok_or(ReachabilityCheckError::ExternalAddrsShouldNotBeEmpty)?;

        info!(
            "Reachable address: {reachable_addr:?} and corresponding local adapter: {local_adapter_addrs:?}. Returning the first local adapter address."
        );

        Ok(Some(ReachabilityStatus::Reachable {
            addr: *local_adapter_addrs
                .iter()
                .next()
                .ok_or(ReachabilityCheckError::LocalAdapterShouldNotBeEmpty)?,
            upnp: self.upnp_supported,
        }))
    }

    /// We received our observed addrs via identify. We would now determine if we're reachable/unreachable via the
    /// identify external addr.
    ///
    /// Returns a vector of addresses that are reachable and a boolean to retry the entire process.
    fn determine_reachability_via_external_addr(
        &self,
    ) -> Result<ExternalAddrResult, ReachabilityCheckError> {
        let mut result = ExternalAddrResult {
            retry: false,
            terminate: false,
            reachable_addresses: vec![],
        };
        info!(
            "Determining reachability status based on observed addresses: {:?}",
            self.dial_manager.dialer.identify_observed_external_addr
        );

        if self
            .dial_manager
            .dialer
            .identify_observed_external_addr
            .is_empty()
        {
            info!("No observed addresses found. Check if we atleast made any successful dials.");
            if self.dial_manager.are_we_faulty() {
                error!(
                    "We are faulty. We have not made any successful dials. Terminating the node immediately."
                );
                result.terminate = true;
            } else {
                info!(
                    "We are not faulty, but we have not made any successful dials. Retrying again, else terminating."
                );
                result.terminate = true;
                result.retry = true;
            }
        } else if self
            .dial_manager
            .dialer
            .identify_observed_external_addr
            .len()
            < 3
        {
            info!("We have observed less than 3 addresses. Trying again or terminating.");
            result.terminate = true;
            result.retry = true;
        }

        if result.retry || result.terminate {
            return Ok(result);
        }

        let mut ports = HashSet::new();
        let mut ips = HashSet::new();
        for addresses in self
            .dial_manager
            .dialer
            .identify_observed_external_addr
            .values()
        {
            for (addr, _id) in addresses {
                let _ = ports.insert(addr.port());
                if let IpAddr::V4(ip) = addr.ip() {
                    let _ = ips.insert(ip);
                }
            }
        }

        if ports.len() != 1 {
            error!("Multiple ports observed, we are unreachable. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }

        let port = *ports
            .iter()
            .next()
            .ok_or(ReachabilityCheckError::EmptyPort)?;
        if port == 0 {
            error!("Observed port is 0. This should not happen. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }

        #[allow(clippy::comparison_chain)]
        #[allow(clippy::needless_return)]
        if ips.len() == 1 {
            let ip = ips
                .iter()
                .next()
                .ok_or(ReachabilityCheckError::EmptyIpAddrs)?;
            if ip.is_unspecified() || ip.is_documentation() || ip.is_broadcast() {
                info!("Observed address {ip:?} is unspecified. Terminating the node.");
                result.terminate = true;
                return Ok(result);
            } else if ip.is_private() {
                let addr = SocketAddr::new(IpAddr::V4(*ip), port);
                info!(
                    "Observed IP address {addr:?} is non-global. Reachability status is Reachable."
                );
                result.reachable_addresses.push(addr);
                return Ok(result);
            } else {
                let addr = SocketAddr::new(IpAddr::V4(*ip), port);
                info!("Observed IP address {addr:?} is global. Reachability status is Reachable.");
                result.reachable_addresses.push(addr);
                return Ok(result);
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
                    info!(
                        "We have multiple private IP addresses, picking localhost: {addr:?}. Reachability status is Reachable."
                    );
                    result.reachable_addresses.push(addr);
                    return Ok(result);
                }

                let addrs = private_ip
                    .iter()
                    .map(|ip| SocketAddr::new(IpAddr::V4(**ip), port))
                    .collect::<Vec<_>>();
                info!(
                    "We have multiple private IP addresses {addrs:?}. Reachability status is Reachable."
                );
                result.reachable_addresses.extend(addrs);
                return Ok(result);
            }

            if !public_ip.is_empty() {
                let addrs = public_ip
                    .iter()
                    .map(|ip| SocketAddr::new(IpAddr::V4(**ip), port))
                    .collect::<Vec<_>>();
                info!(
                    "We have multiple public IP addresses {addrs:?}. Reachability status is Reachable."
                );
                result.reachable_addresses.extend(addrs);
                return Ok(result);
            }

            error!(
                "We have multiple IP addresses, but none are private or public. Terminating the node."
            );
            result.terminate = true;
            return Ok(result);
        } else {
            error!("We have no IP addresses. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }
    }

    fn do_we_have_retries_left(&self) -> bool {
        self.dial_manager.current_workflow_attempt < MAX_WORKFLOW_ATTEMPTS
    }
}

struct ExternalAddrResult {
    retry: bool,
    terminate: bool,
    reachable_addresses: Vec<SocketAddr>,
}
