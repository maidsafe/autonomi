use libp2p::autonat;
use tracing::{debug, info, warn};

use crate::App;

impl App {
    pub(crate) fn on_event_autonat(&mut self, event: autonat::Event) {
        match event {
            autonat::Event::InboundProbe(event) => match event {
                autonat::InboundProbeEvent::Request {
                    probe_id,
                    peer: peer_id,
                    addresses,
                } => {
                    info!(?probe_id, %peer_id, ?addresses, "Received a request to probe peer")
                }
                autonat::InboundProbeEvent::Response {
                    probe_id,
                    peer: peer_id,
                    address,
                } => {
                    debug!(?probe_id, %peer_id, ?address, "Successfully probed a peer");
                }
                autonat::InboundProbeEvent::Error {
                    probe_id,
                    peer: peer_id,
                    error,
                } => {
                    warn!(?probe_id, %peer_id, ?error, "Probing a peer failed")
                }
            },
            autonat::Event::OutboundProbe(event) => match event {
                autonat::OutboundProbeEvent::Request {
                    probe_id,
                    peer: peer_id,
                } => {
                    debug!(?probe_id, %peer_id, "Asking remote to probe us")
                }
                autonat::OutboundProbeEvent::Response {
                    probe_id,
                    peer: peer_id,
                    address,
                } => {
                    info!(?probe_id, %peer_id, ?address, "Remote successfully probed (reached) us")
                }
                autonat::OutboundProbeEvent::Error {
                    probe_id,
                    peer: peer_id,
                    error,
                } => {
                    // Ignore the `NoServer` error if we're a server ourselves.
                    if self.client_state.is_none()
                        && !matches!(error, autonat::OutboundProbeError::NoServer)
                    {
                        warn!(
                            ?probe_id,
                            ?peer_id,
                            ?error,
                            "A request for probing us has failed"
                        )
                    }
                }
            },
            autonat::Event::StatusChanged { old, new } => {
                info!(
                    ?new,
                    ?old,
                    confidence = self.swarm.behaviour().autonat.confidence(),
                    "AutoNAT status changed"
                );
                self.check_state();
            }
        }
    }
}
