// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::Client;
use crate::{client::config::CHUNK_UPLOAD_BATCH_SIZE, networking::NetworkError};
use ant_evm::{
    merkle_payments::{
        MerklePaymentCandidateNode, MerklePaymentCandidatePool, MerklePaymentProof, MerkleTree,
        MidpointProof, CANDIDATES_PER_POOL,
    },
    EvmWallet,
};
use evmlib::merkle_batch_payment::PoolCommitment;
use ant_protocol::{
    storage::{Chunk, ChunkAddress, DataTypes},
    NetworkAddress,
};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::kad::Record;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use xor_name::XorName;

/// Contains the Merkle payment proofs for each XOR address
/// This is the Merkle payment equivalent of [`Receipt`](crate::client::payment::Receipt)
pub type MerklePaymentReceipt = HashMap<XorName, MerklePaymentProof>;

/// Errors that can occur during Merkle batch payment operations
#[derive(Debug, thiserror::Error)]
pub enum MerklePaymentError {
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    #[error("Merkle tree error: {0}")]
    MerkleTree(#[from] ant_evm::merkle_payments::MerkleTreeError),
    #[error("Not enough valid candidate responses: got {got}, needed {needed}")]
    InsufficientCandidates { got: usize, needed: usize },
    #[error("Data type/size mismatch in batch")]
    DataMismatch,
    #[error("Failed to serialize: {0}")]
    Serialization(String),
    #[error("Smart contract error: {0}")]
    SmartContract(String),
    #[error(
        "EVM wallet and client use different EVM networks. Please use the same network for both."
    )]
    EvmWalletNetworkMismatch,
    #[error("Wallet error: {0:?}")]
    EvmWalletError(#[from] ant_evm::EvmWalletError),
    #[error("Failed to get timestamp: {0}")]
    TimestampError(#[from] std::time::SystemTimeError),
}

impl Client {
    /// Get Merkle candidate nodes for a specific target address
    ///
    /// This queries the 20 closest nodes to the target address and requests signed [`MerklePaymentCandidateNode`]
    /// that commits to a `data_type`, `data_size` and `merkle_payment_timestamp` together with the node's reward address.
    ///
    /// # Arguments
    /// * `target_address` - The address to find candidates for (from MidpointProof::address())
    /// * `data_type` - The data type being uploaded (must be same for all data in batch)
    /// * `data_size` - The data size (must be same for all data in batch)
    /// * `merkle_payment_timestamp` - Unix timestamp for the payment
    ///
    /// # Returns
    /// * Vector of exactly 20 MerklePaymentCandidateNode with valid signatures
    pub async fn get_merkle_candidate_pool(
        &self,
        target_address: XorName,
        data_type: DataTypes,
        data_size: usize,
        merkle_payment_timestamp: u64,
    ) -> Result<Vec<MerklePaymentCandidateNode>, MerklePaymentError> {
        // Get 20 closest peers to target
        let network_addr = NetworkAddress::ChunkAddress(ChunkAddress::new(target_address));
        let closest_peers = self
            .network
            .get_closest_peers(network_addr.clone(), Some(CANDIDATES_PER_POOL))
            .await?;

        debug!(
            "Got {} closest peers for target {:?}",
            closest_peers.len(),
            target_address
        );

        // Request quotes from all peers in parallel
        let mut tasks = FuturesUnordered::new();
        for peer in closest_peers {
            let network = self.network.clone();
            let network_addr = network_addr.clone();
            let data_type_index = data_type.get_index();
            tasks.push(async move {
                network
                    .get_merkle_candidate_quote(
                        network_addr,
                        peer,
                        data_type_index,
                        data_size,
                        merkle_payment_timestamp,
                    )
                    .await
            });
        }

        // Collect all candidates - error out immediately if ANY request fails
        let mut candidates = Vec::new();
        use futures::StreamExt;
        while let Some(result) = tasks.next().await {
            candidates.push(result?);
        }

        Ok(candidates)
    }

    /// Build candidate pools for all midpoint proofs
    ///
    /// # Arguments
    /// * `midpoint_proofs` - The midpoint proofs from the Merkle tree
    /// * `data_type` - Data type for all items in batch
    /// * `data_size` - Data size for all items in batch
    ///
    /// # Returns
    /// * Vector of MerklePaymentCandidatePool, one for each midpoint
    async fn build_candidate_pools(
        &self,
        midpoint_proofs: Vec<MidpointProof>,
        data_type: DataTypes,
        data_size: usize,
    ) -> Result<Vec<MerklePaymentCandidatePool>, MerklePaymentError> {
        let mut pools = Vec::new();

        for midpoint_proof in midpoint_proofs {
            let target = midpoint_proof.address();
            let timestamp = midpoint_proof.merkle_payment_timestamp;

            // Get candidates for this pool
            let candidate_nodes = self
                .get_merkle_candidate_pool(target, data_type, data_size, timestamp)
                .await?;

            let pool = MerklePaymentCandidatePool {
                midpoint_proof,
                candidate_nodes,
            };

            pools.push(pool);
        }

        Ok(pools)
    }

    /// Pay for a batch of data addresses using Merkle payment and get the proofs
    ///
    /// This is the Merkle payment equivalent of [`Client::pay`](crate::client::payment::Client::pay)
    ///
    /// # Arguments
    /// * `data_type` - The data type (must be same for all items)
    /// * `content_addrs` - Iterator of (XorName, data_size) pairs
    /// * `wallet` - The EVM wallet to pay with
    ///
    /// # Returns
    /// * `MerklePaymentReceipt` - HashMap mapping each address to its MerklePaymentProof
    ///
    /// # Process
    /// 1. Build Merkle tree from addresses
    /// 2. Query candidate pools from network (one per midpoint)
    /// 3. Submit payment to smart contract
    /// 4. Generate and return proofs for each address
    pub(crate) async fn pay_for_merkle_batch(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
        wallet: &EvmWallet,
    ) -> Result<MerklePaymentReceipt, MerklePaymentError> {
        // Check if the wallet uses the same network as the client
        if wallet.network() != self.evm_network() {
            return Err(MerklePaymentError::EvmWalletNetworkMismatch);
        }

        // Collect addresses and calculate total data size
        let items: Vec<(XorName, usize)> = content_addrs.collect();
        let total_data_size = items.iter().map(|(_, size)| size).sum::<usize>();
        let addresses: Vec<XorName> = items.iter().map(|(addr, _)| *addr).collect();
        info!(
            "Starting Merkle batch payment for {} addresses with data_type {data_type:?} and data_size {total_data_size}",
            addresses.len(),
        );

        // Phase 1: Build Merkle tree
        let tree = MerkleTree::from_xornames(addresses.clone())?;
        let depth = tree.depth();
        info!("Built Merkle tree: depth={depth}");

        // Phase 2: Get timestamp and reward candidates
        let merkle_payment_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        let midpoint_proofs = tree.reward_candidates(merkle_payment_timestamp)?;
        info!("Generated {} candidate pools", midpoint_proofs.len());

        // Phase 3: Query network for candidate pools
        let candidate_pools = self
            .build_candidate_pools(midpoint_proofs, data_type, total_data_size)
            .await?;
        info!("Collected all {} candidate pools", candidate_pools.len());

        // Phase 4: Submit payment to smart contract
        let pool_commitments: Vec<PoolCommitment> = candidate_pools
            .iter()
            .map(|pool| pool.to_commitment())
            .collect();
        
        // Make sure nobody else can use the wallet while we are paying
        debug!("Waiting for wallet lock");
        let lock_guard = wallet.lock().await;
        debug!("Locked wallet");
        let winner_pool_hash = wallet
            .pay_for_merkle_tree(
                depth,
                pool_commitments,
                merkle_payment_timestamp,
            )
            .await?;
        drop(lock_guard);
        debug!("Unlocked wallet");

        info!("Payment submitted, winner pool: {:?}", winner_pool_hash);

        // Phase 5: Generate proofs for all addresses
        let winner_pool = candidate_pools
            .into_iter()
            .find(|pool| pool.hash() == winner_pool_hash)
            .ok_or_else(|| {
                MerklePaymentError::SmartContract(format!("Smart contract returned invalid pool hash: {}", hex::encode(winner_pool_hash)))
            })?;

        let mut receipt = MerklePaymentReceipt::new();
        for (i, address) in addresses.into_iter().enumerate() {
            // Generate address proof
            let address_proof = tree.generate_address_proof(i, address)?;

            // Create payment proof
            let payment_proof = MerklePaymentProof {
                address,
                data_proof: address_proof,
                winner_pool: winner_pool.clone(),
            };

            receipt.insert(address, payment_proof);
        }

        info!("Generated {} Merkle payment proofs", receipt.len());
        Ok(receipt)
    }

    /// Upload a single chunk with its Merkle payment proof
    ///
    /// # Arguments
    /// * `chunk` - The chunk to upload
    /// * `proof` - The Merkle payment proof for this chunk
    ///
    /// # Returns
    /// * ChunkAddress on success
    async fn upload_chunk_with_merkle_proof(
        &self,
        chunk: &Chunk,
        proof: &MerklePaymentProof,
    ) -> Result<ChunkAddress, MerklePaymentError> {
        use ant_protocol::storage::{try_serialize_record, RecordKind};

        let address = *chunk.address();

        // Data is uploaded to nodes closest to the chunk address (normal DHT location)
        let network_addr = NetworkAddress::from(address);
        let storing_nodes = self
            .network
            .get_closest_peers(network_addr.clone(), Some(CANDIDATES_PER_POOL))
            .await?;

        if storing_nodes.is_empty() {
            return Err(MerklePaymentError::SmartContract(
                "No storing nodes found for chunk address".to_string(),
            ));
        }

        debug!("Storing chunk: {chunk:?} to {:?}", storing_nodes);

        let key = network_addr.to_record_key();
        let record_kind = RecordKind::DataWithMerklePayment(DataTypes::Chunk);

        let record = Record {
            key: key.clone(),
            value: try_serialize_record(&(proof.clone(), chunk.clone()), record_kind)
                .map_err(|e| {
                    MerklePaymentError::Serialization(format!(
                        "Failed to serialize chunk with Merkle proof: {e:?}"
                    ))
                })?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        self.network
            .put_record_with_retries(record, storing_nodes.clone(), &self.config.chunks)
            .await?;

        debug!("Successfully stored chunk: {chunk:?} to {storing_nodes:?}");
        Ok(address)
    }

    /// Upload a batch of chunks using Merkle payment proofs
    ///
    /// This is the Merkle payment equivalent of [`Client::chunk_batch_upload`]
    ///
    /// # Arguments
    /// * `chunks` - Iterator of chunks to upload
    /// * `receipt` - Merkle payment receipt containing proofs for each chunk
    ///
    /// # Returns
    /// * `Ok(())` on success
    /// * Error if any upload fails
    ///
    /// # How it works
    /// - Payment goes to nodes closest to the winner pool address (the paid nodes)
    /// - Data is uploaded to nodes closest to the chunk address (normal DHT location)
    /// - Processes chunks in batches of [`CHUNK_UPLOAD_BATCH_SIZE`] to keep memory usage reasonable
    pub async fn chunk_batch_upload_with_merkle_proofs(
        &self,
        chunks: impl Iterator<Item = &ant_protocol::storage::Chunk>,
        receipt: &MerklePaymentReceipt,
    ) -> Result<(), MerklePaymentError> {
        let chunks: Vec<_> = chunks.collect();
        let total_chunks = chunks.len();
        let max_concurrent = *CHUNK_UPLOAD_BATCH_SIZE;

        info!("Uploading {total_chunks} chunks with Merkle proofs (max {max_concurrent} concurrent)");

        let mut tasks = FuturesUnordered::new();
        let mut chunks_iter = chunks.into_iter().enumerate();
        let mut completed = 0;

        // Helper to start an upload task for a chunk
        let start_upload = |_idx: usize, chunk: &Chunk| -> Option<_> {
            let xor_name = *chunk.name();
            let proof = receipt.get(&xor_name)?;

            let self_clone = self.clone();
            let chunk_clone = chunk.clone();
            let proof_clone = proof.clone();

            Some(async move {
                self_clone
                    .upload_chunk_with_merkle_proof(&chunk_clone, &proof_clone)
                    .await
            })
        };

        // Fill up the initial batch with max_concurrent uploads
        for (idx, chunk) in chunks_iter.by_ref().take(max_concurrent) {
            if let Some(task) = start_upload(idx, chunk) {
                tasks.push(task);
            } else {
                error!(
                    "({}/{total_chunks}) Chunk at {:?} has no payment proof, skipping",
                    idx + 1,
                    chunk.address()
                );
            }
        }

        // Process uploads with sliding window: as soon as one completes, start the next
        while let Some(result) = tasks.next().await {
            result?;
            completed += 1;
            debug!("Progress: {completed}/{total_chunks} chunks uploaded");

            // Start the next upload to maintain max_concurrent active tasks
            if let Some((idx, chunk)) = chunks_iter.next() {
                if let Some(task) = start_upload(idx, chunk) {
                    tasks.push(task);
                } else {
                    error!(
                        "({}/{total_chunks}) Chunk at {:?} has no payment proof, skipping",
                        idx + 1,
                        chunk.address()
                    );
                }
            }
        }

        info!("Successfully uploaded all {completed} chunks");
        Ok(())
    }
}
