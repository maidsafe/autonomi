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
use libp2p::{Multiaddr, identify};
use libp2p::{
    Swarm,
    swarm::{NetworkBehaviour, SwarmEvent},
};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use crate::networking::driver::behaviour::upnp;
use crate::networking::error::ReachabilityCheckError;
#[cfg(feature = "open-metrics")]
use crate::networking::metrics::NetworkMetricsRecorder;
use crate::networking::network::endpoint_str;
use crate::networking::reachability_check::listener::get_all_listeners;
use crate::networking::{NetworkError, multiaddr_get_socket_addr, multiaddr_pop_p2p};

/// The maximum number of peers to dial concurrently during the reachability check.
pub(crate) const MAX_CONCURRENT_DIALS: usize = 7;
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
    pub(super) upnp: upnp::behaviour::Behaviour,
    pub(super) identify: libp2p::identify::Behaviour,
}

/// ReachabilityCheckEvent enum
#[derive(CustomDebug)]
pub(crate) enum ReachabilityCheckEvent {
    Upnp(upnp::behaviour::Event),
    Identify(Box<libp2p::identify::Event>),
}

impl From<upnp::behaviour::Event> for ReachabilityCheckEvent {
    fn from(event: upnp::behaviour::Event) -> Self {
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

        println!("Obtaining valid listen addresses for reachability check");
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
                    upnp::behaviour::Event::GatewayNotFound => {
                        info!("UPnP gateway not found. Trying to dial peers.");
                        self.upnp_supported = false;
                        upnp_result_obtained = true;
                    }
                    upnp::behaviour::Event::NewExternalAddr { addr, local_addr } => {
                        info!(
                            "UPnP: New external address: {addr:?}, local address: {local_addr:?}. Trying to dial peers to confirm reachability."
                        );
                        self.upnp_supported = true;
                        upnp_result_obtained = false;
                    }
                    upnp::behaviour::Event::NonRoutableGateway => {
                        warn!("UPnP gateway is not routable. Trying to dial peers.");
                        self.upnp_supported = false;
                        upnp_result_obtained = true;
                    }
                    upnp::behaviour::Event::ExpiredExternalAddr { addr, local_addr } => {
                        info!(
                            "UPnP: External address expired: {addr:?}, local address: {local_addr:?}"
                        );
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
                            self.insert_observed_address(info.observed_addr, connection_id);
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

    fn insert_observed_address(&mut self, address: Multiaddr, connection_id: ConnectionId) {
        let Some(socket_addr) = multiaddr_get_socket_addr(&address) else {
            warn!("Unable to get socket address from: {address:?}");
            return;
        };

        if let Some(addr) = self
            .dial_manager
            .dialer
            .identify_observed_external_addr
            .insert(connection_id, socket_addr)
        {
            warn!(
                "Overwriting existing observed external address {addr:?} for connection id {connection_id:?} with new address {socket_addr:?}. This should not happen."
            );
        }
    }

    /// Get the reachability status of the node if the retries are exhausted.
    ///
    /// If the node is not routable, it will return `ReachabilityStatus::NotRoutable`.
    /// If the node is reachable, it will return `ReachabilityStatus::Reachable`
    /// with the reachable local adapter address and whether UPnP is supported.
    ///
    /// If the node is not yet reachable, it will return `None` and the workflow will be retried.
    fn get_reachability_status(
        &mut self,
    ) -> Result<Option<ReachabilityStatus>, ReachabilityCheckError> {
        let reachability_result = self.obtain_reachable_local_adapter()?;

        if reachability_result.retry {
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
        if reachability_result.terminate {
            info!("Terminating the node as we are not routable.");
            return Ok(Some(ReachabilityStatus::NotRoutable {
                upnp: self.upnp_supported,
            }));
        }

        Ok(Some(ReachabilityStatus::Reachable {
            addr: reachability_result
                .reachable_local_adapter_addrs
                .ok_or(ReachabilityCheckError::LocalAdapterShouldNotBeEmpty)?,
            upnp: self.upnp_supported,
        }))
    }

    /// We have received the external addresses via Identify protocol and also the list of local adapter addresses
    /// from the incoming connections. We now need to determine if the node is reachable or not and return the
    /// reachable local adapter address.
    fn obtain_reachable_local_adapter(&self) -> Result<ReachabilityResult, ReachabilityCheckError> {
        let mut result = ReachabilityResult {
            retry: false,
            terminate: false,
            reachable_local_adapter_addrs: None,
        };

        debug!(
            "External addresses observed: {:?}",
            self.dial_manager.dialer.identify_observed_external_addr
        );
        debug!(
            "Incoming connection local adapter map: {:?}",
            self.dial_manager
                .dialer
                .incoming_connection_local_adapter_map
        );

        let mut external_addr_local_adapter_map: Vec<(SocketAddr, SocketAddr)> = Vec::new();
        for (connection_id, external_addr) in
            &self.dial_manager.dialer.identify_observed_external_addr
        {
            if let Some(local_adapter_addr) = self
                .dial_manager
                .dialer
                .incoming_connection_local_adapter_map
                .get(connection_id)
            {
                info!(
                    "Local adapter address for external_addr {external_addr:?} with connection id {connection_id:?} is {local_adapter_addr:?}"
                );
                external_addr_local_adapter_map.push((*external_addr, *local_adapter_addr));
            } else {
                warn!(
                    "Unable to get local adapter address for external_addr {external_addr:?} with connection id {connection_id:?}"
                );
            }
        }

        info!("External address to local adapter map: {external_addr_local_adapter_map:?}");

        if external_addr_local_adapter_map.is_empty() {
            info!("No observed addresses found. Check if we made any successful dials.");
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
        } else if external_addr_local_adapter_map.len() < get_majority(MAX_CONCURRENT_DIALS) {
            info!(
                "We have observed less than {} addresses. Trying again or terminating.",
                get_majority(MAX_CONCURRENT_DIALS)
            );
            result.terminate = true;
            result.retry = true;
        }

        if result.retry || result.terminate {
            return Ok(result);
        }

        let mut external_addrs = HashSet::new();
        let mut local_adapter_addrs = HashSet::new();
        for (external_addr, local_adapter_addr) in external_addr_local_adapter_map.iter() {
            let _ = external_addrs.insert(*external_addr);
            let _ = local_adapter_addrs.insert(*local_adapter_addr);
        }
        if external_addrs.len() != 1 {
            error!("Multiple external addresses observed. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }
        if local_adapter_addrs.len() != 1 {
            error!("Multiple local adapter addresses observed. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }

        let external_addr = *external_addrs
            .iter()
            .next()
            .expect("This should not be empty, we checked it above");
        let local_adapter_addr = *local_adapter_addrs
            .iter()
            .next()
            .expect("This should not be empty, we checked it above");

        if external_addr.ip().is_unspecified() {
            error!("External address is unspecified. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }

        if local_adapter_addr.ip().is_unspecified() {
            error!("Local adapter address is unspecified. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }

        if local_adapter_addr.port() == 0 {
            error!("Local adapter address port is 0. Terminating the node.");
            result.terminate = true;
            return Ok(result);
        }

        info!(
            "Found external address: {external_addr:?} and its local adapter: {local_adapter_addr:?}"
        );

        result.reachable_local_adapter_addrs = Some(local_adapter_addr);
        Ok(result)
    }

    fn do_we_have_retries_left(&self) -> bool {
        self.dial_manager.current_workflow_attempt < MAX_WORKFLOW_ATTEMPTS
    }
}

fn get_majority(value: usize) -> usize {
    if value == 0 { 0 } else { (value / 2) + 1 }
}

struct ReachabilityResult {
    retry: bool,
    terminate: bool,
    reachable_local_adapter_addrs: Option<SocketAddr>,
}
