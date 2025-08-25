use crate::Multiaddr;

/// A list of addresses of a peer in the routing table.
#[derive(Clone, Debug, Default)]
pub struct Addresses(pub Vec<Multiaddr>);
