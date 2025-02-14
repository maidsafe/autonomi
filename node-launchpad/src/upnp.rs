extern crate igd_next as igd;

use local_ip_address::local_ip;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use strum::Display;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Display, Deserialize)]
pub enum UpnpSupport {
    Supported,
    Unsupported,
    Loading,
    Unknown,
}

pub(crate) fn get_upnp_support() -> UpnpSupport {
    match igd::search_gateway(Default::default()) {
        Err(_) => {
            // No UPnP gateway found.
            info!("No UPnP gateway found");
            UpnpSupport::Unsupported
        }
        Ok(gateway) => {
            if let Ok(local_ip) = local_ip() {
                const PROTOCOL: igd::PortMappingProtocol = igd::PortMappingProtocol::TCP;
                const PORT: u16 = 12356;

                let local_addr = SocketAddr::new(local_ip, PORT);

                match gateway.add_port(PROTOCOL, PORT, local_addr, 60, "Autonomi Launchpad test") {
                    Err(_) => {
                        // UPnP gateway found, but could not open port.
                        info!("UPnP gateway found, but could not open port");
                        UpnpSupport::Unsupported
                    }
                    Ok(()) => {
                        // UPnP successful.
                        info!("UPnP successful");

                        // Try to remove port again, but don't care about the result.
                        // Lease time is only 60s anyway.
                        let _ = gateway.remove_port(PROTOCOL, PORT);

                        UpnpSupport::Supported
                    }
                }
            } else {
                // UPnP gateway found, but could not get local IP
                // This shouldn't happen
                info!("UPnP gateway found, but could not get local IP");
                UpnpSupport::Unsupported
            }
        }
    }
}
