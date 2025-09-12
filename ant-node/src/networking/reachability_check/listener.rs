// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::ReachabilityIssue;
#[cfg(feature = "open-metrics")]
use crate::networking::MetricsRegistries;
use crate::networking::NetworkError;
use crate::networking::driver::behaviour::upnp;
use crate::networking::multiaddr_get_socket_addr;
use crate::networking::network::listen_on_with_retry;
use crate::networking::reachability_check::ReachabilityCheckBehaviour;
#[cfg(feature = "open-metrics")]
use crate::networking::transport;
use futures::StreamExt;
use libp2p::Transport;
use libp2p::core::transport::ListenerId;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{Multiaddr, PeerId};
use libp2p::{Swarm, swarm::SwarmEvent};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tracing::{debug, error, info};

/// Timeout for collecting listen addresses. The timer resets each time a new address is discovered.
const COLLECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Manages the state of listeners for reachability checks.
pub(crate) struct ListenerManager {
    pub(crate) available_listeners: Vec<SocketAddr>,
    pub(crate) upnp_supported: HashSet<SocketAddr>,
    pub(crate) current_listener_index: usize,
    pub(crate) current_listener_id: Option<ListenerId>,
    pub(crate) listener_failures: HashMap<SocketAddr, ReachabilityIssue>,
}

impl ListenerManager {
    /// Initialize the ListenerManager by discovering available listen addresses.
    pub(crate) async fn new(
        keypair: &Keypair,
        local: bool,
        listen_addr: SocketAddr,
        no_upnp: bool,
    ) -> Result<Self, NetworkError> {
        let peer_id = PeerId::from(keypair.public());

        #[cfg(feature = "open-metrics")]
        let mut metrics_registries = MetricsRegistries::default();
        #[cfg(feature = "open-metrics")]
        let transport = transport::build_transport(keypair, &mut metrics_registries);
        #[cfg(not(feature = "open-metrics"))]
        let transport = transport::build_transport(keypair);
        let transport = if !local {
            // Wrap upper in a transport that prevents dialing local addresses.
            libp2p::core::transport::global_only::Transport::new(transport).boxed()
        } else {
            transport
        };

        let behaviour: Toggle<upnp::behaviour::Behaviour> = if !no_upnp {
            Some(upnp::behaviour::Behaviour::default()).into()
        } else {
            None.into()
        };
        let swarm_config = libp2p::swarm::Config::with_tokio_executor();
        let mut swarm = Swarm::new(transport, behaviour, peer_id, swarm_config);
        let mut listener_ids = HashSet::new();

        // Listen on QUIC
        let addr_quic = Multiaddr::from(listen_addr.ip())
            .with(Protocol::Udp(listen_addr.port()))
            .with(Protocol::QuicV1);
        let listener_id = listen_on_with_retry(&mut swarm, addr_quic.clone())?;
        let _ = listener_ids.insert(listener_id);

        info!("Starting listener swarm to collect all listen addresses.");

        let mut addresses = HashSet::new();
        let mut upnp_supported = HashSet::new();
        let mut last_address_time = Instant::now();
        let mut upnp_result_found = false; // Either GatewayNotFound or NonRoutableGateway or NewExternalAddr (which we return anyway)

        while last_address_time.elapsed() < COLLECTION_TIMEOUT || !upnp_result_found {
            match tokio::time::timeout(COLLECTION_TIMEOUT, swarm.select_next_some()).await {
                Ok(swarm_event) => {
                    match swarm_event {
                        SwarmEvent::NewListenAddr {
                            address,
                            listener_id,
                        } => {
                            info!("New listen address: {address:?} on listener {listener_id:?}");
                            if let Some(socket_addr) = multiaddr_get_socket_addr(&address) {
                                if socket_addr.ip().is_unspecified() {
                                    error!(
                                        "Unspecified IP address found for listener {listener_id:?}"
                                    );
                                    continue;
                                }
                                let _ = addresses.insert(socket_addr);
                            } else {
                                error!("Failed to parse socket address from {address:?}");
                            }
                            last_address_time = Instant::now();
                        }
                        SwarmEvent::ListenerError { listener_id, error } => {
                            error!("Listener error on {listener_id:?}: {error}");
                        }
                        SwarmEvent::Behaviour(upnp::behaviour::Event::NewExternalAddr {
                            addr,
                            local_addr,
                        }) => {
                            info!(
                                "UPnP mapped external address: {addr} for local address {local_addr}."
                            );
                            if let Some(socket_addr) = multiaddr_get_socket_addr(&local_addr) {
                                let _ = addresses.insert(socket_addr);
                                let _ = upnp_supported.insert(socket_addr);
                            } else {
                                error!("Failed to parse socket address from UPnP address {addr:?}");
                            }

                            // just in case we failed to parse socket addr
                            upnp_result_found = true;
                            last_address_time = Instant::now();
                        }
                        SwarmEvent::Behaviour(upnp::behaviour::Event::ExpiredExternalAddr {
                            addr,
                            local_addr,
                        }) => {
                            info!(
                                "UPnP external address expired: {addr} for local address {local_addr}"
                            );
                            upnp_result_found = true;
                        }
                        SwarmEvent::Behaviour(upnp::behaviour::Event::GatewayNotFound) => {
                            error!("No UPnP gateway found.");
                            upnp_result_found = true;
                        }
                        SwarmEvent::Behaviour(upnp::behaviour::Event::NonRoutableGateway) => {
                            error!("UPnP gateway is not routable.");
                            upnp_result_found = true;
                        }
                        _ => {
                            // Other events are not relevant for collecting listen addresses
                            debug!("Ignoring swarm event: {swarm_event:?}");
                        }
                    }
                }
                Err(_) => {
                    info!(
                        "Collection timeout reached after {} seconds since last address, collected {} addresses",
                        COLLECTION_TIMEOUT.as_secs(),
                        addresses.len()
                    );
                    break;
                }
            }
        }

        for listener_id in listener_ids {
            if swarm.remove_listener(listener_id) {
                info!("Removed listener {listener_id:?}");
            } else {
                info!("Failed to remove listener {listener_id:?}");
            }
        }

        info!(
            "Collected {} listen addresses: {:?}",
            addresses.len(),
            addresses
        );

        if addresses.is_empty() {
            error!("No listen addresses found. Cannot start reachability check.");
            println!("No valid listeners found. Exiting.");
            return Err(NetworkError::NoListenAddressesFound);
        }

        info!(
            "Found {} valid listeners for reachability check: {:?}",
            addresses.len(),
            addresses
        );

        // move listeners with upnp support to the front
        let mut addresses: Vec<SocketAddr> = addresses.into_iter().collect();

        // Log before and after state if UPnP addresses are found
        if !upnp_supported.is_empty() {
            info!("Before sorting - addresses: {addresses:?}");
            info!("UPnP supported addresses: {upnp_supported:?}");
        }

        addresses.sort_by_key(|addr| {
            if upnp_supported.contains(addr) {
                1 // UPnP addresses first (priority 1)
            } else {
                2 // Non-UPnP addresses second (priority 2)
            }
        });

        if !upnp_supported.is_empty() {
            info!("After sorting - UPnP addresses moved to front: {addresses:?}");
        }

        Ok(Self {
            available_listeners: addresses.into_iter().collect(),
            upnp_supported,
            current_listener_index: 0,
            current_listener_id: None,
            listener_failures: HashMap::new(),
        })
    }

    /// Bind to the current index. Returns Ok if successful, Err if no more listeners.
    pub(crate) fn bind_listener(
        &mut self,
        swarm: &mut Swarm<ReachabilityCheckBehaviour>,
    ) -> Result<(), NetworkError> {
        // Remove current listener if one exists
        if let Some(listener_id) = self.current_listener_id.take() {
            if !swarm.remove_listener(listener_id) {
                error!("CRITICAL: Failed to remove listener {listener_id:?}");
                return Err(NetworkError::ListenerCleanupFailed);
            }
            info!("Successfully removed previous listener {listener_id:?}");
        }

        // Check if we have more listeners to try
        if self.current_listener_index >= self.available_listeners.len() {
            error!("No more listeners available to try");
            return Err(NetworkError::NoListenAddressesFound);
        }

        let listener_addr = self.available_listeners[self.current_listener_index];
        let addr_quic = Multiaddr::from(listener_addr.ip())
            .with(Protocol::Udp(listener_addr.port()))
            .with(Protocol::QuicV1);

        info!(
            "Attempting to bind to listener {} of {}: {addr_quic:?}",
            self.current_listener_index + 1,
            self.available_listeners.len()
        );
        println!(
            "\nTrying listener {} of {}: {addr_quic:?}",
            self.current_listener_index + 1,
            self.available_listeners.len()
        );

        let listener_id = listen_on_with_retry(swarm, addr_quic.clone())?;
        self.current_listener_id = Some(listener_id);

        info!("Successfully bound to {addr_quic:?} with {listener_id:?}");
        println!("Successfully bound to {addr_quic:?}");

        Ok(())
    }

    pub(crate) fn record_failure(&mut self, issue: ReachabilityIssue) {
        let listener_addr = self.available_listeners[self.current_listener_index];
        let _ = self.listener_failures.insert(listener_addr, issue);
    }

    pub(crate) fn increment_listener_index(&mut self) {
        self.current_listener_index += 1;
    }

    pub(crate) fn current_listener_index(&self) -> usize {
        self.current_listener_index
    }

    pub(crate) fn current_listener_addr(&self) -> Option<SocketAddr> {
        self.available_listeners
            .get(self.current_listener_index)
            .cloned()
    }

    pub(crate) fn total_listeners(&self) -> usize {
        self.available_listeners.len()
    }

    pub(crate) fn listener_failures(&self) -> &HashMap<SocketAddr, ReachabilityIssue> {
        &self.listener_failures
    }

    pub(crate) fn has_more_listeners(&self) -> bool {
        self.current_listener_index + 1 < self.available_listeners.len()
    }

    pub(crate) fn upnp_supported(&self, addr: &SocketAddr) -> bool {
        self.upnp_supported.contains(addr)
    }
}
