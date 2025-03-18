use crate::Client;
use ant_networking::version::Version;
use ant_networking::{Addresses, NetworkError};
use ant_protocol::NetworkAddress;
use libp2p::PeerId;

impl Client {
    /// Request the node version of a peer on the network.
    pub async fn get_node_version(&self, peer_id: PeerId) -> Result<Version, String> {
        self.network.get_node_version(peer_id).await
    }

    /// Retrieve the closest peers to the given network address.
    /// This function queries the network to find all peers in the close group nearest to the provided network address.
    pub async fn get_closest_to_address(
        &self,
        network_address: &NetworkAddress,
    ) -> Result<Vec<(PeerId, Addresses)>, NetworkError> {
        self.network
            .client_get_all_close_peers_in_range_or_close_group(network_address)
            .await
    }
}
