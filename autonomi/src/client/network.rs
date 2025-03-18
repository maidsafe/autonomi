use crate::Client;
use ant_networking::version::Version;
use libp2p::PeerId;

impl Client {
    /// Request the node version of a peer on the network.
    pub async fn get_node_version(&self, peer_id: PeerId) -> Result<Version, String> {
        self.network.get_node_version(peer_id).await
    }
}
