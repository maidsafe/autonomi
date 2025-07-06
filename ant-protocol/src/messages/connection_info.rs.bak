use libp2p::Multiaddr;
use libp2p::PeerId;
use std::fmt::Display;

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// The Peer ID of the peer that sent the response.
    pub peer_id: PeerId,
    /// The origin of the response.
    pub response_origin: Multiaddr,
}

impl Display for ConnectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ConnectionInfo (peer_id: {}, multiaddr: {})",
            self.peer_id, self.response_origin
        )
    }
}
