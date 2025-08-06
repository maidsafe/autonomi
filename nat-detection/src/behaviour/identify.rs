// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use libp2p::{Multiaddr, autonat, identify, multiaddr::Protocol};
use tracing::{debug, info, warn};

use crate::{App, behaviour::PROTOCOL_VERSION};

/// Verifies if `Multiaddr` contains IPv4 address that is not global.
/// This is used to filter out unroutable addresses from the Kademlia routing table.
pub fn multiaddr_is_global(multiaddr: &Multiaddr) -> bool {
    !multiaddr.iter().any(|addr| match addr {
        Protocol::Ip4(ip) => {
            // Based on the nightly `is_global` method (`Ipv4Addrs::is_global`), only using what is available in stable.
            // Missing `is_shared`, `is_benchmarking` and `is_reserved`.
            ip.is_unspecified()
                | ip.is_private()
                | ip.is_loopback()
                | ip.is_link_local()
                | ip.is_documentation()
                | ip.is_broadcast()
        }
        _ => false,
    })
}

impl App {
    pub(crate) fn on_event_identify(&mut self, event: identify::Event) {
        match event {
            identify::Event::Received {
                peer_id,
                info,
                connection_id,
            } => {
                debug!(
                    %peer_id,
                    protocols=?info.protocols,
                    observed_address=%info.observed_addr,
                    protocol_version=%info.protocol_version,
                    "Received peer info"
                );

                // Disconnect if peer has incompatible protocol version.
                if info.protocol_version != PROTOCOL_VERSION {
                    warn!(conn_id=%connection_id, %peer_id, "Incompatible protocol version. Disconnecting from peer.");
                    let _ = self.swarm.disconnect_peer_id(peer_id);
                    return;
                }

                // Disconnect if peer has no AutoNAT support.
                if !info.protocols.contains(&autonat::DEFAULT_PROTOCOL_NAME) {
                    warn!(conn_id=%connection_id, %peer_id, "Peer does not support AutoNAT. Disconnecting from peer.");
                    let _ = self.swarm.disconnect_peer_id(peer_id);
                    return;
                }

                info!(conn_id=%connection_id, %peer_id, "Received peer info: confirmed it supports AutoNAT");

                // If we're a client and the peer has (a) global listen address(es),
                // add it as an AutoNAT server.
                if self.client_state.is_some() {
                    for addr in info.listen_addrs.into_iter().filter(multiaddr_is_global) {
                        self.swarm
                            .behaviour_mut()
                            .autonat
                            .add_server(peer_id, Some(addr));
                    }
                }
                self.check_state();
            }
            identify::Event::Sent { .. } => { /* ignore */ }
            identify::Event::Pushed { .. } => { /* ignore */ }
            identify::Event::Error { .. } => { /* ignore */ }
        }
    }
}
