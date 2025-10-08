// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::SwarmDriver;
use crate::{
    event::TerminateNodeReason,
    networking::{
        NetworkEvent, NodeIssue, Result,
        driver::behaviour::upnp,
        error::{dial_error_to_str, listen_error_to_str},
        network::endpoint_str,
    },
};
use itertools::Itertools;
#[cfg(feature = "open-metrics")]
use libp2p::metrics::Recorder;
use libp2p::{
    Multiaddr, TransportError,
    core::ConnectedPoint,
    multiaddr::Protocol,
    swarm::{ConnectionId, DialError, SwarmEvent},
};
use std::time::Instant;
use tokio::time::Duration;

use super::NodeEvent;

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
            SwarmEvent::Behaviour(NodeEvent::Upnp(upnp_event)) => {
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&upnp_event);
                }
                event_string = "upnp_event";
                info!(?upnp_event, "UPnP event");
                match upnp_event {
                    upnp::behaviour::Event::GatewayNotFound => {
                        warn!(
                            "UPnP is not enabled/supported on the gateway. Please rerun with the `--no-upnp` flag"
                        );
                        self.send_event(NetworkEvent::TerminateNode {
                            reason: TerminateNodeReason::UpnpGatewayNotFound,
                        });
                    }
                    upnp::behaviour::Event::NewExternalAddr { addr, local_addr } => {
                        info!(
                            "UPnP: New external address found: {addr:?}, local address: {local_addr:?}"
                        );
                        self.initial_bootstrap_trigger.upnp_gateway_result_obtained = true;
                    }
                    upnp::behaviour::Event::NonRoutableGateway => {
                        warn!("UPnP gateway is not routable");
                        self.initial_bootstrap_trigger.upnp_gateway_result_obtained = true;
                    }
                    upnp::behaviour::Event::ExpiredExternalAddr { addr, local_addr } => {
                        info!(
                            "UPnP External address expired: {addr:?}, local address: {local_addr:?}"
                        );
                    }
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

                if self.local {
                    // all addresses are effectively external here...
                    // this is needed for Kad Mode::Server
                    self.swarm.add_external_address(address.clone());
                    if let Err(err) = self.add_sync_and_flush_cache(address.clone()) {
                        warn!("Failed to sync and flush cache during NewListenAddr: {err:?}");
                    }
                } else if let Some(external_address_manager) =
                    self.external_address_manager.as_mut()
                {
                    external_address_manager.on_new_listen_addr(address.clone(), &mut self.swarm);
                } else {
                    // just for future reference.
                    warn!(
                        "External address manager is not enabled for a public node. This should not happen."
                    );
                }

                if tracing::level_enabled!(tracing::Level::DEBUG) {
                    let all_external_addresses = self.swarm.external_addresses().collect_vec();
                    let all_listeners = self.swarm.listeners().collect_vec();
                    debug!("All our listeners: {all_listeners:?}");
                    debug!("All our external addresses: {all_external_addresses:?}");
                }

                self.initial_bootstrap_trigger.listen_addr_obtained = true;

                self.send_event(NetworkEvent::NewListenAddr(address));
            }
            SwarmEvent::ListenerClosed {
                listener_id,
                addresses,
                reason,
            } => {
                event_string = "listener closed";
                info!(
                    "Listener {listener_id:?} with add {addresses:?} has been closed for {reason:?}"
                );
                self.send_event(NetworkEvent::ExpiredListenAddresses(addresses));
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

                if let Some(external_address_manager) = self.external_address_manager.as_mut()
                    && let ConnectedPoint::Listener { local_addr, .. } = &endpoint
                {
                    external_address_manager.on_established_incoming_connection(local_addr.clone());
                }

                let _ = self.live_connected_peers.insert(
                    connection_id,
                    (
                        peer_id,
                        endpoint.get_remote_address().clone(),
                        Instant::now() + Duration::from_secs(60),
                    ),
                );

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

                self.record_connection_metrics();
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                peer_id: None,
                error,
            } => {
                event_string = "OutgoingConnErrWithoutPeerId";

                debug!("OutgoingConnectionError on {connection_id:?} - {error:?}");

                let remote_peer = "";
                // ELK logging. Do not update without proper testing.
                for (error_str, level) in dial_error_to_str(&error) {
                    match level {
                        tracing::Level::ERROR => error!(
                            "Node {:?} Remote {remote_peer:?} - Outgoing Connection Error - {error_str:?}",
                            self.self_peer_id,
                        ),
                        _ => debug!(
                            "Node {:?} Remote {remote_peer:?} - Outgoing Connection Error - {error_str:?}",
                            self.self_peer_id,
                        ),
                    }
                }

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
                debug!(
                    "OutgoingConnectionError to {failed_peer_id:?} on {connection_id:?} - {error:?}"
                );

                // ELK logging. Do not update without proper testing.
                for (error_str, level) in dial_error_to_str(&error) {
                    match level {
                        tracing::Level::ERROR => error!(
                            "Node {:?} Remote {failed_peer_id:?} - Outgoing Connection Error - {error_str:?}",
                            self.self_peer_id,
                        ),
                        _ => debug!(
                            "Node {:?} Remote {failed_peer_id:?} - Outgoing Connection Error - {error_str:?}",
                            self.self_peer_id,
                        ),
                    }
                }
                let _ = self.live_connected_peers.remove(&connection_id);
                self.record_connection_metrics();

                self.initial_bootstrap.on_outgoing_connection_error(
                    Some(failed_peer_id),
                    &mut self.swarm,
                    self.peers_in_rt,
                );

                // we need to decide if this was a critical error and if we should report it to the Issue tracker
                let is_critical_error = match &error {
                    DialError::Transport(errors) => {
                        // as it's an outgoing error, if it's transport based we can assume it is _our_ fault
                        //
                        // (eg, could not get a port for a tcp connection)
                        // so we default to it not being a real issue
                        // unless there are _specific_ errors (connection refused eg)
                        debug!("Dial errors len : {:?} on {connection_id:?}", errors.len());
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
                                    debug!(
                                        "OutgoingConnectionError: Transport::MultiaddrNotSupported {addr:?}. This can be ignored if the peer has atleast one global address."
                                    );
                                    #[cfg(feature = "loud")]
                                    {
                                        debug!(
                                            "OutgoingConnectionError: Transport::MultiaddrNotSupported {addr:?}. This can be ignored if the peer has atleast one global address."
                                        );
                                        println!(
                                            "If this was your bootstrap peer, restart your node with a supported multiaddr"
                                        );
                                    }
                                }
                                TransportError::Other(err) => {
                                    debug!("OutgoingConnectionError: Transport::Other {err:?}");

                                    all_multiaddr_not_supported = false;
                                    let problematic_errors = [
                                        "ConnectionRefused",
                                        "HostUnreachable",
                                        "HandshakeTimedOut",
                                    ];

                                    if self.initial_bootstrap.is_bootstrap_peer(&failed_peer_id)
                                        && !self.initial_bootstrap.has_terminated()
                                    {
                                        debug!(
                                            "OutgoingConnectionError: On bootstrap peer {failed_peer_id:?}, while still in bootstrap mode, ignoring"
                                        );
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
                                            debug!("Problematic error encountered: {error_msg}");
                                            there_is_a_serious_issue = true;
                                        }
                                    }
                                }
                            }
                        }
                        if all_multiaddr_not_supported {
                            debug!(
                                "All multiaddrs had MultiaddrNotSupported error for {failed_peer_id:?}. Marking it as a serious issue."
                            );
                            there_is_a_serious_issue = true;
                        }
                        there_is_a_serious_issue
                    }
                    DialError::NoAddresses => {
                        // We provided no address, and while we can't really blame the peer
                        // we also can't connect, so we opt to cleanup...
                        debug!("OutgoingConnectionError: No address provided");
                        true
                    }
                    DialError::Aborted => {
                        // not their fault
                        debug!("OutgoingConnectionError: Aborted");
                        false
                    }
                    DialError::DialPeerConditionFalse(_) => {
                        // we could not dial due to an internal condition, so not their issue
                        debug!("OutgoingConnectionError: DialPeerConditionFalse");
                        false
                    }
                    DialError::LocalPeerId { address } => {
                        // This is actually _us_ So we should remove this from the RT
                        debug!("OutgoingConnectionError: LocalPeerId: {address}");
                        true
                    }
                    DialError::WrongPeerId { obtained, address } => {
                        // The peer id we attempted to dial was not the one we expected
                        // cleanup
                        debug!(
                            "OutgoingConnectionError: WrongPeerId: obtained: {obtained:?}, address: {address:?}"
                        );
                        true
                    }
                    DialError::Denied { cause } => {
                        // The peer denied our connection
                        // cleanup
                        debug!("OutgoingConnectionError: Denied: {cause:?}");
                        true
                    }
                };

                if is_critical_error {
                    warn!(
                        "Outgoing Connection error to {failed_peer_id:?} is considered as critical. Marking it as an issue. Error: {error:?}"
                    );
                    self.record_node_issue(failed_peer_id, NodeIssue::ConnectionIssue);
                }
            }
            SwarmEvent::IncomingConnectionError {
                connection_id,
                local_addr,
                send_back_addr,
                error,
                peer_id,
            } => {
                event_string = "Incoming ConnErr";
                debug!(
                    "IncomingConnectionError from local_addr {local_addr:?}, send_back_addr {send_back_addr:?} on {connection_id:?} with error {error:?}"
                );

                // ELK logging. Do not update without proper testing.
                let (error_str, level) = listen_error_to_str(&error);
                match level {
                    tracing::Level::ERROR => error!(
                        "Node {:?} Remote {peer_id:?} - Incoming Connection Error - {error_str:?}",
                        self.self_peer_id,
                    ),
                    _ => debug!(
                        "Node {:?} Remote {peer_id:?} - Incoming Connection Error - {error_str:?}",
                        self.self_peer_id,
                    ),
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
                debug!("New external address candidate: {address:?}");
                if let Some(external_address_manager) = self.external_address_manager.as_mut() {
                    external_address_manager
                        .add_external_address_candidate(address, &mut self.swarm);
                }
            }
            SwarmEvent::ExternalAddrConfirmed { address } => {
                event_string = "ExternalAddrConfirmed";
                info!("External address has been confirmed: {address:?}");
            }
            SwarmEvent::ExternalAddrExpired { address } => {
                event_string = "ExternalAddrExpired";
                info!("External address has expired: {address:?}");
            }
            SwarmEvent::ExpiredListenAddr {
                listener_id,
                address,
            } => {
                event_string = "ExpiredListenAddr";
                info!("Listen address has expired. {listener_id:?} on {address:?}");
                if let Some(external_address_manager) = self.external_address_manager.as_mut() {
                    external_address_manager.on_expired_listen_addr(address, &self.swarm);
                }
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
            if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(*peer_id)
                && kbucket
                    .iter()
                    .any(|peer_entry| *peer_id == *peer_entry.node.key.preimage())
                {
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
            let _ = metrics_recorder
                .open_connections
                .set(self.live_connected_peers.len() as i64);
            let _ = metrics_recorder
                .connected_peers
                .set(self.swarm.connected_peers().count() as i64);
        }
    }

    /// Insert the latest established connection id into the list.
    fn insert_latest_established_connection_ids(&mut self, id: ConnectionId, addr: &Multiaddr) {
        let Ok(id) = format!("{id}").parse::<usize>() else {
            return;
        };

        let _ = self
            .latest_established_connection_ids
            .insert(id, (addr.clone(), Instant::now()));

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

            let _ = self.latest_established_connection_ids.remove(&oldest_key);
        }
    }
}
