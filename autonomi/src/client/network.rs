// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::version::PackageVersion;
use crate::networking::NetworkError;
use crate::Client;
use ant_protocol::NetworkAddress;
use libp2p::kad::PeerInfo;

impl Client {
    /// Retrieve the closest peers to the given network address.
    pub async fn get_closest_to_address(
        &self,
        network_address: impl Into<NetworkAddress>,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        self.network
            .get_closest_peers_with_retries(network_address.into())
            .await
    }

    pub async fn get_node_version(&self, peer: PeerInfo) -> Result<PackageVersion, String> {
        self.network.get_node_version(peer).await
    }
}
