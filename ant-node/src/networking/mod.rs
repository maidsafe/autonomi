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
mod external_address;
mod interface;
mod log_markers;
#[cfg(feature = "open-metrics")]
mod metrics;
mod network;
mod reachability_check;
mod record_store;
mod relay_manager;
mod replication_fetcher;
mod transport;

// re-export arch dependent deps for use in the crate, or above
pub use self::interface::SwarmLocalState;
pub use self::reachability_check::{ReachabilityIssue, ReachabilityStatus};
pub(crate) use self::{
    error::NetworkError,
    interface::{NetworkEvent, NodeIssue},
    network::{Network, NetworkConfig, init_reachability_check_swarm},
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
use std::net::IpAddr;
use std::net::SocketAddr;

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

/// Verifies if `Multiaddr` contains IPv4 address that is not global.
/// This is used to filter out unroutable addresses from the Kademlia routing table.
pub(crate) fn multiaddr_is_global(multiaddr: &Multiaddr) -> bool {
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

/// Pop off the `/p2p/<peer_id>`. This mutates the `Multiaddr` and returns the `PeerId` if it exists.
pub(crate) fn multiaddr_pop_p2p(multiaddr: &mut Multiaddr) -> Option<PeerId> {
    if let Some(Protocol::P2p(peer_id)) = multiaddr.iter().last() {
        // Only actually strip the last protocol if it's indeed the peer ID.
        let _ = multiaddr.pop();
        Some(peer_id)
    } else {
        None
    }
}

/// Return the last `PeerId` from the `Multiaddr` if it exists.
pub(crate) fn multiaddr_get_p2p(multiaddr: &Multiaddr) -> Option<PeerId> {
    if let Some(Protocol::P2p(peer_id)) = multiaddr.iter().last() {
        Some(peer_id)
    } else {
        None
    }
}

/// Get the `IpAddr` from the `Multiaddr`
pub(crate) fn multiaddr_get_ip(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|p| match p {
        Protocol::Ip4(addr) => Some(IpAddr::V4(addr)),
        Protocol::Ip6(addr) => Some(IpAddr::V6(addr)),
        _ => None,
    })
}

pub(crate) fn multiaddr_get_port(addr: &Multiaddr) -> Option<u16> {
    addr.iter().find_map(|p| match p {
        Protocol::Udp(port) => Some(port),
        _ => None,
    })
}

/// Get the `SocketAddr` from the `Multiaddr`
pub(crate) fn multiaddr_get_socket_addr(addr: &Multiaddr) -> Option<SocketAddr> {
    let ip = multiaddr_get_ip(addr)?;
    let port = multiaddr_get_port(addr)?;
    Some(SocketAddr::new(ip, port))
}
