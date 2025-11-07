// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{Client, networking::NetworkError};
use ant_evm::{
    EvmWallet,
    merkle_payments::{
        CANDIDATES_PER_POOL, MerklePaymentCandidateNode, MerklePaymentCandidatePool,
        MerklePaymentProof, MerklePaymentVerificationError, MerkleTree, MidpointProof,
    },
};
use ant_protocol::{
    NetworkAddress,
    storage::{ChunkAddress, DataTypes},
};
use evmlib::merkle_batch_payment::PoolCommitment;
use futures::stream::FuturesUnordered;
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
    #[error("Candidate pool verification failed: {0}")]
    PoolVerification(#[from] MerklePaymentVerificationError),
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
    /// * `data_size` - The per-record data size (typically MAX_CHUNK_SIZE for chunks)
    /// * `merkle_payment_timestamp` - Unix timestamp for the payment
    ///
    /// # Returns
    /// * Vector of exactly 20 MerklePaymentCandidateNode with valid signatures
    async fn get_merkle_candidate_pool(
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

        if closest_peers.len() < CANDIDATES_PER_POOL {
            return Err(MerklePaymentError::InsufficientCandidates {
                got: closest_peers.len(),
                needed: CANDIDATES_PER_POOL,
            });
        }

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
    /// * `data_size` - The per-record data size (typically MAX_CHUNK_SIZE for chunks)
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

            // Validate signatures before accepting the pool
            pool.verify_signatures(timestamp)?;

            pools.push(pool);
        }

        Ok(pools)
    }

    /// Pay for a batch of data addresses using Merkle payment and get the proofs
    ///
    /// # Arguments
    /// * `data_type` - The data type (must be same for all items)
    /// * `content_addrs` - Iterator of XorName addresses
    /// * `data_size` - The per-record data size that nodes will store (typically MAX_CHUNK_SIZE for chunks)
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
    pub async fn pay_for_merkle_batch(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = XorName>,
        data_size: usize,
        wallet: &EvmWallet,
    ) -> Result<MerklePaymentReceipt, MerklePaymentError> {
        // Check if the wallet uses the same network as the client
        if wallet.network() != self.evm_network() {
            return Err(MerklePaymentError::EvmWalletNetworkMismatch);
        }

        // Collect addresses
        let addresses: Vec<XorName> = content_addrs.collect();
        info!(
            "Starting Merkle batch payment for {} addresses with data_type {data_type:?} and data_size {data_size}",
            addresses.len(),
        );

        // Phase 1: Build Merkle tree
        let tree = MerkleTree::from_xornames(addresses.clone())?;
        let depth = tree.depth();
        info!("Built Merkle tree: depth={depth}");

        // Phase 2: Get timestamp and reward candidates
        let merkle_payment_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let midpoint_proofs = tree.reward_candidates(merkle_payment_timestamp)?;
        info!("Generated {} candidate pools", midpoint_proofs.len());

        // Phase 3: Query network for candidate pools with signature validation
        let candidate_pools = self
            .build_candidate_pools(midpoint_proofs, data_type, data_size)
            .await?;
        info!(
            "Collected and validated all {} candidate pools",
            candidate_pools.len()
        );

        // Phase 4: Submit payment to smart contract
        let pool_commitments: Vec<PoolCommitment> = candidate_pools
            .iter()
            .map(|pool| pool.to_commitment())
            .collect();

        // Make sure nobody else can use the wallet while we are paying
        debug!("Waiting for wallet lock");
        let lock_guard = wallet.lock().await;
        debug!("Locked wallet");
        let winner_pool_hash =
            wallet.pay_for_merkle_tree(depth, pool_commitments, merkle_payment_timestamp)?;
        drop(lock_guard);
        debug!("Unlocked wallet");

        info!("Payment submitted, winner pool: {:?}", winner_pool_hash);

        // Phase 5: Generate proofs for all addresses
        let winner_pool = candidate_pools
            .into_iter()
            .find(|pool| pool.hash() == winner_pool_hash)
            .ok_or_else(|| {
                MerklePaymentError::SmartContract(format!(
                    "Smart contract returned invalid pool hash: {}",
                    hex::encode(winner_pool_hash)
                ))
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
}
