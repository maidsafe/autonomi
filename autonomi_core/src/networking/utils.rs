use crate::Multiaddr;
use ant_protocol::CLOSE_GROUP_SIZE;
use libp2p::kad::Quorum;
use libp2p::multiaddr::Protocol;
// @anselme: this is a duplicate function from ant_networking, wasn't sure where to place it

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

/// Majority of a given group (i.e. > 1/2).
pub const fn close_group_majority() -> usize {
    // Calculate the majority of the close group size by dividing it by 2 and adding 1.
    // This ensures that the majority is always greater than half.
    CLOSE_GROUP_SIZE / 2 + 1
}

/// Get the value of the provided `Quorum` as usize.
pub fn get_quorum_amount(quorum: &Quorum) -> usize {
    match quorum {
        Quorum::Majority => close_group_majority(),
        Quorum::All => CLOSE_GROUP_SIZE,
        Quorum::N(v) => v.get(),
        Quorum::One => 1,
    }
}
