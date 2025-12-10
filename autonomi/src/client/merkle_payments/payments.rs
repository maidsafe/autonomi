// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{Client, networking::NetworkError};
use ant_evm::{
    AttoTokens, EvmWallet,
    merkle_payments::{
        CANDIDATES_PER_POOL, MAX_LEAVES, MerklePaymentCandidateNode, MerklePaymentCandidatePool,
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
use tracing::{debug, info};
use xor_name::XorName;

/// Contains the Merkle payment proofs for each XOR address and per-file chunk counts
/// This is the Merkle payment equivalent of [`Receipt`](crate::client::payment::Receipt)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MerklePaymentReceipt {
    /// Merkle payment proofs for each XOR address
    pub proofs: HashMap<XorName, MerklePaymentProof>,
    /// Chunk count for each file path
    pub file_chunk_counts: HashMap<String, usize>,
    /// Total amount paid for this Merkle batch
    pub amount_paid: AttoTokens,
}

impl Default for MerklePaymentReceipt {
    fn default() -> Self {
        Self {
            proofs: HashMap::new(),
            file_chunk_counts: HashMap::new(),
            amount_paid: AttoTokens::zero(),
        }
    }
}

impl MerklePaymentReceipt {
    /// Merge another receipt into this one
    pub fn merge(&mut self, other: Self) {
        self.proofs.extend(other.proofs);
        self.file_chunk_counts.extend(other.file_chunk_counts);
        self.amount_paid = AttoTokens::from_atto(
            self.amount_paid
                .as_atto()
                .saturating_add(other.amount_paid.as_atto()),
        );
    }
}

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
    /// This queries nodes close to the target address and collects signed [`MerklePaymentCandidateNode`]
    /// responses. To provide fault tolerance against unresponsive or malicious nodes, we request
    /// quotes from 25% more peers than needed and select the [`CANDIDATES_PER_POOL`] closest successful responses.
    ///
    /// # Arguments
    /// * `target_address` - The address to find candidates for (from MidpointProof::address())
    /// * `data_type` - The data type being uploaded (must be same for all data in batch)
    /// * `data_size` - The per-record data size (typically MAX_CHUNK_SIZE for chunks)
    /// * `merkle_payment_timestamp` - Unix timestamp for the payment
    ///
    /// # Returns
    /// * Array of exactly [`CANDIDATES_PER_POOL`] MerklePaymentCandidateNode with valid signatures,
    ///   selected from the closest successful responses
    async fn get_merkle_candidate_pool(
        &self,
        target_address: XorName,
        data_type: DataTypes,
        data_size: usize,
        merkle_payment_timestamp: u64,
    ) -> Result<[MerklePaymentCandidateNode; CANDIDATES_PER_POOL], MerklePaymentError> {
        // Request from 25% more peers than needed to provide fault tolerance
        // This allows up to 25% of peers to fail without blocking the payment
        const PEERS_TO_QUERY: usize = CANDIDATES_PER_POOL + (CANDIDATES_PER_POOL / 4);

        let network_addr = NetworkAddress::ChunkAddress(ChunkAddress::new(target_address));
        let closest_peers = self
            .network
            .get_closest_peers_with_retries(network_addr.clone(), Some(PEERS_TO_QUERY))
            .await?;

        // Deduplicate peers by peer_id using HashMap (prevents duplicate candidates)
        let unique_peers: HashMap<libp2p::PeerId, libp2p::kad::PeerInfo> = closest_peers
            .into_iter()
            .map(|peer_info| (peer_info.peer_id, peer_info))
            .collect();

        if unique_peers.len() < CANDIDATES_PER_POOL {
            return Err(MerklePaymentError::InsufficientCandidates {
                got: unique_peers.len(),
                needed: CANDIDATES_PER_POOL,
            });
        }

        // Store peer infos with their distance to target for later sorting
        let peer_info_with_distances: Vec<_> = unique_peers
            .values()
            .map(|peer_info| {
                let peer_addr = NetworkAddress::from(peer_info.peer_id);
                let distance = network_addr.distance(&peer_addr);
                (peer_info.clone(), distance)
            })
            .collect();

        // Request quotes from all peers in parallel
        let mut tasks = FuturesUnordered::new();
        for (peer_info, _distance) in &peer_info_with_distances {
            let network = self.network.clone();
            let network_addr = network_addr.clone();
            let data_type_index = data_type.get_index();
            let peer_info = peer_info.clone();
            let peer_id = peer_info.peer_id;
            tasks.push(async move {
                let result = network
                    .get_merkle_candidate_quote(
                        network_addr,
                        peer_info,
                        data_type_index,
                        data_size,
                        merkle_payment_timestamp,
                    )
                    .await;
                (peer_id, result)
            });
        }

        // Collect successful responses (tolerate failures)
        let mut successful_candidates: Vec<(libp2p::PeerId, MerklePaymentCandidateNode)> =
            Vec::new();
        use futures::StreamExt;
        while let Some((peer_id, result)) = tasks.next().await {
            match result {
                Ok(candidate) => {
                    successful_candidates.push((peer_id, candidate));
                }
                Err(e) => {
                    warn!(
                        "Failed to get quote from peer {peer_id:?} for target {target_address:?}: {e}"
                    );
                    // Continue to next peer instead of failing entire payment
                }
            }
        }

        debug!(
            "Got {} successful responses out of {} queried peers for target {target_address:?}",
            successful_candidates.len(),
            peer_info_with_distances.len(),
        );

        // Check if we have enough successful responses
        if successful_candidates.len() < CANDIDATES_PER_POOL {
            return Err(MerklePaymentError::InsufficientCandidates {
                got: successful_candidates.len(),
                needed: CANDIDATES_PER_POOL,
            });
        }

        // Sort successful candidates by distance to target and take the 20 closest
        successful_candidates.sort_by_key(|(peer_id, _candidate)| {
            let peer_addr = NetworkAddress::from(*peer_id);
            network_addr.distance(&peer_addr)
        });

        // Take the CANDIDATES_PER_POOL closest successful responses
        let closest_successful: Vec<MerklePaymentCandidateNode> = successful_candidates
            .into_iter()
            .take(CANDIDATES_PER_POOL)
            .map(|(_peer_id, candidate)| candidate)
            .collect();

        // Convert to exact-sized array
        let candidates_array: [MerklePaymentCandidateNode; CANDIDATES_PER_POOL] =
            closest_successful.try_into().map_err(|v: Vec<_>| {
                MerklePaymentError::InsufficientCandidates {
                    got: v.len(),
                    needed: CANDIDATES_PER_POOL,
                }
            })?;

        Ok(candidates_array)
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
    pub(crate) async fn build_candidate_pools(
        &self,
        midpoint_proofs: Vec<MidpointProof>,
        data_type: DataTypes,
        data_size: usize,
    ) -> Result<Vec<MerklePaymentCandidatePool>, MerklePaymentError> {
        let mut pools = Vec::new();

        for midpoint_proof in midpoint_proofs {
            let target = midpoint_proof.address();
            let timestamp = midpoint_proof.merkle_payment_timestamp;

            // Get candidates for this pool (returns exact-sized array)
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
    /// Automatically splits large batches (>4096 addresses) into multiple Merkle trees.
    ///
    /// # Arguments
    /// * `data_type` - The data type (must be same for all items)
    /// * `content_addrs` - Iterator of XorName addresses
    /// * `data_size` - The per-record data size that nodes will store (typically MAX_CHUNK_SIZE for chunks)
    /// * `wallet` - The EVM wallet to pay with
    ///
    /// # Returns
    /// * `MerklePaymentReceipt` - HashMap mapping each address to its MerklePaymentProof
    pub async fn pay_for_merkle_batch(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = XorName>,
        data_size: usize,
        wallet: &EvmWallet,
    ) -> Result<MerklePaymentReceipt, MerklePaymentError> {
        if wallet.network() != self.evm_network() {
            return Err(MerklePaymentError::EvmWalletNetworkMismatch);
        }

        let addresses: Vec<XorName> = content_addrs.collect();
        let batches: Vec<Vec<XorName>> = addresses.chunks(MAX_LEAVES).map(|c| c.to_vec()).collect();
        let batches_len = batches.len();
        let addresses_len = addresses.len();
        #[cfg(feature = "loud")]
        println!("Paying for {addresses_len} addresses in {batches_len} batch(es)");
        info!("Paying for {addresses_len} addresses in {batches_len} batch(es)");

        let mut merged_receipt = MerklePaymentReceipt::default();
        for (i, batch) in batches.into_iter().enumerate() {
            #[cfg(feature = "loud")]
            println!("Processing batch {}/{batches_len}", i + 1);
            info!("Processing batch {}/{batches_len}", i + 1);
            let receipt = self
                .pay_for_single_merkle_batch(data_type, batch, data_size, wallet)
                .await?;
            merged_receipt.merge(receipt);
        }

        Ok(merged_receipt)
    }

    /// Prepare a Merkle batch - builds tree, queries candidate pools
    /// Returns (tree, candidate_pools, pool_commitments, timestamp)
    pub(crate) async fn prepare_merkle_batch(
        &self,
        data_type: DataTypes,
        addresses: Vec<XorName>,
        data_size: usize,
    ) -> Result<
        (
            MerkleTree,
            Vec<MerklePaymentCandidatePool>,
            Vec<PoolCommitment>,
            u64,
        ),
        MerklePaymentError,
    > {
        info!(
            "Preparing Merkle batch for {} addresses with data_type {data_type:?}",
            addresses.len()
        );

        // Build Merkle tree
        let tree = MerkleTree::from_xornames(addresses)?;
        let depth = tree.depth();
        info!("Built Merkle tree: depth={depth}");

        // Get timestamp and reward candidates
        let merkle_payment_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let midpoint_proofs = tree.reward_candidates(merkle_payment_timestamp)?;
        info!("Generated {} midpoint proofs", midpoint_proofs.len());

        // Query network for candidate pools with signature validation
        let candidate_pools = self
            .build_candidate_pools(midpoint_proofs, data_type, data_size)
            .await?;
        info!(
            "Collected and validated all {} candidate pools",
            candidate_pools.len()
        );

        // Convert to pool commitments
        let pool_commitments: Vec<PoolCommitment> = candidate_pools
            .iter()
            .map(|pool| pool.to_commitment())
            .collect();

        Ok((
            tree,
            candidate_pools,
            pool_commitments,
            merkle_payment_timestamp,
        ))
    }

    /// Pay for a single batch of up to MAX_LEAVES addresses
    pub(crate) async fn pay_for_single_merkle_batch(
        &self,
        data_type: DataTypes,
        addresses: Vec<XorName>,
        data_size: usize,
        wallet: &EvmWallet,
    ) -> Result<MerklePaymentReceipt, MerklePaymentError> {
        // Prepare the batch (build tree, query pools)
        let (tree, candidate_pools, pool_commitments, merkle_payment_timestamp) = self
            .prepare_merkle_batch(data_type, addresses.clone(), data_size)
            .await?;
        let depth = tree.depth();

        // Submit payment to smart contract
        debug!("Waiting for wallet lock");
        let lock_guard = wallet.lock().await;
        debug!("Locked wallet");
        let (winner_pool_hash, amount) = wallet
            .pay_for_merkle_tree(depth, pool_commitments, merkle_payment_timestamp)
            .await?;
        let amount = AttoTokens::from_atto(amount);
        drop(lock_guard);
        debug!("Unlocked wallet");

        info!("Payment submitted, winner pool: {winner_pool_hash:?}, amount: {amount}");

        // Find winner pool and generate proofs
        let winner_pool = candidate_pools
            .into_iter()
            .find(|pool| pool.hash() == winner_pool_hash)
            .ok_or_else(|| {
                MerklePaymentError::SmartContract(format!(
                    "Smart contract returned invalid pool hash: {}",
                    hex::encode(winner_pool_hash)
                ))
            })?;

        let mut proofs = HashMap::new();
        for (i, address) in addresses.into_iter().enumerate() {
            let address_proof = tree.generate_address_proof(i, address)?;
            let payment_proof = MerklePaymentProof {
                address,
                data_proof: address_proof,
                winner_pool: winner_pool.clone(),
            };
            proofs.insert(address, payment_proof);
        }

        let receipt = MerklePaymentReceipt {
            proofs,
            file_chunk_counts: HashMap::new(),
            amount_paid: amount,
        };

        info!(
            "Generated {} Merkle payment proofs, total amount: {amount}",
            receipt.proofs.len()
        );
        Ok(receipt)
    }
}
