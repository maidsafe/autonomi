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

use crate::common::{Address as RewardsAddress, U256};
use crate::contract::data_type_conversion;
use crate::quoting_metrics::QuotingMetrics;
use serde::{Deserialize, Serialize};

#[cfg(any(test, feature = "test-utils"))]
use crate::common::Amount;

#[cfg(any(test, feature = "test-utils"))]
use std::path::PathBuf;

#[cfg(any(test, feature = "test-utils"))]
use thiserror::Error;

/// Error returned when `total_cost_unit` exceeds the 248-bit limit during packing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostUnitOverflow;

impl std::fmt::Display for CostUnitOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "total_cost_unit exceeds {TOTAL_COST_UNIT_BITS}-bit limit (top 8 bits reserved for packing)"
        )
    }
}

impl std::error::Error for CostUnitOverflow {}

/// Pool hash type (32 bytes) - compatible with XorName without the dependency
pub type PoolHash = [u8; 32];

/// Number of candidate nodes per pool (provides redundancy)
pub const CANDIDATES_PER_POOL: usize = 16;

/// Maximum supported Merkle tree depth
pub const MAX_MERKLE_DEPTH: u8 = 8;

/// Number of bits available for total_cost_unit when packed with data_type (u8 = 8 bits)
const TOTAL_COST_UNIT_BITS: usize = 248;

/// Cost unit weights per data type, matching the production contract's `costUnitPerDataType` mapping.
/// These weights determine the relative storage cost of each data type.
const COST_UNIT_GRAPH_ENTRY: u64 = 1;
const COST_UNIT_SCRATCHPAD: u64 = 100;
const COST_UNIT_CHUNK: u64 = 10;
const COST_UNIT_POINTER: u64 = 20;

/// Get the cost unit for a Solidity DataType index.
///
/// Matches the contract's `costUnitPerDataType` mapping:
///   GraphEntry(0) = 1, Scratchpad(1) = 100, Chunk(2) = 10, Pointer(3) = 20
fn cost_unit_for_data_type(solidity_data_type: u8) -> U256 {
    match solidity_data_type {
        0 => U256::from(COST_UNIT_GRAPH_ENTRY),
        1 => U256::from(COST_UNIT_SCRATCHPAD),
        2 => U256::from(COST_UNIT_CHUNK),
        3 => U256::from(COST_UNIT_POINTER),
        _ => U256::ZERO,
    }
}

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

    /// Candidate nodes with metrics
    pub candidates: [CandidateNode; CANDIDATES_PER_POOL],
}

/// Candidate node with metrics for pool commitment
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateNode {
    /// Rewards address of the candidate node
    pub rewards_address: RewardsAddress,

    /// Metrics of the candidate node
    pub metrics: QuotingMetrics,
}

/// Packed candidate node for compact calldata (v2)
///
/// This struct packs the data type and total cost unit into a single U256 to reduce calldata size.
/// The packing format is: `packed = (totalCostUnit << 8) | dataType`
/// - dataType occupies bits 0-7 (lower 8 bits)
/// - totalCostUnit occupies bits 8-255
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateNodePacked {
    /// Rewards address of the candidate node
    pub rewards_address: RewardsAddress,

    /// Packed data: (totalCostUnit << 8) | dataType
    pub data_type_and_total_cost_unit: U256,
}

/// Packed pool commitment for compact calldata (v2)
///
/// Uses CandidateNodePacked instead of CandidateNode for smaller calldata.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PoolCommitmentPacked {
    /// Hash of the full MerklePaymentCandidatePool (cryptographic commitment)
    pub pool_hash: PoolHash,

    /// Packed candidate nodes
    pub candidates: [CandidateNodePacked; CANDIDATES_PER_POOL],
}

/// Encode data type and total cost unit into a single U256
///
/// Format: `packed = (totalCostUnit << 8) | dataType`
/// - dataType occupies bits 0-7 (lower 8 bits)
/// - totalCostUnit occupies bits 8-255
pub fn encode_data_type_and_cost(
    data_type: u8,
    total_cost_unit: U256,
) -> Result<U256, CostUnitOverflow> {
    if total_cost_unit >= (U256::from(1) << TOTAL_COST_UNIT_BITS) {
        return Err(CostUnitOverflow);
    }
    Ok((total_cost_unit << 8) | U256::from(data_type))
}

/// Decode data type and total cost unit from a packed U256
///
/// Returns (data_type, total_cost_unit)
#[cfg(test)]
pub fn decode_data_type_and_cost(packed: U256) -> (u8, U256) {
    let data_type = (packed & U256::from(0xFF)).to::<u8>();
    let total_cost_unit = packed >> 8;
    (data_type, total_cost_unit)
}

/// Calculate total cost unit from QuotingMetrics
///
/// Matches the contract's `_getTotalCostUnit`: for each record type, multiplies the record
/// count by that type's `costUnitPerDataType` weight, then sums the results.
/// Falls back to close_records_stored if records_per_type is empty (fresh nodes).
/// Uses a minimum of 1 to ensure non-zero cost unit.
pub fn calculate_total_cost_unit(metrics: &QuotingMetrics) -> U256 {
    // Sum: costUnitPerDataType[dataType] * records, matching the contract
    let total_from_types: U256 =
        metrics
            .records_per_type
            .iter()
            .fold(U256::ZERO, |acc, (data_type, count)| {
                let solidity_type = data_type_conversion(*data_type);
                acc + cost_unit_for_data_type(solidity_type) * U256::from(*count)
            });

    if total_from_types > U256::ZERO {
        total_from_types
    } else {
        // Use close_records_stored as fallback for fresh nodes, with minimum of 1
        let fallback = std::cmp::max(metrics.close_records_stored as u64, 1);
        let solidity_type = data_type_conversion(metrics.data_type);
        cost_unit_for_data_type(solidity_type) * U256::from(fallback)
    }
}

impl CandidateNode {
    /// Convert to packed format for v2 contract calls
    pub fn to_packed(&self) -> Result<CandidateNodePacked, CostUnitOverflow> {
        let data_type = data_type_conversion(self.metrics.data_type);
        let total_cost_unit = calculate_total_cost_unit(&self.metrics);
        Ok(CandidateNodePacked {
            rewards_address: self.rewards_address,
            data_type_and_total_cost_unit: encode_data_type_and_cost(data_type, total_cost_unit)?,
        })
    }
}

impl PoolCommitment {
    /// Convert to packed format for v2 contract calls
    pub fn to_packed(&self) -> Result<PoolCommitmentPacked, CostUnitOverflow> {
        let mut packed_candidates = Vec::with_capacity(CANDIDATES_PER_POOL);
        for c in &self.candidates {
            packed_candidates.push(c.to_packed()?);
        }
        let candidates: [CandidateNodePacked; CANDIDATES_PER_POOL] = packed_candidates
            .try_into()
            .expect("Vec length matches CANDIDATES_PER_POOL");
        Ok(PoolCommitmentPacked {
            pool_hash: self.pool_hash,
            candidates,
        })
    }
}

#[cfg(any(test, feature = "test-utils"))]
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

#[cfg(any(test, feature = "test-utils"))]
/// Disk-based Merkle payment contract (mock for testing)
///
/// This simulates smart contract behavior by storing payment data to disk.
/// Only available for testing.
pub struct DiskMerklePaymentContract {
    storage_path: PathBuf, // ~/.autonomi/merkle_payments/
}

#[cfg(any(test, feature = "test-utils"))]
impl DiskMerklePaymentContract {
    /// Create a new contract with a specific storage path
    pub fn new_with_path(storage_path: PathBuf) -> Result<Self, SmartContractError> {
        std::fs::create_dir_all(&storage_path)?;
        Ok(Self { storage_path })
    }

    /// Create a new contract with the default storage path
    /// Uses: DATA_DIR/autonomi/merkle_payments/
    pub fn new() -> Result<Self, SmartContractError> {
        let storage_path = if let Some(data_dir) = dirs_next::data_local_dir() {
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
    /// * `amount` - Amount paid for the Merkle tree
    pub fn pay_for_merkle_tree(
        &self,
        depth: u8,
        pool_commitments: Vec<PoolCommitment>,
        merkle_payment_timestamp: u64,
    ) -> Result<(PoolHash, Amount), SmartContractError> {
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

        // Validate: each pool has exactly CANDIDATES_PER_POOL candidates
        for pool in &pool_commitments {
            if pool.candidates.len() != CANDIDATES_PER_POOL {
                return Err(SmartContractError::WrongCandidateCount {
                    expected: CANDIDATES_PER_POOL,
                    got: pool.candidates.len(),
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
            let idx = rand::random::<usize>() % winner_pool.candidates.len();
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
            let addr = winner_pool.candidates[node_idx].rewards_address;
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

        // placeholder amount based on depth
        let placeholder_amount = Amount::from(2_u64.pow(depth as u32));

        Ok((winner_pool_hash, placeholder_amount))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_data_type_and_cost() {
        // Test basic encoding/decoding
        let data_type: u8 = 2; // Chunk
        let total_cost_unit = U256::from(1000u64); // Record count

        let packed = encode_data_type_and_cost(data_type, total_cost_unit).unwrap();
        let (decoded_type, decoded_cost) = decode_data_type_and_cost(packed);

        assert_eq!(decoded_type, data_type);
        assert_eq!(decoded_cost, total_cost_unit);
    }

    #[test]
    fn test_encode_decode_boundary_values() {
        // Test with max u8 data type
        let data_type: u8 = 255;
        let total_cost_unit = U256::from(100u64);

        let packed = encode_data_type_and_cost(data_type, total_cost_unit).unwrap();
        let (decoded_type, decoded_cost) = decode_data_type_and_cost(packed);

        assert_eq!(decoded_type, data_type);
        assert_eq!(decoded_cost, total_cost_unit);
    }

    #[test]
    fn test_encode_decode_zero_values() {
        // Test with zero values
        let packed = encode_data_type_and_cost(0, U256::ZERO).unwrap();
        let (decoded_type, decoded_cost) = decode_data_type_and_cost(packed);

        assert_eq!(decoded_type, 0);
        assert_eq!(decoded_cost, U256::ZERO);
    }

    #[test]
    fn test_encode_returns_error_on_overflow() {
        // total_cost_unit that uses all 256 bits should fail
        let overflow_value = U256::MAX;
        let result = encode_data_type_and_cost(0, overflow_value);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_large_cost() {
        // Test with large cost (U256 max - some value to avoid overflow when shifting)
        let data_type: u8 = 1;
        let large_cost = U256::from(u128::MAX);

        let packed = encode_data_type_and_cost(data_type, large_cost).unwrap();
        let (decoded_type, decoded_cost) = decode_data_type_and_cost(packed);

        assert_eq!(decoded_type, data_type);
        assert_eq!(decoded_cost, large_cost);
    }

    #[test]
    fn test_calculate_total_cost_unit() {
        let metrics = QuotingMetrics {
            data_type: 0,
            data_size: 1024 * 1024,
            close_records_stored: 100,
            records_per_type: vec![(0, 10), (1, 20), (2, 5)],
            max_records: 1000,
            received_payment_count: 50,
            live_time: 3600,
            network_density: None,
            network_size: Some(1000),
        };

        let total_cost = calculate_total_cost_unit(&metrics);

        // Rust type 0 -> Chunk(cost=10):   10 * 10 = 100
        // Rust type 1 -> GraphEntry(cost=1): 20 * 1  = 20
        // Rust type 2 -> Pointer(cost=20):   5  * 20 = 100
        // Total = 220
        assert_eq!(total_cost, U256::from(220u64));
    }

    #[test]
    fn test_calculate_total_cost_unit_empty_records() {
        let metrics = QuotingMetrics {
            data_type: 0,
            data_size: 1024,
            close_records_stored: 0,
            records_per_type: vec![],
            max_records: 1000,
            received_payment_count: 0,
            live_time: 0,
            network_density: None,
            network_size: None,
        };

        let total_cost = calculate_total_cost_unit(&metrics);
        // Fallback: max(0, 1) = 1 record, data_type 0 -> Chunk(cost=10), total = 10
        assert_eq!(total_cost, U256::from(10u64));
    }

    #[test]
    fn test_candidate_node_to_packed() {
        let metrics = QuotingMetrics {
            data_type: 0, // Will be converted to 2 (Chunk) by data_type_conversion
            data_size: 1024 * 1024,
            close_records_stored: 100,
            records_per_type: vec![(0, 10)],
            max_records: 1000,
            received_payment_count: 50,
            live_time: 3600,
            network_density: None,
            network_size: Some(1000),
        };

        let candidate = CandidateNode {
            rewards_address: RewardsAddress::from([0x42; 20]),
            metrics,
        };

        let packed = candidate.to_packed().unwrap();

        assert_eq!(packed.rewards_address, candidate.rewards_address);

        // Decode and verify
        let (data_type, total_cost) =
            decode_data_type_and_cost(packed.data_type_and_total_cost_unit);
        assert_eq!(data_type, 2); // Chunk data type after conversion
        assert_eq!(total_cost, U256::from(100u64)); // 10 records * Chunk cost unit (10)
    }

    #[test]
    fn test_pool_commitment_to_packed() {
        let candidates: [CandidateNode; CANDIDATES_PER_POOL] =
            std::array::from_fn(|i| CandidateNode {
                rewards_address: RewardsAddress::from([i as u8; 20]),
                metrics: QuotingMetrics {
                    data_type: 0,
                    data_size: 1024,
                    close_records_stored: i * 10,
                    records_per_type: vec![(0, i as u32)],
                    max_records: 1000,
                    received_payment_count: i,
                    live_time: 3600,
                    network_density: None,
                    network_size: None,
                },
            });

        let pool = PoolCommitment {
            pool_hash: [0x42; 32],
            candidates,
        };

        let packed = pool.to_packed().unwrap();

        assert_eq!(packed.pool_hash, pool.pool_hash);
        assert_eq!(packed.candidates.len(), CANDIDATES_PER_POOL);

        // Verify first candidate
        assert_eq!(
            packed.candidates[0].rewards_address,
            pool.candidates[0].rewards_address
        );
    }
}
