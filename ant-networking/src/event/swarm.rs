// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    event::NodeEvent, multiaddr_get_ip, time::Instant, NetworkEvent, NodeIssue, Result, SwarmDriver,
};
use ant_bootstrap::{multiaddr_get_peer_id, BootstrapCacheStore};
use itertools::Itertools;
#[cfg(feature = "open-metrics")]
use libp2p::metrics::Recorder;
use libp2p::{
    multiaddr::Protocol,
    swarm::{ConnectionId, DialError, SwarmEvent},
    Multiaddr, TransportError,
};
use tokio::time::Duration;

impl SwarmDriver {
    /// Handle `SwarmEvents`
    pub(crate) fn handle_swarm_events(&mut self, event: SwarmEvent<NodeEvent>) -> Result<()> {
        // This does not record all the events. `SwarmEvent::Behaviour(_)` are skipped. Hence `.record()` has to be
        // called individually on each behaviour.
        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = &self.metrics_recorder {
            metrics_recorder.record(&event);
        }
        let start = Instant::now();
        let event_string;
        match event {
            SwarmEvent::Behaviour(NodeEvent::MsgReceived(event)) => {
                event_string = "msg_received";
                if let Err(e) = self.handle_req_resp_events(event) {
                    warn!("MsgReceivedError: {e:?}");
                }
            }
            SwarmEvent::Behaviour(NodeEvent::Kademlia(kad_event)) => {
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&kad_event);
                }
                event_string = "kad_event";
                self.handle_kad_event(kad_event)?;
            }
            SwarmEvent::Behaviour(NodeEvent::RelayClient(event)) => {
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&(*event));
                }
                event_string = "relay_client_event";

                info!(?event, "relay client event");

                if let libp2p::relay::client::Event::ReservationReqAccepted {
                    relay_peer_id,
                    renewal,
                    ..
                } = *event
                {
                    if !renewal {
                        if let Some(relay_manager) = self.relay_manager.as_mut() {
                            relay_manager.on_successful_reservation_by_client(
                                &relay_peer_id,
                                &mut self.swarm,
                                &self.live_connected_peers,
                            );
                        }
                    } else {
                        info!("Relay reservation was renewed with {relay_peer_id:?}");
                    }
                }
            }
            SwarmEvent::Behaviour(NodeEvent::Upnp(upnp_event)) => {
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&upnp_event);
                }
                event_string = "upnp_event";
                info!(?upnp_event, "UPnP event");
                match upnp_event {
                    libp2p::upnp::Event::GatewayNotFound => {
                        warn!("UPnP is not enabled/supported on the gateway. Please rerun without the `--upnp` flag");
                        self.send_event(NetworkEvent::TerminateNode {
                            reason: crate::event::TerminateNodeReason::UpnpGatewayNotFound,
                        });
                    }
                    libp2p::upnp::Event::NewExternalAddr(addr) => {
                        info!("UPnP: New external address: {addr:?}");
                        self.initial_bootstrap_trigger.upnp_gateway_result_obtained = true;
                    }
                    libp2p::upnp::Event::NonRoutableGateway => {
                        warn!("UPnP gateway is not routable");
                        self.initial_bootstrap_trigger.upnp_gateway_result_obtained = true;
                    }
                    _ => {
                        debug!("UPnP event: {upnp_event:?}");
                    }
                }
            }

            SwarmEvent::Behaviour(NodeEvent::RelayServer(event)) => {
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&(*event));
                }

                event_string = "relay_server_event";

                info!(?event, "relay server event");

                match *event {
                    libp2p::relay::Event::ReservationReqAccepted {
                        src_peer_id,
                        renewed: _,
                    } => {
                        self.connected_relay_clients.insert(src_peer_id);
                        info!("Relay reservation accepted from {src_peer_id:?}. Relay client count: {}", self.connected_relay_clients.len());

                        #[cfg(feature = "open-metrics")]
                        if let Some(metrics_recorder) = &self.metrics_recorder {
                            metrics_recorder
                                .connected_relay_clients
                                .set(self.connected_relay_clients.len() as i64);
                        }
                    }
                    libp2p::relay::Event::ReservationTimedOut { src_peer_id } => {
                        self.connected_relay_clients.remove(&src_peer_id);
                        info!("Relay reservation timed out from {src_peer_id:?}. Relay client count: {}", self.connected_relay_clients.len());

                        #[cfg(feature = "open-metrics")]
                        if let Some(metrics_recorder) = &self.metrics_recorder {
                            metrics_recorder
                                .connected_relay_clients
                                .set(self.connected_relay_clients.len() as i64);
                        }
                    }
                    _ => {}
                }
            }
            SwarmEvent::Behaviour(NodeEvent::Identify(event)) => {
                // Record the Identify event for metrics if the feature is enabled.
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&(*event));
                }
                event_string = "identify";
                self.handle_identify_event(*event);
            }
            SwarmEvent::NewListenAddr {
                mut address,
                listener_id,
            } => {
                event_string = "new listen addr";

                info!("Local node is listening {listener_id:?} on {address:?}");

                let local_peer_id = *self.swarm.local_peer_id();
                // Make sure the address ends with `/p2p/<local peer ID>`. In case of relay, `/p2p` is already there.
                if address.iter().last() != Some(Protocol::P2p(local_peer_id)) {
                    address.push(Protocol::P2p(local_peer_id));
                }

                // Trigger server mode if we're not a client and we should not add our own address if we're behind
                // home network.
                if !self.is_client && !self.is_behind_home_network {
                    // Others won't read our advertised external address, but use the addr from connection info.
                    // this is needed for Kad Mode::Server
                    self.swarm.add_external_address(address.clone());

                    // If we are local, add our own address(es) to cache
                    if let Some(bootstrap_cache) = self.bootstrap_cache.as_mut() {
                        tracing::info!("Adding listen address to bootstrap cache");

                        let config = bootstrap_cache.config().clone();
                        let mut old_cache = bootstrap_cache.clone();

                        if let Ok(new) = BootstrapCacheStore::new(config) {
                            self.bootstrap_cache = Some(new);
                            old_cache.add_addr(address.clone());

                            // Save cache to disk.
                            crate::time::spawn(async move {
                                if let Err(err) = old_cache.sync_and_flush_to_disk(true) {
                                    error!("Failed to save bootstrap cache: {err}");
                                }
                            });
                        }
                    }
                }

                if tracing::level_enabled!(tracing::Level::DEBUG) {
                    let all_external_addresses = self.swarm.external_addresses().collect_vec();
                    let all_listeners = self.swarm.listeners().collect_vec();
                    debug!("All our listeners: {all_listeners:?}");
                    debug!("All our external addresses: {all_external_addresses:?}");
                }

                self.initial_bootstrap_trigger.listen_addr_obtained = true;

                self.send_event(NetworkEvent::NewListenAddr(address.clone()));
            }
            SwarmEvent::ListenerClosed {
                listener_id,
                addresses,
                reason,
            } => {
                event_string = "listener closed";
                info!("Listener {listener_id:?} with add {addresses:?} has been closed for {reason:?}");
                if let Some(relay_manager) = self.relay_manager.as_mut() {
                    relay_manager.on_listener_closed(&listener_id, &mut self.swarm);
                }
            }
            SwarmEvent::IncomingConnection {
                connection_id,
                local_addr,
                send_back_addr,
            } => {
                event_string = "incoming";
                debug!("IncomingConnection ({connection_id:?}) with local_addr: {local_addr:?} send_back_addr: {send_back_addr:?}");
                #[cfg(feature = "open-metrics")]
                if let Some(relay_manager) = self.relay_manager.as_mut() {
                    relay_manager.on_incoming_connection(
                        &connection_id,
                        &local_addr,
                        &send_back_addr,
                    );
                }
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint,
                num_established,
                connection_id,
                concurrent_dial_errors,
                established_in,
            } => {
                event_string = "ConnectionEstablished";
                debug!(%peer_id, num_established, ?concurrent_dial_errors, "ConnectionEstablished ({connection_id:?}) in {established_in:?}: {}", endpoint_str(&endpoint));

                self.initial_bootstrap.on_connection_established(
                    &endpoint,
                    &mut self.swarm,
                    self.peers_in_rt,
                );

                #[cfg(feature = "open-metrics")]
                if let Some(relay_manager) = self.relay_manager.as_mut() {
                    relay_manager.on_connection_established(&peer_id, &connection_id);
                }

                let _ = self.live_connected_peers.insert(
                    connection_id,
                    (
                        peer_id,
                        endpoint.get_remote_address().clone(),
                        Instant::now() + Duration::from_secs(60),
                    ),
                );

                if let Some(bootstrap_cache) = self.bootstrap_cache.as_mut() {
                    bootstrap_cache.update_addr_status(endpoint.get_remote_address(), true);
                }

                self.insert_latest_established_connection_ids(
                    connection_id,
                    endpoint.get_remote_address(),
                );
                self.record_connection_metrics();

                if endpoint.is_dialer() {
                    self.dialed_peers.push(peer_id);
                }
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                endpoint,
                cause,
                num_established,
                connection_id,
            } => {
                event_string = "ConnectionClosed";
                debug!(%peer_id, ?connection_id, ?cause, num_established, "ConnectionClosed: {}", endpoint_str(&endpoint));
                let _ = self.live_connected_peers.remove(&connection_id);

                if num_established == 0 && self.connected_relay_clients.remove(&peer_id) {
                    info!(
                        "Relay client has been disconnected: {peer_id:?}. Relay client count: {}",
                        self.connected_relay_clients.len()
                    );
                }

                self.record_connection_metrics();
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                peer_id: None,
                error,
            } => {
                event_string = "OutgoingConnErrWithoutPeerId";
                warn!("OutgoingConnectionError on {connection_id:?} - {error:?}");
                let remote_peer = "";
                error!(
                    "Node {:?} Remote {remote_peer:?} - Outgoing Connection Error",
                    self.self_peer_id,
                );
                self.record_connection_metrics();

                self.initial_bootstrap.on_outgoing_connection_error(
                    None,
                    &mut self.swarm,
                    self.peers_in_rt,
                );
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id: Some(failed_peer_id),
                error,
                connection_id,
            } => {
                event_string = "OutgoingConnErr";
                warn!("OutgoingConnectionError to {failed_peer_id:?} on {connection_id:?} - {error:?}");
                error!(
                    "Node {:?} Remote {failed_peer_id:?} - Outgoing Connection Error",
                    self.self_peer_id,
                );
                let connection_details = self.live_connected_peers.remove(&connection_id);
                self.record_connection_metrics();

                self.initial_bootstrap.on_outgoing_connection_error(
                    Some(failed_peer_id),
                    &mut self.swarm,
                    self.peers_in_rt,
                );

                // we need to decide if this was a critical error and if we should report it to the Issue tracker
                let is_critical_error = match error {
                    DialError::Transport(errors) => {
                        // as it's an outgoing error, if it's transport based we can assume it is _our_ fault
                        //
                        // (eg, could not get a port for a tcp connection)
                        // so we default to it not being a real issue
                        // unless there are _specific_ errors (connection refused eg)
                        error!("Dial errors len : {:?}", errors.len());
                        let mut there_is_a_serious_issue = false;
                        // Libp2p throws errors for all the listen addr (including private) of the remote peer even
                        // though we try to dial just the global/public addr. This would mean that we get
                        // MultiaddrNotSupported error for the private addr of the peer.
                        //
                        // Just a single MultiaddrNotSupported error is not a critical issue, but if all the listen
                        // addrs of the peer are private, then it is a critical issue.
                        let mut all_multiaddr_not_supported = true;
                        for (_addr, err) in errors {
                            match err {
                                TransportError::MultiaddrNotSupported(addr) => {
                                    warn!("OutgoingConnectionError: Transport::MultiaddrNotSupported {addr:?}. This can be ignored if the peer has atleast one global address.");
                                    #[cfg(feature = "loud")]
                                    {
                                        warn!("OutgoingConnectionError: Transport::MultiaddrNotSupported {addr:?}. This can be ignored if the peer has atleast one global address.");
                                        println!("If this was your bootstrap peer, restart your node with a supported multiaddr");
                                    }
                                }
                                TransportError::Other(err) => {
                                    error!("OutgoingConnectionError: Transport::Other {err:?}");

                                    all_multiaddr_not_supported = false;
                                    let problematic_errors = [
                                        "ConnectionRefused",
                                        "HostUnreachable",
                                        "HandshakeTimedOut",
                                    ];

                                    if self.initial_bootstrap.is_bootstrap_peer(&failed_peer_id)
                                        && !self.initial_bootstrap.has_terminated()
                                    {
                                        warn!("OutgoingConnectionError: On bootstrap peer {failed_peer_id:?}, while still in bootstrap mode, ignoring");
                                        there_is_a_serious_issue = false;
                                    } else {
                                        // It is really difficult to match this error, due to being eg:
                                        // Custom { kind: Other, error: Left(Left(Os { code: 61, kind: ConnectionRefused, message: "Connection refused" })) }
                                        // if we can match that, let's. But meanwhile we'll check the message
                                        let error_msg = format!("{err:?}");
                                        if problematic_errors
                                            .iter()
                                            .any(|err| error_msg.contains(err))
                                        {
                                            warn!("Problematic error encountered: {error_msg}");
                                            there_is_a_serious_issue = true;
                                        }
                                    }
                                }
                            }
                        }
                        if all_multiaddr_not_supported {
                            warn!("All multiaddrs had MultiaddrNotSupported error for {failed_peer_id:?}. Marking it as a serious issue.");
                            there_is_a_serious_issue = true;
                        }
                        there_is_a_serious_issue
                    }
                    DialError::NoAddresses => {
                        // We provided no address, and while we can't really blame the peer
                        // we also can't connect, so we opt to cleanup...
                        warn!("OutgoingConnectionError: No address provided");
                        true
                    }
                    DialError::Aborted => {
                        // not their fault
                        warn!("OutgoingConnectionError: Aborted");
                        false
                    }
                    DialError::DialPeerConditionFalse(_) => {
                        // we could not dial due to an internal condition, so not their issue
                        warn!("OutgoingConnectionError: DialPeerConditionFalse");
                        false
                    }
                    DialError::LocalPeerId { endpoint, .. } => {
                        // This is actually _us_ So we should remove this from the RT
                        error!(
                            "OutgoingConnectionError: LocalPeerId: {}",
                            endpoint_str(&endpoint)
                        );
                        true
                    }
                    DialError::WrongPeerId { obtained, endpoint } => {
                        // The peer id we attempted to dial was not the one we expected
                        // cleanup
                        error!("OutgoingConnectionError: WrongPeerId: obtained: {obtained:?}, endpoint: {endpoint:?}");
                        true
                    }
                    DialError::Denied { cause } => {
                        // The peer denied our connection
                        // cleanup
                        error!("OutgoingConnectionError: Denied: {cause:?}");
                        true
                    }
                };

                if is_critical_error {
                    warn!("Outgoing Connection error to {failed_peer_id:?} is considered as critical. Marking it as an issue.");
                    self.record_node_issue(failed_peer_id, NodeIssue::ConnectionIssue);

                    if let (Some((_, failed_addr, _)), Some(bootstrap_cache)) =
                        (connection_details, self.bootstrap_cache.as_mut())
                    {
                        bootstrap_cache.update_addr_status(&failed_addr, false);
                    }
                }
            }
            SwarmEvent::IncomingConnectionError {
                connection_id,
                local_addr,
                send_back_addr,
                error,
            } => {
                event_string = "Incoming ConnErr";
                // Only log as ERROR if the connection is not adjacent to an already established connection id from
                // the same IP address.
                //
                // If a peer contains multiple transports/listen addrs, we might try to open multiple connections,
                // and if the first one passes, we would get error on the rest. We don't want to log these.
                //
                // Also sometimes we get the ConnectionEstablished event immediately after this event.
                // So during tokio::select! of the events, we skip processing IncomingConnectionError for one round,
                // giving time for ConnectionEstablished to be hopefully processed.
                // And since we don't do anything critical with this event, the order and time of processing is
                // not critical.

                if self.is_incoming_connection_error_valid(connection_id, &send_back_addr) {
                    let remote_peer_id = match multiaddr_get_peer_id(&send_back_addr) {
                        Some(peer_id) => format!("{peer_id:?}"),
                        None => String::new(),
                    };
                    error!("IncomingConnectionError Valid from local_addr {local_addr:?}, send_back_addr {send_back_addr:?} on {connection_id:?} with error {error:?}");
                    error!(
                        "Node {:?} Remote {remote_peer_id:?} - Incoming Connection Error",
                        self.self_peer_id
                    );
                } else {
                    debug!("IncomingConnectionError InValid from local_addr {local_addr:?}, send_back_addr {send_back_addr:?} on {connection_id:?} with error {error:?}");
                }

                #[cfg(feature = "open-metrics")]
                if let Some(relay_manager) = self.relay_manager.as_mut() {
                    relay_manager.on_incomming_connection_error(&send_back_addr, &connection_id);
                }

                let _ = self.live_connected_peers.remove(&connection_id);
                self.record_connection_metrics();
            }
            SwarmEvent::Dialing {
                peer_id,
                connection_id,
            } => {
                event_string = "Dialing";
                debug!("Dialing {peer_id:?} on {connection_id:?}");
            }
            SwarmEvent::NewExternalAddrCandidate { address } => {
                event_string = "NewExternalAddrCandidate";
                info!(?address, "new external address candidate");
            }
            SwarmEvent::ExternalAddrConfirmed { address } => {
                event_string = "ExternalAddrConfirmed";
                info!(%address, "external address: confirmed");
            }
            SwarmEvent::ExternalAddrExpired { address } => {
                event_string = "ExternalAddrExpired";
                info!(%address, "external address: expired");
            }
            SwarmEvent::ExpiredListenAddr {
                listener_id,
                address,
            } => {
                event_string = "ExpiredListenAddr";
                info!("Listen address has expired. {listener_id:?} on {address:?}");
            }
            SwarmEvent::ListenerError { listener_id, error } => {
                event_string = "ListenerError";
                warn!("ListenerError {listener_id:?} with non-fatal error {error:?}");
            }
            other => {
                event_string = "Other";

                debug!("SwarmEvent has been ignored: {other:?}")
            }
        }
        self.remove_outdated_connections();

        self.log_handling(event_string.to_string(), start.elapsed());

        trace!(
            "SwarmEvent handled in {:?}: {event_string:?}",
            start.elapsed()
        );
        Ok(())
    }

    // Remove outdated connection to a peer if it is not in the RT.
    // Optionally force remove all the connections for a provided peer.
    fn remove_outdated_connections(&mut self) {
        // To avoid this being called too frequenctly, only carry out prunning intervally.
        if Instant::now() < self.last_connection_pruning_time + Duration::from_secs(30) {
            return;
        }
        self.last_connection_pruning_time = Instant::now();

        let mut removed_conns = 0;
        self.live_connected_peers.retain(|connection_id, (peer_id, _addr, timeout_time)| {

            // skip if timeout isn't reached yet
            if Instant::now() < *timeout_time {
                return true; // retain peer
            }

            // ignore if peer is present in our RT
            if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(*peer_id) {
                if kbucket
                    .iter()
                    .any(|peer_entry| *peer_id == *peer_entry.node.key.preimage())
                {
                    return true; // retain peer
                }
            }

            // skip if the peer is a relay server that we're connected to
            if let Some(relay_manager) = self.relay_manager.as_ref() {
                if relay_manager.keep_alive_peer(peer_id) {
                    return true; // retain peer
                }
            }

            // skip if the peer is a node that is being relayed through us
            if self.connected_relay_clients.contains(peer_id) {
                return true; // retain peer
            }

            // actually remove connection
            let result = self.swarm.close_connection(*connection_id);
            debug!("Removed outdated connection {connection_id:?} to {peer_id:?} with result: {result:?}");

            removed_conns += 1;

            // do not retain this connection as it has been closed
            false
        });

        if removed_conns == 0 {
            return;
        }

        self.record_connection_metrics();

        debug!(
            "Current libp2p peers pool stats is {:?}",
            self.swarm.network_info()
        );
        debug!(
            "Removed {removed_conns} outdated live connections, still have {} left.",
            self.live_connected_peers.len()
        );
    }

    /// Record the metrics on update of connection state.
    fn record_connection_metrics(&self) {
        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = &self.metrics_recorder {
            metrics_recorder
                .open_connections
                .set(self.live_connected_peers.len() as i64);
            metrics_recorder
                .connected_peers
                .set(self.swarm.connected_peers().count() as i64);

            metrics_recorder
                .connected_relay_clients
                .set(self.connected_relay_clients.len() as i64);
        }
    }

    /// Insert the latest established connection id into the list.
    fn insert_latest_established_connection_ids(&mut self, id: ConnectionId, addr: &Multiaddr) {
        let Ok(id) = format!("{id}").parse::<usize>() else {
            return;
        };
        let Some(ip_addr) = multiaddr_get_ip(addr) else {
            return;
        };

        let _ = self
            .latest_established_connection_ids
            .insert(id, (ip_addr, Instant::now()));

        while self.latest_established_connection_ids.len() >= 50 {
            // remove the oldest entry
            let Some(oldest_key) = self
                .latest_established_connection_ids
                .iter()
                .min_by_key(|(_, (_, time))| *time)
                .map(|(id, _)| *id)
            else {
                break;
            };

            self.latest_established_connection_ids.remove(&oldest_key);
        }
    }

    // Do not log IncomingConnectionError if the ConnectionId is adjacent to an already established connection.
    fn is_incoming_connection_error_valid(&self, id: ConnectionId, addr: &Multiaddr) -> bool {
        let Ok(id) = format!("{id}").parse::<usize>() else {
            return true;
        };
        let Some(ip_addr) = multiaddr_get_ip(addr) else {
            return true;
        };

        // This should prevent most of the cases where we get an IncomingConnectionError for a peer with multiple
        // transports/listen addrs.
        if let Some((established_ip_addr, _)) =
            self.latest_established_connection_ids.get(&(id - 1))
        {
            if established_ip_addr == &ip_addr {
                return false;
            }
        } else if let Some((established_ip_addr, _)) =
            self.latest_established_connection_ids.get(&(id + 1))
        {
            if established_ip_addr == &ip_addr {
                return false;
            }
        }

        true
    }
}

/// Helper function to print formatted connection role info.
fn endpoint_str(endpoint: &libp2p::core::ConnectedPoint) -> String {
    match endpoint {
        libp2p::core::ConnectedPoint::Dialer { address, .. } => {
            format!("outgoing ({address})")
        }
        libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } => {
            format!("incoming ({send_back_addr})")
        }
    }
}
