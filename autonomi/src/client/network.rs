// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use crate::networking::NetworkError;
use crate::networking::version::PackageVersion;
use ant_protocol::NetworkAddress;
use libp2p::PeerId;
use libp2p::kad::{PeerInfo, Quorum, Record};

impl Client {
    /// Retrieve the closest peers to the given network address.
    ///
    /// Optionally specify a count of peers to retrieve; if None, CLOSE_GROUP+2 peers will be returned.
    pub async fn get_closest_to_address(
        &self,
        network_address: impl Into<NetworkAddress>,
        count: Option<usize>,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        self.network
            .get_closest_peers_with_retries(network_address.into(), count)
            .await
    }

    /// Get a record directly from a specific peer.
    /// Returns:
    /// - Some(Record) if the peer holds the record
    /// - None if the peer doesn't hold the record or the request fails
    pub async fn get_record_from_peer(
        &self,
        network_address: impl Into<NetworkAddress>,
        peer: PeerInfo,
    ) -> Result<Option<Record>, NetworkError> {
        self.network
            .get_record_from_peer(network_address.into(), peer)
            .await
    }

    /// Get a record from the network and the list of peers holding it.
    /// Returns the record if successful along with the peers that handed it to us.
    /// If the record is not found, the result will be None and an empty list of peers.
    /// If the Quorum is not met, the result will be None and the list of peers that did manage to deliver the record.
    /// As soon as the quorum is met, the request will complete and the result will be returned.
    /// Note that the holders returned is not an exhaustive list of all holders of the record,
    /// it only contains the peers that responded to the request before the quorum was met.
    pub async fn get_record_and_holders(
        &self,
        network_address: impl Into<NetworkAddress>,
        quorum: Quorum,
    ) -> Result<(Option<Record>, Vec<PeerId>), NetworkError> {
        self.network
            .get_record_and_holders(network_address.into(), quorum)
            .await
    }

    /// Get storage proofs directly from a specific peer.
    /// Returns a vector of (NetworkAddress, ChunkProof) tuples
    pub async fn get_storage_proofs_from_peer(
        &self,
        network_address: impl Into<NetworkAddress>,
        peer: PeerInfo,
        nonce: u64,
        difficulty: usize,
    ) -> Result<
        Vec<(
            NetworkAddress,
            Result<ant_protocol::messages::ChunkProof, ant_protocol::error::Error>,
        )>,
        NetworkError,
    > {
        self.network
            .get_storage_proofs_from_peer(network_address.into(), peer, nonce, difficulty)
            .await
    }

    pub async fn get_node_version(&self, peer: PeerInfo) -> Result<PackageVersion, String> {
        self.network.get_node_version(peer).await
    }
}
