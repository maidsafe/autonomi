// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Merkle batch payment types and disk-based mock smart contract
//!
//! This module contains the minimal types needed for Merkle batch payments and a disk-based
//! mock implementation of the smart contract. When the real smart contract is ready, the
//! disk contract will be replaced with actual on-chain calls.

use crate::common::Address as RewardsAddress;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Pool hash type (32 bytes) - compatible with XorName without the dependency
pub type PoolHash = [u8; 32];

/// Number of candidate nodes per pool (provides redundancy)
pub const CANDIDATES_PER_POOL: usize = 20;

/// Maximum supported Merkle tree depth
pub const MAX_MERKLE_DEPTH: u8 = 16;

/// Calculate expected number of reward pools for a given tree depth
///
/// Formula: 2^ceil(depth/2)
pub fn expected_reward_pools(depth: u8) -> usize {
    let half_depth = depth.div_ceil(2);
    1 << half_depth
}

/// Minimal pool commitment for smart contract submission
///
/// Contains only what's needed on-chain, with cryptographic commitment to full off-chain data.
/// This is sent to the smart contract as part of the batch payment transaction.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PoolCommitment {
    /// Hash of the full MerklePaymentCandidatePool (cryptographic commitment)
    /// This commits to the midpoint proof and all node signatures
    pub pool_hash: PoolHash,

    /// Reward addresses of candidate nodes (20 nodes per pool)
    /// Smart contract selects winners from these addresses
    pub candidate_addresses: Vec<RewardsAddress>,
}

/// Errors that can occur during smart contract operations
#[derive(Debug, Error)]
pub enum SmartContractError {
    #[error("Wrong number of candidate nodes: expected {expected}, got {got}")]
    WrongCandidateCount { expected: usize, got: usize },

    #[error("Wrong number of candidate pools: expected {expected}, got {got}")]
    WrongPoolCount { expected: usize, got: usize },

    #[error("Depth {depth} exceeds maximum supported depth {max}")]
    DepthTooLarge { depth: u8, max: u8 },

    #[error("Payment not found for winner pool hash: {0}")]
    PaymentNotFound(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// What's stored on-chain (or disk) - indexed by winner_pool_hash
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnChainPaymentInfo {
    /// Tree depth
    pub depth: u8,

    /// Merkle payment timestamp provided by client (unix seconds)
    /// This is the timestamp that all nodes in the pool used for their quotes
    pub merkle_payment_timestamp: u64,

    /// Addresses of the 'depth' nodes that were paid along with their indices in the winner pool
    pub paid_node_addresses: Vec<(RewardsAddress, usize)>,
}

/// Disk-based Merkle payment contract (temporary implementation)
///
/// This simulates smart contract behavior by storing payment data to disk.
/// Replace this entire implementation with a real smart contract when ready.
pub struct DiskMerklePaymentContract {
    storage_path: PathBuf, // ~/.autonomi/merkle_payments/
}

impl DiskMerklePaymentContract {
    /// Create a new contract with a specific storage path
    pub fn new_with_path(storage_path: PathBuf) -> Result<Self, SmartContractError> {
        std::fs::create_dir_all(&storage_path)?;
        Ok(Self { storage_path })
    }

    /// Create a new contract with the default storage path
    /// Uses: DATA_DIR/autonomi/merkle_payments/
    pub fn new() -> Result<Self, SmartContractError> {
        let storage_path = if let Some(data_dir) = dirs_next::data_dir() {
            data_dir.join("autonomi").join("merkle_payments")
        } else {
            // Fallback to current directory if data_dir is not available
            PathBuf::from(".autonomi").join("merkle_payments")
        };
        Self::new_with_path(storage_path)
    }

    /// Submit batch payment (simulates smart contract logic)
    ///
    /// # Arguments
    /// * `depth` - Tree depth
    /// * `pool_commitments` - Minimal pool commitments (2^ceil(depth/2) pools with hashes + addresses)
    /// * `merkle_payment_timestamp` - Client-defined timestamp committed to by all nodes in their quotes
    ///
    /// # Returns
    /// * `winner_pool_hash` - Hash of winner pool (storage key for verification)
    pub fn pay_for_merkle_tree(
        &self,
        depth: u8,
        pool_commitments: Vec<PoolCommitment>,
        merkle_payment_timestamp: u64,
    ) -> Result<PoolHash, SmartContractError> {
        // Validate: depth is within supported range
        if depth > MAX_MERKLE_DEPTH {
            return Err(SmartContractError::DepthTooLarge {
                depth,
                max: MAX_MERKLE_DEPTH,
            });
        }

        // Validate: correct number of pools (2^ceil(depth/2))
        let expected_pools = expected_reward_pools(depth);
        if pool_commitments.len() != expected_pools {
            return Err(SmartContractError::WrongPoolCount {
                expected: expected_pools,
                got: pool_commitments.len(),
            });
        }

        // Validate: each pool has exactly CANDIDATES_PER_POOL addresses
        for pool in &pool_commitments {
            if pool.candidate_addresses.len() != CANDIDATES_PER_POOL {
                return Err(SmartContractError::WrongCandidateCount {
                    expected: CANDIDATES_PER_POOL,
                    got: pool.candidate_addresses.len(),
                });
            }
        }

        // Select winner pool using random selection
        let winner_pool_idx = rand::random::<usize>() % pool_commitments.len();

        let winner_pool = &pool_commitments[winner_pool_idx];
        let winner_pool_hash = winner_pool.pool_hash;

        println!("\n=== MERKLE BATCH PAYMENT ===");
        println!("Depth: {depth}");
        println!("Total pools: {}", pool_commitments.len());
        println!("Nodes per pool: {CANDIDATES_PER_POOL}");
        println!("Winner pool index: {winner_pool_idx}");
        println!("Winner pool hash: {}", hex::encode(winner_pool_hash));

        // Select 'depth' unique winner nodes within the winner pool
        use std::collections::HashSet;
        let mut winner_node_indices = HashSet::new();
        while winner_node_indices.len() < depth as usize {
            let idx = rand::random::<usize>() % winner_pool.candidate_addresses.len();
            winner_node_indices.insert(idx);
        }
        let winner_node_indices: Vec<usize> = winner_node_indices.into_iter().collect();

        println!(
            "\nSelected {} winner nodes from pool:",
            winner_node_indices.len()
        );

        // Extract paid node addresses, along with their indices
        let mut paid_node_addresses = Vec::new();
        for (i, &node_idx) in winner_node_indices.iter().enumerate() {
            let addr = winner_pool.candidate_addresses[node_idx];
            paid_node_addresses.push((addr, node_idx));
            println!("  Node {}: {addr}", i + 1);
        }

        println!(
            "\nSimulating payment to {} nodes...",
            paid_node_addresses.len()
        );
        println!("=========================\n");

        // Store payment info on 'blockchain' (indexed by winner_pool_hash)
        let info = OnChainPaymentInfo {
            depth,
            merkle_payment_timestamp,
            paid_node_addresses,
        };

        let file_path = self
            .storage_path
            .join(format!("{}.json", hex::encode(winner_pool_hash)));
        let json = serde_json::to_string_pretty(&info)?;
        std::fs::write(&file_path, json)?;

        println!("✓ Stored payment info to: {}", file_path.display());

        Ok(winner_pool_hash)
    }

    /// Get payment info by winner pool hash
    pub fn get_payment_info(
        &self,
        winner_pool_hash: PoolHash,
    ) -> Result<OnChainPaymentInfo, SmartContractError> {
        let file_path = self
            .storage_path
            .join(format!("{}.json", hex::encode(winner_pool_hash)));
        let json = std::fs::read_to_string(&file_path)
            .map_err(|_| SmartContractError::PaymentNotFound(hex::encode(winner_pool_hash)))?;
        let info = serde_json::from_str(&json)?;
        Ok(info)
    }
}
