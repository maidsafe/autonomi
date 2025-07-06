use libp2p::upnp;
use tracing::{debug, info};
use tracing_log::log::error;

use crate::App;

impl App {
    pub(crate) fn on_event_upnp(&mut self, event: upnp::Event) {
        match event {
            upnp::Event::NewExternalAddr(addr) => {
                info!(%addr, "Successfully mapped UPnP port");
            }
            upnp::Event::ExpiredExternalAddr(addr) => {
                debug!(%addr, "External UPnP port mapping expired");
            }
            upnp::Event::GatewayNotFound => {
                error!("No UPnP gateway not found");
            }
            upnp::Event::NonRoutableGateway => {
                error!("UPnP gateway is not routable");
            }
        }
    }
}
