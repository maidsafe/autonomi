// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use crate::networking::version::PackageVersion;
use crate::networking::{NetworkError, PeerQuoteWithStorageProof};
use crate::utils::process_tasks_with_max_concurrency;
use ant_protocol::NetworkAddress;
use ant_protocol::storage::DataTypes;
use libp2p::PeerId;
use libp2p::kad::{PeerInfo, Quorum, Record};
use std::collections::HashSet;

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
    /// Returns an optional PaymentQuote and a vector of (NetworkAddress, ChunkProof) tuples
    pub async fn get_storage_proofs_from_peer(
        &self,
        network_address: impl Into<NetworkAddress>,
        peer: PeerInfo,
        nonce: u64,
        difficulty: usize,
        data_type: DataTypes,
        data_size: usize,
    ) -> Result<PeerQuoteWithStorageProof, NetworkError> {
        self.network
            .get_storage_proofs_from_peer(
                network_address.into(),
                peer,
                nonce,
                difficulty,
                data_type,
                data_size,
            )
            .await
    }

    pub async fn get_node_version(&self, peer: PeerInfo) -> Result<PackageVersion, String> {
        self.network.get_node_version(peer).await
    }

    /// Check which records already exist on the network (in parallel).
    ///
    /// This performs a fast existence check by querying a single node per address.
    /// For more reliable results, use a quorum-based check, but this is optimized
    /// for speed when checking many addresses (e.g., before batch uploads).
    ///
    /// # Arguments
    /// * `addresses` - Iterator of network addresses to check
    /// * `batch_size` - Maximum number of concurrent checks
    ///
    /// # Returns
    /// * `HashSet<NetworkAddress>` - Set of addresses that already exist on the network
    pub async fn check_records_exist_batch(
        &self,
        addresses: &[NetworkAddress],
        batch_size: usize,
    ) -> HashSet<NetworkAddress> {
        if addresses.is_empty() {
            return HashSet::new();
        }

        let total = addresses.len();
        let mut existing: HashSet<NetworkAddress> = HashSet::new();
        let mut checked = 0;

        // Process in batches to provide progress feedback
        for batch in addresses.chunks(batch_size) {
            let tasks: Vec<_> = batch
                .iter()
                .map(|addr| {
                    let network = self.network.clone();
                    async move {
                        // Use Quorum::One for fast existence check
                        match network.get_record(addr.clone(), Quorum::One).await {
                            Ok(Some(_)) => Some(addr),
                            Ok(None) => None,
                            // SplitRecord means record exists (just has conflicts)
                            Err(NetworkError::SplitRecord { .. }) => Some(addr),
                            Err(_) => None,
                        }
                    }
                })
                .collect();

            let results = process_tasks_with_max_concurrency(tasks, batch_size).await;
            existing.extend(results.into_iter().flatten().cloned());

            checked += batch.len();
            crate::loud_info!("Checked {checked}/{total} chunks for existence...");
        }

        existing
    }
}
