// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(feature = "open-metrics")]
use crate::networking::MetricsRegistries;
use crate::networking::NetworkError;
use crate::networking::driver::behaviour::upnp;
use crate::networking::multiaddr_get_socket_addr;
use crate::networking::network::listen_on_with_retry;
#[cfg(feature = "open-metrics")]
use crate::networking::transport;
use futures::StreamExt;
use libp2p::Transport;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{Multiaddr, PeerId};
use libp2p::{Swarm, swarm::SwarmEvent};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tracing::{debug, error, info};

/// Run a dummy swarm to collect all listen addresses.
/// This is used to determine the addresses the node is listening on, which is useful for
/// reachability checks and other network operations.
pub(crate) async fn get_all_listeners(
    keypair: &Keypair,
    local: bool,
    listen_addr: SocketAddr,
    no_upnp: bool,
) -> Result<HashSet<SocketAddr>, NetworkError> {
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

    // Collect addresses with timeout
    let mut addresses = HashSet::new();
    let collection_timeout = Duration::from_secs(10);
    let start_time = Instant::now();
    let mut last_address_time = start_time;

    while start_time.elapsed() < collection_timeout {
        // Wait for events with a timeout based on time since last address
        let remaining_timeout = collection_timeout.saturating_sub(last_address_time.elapsed());

        match tokio::time::timeout(remaining_timeout, swarm.select_next_some()).await {
            Ok(swarm_event) => {
                match swarm_event {
                    SwarmEvent::NewListenAddr {
                        address,
                        listener_id,
                    } => {
                        info!("New listen address: {address:?} on listener {listener_id:?}");
                        if let Some(socket_addr) = multiaddr_get_socket_addr(&address) {
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
                            "UPnP mapped external address: {addr} for local address {local_addr}. Returning the local address as the only listen address."
                        );
                        if let Some(socket_addr) = multiaddr_get_socket_addr(&local_addr) {
                            return Ok(HashSet::from([socket_addr]));
                        } else {
                            error!("Failed to parse socket address from UPnP address {addr:?}");
                        }

                        last_address_time = Instant::now();
                    }
                    SwarmEvent::Behaviour(upnp::behaviour::Event::ExpiredExternalAddr {
                        addr,
                        local_addr,
                    }) => {
                        info!(
                            "UPnP external address expired: {addr} for local address {local_addr}"
                        );
                    }
                    SwarmEvent::Behaviour(upnp::behaviour::Event::GatewayNotFound) => {
                        error!("No UPnP gateway found")
                    }
                    SwarmEvent::Behaviour(upnp::behaviour::Event::NonRoutableGateway) => {
                        error!("UPnP gateway is not routable");
                    }
                    _ => {
                        // Other events are not relevant for collecting listen addresses
                        debug!("Ignoring swarm event: {swarm_event:?}");
                    }
                }
            }
            Err(_) => {
                info!(
                    "Collection timeout reached after {} seconds, collected {} addresses",
                    collection_timeout.as_secs(),
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

    Ok(addresses)
}
