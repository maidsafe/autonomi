// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use ant_networking::{Addresses, NetworkError};
use ant_protocol::NetworkAddress;
use libp2p::PeerId;

impl Client {
    /// Retrieve the closest peers to the given network address.
    /// This function queries the network to find all peers in the close group nearest to the provided network address.
    pub async fn get_closest_to_address(
        &self,
        network_address: impl Into<NetworkAddress>,
    ) -> Result<Vec<(PeerId, Addresses)>, NetworkError> {
        self.network
            .client_get_all_close_peers_in_range_or_close_group(&network_address.into())
            .await
    }
}
