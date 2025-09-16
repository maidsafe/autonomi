// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod dialer;
mod listener;
mod progress;

use custom_debug::Debug as CustomDebug;
use dialer::DialManager;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{ConnectionId, DialError};
use libp2p::{Multiaddr, identify};
use libp2p::{
    Swarm,
    swarm::{NetworkBehaviour, SwarmEvent},
};
use progress::ProgressCalculator;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::time::Instant;

#[cfg(feature = "open-metrics")]
use crate::networking::metrics::NetworkMetricsRecorder;
use crate::networking::network::endpoint_str;
use crate::networking::reachability_check::listener::ListenerManager;
use crate::networking::{NetworkError, multiaddr_get_socket_addr, multiaddr_pop_p2p};

/// The maximum number of peers to dial concurrently during the reachability check.
pub(crate) const MAX_CONCURRENT_DIALS: usize = 7;
const MAX_WORKFLOW_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
/// The reachability status of the node.
pub enum ReachabilityStatus {
    /// We are reachable and have an external address.
    Reachable {
        /// The local adapter address we are reachable at.
        local_addr: SocketAddr,
        /// The external address we are reachable at.
        external_addr: SocketAddr,
        /// Whether UPnP is supported or not.
        upnp: bool,
    },
    /// We are not externally reachable.
    NotReachable {
        /// The reasons for not being reachable, mapped by listener address.
        ///
        /// Key: (SocketAddr, bool) - The SocketAddr is the listener address, and the bool indicates if UPnP is supported for that address.
        reasons: HashMap<(SocketAddr, bool), ReachabilityIssue>,
    },
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum ReachabilityIssue {
    #[error("We were not able to make any outbound connections. Attempt cannot be retried.")]
    NoOutboundConnection,
    #[error("We were not able to get any dial backs from other peers. Retrying if possible.")]
    NoDialBacks,
    #[error(
        "We did not get enough dial backs. We need at least {required} but found {found}. Retrying if possible."
    )]
    NotEnoughDialBacks { required: usize, found: usize },
    #[error(
        "We found multiple valid external addresses. Make sure that you call run the node again with a single address via the '--ip <ip_address>' flag."
    )]
    MultipleExternalAddresses,
    #[error(
        "We found multiple valid local adapter addresses. Make sure that you call run the node again with a single address via the '--ip <ip_address>' flag."
    )]
    MultipleLocalAdapterAddresses,
    #[error("We found an unspecified external address. Attempt cannot be retried.")]
    UnspecifiedExternalAddress,
    #[error("We found an unspecified local adapter address. Attempt cannot be retried.")]
    UnspecifiedLocalAdapterAddress,
    #[error("We found a local adapter port that is zero. Attempt cannot be retried.")]
    LocalAdapterPortZero,
}

impl ReachabilityIssue {
    fn retryable(&self) -> bool {
        matches!(
            self,
            ReachabilityIssue::NoDialBacks
                | ReachabilityIssue::NotEnoughDialBacks { .. }
                | ReachabilityIssue::MultipleLocalAdapterAddresses
        )
    }
}

/// The behaviors are polled in the order they are defined.
/// The first struct member is polled until it returns Poll::Pending before moving on to later members.
/// Prioritize the behaviors related to connection handling.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "ReachabilityCheckEvent")]
pub(crate) struct ReachabilityCheckBehaviour {
    pub(super) identify: libp2p::identify::Behaviour,
}

/// ReachabilityCheckEvent enum
#[derive(CustomDebug)]
pub(crate) enum ReachabilityCheckEvent {
    Identify(Box<libp2p::identify::Event>),
}

impl From<libp2p::identify::Event> for ReachabilityCheckEvent {
    fn from(event: libp2p::identify::Event) -> Self {
        ReachabilityCheckEvent::Identify(Box::new(event))
    }
}

pub(crate) struct ReachabilityCheckSwarmDriver {
    pub(crate) swarm: Swarm<ReachabilityCheckBehaviour>,
    pub(crate) dial_manager: DialManager,
    pub(crate) listener_manager: ListenerManager,
    pub(crate) progress_calculator: ProgressCalculator,
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
        no_upnp: bool,
        #[cfg(feature = "open-metrics")] metrics_recorder: Option<NetworkMetricsRecorder>,
    ) -> Result<Self, NetworkError> {
        let swarm = swarm;

        println!("Obtaining valid listeners for the reachability check workflow..");

        Ok(Self {
            swarm,
            dial_manager: DialManager::new(initial_contacts),
            progress_calculator: ProgressCalculator::new(),
            listener_manager: ListenerManager::new(keypair, local, listen_addr, no_upnp).await?,
            #[cfg(feature = "open-metrics")]
            metrics_recorder,
        })
    }

    /// Move to the next listener and reset the workflow state
    fn try_next_listener(&mut self) -> Result<(), NetworkError> {
        self.listener_manager.increment_listener_index();

        // Bind to the new listener index
        self.listener_manager.bind_listener(&mut self.swarm)?;

        self.dial_manager.reset_workflow_for_new_listener();

        Ok(())
    }

    /// Runs the reachability check workflow.
    pub(crate) async fn detect(mut self) -> Result<ReachabilityStatus, NetworkError> {
        println!("Starting reachability check workflow. This could take 3 to 10 minutes..");

        // Bind to the first listener
        self.listener_manager.bind_listener(&mut self.swarm)?;

        info!(
            "Starting reachability check workflow. Listener {} of {}, Attempt {} of {MAX_WORKFLOW_ATTEMPTS}",
            self.listener_manager.current_listener_index() + 1,
            self.listener_manager.total_listeners(),
            self.dial_manager.current_workflow_attempt
        );
        println!(
            "\nReachability Workflow Summary - Listener {} of {}, Attempt {} of {MAX_WORKFLOW_ATTEMPTS}",
            self.listener_manager.current_listener_index() + 1,
            self.listener_manager.total_listeners(),
            self.dial_manager.current_workflow_attempt
        );
        let mut dial_check_interval = tokio::time::interval(std::time::Duration::from_secs(5));
        let _ = dial_check_interval.tick().await; // first tick is immediate
        loop {
            tokio::select! {
                // next take and react to external swarm events
                swarm_event = self.swarm.select_next_some() => {
                    // logging for handling events happens inside handle_swarm_events
                    // otherwise we're rewriting match statements etc around this anwyay
                    if let Some(status) = self.handle_swarm_events(swarm_event) {
                        info!("Reachability status has been found to be: {status:?}");
                        return Ok(status);
                    }

                }
                _ = dial_check_interval.tick() => {
                    if let Some(status) = self.handle_dial_check_interval() {
                        return Ok(status)
                    }

                    if let Some(recorder) = &self.metrics_recorder {
                        let _ = recorder
                            .reachability_check_progress
                            .set(self.workflow_progress());
                    }
                }
            }
        }
    }

    fn handle_swarm_events(
        &mut self,
        event: SwarmEvent<ReachabilityCheckEvent>,
    ) -> Option<ReachabilityStatus> {
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

                    self.trigger_dial();
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
                            info!(
                                "Dialing completed with listener {}. Checking the reachability status now.",
                                self.listener_manager
                                    .current_listener_addr()
                                    .map(|addr| addr.to_string())
                                    .unwrap_or_else(|| "unknown".to_string())
                            );
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

        None
    }

    fn handle_dial_check_interval(&mut self) -> Option<ReachabilityStatus> {
        // check if we have any ongoing dial attempts
        self.dial_manager.cleanup_dial_attempts();
        self.trigger_dial();

        if self.dial_manager.has_dialing_completed() {
            info!("Dialing completed. Checking the reachability status now.");
            match self.get_reachability_status() {
                Some(status) => {
                    info!("Reachability status has been found to be: {status:?}");
                    if let Some(recorder) = &self.metrics_recorder {
                        let _ = recorder.reachability_check_progress.set(100.0);
                    }
                    Some(status)
                }
                None => {
                    info!("Reachability status is not yet determined.");
                    None
                }
            }
        } else {
            None
        }
    }

    fn trigger_dial(&mut self) {
        while self.dial_manager.can_dial_new_peer() {
            let Some(mut addr) = self.dial_manager.get_next_contact() else {
                error!(
                    "Dialer has no more contacts to dial. The get_reachability_status method will now calculate the reachability status."
                );
                return;
            };

            let addr_clone = addr.clone();
            let Some(peer_id) = multiaddr_pop_p2p(&mut addr) else {
                warn!("PeerId for {addr:?} not found, fetching the next contact");
                continue;
            };

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
    /// If the node is not reachable, it will return `ReachabilityStatus::NotReachable`.
    /// If the node is reachable, it will return `ReachabilityStatus::Reachable`
    /// with the reachable local adapter address and whether UPnP is supported.
    ///
    /// If the node is not yet reachable, it will return `None` and the workflow will be retried.
    fn get_reachability_status(&mut self) -> Option<ReachabilityStatus> {
        let reachable_addrs = self.obtain_reachable_addrs();

        match reachable_addrs {
            Ok((external_addr, local_adapter)) => {
                let upnp_supported = self.listener_manager.upnp_supported(&local_adapter);
                info!("Node is reachable via local adapter: {local_adapter}");
                println!("Reachability status: Reachable with UPnP status: {upnp_supported}\n");
                Some(ReachabilityStatus::Reachable {
                    local_addr: local_adapter,
                    external_addr,
                    upnp: upnp_supported,
                })
            }
            Err(reason) => {
                println!("{reason}");
                if reason.retryable() && self.has_retries_remaining() {
                    // Try another workflow attempt with the same listener
                    info!(
                        "Retrying reachability check workflow (failed due to: {reason:?}). Listener {} of {}, Attempt {} of {MAX_WORKFLOW_ATTEMPTS}",
                        self.listener_manager.current_listener_index() + 1,
                        self.listener_manager.total_listeners(),
                        self.dial_manager.current_workflow_attempt + 1
                    );
                    println!(
                        "\nReachability Workflow Summary - Listener {} of {}, Attempt {} of {MAX_WORKFLOW_ATTEMPTS}",
                        self.listener_manager.current_listener_index() + 1,
                        self.listener_manager.total_listeners(),
                        self.dial_manager.current_workflow_attempt + 1
                    );
                    self.dial_manager.increment_workflow();
                    self.trigger_dial();
                    return None;
                }

                // All workflow attempts exhausted for this listener, record the failure and try next listener
                self.listener_manager.record_failure(reason.clone());

                if self.listener_manager.has_more_listeners() {
                    info!(
                        "Max workflow attempts reached for listener {}. Trying next listener.",
                        self.listener_manager.current_listener_index() + 1
                    );
                    println!(
                        "Listener {} failed after MAX attempts. Trying next listener...",
                        self.listener_manager.current_listener_index() + 1,
                    );

                    // Try to bind to the next listener
                    match self.try_next_listener() {
                        Ok(()) => {
                            println!(
                                "\nReachability Workflow Summary - Listener {} of {}, Attempt {} of {MAX_WORKFLOW_ATTEMPTS}",
                                self.listener_manager.current_listener_index() + 1,
                                self.listener_manager.total_listeners(),
                                self.dial_manager.current_workflow_attempt
                            );
                            self.trigger_dial();
                            return None;
                        }
                        Err(err) => {
                            error!("Failed to bind to next listener: {err:?}");
                            return Some(ReachabilityStatus::NotReachable {
                                reasons: self.failure_reasons(),
                            });
                        }
                    }
                } else {
                    error!(
                        "All listeners exhausted. Max reachability check workflow attempts reached. Cannot determine reachability status."
                    );
                }
                println!("We are not reachable. Terminating the reachability check workflow.\n");
                let reasons = self.failure_reasons();
                Some(ReachabilityStatus::NotReachable { reasons })
            }
        }
    }

    /// We have received the external addresses via Identify protocol and also the list of local adapter addresses
    /// from the incoming connections. We now need to determine if the node is reachable or not and return the
    /// reachable local adapter address.
    ///
    /// Returns (external, local_adapter) addrs
    /// Undesirable states are returned as an error.
    fn obtain_reachable_addrs(&self) -> Result<(SocketAddr, SocketAddr), ReachabilityIssue> {
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
                    "External address {external_addr:?} obtained via Local adapter {local_adapter_addr:?} on {connection_id:?}"
                );
                println!(
                    "External address {external_addr:?} obtained via Local adapter {local_adapter_addr:?} on {connection_id:?}"
                );
                external_addr_local_adapter_map.push((*external_addr, *local_adapter_addr));
            } else {
                warn!(
                    "Unable to get Local adapter address for External address {external_addr:?} on {connection_id:?}"
                );
                println!(
                    "Unable to get Local adapter address for External address {external_addr:?} on {connection_id:?}"
                );
            }
        }

        info!("External address to local adapter map: {external_addr_local_adapter_map:?}");

        if external_addr_local_adapter_map.is_empty() {
            info!("No observed addresses found. Check if we made any successful dials.");
            if self.dial_manager.is_faulty() {
                error!(
                    "We have not made any outbound connections. Terminating the workflow for the listener."
                );
                return Err(ReachabilityIssue::NoOutboundConnection);
            } else {
                info!(
                    "We made outbound connections, but did not receive any inbounds. Retrying the workflow with the listener if possible."
                );
                return Err(ReachabilityIssue::NoDialBacks);
            }
        } else if external_addr_local_adapter_map.len() < get_majority(MAX_CONCURRENT_DIALS) {
            info!(
                "We have observed less than {} addresses. Retrying the workflow with the listener if possible.",
                get_majority(MAX_CONCURRENT_DIALS)
            );
            return Err(ReachabilityIssue::NotEnoughDialBacks {
                required: get_majority(MAX_CONCURRENT_DIALS),
                found: external_addr_local_adapter_map.len(),
            });
        }

        let mut external_addrs = HashSet::new();
        let mut local_adapter_addrs = HashSet::new();
        for (external_addr, local_adapter_addr) in external_addr_local_adapter_map.iter() {
            let _ = external_addrs.insert(*external_addr);
            let _ = local_adapter_addrs.insert(*local_adapter_addr);
        }
        if external_addrs.len() != 1 {
            error!("Multiple external addresses observed. Terminating the node.");
            return Err(ReachabilityIssue::MultipleExternalAddresses);
        }
        if local_adapter_addrs.len() != 1 {
            error!("Multiple local adapter addresses observed. Terminating the node.");
            return Err(ReachabilityIssue::MultipleLocalAdapterAddresses);
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
            return Err(ReachabilityIssue::UnspecifiedExternalAddress);
        }

        if local_adapter_addr.ip().is_unspecified() {
            error!("Local adapter address is unspecified. Terminating the node.");
            return Err(ReachabilityIssue::UnspecifiedLocalAdapterAddress);
        }

        if local_adapter_addr.port() == 0 {
            error!("Local adapter address port is 0. Terminating the node.");
            return Err(ReachabilityIssue::LocalAdapterPortZero);
        }

        info!(
            "Found external address: {external_addr:?} and its local adapter: {local_adapter_addr:?}"
        );

        Ok((external_addr, local_adapter_addr))
    }

    fn has_retries_remaining(&self) -> bool {
        self.dial_manager.current_workflow_attempt < MAX_WORKFLOW_ATTEMPTS
    }

    fn workflow_progress(&self) -> f64 {
        let total_listeners = self.listener_manager.total_listeners() as f64;
        let current_listener = self.listener_manager.current_listener_index() as f64;
        let listener_progress = current_listener / total_listeners;

        let workflow_progress = self
            .progress_calculator
            .calculate_progress(&self.dial_manager)
            / 100.0;
        let within_listener_progress = workflow_progress / total_listeners;

        ((listener_progress + within_listener_progress) * 100.0).min(100.0)
    }

    fn failure_reasons(&self) -> HashMap<(SocketAddr, bool), ReachabilityIssue> {
        self.listener_manager
            .listener_failures()
            .iter()
            .map(|(addr, reason)| {
                let is_upnp = self.listener_manager.upnp_supported(addr);
                ((*addr, is_upnp), reason.clone())
            })
            .collect()
    }
}

pub(crate) fn get_majority(value: usize) -> usize {
    if value == 0 { 0 } else { (value / 2) + 1 }
}
