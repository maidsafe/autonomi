// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::large_enum_variant)]
#![allow(clippy::result_large_err)]

mod bootstrap;
mod circular_vec;
mod driver;
mod error;
mod interface;
mod log_markers;
#[cfg(feature = "open-metrics")]
mod metrics;
mod network;
mod reachability_check;
mod record_store;
mod replication_fetcher;
mod transport;

// re-export arch dependent deps for use in the crate, or above
pub use self::interface::SwarmLocalState;
pub use self::reachability_check::ReachabilityIssue;
pub use self::reachability_check::ReachabilityStatus;
pub(crate) use self::{
    error::NetworkError,
    interface::{NetworkEvent, NodeIssue},
    network::{Network, NetworkConfig},
    record_store::NodeRecordStore,
};

#[cfg(feature = "open-metrics")]
pub(crate) use metrics::service::MetricsRegistries;

use self::error::Result;
use ant_protocol::{CLOSE_GROUP_SIZE, NetworkAddress};
use libp2p::{
    Multiaddr, PeerId,
    kad::{KBucketDistance, KBucketKey},
    multiaddr::Protocol,
};

/// Sort the provided peers by their distance to the given `KBucketKey`.
/// Return with the closest expected number of entries it has.
pub fn sort_peers_by_key<T>(
    peers: Vec<(PeerId, Addresses)>,
    key: &KBucketKey<T>,
    expected_entries: usize,
) -> Result<Vec<(PeerId, Addresses)>> {
    // Check if there are enough peers to satisfy the request.
    // bail early if that's not the case
    if CLOSE_GROUP_SIZE > peers.len() {
        warn!("Not enough peers in the k-bucket to satisfy the request");
        return Err(NetworkError::NotEnoughPeers {
            found: peers.len(),
            required: CLOSE_GROUP_SIZE,
        });
    }

    // Create a vector of tuples where each tuple is a reference to a peer and its distance to the key.
    // This avoids multiple computations of the same distance in the sorting process.
    let mut peer_distances: Vec<(PeerId, Addresses, KBucketDistance)> =
        Vec::with_capacity(peers.len());

    for (peer_id, addrs) in peers.into_iter() {
        let addr = NetworkAddress::from(peer_id);
        let distance = key.distance(&addr.as_kbucket_key());
        peer_distances.push((peer_id, addrs, distance));
    }

    // Sort the vector of tuples by the distance.
    peer_distances.sort_by(|a, b| a.2.cmp(&b.2));

    // Collect the sorted peers into a new vector.
    let sorted_peers: Vec<(PeerId, Addresses)> = peer_distances
        .into_iter()
        .take(expected_entries)
        .map(|(peer_id, addrs, _)| (peer_id, addrs))
        .collect();

    Ok(sorted_peers)
}

/// A list of addresses of a peer in the routing table.
#[derive(Clone, Debug, Default)]
pub struct Addresses(pub Vec<Multiaddr>);

pub(crate) fn multiaddr_get_ip(addr: &Multiaddr) -> Option<std::net::IpAddr> {
    addr.iter().find_map(|p| match p {
        Protocol::Ip4(ip) => Some(std::net::IpAddr::V4(ip)),
        Protocol::Ip6(ip) => Some(std::net::IpAddr::V6(ip)),
        _ => None,
    })
}

pub(crate) fn multiaddr_get_port(addr: &Multiaddr) -> Option<u16> {
    addr.iter().find_map(|p| match p {
        Protocol::Udp(port) => Some(port),
        _ => None,
    })
}

/// Helper function to print formatted connection role info.
pub(crate) fn endpoint_str(endpoint: &libp2p::core::ConnectedPoint) -> String {
    match endpoint {
        libp2p::core::ConnectedPoint::Dialer { address, .. } => {
            format!("outgoing ({address})")
        }
        libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } => {
            format!("incoming ({send_back_addr})")
        }
    }
}

pub(crate) fn multiaddr_pop_p2p(addr: &mut Multiaddr) -> Option<PeerId> {
    if let Some(Protocol::P2p(peer_id)) = addr.iter().last() {
        let _ = addr.pop();
        Some(peer_id)
    } else {
        None
    }
}

pub(crate) fn multiaddr_get_p2p(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|p| match p {
        Protocol::P2p(peer_id) => Some(peer_id),
        _ => None,
    })
}

pub(crate) fn multiaddr_get_ip(addr: &Multiaddr) -> Option<std::net::IpAddr> {
    addr.iter().find_map(|p| match p {
        Protocol::Ip4(ip) => Some(std::net::IpAddr::V4(ip)),
        Protocol::Ip6(ip) => Some(std::net::IpAddr::V6(ip)),
        _ => None,
    })
}

pub(crate) fn multiaddr_get_socket_addr(addr: &Multiaddr) -> Option<std::net::SocketAddr> {
    let ip = multiaddr_get_ip(addr)?;
    let port = multiaddr_get_port(addr)?;
    Some(std::net::SocketAddr::new(ip, port))
}
