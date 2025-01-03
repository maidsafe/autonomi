// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    event::NodeEvent, multiaddr_get_ip, multiaddr_is_global, multiaddr_strip_p2p,
    relay_manager::is_a_relayed_peer, time::Instant, NetworkEvent, Result, SwarmDriver,
};
use ant_protocol::version::{IDENTIFY_NODE_VERSION_STR, IDENTIFY_PROTOCOL_STR};
use libp2p::mdns;
#[cfg(feature = "open-metrics")]
use libp2p::metrics::Recorder;
use libp2p::{
    core::ConnectedPoint,
    kad::K_VALUE,
    multiaddr::Protocol,
    swarm::{
        dial_opts::{DialOpts, PeerCondition},
        ConnectionId, DialError, SwarmEvent,
    },
    Multiaddr, PeerId, TransportError,
};
use std::collections::HashSet;
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
                event_string = "relay_client_event";

                info!(?event, "relay client event");

                if let libp2p::relay::client::Event::ReservationReqAccepted {
                    relay_peer_id, ..
                } = *event
                {
                    if let Some(relay_manager) = self.relay_manager.as_mut() {
                        relay_manager
                            .on_successful_reservation_by_client(&relay_peer_id, &mut self.swarm);
                    }
                }
            }
            #[cfg(feature = "upnp")]
            SwarmEvent::Behaviour(NodeEvent::Upnp(upnp_event)) => {
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&upnp_event);
                }
                event_string = "upnp_event";
                info!(?upnp_event, "UPnP event");
                if let libp2p::upnp::Event::GatewayNotFound = upnp_event {
                    warn!("UPnP is not enabled/supported on the gateway. Please rerun without the `--upnp` flag");
                    self.send_event(NetworkEvent::TerminateNode {
                        reason: crate::event::TerminateNodeReason::UpnpGatewayNotFound,
                    });
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
                    }
                    libp2p::relay::Event::ReservationTimedOut { src_peer_id } => {
                        self.connected_relay_clients.remove(&src_peer_id);
                    }
                    _ => {}
                }
            }
            SwarmEvent::Behaviour(NodeEvent::Identify(iden)) => {
                // Record the Identify event for metrics if the feature is enabled.
                #[cfg(feature = "open-metrics")]
                if let Some(metrics_recorder) = &self.metrics_recorder {
                    metrics_recorder.record(&(*iden));
                }
                event_string = "identify";

                match *iden {
                    libp2p::identify::Event::Received {
                        peer_id,
                        info,
                        connection_id,
                    } => {
                        debug!(conn_id=%connection_id, %peer_id, ?info, "identify: received info");

                        let our_identify_protocol = IDENTIFY_PROTOCOL_STR.read().expect("IDENTIFY_PROTOCOL_STR has been locked to write. A call to set_network_id performed. This should not happen.").to_string();

                        if info.protocol_version != our_identify_protocol {
                            warn!(?info.protocol_version, "identify: {peer_id:?} does not have the same protocol. Our IDENTIFY_PROTOCOL_STR: {our_identify_protocol:?}");

                            self.send_event(NetworkEvent::PeerWithUnsupportedProtocol {
                                our_protocol: our_identify_protocol,
                                their_protocol: info.protocol_version,
                            });
                            // Block the peer from any further communication.
                            self.swarm.behaviour_mut().blocklist.block_peer(peer_id);
                            if let Some(dead_peer) =
                                self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id)
                            {
                                error!("Clearing out a protocol mistmatch peer from RT. Something went wrong, we should not have added this peer to RT: {peer_id:?}");
                                self.update_on_peer_removal(*dead_peer.node.key.preimage());
                            }

                            return Ok(());
                        }

                        let our_agent_version = IDENTIFY_NODE_VERSION_STR.read().expect("IDENTIFY_NODE_VERSION_STR has been locked to write. A call to set_network_id performed. This should not happen.").to_string();
                        // if client, return.
                        if info.agent_version != our_agent_version {
                            return Ok(());
                        }

                        let has_dialed = self.dialed_peers.contains(&peer_id);

                        // If we're not in local mode, only add globally reachable addresses.
                        // Strip the `/p2p/...` part of the multiaddresses.
                        // Collect into a HashSet directly to avoid multiple allocations and handle deduplication.
                        let mut addrs: HashSet<Multiaddr> = match self.local {
                            true => info
                                .listen_addrs
                                .into_iter()
                                .map(|addr| multiaddr_strip_p2p(&addr))
                                .collect(),
                            false => info
                                .listen_addrs
                                .into_iter()
                                .filter(multiaddr_is_global)
                                .map(|addr| multiaddr_strip_p2p(&addr))
                                .collect(),
                        };

                        let has_relayed = is_a_relayed_peer(&addrs);

                        let is_bootstrap_peer = self
                            .bootstrap_peers
                            .iter()
                            .any(|(_ilog2, peers)| peers.contains(&peer_id));

                        // Do not use an `already relayed` peer as `potential relay candidate`.
                        if !has_relayed && !is_bootstrap_peer {
                            if let Some(relay_manager) = self.relay_manager.as_mut() {
                                debug!("Adding candidate relay server {peer_id:?}, it's not a bootstrap node");
                                relay_manager.add_potential_candidates(
                                    &peer_id,
                                    &addrs,
                                    &info.protocols,
                                );
                            }
                        }

                        // When received an identify from un-dialed peer, try to dial it
                        // The dial shall trigger the same identify to be sent again and confirm
                        // peer is external accessible, hence safe to be added into RT.
                        if !self.local && !has_dialed {
                            // Only need to dial back for not fulfilled kbucket
                            let (kbucket_full, already_present_in_rt, ilog2) =
                                if let Some(kbucket) =
                                    self.swarm.behaviour_mut().kademlia.kbucket(peer_id)
                                {
                                    let ilog2 = kbucket.range().0.ilog2();
                                    let num_peers = kbucket.num_entries();
                                    let is_bucket_full = num_peers >= K_VALUE.into();

                                    // check if peer_id is already a part of RT
                                    let already_present_in_rt = kbucket
                                        .iter()
                                        .any(|entry| entry.node.key.preimage() == &peer_id);

                                    // // If the bucket contains any of a bootstrap node,
                                    // // consider the bucket is not full and dial back
                                    // // so that the bootstrap nodes can be replaced.
                                    // if is_bucket_full {
                                    //     if let Some(peers) = self.bootstrap_peers.get(&ilog2) {
                                    //         if kbucket.iter().any(|entry| {
                                    //             peers.contains(entry.node.key.preimage())
                                    //         }) {
                                    //             is_bucket_full = false;
                                    //         }
                                    //     }
                                    // }

                                    (is_bucket_full, already_present_in_rt, ilog2)
                                } else {
                                    return Ok(());
                                };

                            if kbucket_full {
                                debug!("received identify for a full bucket {ilog2:?}, not dialing {peer_id:?} on {addrs:?}");
                                return Ok(());
                            } else if already_present_in_rt {
                                debug!("received identify for {peer_id:?} that is already part of the RT. Not dialing {peer_id:?} on {addrs:?}");
                                return Ok(());
                            }

                            info!(%peer_id, ?addrs, "received identify info from undialed peer for not full kbucket {ilog2:?}, dial back to confirm external accessible");
                            if let Err(err) = self.swarm.dial(
                                DialOpts::peer_id(peer_id)
                                    .condition(PeerCondition::NotDialing)
                                    .addresses(addrs.iter().cloned().collect())
                                    .build(),
                            ) {
                                warn!(%peer_id, ?addrs, "dialing error: {err:?}");
                            }

                            trace!(
                                "SwarmEvent handled in {:?}: {event_string:?}",
                                start.elapsed()
                            );
                            return Ok(());
                        }

                        // If we are not local, we care only for peers that we dialed and thus are reachable.
                        if self.local || has_dialed {
                            // A bad node cannot establish a connection with us. So we can add it to the RT directly.

                            // With the new bootstrap cache, the workload is distributed,
                            // hence no longer need to replace bootstrap nodes for workload share.
                            // self.remove_bootstrap_from_full(peer_id);

                            // Avoid have `direct link format` addrs co-exists with `relay` addr
                            if has_relayed {
                                addrs.retain(|multiaddr| {
                                    multiaddr.iter().any(|p| matches!(p, Protocol::P2pCircuit))
                                });
                            }

                            debug!(%peer_id, ?addrs, "identify: attempting to add addresses to routing table");

                            // Attempt to add the addresses to the routing table.
                            for multiaddr in addrs {
                                let _routing_update = self
                                    .swarm
                                    .behaviour_mut()
                                    .kademlia
                                    .add_address(&peer_id, multiaddr);
                            }
                        }
                        trace!(
                            "SwarmEvent handled in {:?}: {event_string:?}",
                            start.elapsed()
                        );
                    }
                    // Log the other Identify events.
                    libp2p::identify::Event::Sent { .. } => debug!("identify: {iden:?}"),
                    libp2p::identify::Event::Pushed { .. } => debug!("identify: {iden:?}"),
                    libp2p::identify::Event::Error { .. } => debug!("identify: {iden:?}"),
                }
            }
            SwarmEvent::Behaviour(NodeEvent::Mdns(mdns_event)) => {
                event_string = "mdns";
                // mDNS is only relevant in local mode
                if self.local {
                    match *mdns_event {
                        mdns::Event::Discovered(list) => {
                            if self.local {
                                for (peer_id, addr) in list {
                                    // The multiaddr does not contain the peer ID, so add it.
                                    let addr = addr.with(Protocol::P2p(peer_id));

                                    info!(%addr, "mDNS node discovered and dialing");

                                    if let Err(err) = self.dial(addr.clone()) {
                                        warn!(%addr, "mDNS node dial error: {err:?}");
                                    }
                                }
                            }
                        }
                        mdns::Event::Expired(peer) => {
                            debug!("mdns peer {peer:?} expired");
                        }
                    }
                }
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
                    if self.local {
                        // all addresses are effectively external here...
                        // this is needed for Kad Mode::Server
                        self.swarm.add_external_address(address.clone());
                    } else if let Some(external_add_manager) =
                        self.external_address_manager.as_mut()
                    {
                        external_add_manager.on_new_listen_addr(address.clone(), &mut self.swarm);
                    } else {
                        // just for future reference.
                        warn!("External address manager is not enabled for a public node. This should not happen.");
                    }
                }

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
                if let Some(external_addr_manager) = self.external_address_manager.as_mut() {
                    if let ConnectedPoint::Listener { local_addr, .. } = &endpoint {
                        external_addr_manager
                            .on_established_incoming_connection(local_addr.clone());
                    }
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
                self.record_connection_metrics();
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id: Some(failed_peer_id),
                error,
                connection_id,
            } => {
                event_string = "OutgoingConnErr";
                warn!("OutgoingConnectionError to {failed_peer_id:?} on {connection_id:?} - {error:?}");
                let connection_details = self.live_connected_peers.remove(&connection_id);
                self.record_connection_metrics();

                // we need to decide if this was a critical error and the peer should be removed from the routing table
                let should_clean_peer = match error {
                    DialError::Transport(errors) => {
                        // as it's an outgoing error, if it's transport based we can assume it is _our_ fault
                        //
                        // (eg, could not get a port for a tcp connection)
                        // so we default to it not being a real issue
                        // unless there are _specific_ errors (connection refused eg)
                        error!("Dial errors len : {:?}", errors.len());
                        let mut there_is_a_serious_issue = false;
                        for (_addr, err) in errors {
                            error!("OutgoingTransport error : {err:?}");

                            match err {
                                TransportError::MultiaddrNotSupported(addr) => {
                                    warn!("Multiaddr not supported : {addr:?}");
                                    #[cfg(feature = "loud")]
                                    {
                                        println!("Multiaddr not supported : {addr:?}");
                                        println!("If this was your bootstrap peer, restart your node with a supported multiaddr");
                                    }
                                    // if we can't dial a peer on a given address, we should remove it from the routing table
                                    there_is_a_serious_issue = true
                                }
                                TransportError::Other(err) => {
                                    let problematic_errors = [
                                        "ConnectionRefused",
                                        "HostUnreachable",
                                        "HandshakeTimedOut",
                                    ];

                                    let is_bootstrap_peer = self
                                        .bootstrap_peers
                                        .iter()
                                        .any(|(_ilog2, peers)| peers.contains(&failed_peer_id));

                                    if is_bootstrap_peer
                                        && self.peers_in_rt < self.bootstrap_peers.len()
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

                if should_clean_peer {
                    warn!("Tracking issue of {failed_peer_id:?}. Clearing it out for now");

                    // Just track failures during outgoing connection with `failed_peer_id` inside the bootstrap cache.
                    // OutgoingConnectionError without peer_id can happen when dialing multiple addresses of a peer.
                    // And similarly IncomingConnectionError can happen when a peer has multiple transports/listen addrs.
                    if let (Some((_, failed_addr, _)), Some(bootstrap_cache)) =
                        (connection_details, self.bootstrap_cache.as_mut())
                    {
                        bootstrap_cache.update_addr_status(&failed_addr, false);
                    }

                    if let Some(dead_peer) = self
                        .swarm
                        .behaviour_mut()
                        .kademlia
                        .remove_peer(&failed_peer_id)
                    {
                        self.update_on_peer_removal(*dead_peer.node.key.preimage());
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
                // Only log as ERROR if the the connection is not adjacent to an already established connection id from
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
                if self.should_we_log_incoming_connection_error(connection_id, &send_back_addr) {
                    error!("IncomingConnectionError from local_addr:?{local_addr:?}, send_back_addr {send_back_addr:?} on {connection_id:?} with error {error:?}");
                } else {
                    debug!("IncomingConnectionError from local_addr:?{local_addr:?}, send_back_addr {send_back_addr:?} on {connection_id:?} with error {error:?}");
                }
                if let Some(external_addr_manager) = self.external_address_manager.as_mut() {
                    external_addr_manager
                        .on_incoming_connection_error(local_addr.clone(), &mut self.swarm);
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

                if let Some(external_addr_manager) = self.external_address_manager.as_mut() {
                    external_addr_manager.add_external_address_candidate(address, &mut self.swarm);
                }
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
                if let Some(external_addr_manager) = self.external_address_manager.as_mut() {
                    external_addr_manager.on_expired_listen_addr(address, &self.swarm);
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

    // if target bucket is full, remove a bootstrap node if presents.
    #[allow(dead_code)]
    fn remove_bootstrap_from_full(&mut self, peer_id: PeerId) {
        let mut shall_removed = None;

        let mut bucket_index = Some(0);

        if let Some(kbucket) = self.swarm.behaviour_mut().kademlia.kbucket(peer_id) {
            if kbucket.num_entries() >= K_VALUE.into() {
                bucket_index = kbucket.range().0.ilog2();
                if let Some(peers) = self.bootstrap_peers.get(&bucket_index) {
                    for peer_entry in kbucket.iter() {
                        if peers.contains(peer_entry.node.key.preimage()) {
                            shall_removed = Some(*peer_entry.node.key.preimage());
                            break;
                        }
                    }
                }
            }
        }
        if let Some(to_be_removed_bootstrap) = shall_removed {
            info!("Bootstrap node {to_be_removed_bootstrap:?} to be replaced by peer {peer_id:?}");
            let entry = self
                .swarm
                .behaviour_mut()
                .kademlia
                .remove_peer(&to_be_removed_bootstrap);
            if let Some(removed_peer) = entry {
                self.update_on_peer_removal(*removed_peer.node.key.preimage());
            }

            // With the switch to using bootstrap cache, workload is distributed already.
            // to avoid peers keeps being replaced by each other,
            // there shall be just one time of removal to be undertaken.
            if let Some(peers) = self.bootstrap_peers.get_mut(&bucket_index) {
                let _ = peers.remove(&to_be_removed_bootstrap);
            }
        }
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
        if let Some(metrics) = &self.metrics_recorder {
            metrics
                .open_connections
                .set(self.live_connected_peers.len() as i64);
            metrics
                .connected_peers
                .set(self.swarm.connected_peers().count() as i64);
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
    fn should_we_log_incoming_connection_error(&self, id: ConnectionId, addr: &Multiaddr) -> bool {
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
