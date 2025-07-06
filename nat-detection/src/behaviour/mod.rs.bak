use libp2p::identity;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::swarm::NetworkBehaviour;
use std::time::Duration;

use crate::CONFIDENCE_MAX;

mod autonat;
mod identify;
mod upnp;

pub(crate) const PROTOCOL_VERSION: &str = "/sn_nat_detection/0.1.0";

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    pub autonat: libp2p::autonat::Behaviour,
    pub identify: libp2p::identify::Behaviour,
    pub upnp: Toggle<libp2p::upnp::tokio::Behaviour>,
}

impl Behaviour {
    pub(crate) fn new(
        local_public_key: identity::PublicKey,
        client_mode: bool,
        upnp: bool,
    ) -> Self {
        let far_future = Duration::MAX / 10; // `MAX` on itself causes overflows. This is a workaround.
        Self {
            autonat: libp2p::autonat::Behaviour::new(
                local_public_key.to_peer_id(),
                if client_mode {
                    libp2p::autonat::Config {
                        // Use dialed peers for probing.
                        use_connected: true,
                        // Start probing a few seconds after swarm init. This gives us time to connect to the dialed server.
                        // With UPnP enabled, give it a bit more time to possibly open up the port.
                        boot_delay: if upnp {
                            Duration::from_secs(7)
                        } else {
                            Duration::from_secs(3)
                        },
                        // Reuse probe server immediately even if it's the only one.
                        throttle_server_period: Duration::ZERO,
                        retry_interval: Duration::from_secs(10),
                        // We do not want to refresh.
                        refresh_interval: far_future,
                        confidence_max: CONFIDENCE_MAX,
                        ..Default::default()
                    }
                } else {
                    libp2p::autonat::Config {
                        // Do not ask for dial-backs, essentially putting us in server mode.
                        use_connected: false,
                        // Never start probing, as we are a server.
                        boot_delay: far_future,
                        ..Default::default()
                    }
                },
            ),
            identify: libp2p::identify::Behaviour::new(
                libp2p::identify::Config::new(
                    PROTOCOL_VERSION.to_string(),
                    local_public_key.clone(),
                )
                // Exchange information every 5 minutes.
                .with_interval(Duration::from_secs(5 * 60)),
            ),
            upnp: upnp.then(libp2p::upnp::tokio::Behaviour::default).into(),
        }
    }
}
