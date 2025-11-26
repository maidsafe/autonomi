// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::{Address, Amount, Calldata};
use crate::contract::data_type_conversion;
use crate::merkle_batch_payment::CANDIDATES_PER_POOL;
use crate::quoting_metrics::QuotingMetrics;
use crate::retry::send_transaction_with_retries;
use crate::transaction_config::TransactionConfig;
use crate::utils::http_provider;
use alloy::network::{Network, ReceiptResponse};
use alloy::primitives::U256;
use alloy::providers::Provider;
use alloy::sol;
use alloy::sol_types::SolError;

pub mod implementation;

// Generate bindings from ABI
sol!(
    #[allow(missing_docs)]
    #[derive(Debug)]
    #[sol(rpc)]
    IMerklePaymentVault,
    "abi/IMerklePaymentVault.json"
);

// Re-export contract instance type
pub use IMerklePaymentVault::IMerklePaymentVaultInstance;

// Re-export PoolHash (doesn't conflict with generated types)
pub use crate::merkle_batch_payment::PoolHash;

// Implement conversions from our API types to contract types
impl From<crate::merkle_batch_payment::PoolCommitment> for IMerklePaymentVault::PoolCommitment {
    fn from(pool: crate::merkle_batch_payment::PoolCommitment) -> Self {
        // Convert the exact-sized array directly
        let candidates_array: [IMerklePaymentVault::CandidateNode; CANDIDATES_PER_POOL] =
            pool.candidates.map(|c| c.into());

        Self {
            poolHash: pool.pool_hash.into(),
            candidates: candidates_array,
        }
    }
}

impl From<crate::merkle_batch_payment::CandidateNode> for IMerklePaymentVault::CandidateNode {
    fn from(node: crate::merkle_batch_payment::CandidateNode) -> Self {
        Self {
            rewardsAddress: node.rewards_address,
            metrics: node.metrics.into(),
        }
    }
}

impl From<QuotingMetrics> for IMerklePaymentVault::QuotingMetrics {
    fn from(metrics: QuotingMetrics) -> Self {
        Self {
            dataType: data_type_conversion(metrics.data_type),
            dataSize: U256::from(metrics.data_size),
            closeRecordsStored: U256::from(metrics.close_records_stored),
            recordsPerType: metrics
                .records_per_type
                .into_iter()
                .map(|(data_type, records)| IMerklePaymentVault::Record {
                    dataType: data_type_conversion(data_type),
                    records: U256::from(records),
                })
                .collect(),
            maxRecords: U256::from(metrics.max_records),
            receivedPaymentCount: U256::from(metrics.received_payment_count),
            liveTime: U256::from(metrics.live_time),
            networkDensity: metrics
                .network_density
                .map(|d| U256::from_be_bytes(d))
                .unwrap_or_default(),
            networkSize: metrics.network_size.map(U256::from).unwrap_or_default(),
        }
    }
}

// Handler implementation

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Contract error: {0}")]
    Contract(#[from] alloy::contract::Error),
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("Payment not found for pool hash: {0}")]
    PaymentNotFound(String),
    #[error("Merkle payments address not configured for this network")]
    MerklePaymentsAddressNotConfigured,

    // Smart contract custom errors
    #[error("Merkle tree depth {depth} exceeds maximum allowed depth {max}")]
    DepthTooLarge { depth: u8, max: u8 },
    #[error("Wrong pool count: expected {expected}, got {got}")]
    WrongPoolCount { expected: u64, got: u64 },
    #[error("Wrong candidate count in pool {pool_idx}: expected {expected}, got {got}")]
    WrongCandidateCount {
        pool_idx: u64,
        expected: u64,
        got: u64,
    },
    #[error("Insufficient token balance: have {have}, need {need}")]
    InsufficientBalance { have: Amount, need: Amount },
    #[error(
        "Insufficient token allowance for MerklePaymentVault contract: have {have}, need {need}. Please approve more tokens using wallet.approve()"
    )]
    InsufficientAllowance { have: Amount, need: Amount },
    #[error("Token transfer failed")]
    TransferFailed,
    #[error("Payment already exists for pool hash: {0}")]
    PaymentAlreadyExists(String),
}

/// Helper function to decode contract errors into our error types
/// Tries to match the contract error data to known custom errors
fn decode_contract_error(contract_err: alloy::contract::Error) -> Error {
    // Try to decode each custom error type
    if let Some(error_data) = contract_err.as_revert_data() {
        // Try DepthTooLarge
        if let Ok(err) = IMerklePaymentVault::DepthTooLarge::abi_decode(&error_data) {
            return Error::DepthTooLarge {
                depth: err.depth,
                max: err.max,
            };
        }

        // Try WrongPoolCount
        if let Ok(err) = IMerklePaymentVault::WrongPoolCount::abi_decode(&error_data) {
            let expected = err.expected.to::<u64>();
            let got = err.got.to::<u64>();
            return Error::WrongPoolCount { expected, got };
        }

        // Try WrongCandidateCount
        if let Ok(err) = IMerklePaymentVault::WrongCandidateCount::abi_decode(&error_data) {
            let pool_idx = err.poolIdx.to::<u64>();
            let expected = err.expected.to::<u64>();
            let got = err.got.to::<u64>();
            return Error::WrongCandidateCount {
                pool_idx,
                expected,
                got,
            };
        }

        // Try InsufficientBalance
        if let Ok(err) = IMerklePaymentVault::InsufficientBalance::abi_decode(&error_data) {
            return Error::InsufficientBalance {
                have: err.have,
                need: err.need,
            };
        }

        // Try InsufficientAllowance
        if let Ok(err) = IMerklePaymentVault::InsufficientAllowance::abi_decode(&error_data) {
            return Error::InsufficientAllowance {
                have: err.have,
                need: err.need,
            };
        }

        // Try TransferFailed
        if IMerklePaymentVault::TransferFailed::abi_decode(&error_data).is_ok() {
            return Error::TransferFailed;
        }

        // Try PaymentAlreadyExists
        if let Ok(err) = IMerklePaymentVault::PaymentAlreadyExists::abi_decode(&error_data) {
            return Error::PaymentAlreadyExists(format!("{:?}", err.poolHash));
        }

        // Try PaymentNotFound (for completeness)
        if let Ok(err) = IMerklePaymentVault::PaymentNotFound::abi_decode(&error_data) {
            return Error::PaymentNotFound(format!("{:?}", err.poolHash));
        }
    }

    // If we couldn't decode to a specific error, return the generic contract error
    Error::Contract(contract_err)
}

/// Helper function to get payment info for a Merkle payment verification
/// Returns the payment info if the payment exists on-chain
pub async fn get_merkle_payment_info(
    network: &crate::Network,
    winner_pool_hash: PoolHash,
) -> Result<IMerklePaymentVault::PaymentInfo, Error> {
    let merkle_vault_address = network
        .merkle_payments_address()
        .ok_or(Error::MerklePaymentsAddressNotConfigured)?;

    let provider = http_provider(network.rpc_url().clone());
    let merkle_vault = MerklePaymentVaultHandler::new(*merkle_vault_address, provider);

    merkle_vault.get_payment_info(winner_pool_hash).await
}

pub struct MerklePaymentVaultHandler<P: Provider<N>, N: Network> {
    pub contract: IMerklePaymentVaultInstance<P, N>,
}

impl<P, N> MerklePaymentVaultHandler<P, N>
where
    P: Provider<N>,
    N: Network,
{
    /// Create a new handler instance
    pub fn new(contract_address: Address, provider: P) -> Self {
        let contract = IMerklePaymentVault::new(contract_address, provider);
        Self { contract }
    }

    /// Set the provider
    pub fn set_provider(&mut self, provider: P) {
        let address = *self.contract.address();
        self.contract = IMerklePaymentVault::new(address, provider);
    }

    /// Pay for Merkle tree batch
    ///
    /// # Arguments
    /// * `depth` - Merkle tree depth
    /// * `pool_commitments` - Pool commitments with metrics
    /// * `merkle_payment_timestamp` - Payment timestamp
    /// * `transaction_config` - Transaction configuration
    ///
    /// # Returns
    /// * `(winner_pool_hash, total_amount)` - Pool hash and total amount paid
    pub async fn pay_for_merkle_tree<I, T>(
        &self,
        depth: u8,
        pool_commitments: I,
        merkle_payment_timestamp: u64,
        transaction_config: &TransactionConfig,
    ) -> Result<(PoolHash, Amount), Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<IMerklePaymentVault::PoolCommitment>,
    {
        debug!("Paying for Merkle tree: depth={depth}, timestamp={merkle_payment_timestamp}",);

        // Convert pool commitments to Vec so we can use it for both simulation and actual call
        let pool_commitments: Vec<IMerklePaymentVault::PoolCommitment> = pool_commitments
            .into_iter()
            .map(|item| item.into())
            .collect();

        // Pre-flight simulation: call the contract to check for errors before spending gas
        debug!("Running pre-flight simulation for Merkle payment");
        let simulation_result = self
            .contract
            .payForMerkleTree(depth, pool_commitments.clone(), merkle_payment_timestamp)
            .call()
            .await;

        // Check simulation result and decode any errors
        if let Err(contract_err) = simulation_result {
            debug!("Pre-flight simulation failed: {contract_err}");
            return Err(decode_contract_error(contract_err));
        }

        debug!("Pre-flight simulation succeeded");

        // Get calldata for actual transaction (keep pool_commitments for potential error replay)
        let (calldata, to) = self.pay_for_merkle_tree_calldata(
            depth,
            pool_commitments.clone(),
            merkle_payment_timestamp,
        )?;

        // Send transaction
        let tx_hash = send_transaction_with_retries(
            self.contract.provider(),
            calldata,
            to,
            "pay for merkle tree",
            transaction_config,
        )
        .await
        .map_err(|e| Error::Rpc(e.to_string()))?;

        debug!("Merkle payment transaction sent: {tx_hash}");

        // Wait for the transaction to be mined by getting the receipt
        let receipt = self
            .contract
            .provider()
            .get_transaction_receipt(tx_hash)
            .await
            .map_err(|e| Error::Rpc(format!("Failed to get transaction receipt: {e}")))?
            .ok_or_else(|| {
                Error::Rpc(format!("Transaction receipt not found for tx: {tx_hash}"))
            })?;

        let block_number = receipt
            .block_number()
            .ok_or_else(|| Error::Rpc(format!("Receipt has no block number for tx: {tx_hash}")))?;

        // Check if the transaction succeeded (reverted transactions don't emit events)
        if !receipt.status() {
            // Transaction reverted - replay it with .call() to get the actual error reason
            debug!("Transaction {tx_hash} reverted, replaying to get error reason");
            let replay_result = self
                .contract
                .payForMerkleTree(depth, pool_commitments.clone(), merkle_payment_timestamp)
                .call()
                .await;

            // If replay fails, decode the error - otherwise return generic message
            if let Err(contract_err) = replay_result {
                return Err(decode_contract_error(contract_err));
            }

            // Replay succeeded but original tx failed - this can happen if:
            // 1. Gas estimation was insufficient for the original transaction
            // 2. Concurrent transactions modified state between tx and replay
            // 3. Network conditions changed (reorg, etc.)
            // Since replay succeeded, the payment parameters are valid - retry might work
            return Err(Error::Rpc(format!(
                "Payment transaction {tx_hash} reverted in block {block_number}. \
                The transaction parameters are valid (replay succeeded), so this may be a \
                transient issue. Possible causes: insufficient gas, concurrent transactions, \
                or network instability. Retrying the upload may succeed."
            )));
        }

        debug!("Transaction {tx_hash} confirmed successfully in block {block_number}");

        // Parse MerklePaymentMade event from transaction logs
        // Strategy:
        // 1. Query events from the exact block
        // 2. If not found, widen range (± 2 blocks) for RPC lag/reorg resilience
        // 3. If still not found, fall back to get_payment_info for each pool hash

        // First try: exact block query
        let filter = self
            .contract
            .MerklePaymentMade_filter()
            .from_block(block_number)
            .to_block(block_number);

        let events = filter.query().await.unwrap_or_default();
        debug!(
            "Found {} MerklePaymentMade event(s) in block {block_number}",
            events.len()
        );

        // Find the event matching our transaction
        for (evt, log) in &events {
            if log.transaction_hash == Some(tx_hash) {
                let winner_pool_hash: [u8; 32] = evt.winnerPoolHash.into();
                let total_amount = evt.totalAmount;

                debug!(
                    "Extracted from event: pool_hash={}, amount={total_amount}",
                    hex::encode(winner_pool_hash)
                );

                return Ok((winner_pool_hash, total_amount));
            }
        }

        // Second try: widen range to ± 2 blocks for RPC indexing lag
        debug!("Event not found in exact block, trying wider range");
        let from_block = block_number.saturating_sub(2);
        let to_block = block_number.saturating_add(2);

        let filter = self
            .contract
            .MerklePaymentMade_filter()
            .from_block(from_block)
            .to_block(to_block);

        let events = filter.query().await.unwrap_or_default();
        debug!(
            "Found {} event(s) in blocks {from_block}-{to_block}",
            events.len()
        );

        for (evt, log) in &events {
            if log.transaction_hash == Some(tx_hash) {
                let winner_pool_hash: [u8; 32] = evt.winnerPoolHash.into();
                let total_amount = evt.totalAmount;

                debug!(
                    "Extracted from event (wider range): pool_hash={}, amount={total_amount}",
                    hex::encode(winner_pool_hash)
                );

                return Ok((winner_pool_hash, total_amount));
            }
        }

        // Third fallback: query get_payment_info for each pool hash to find the winner
        // This is O(n) but ensures we get the result even if event indexing fails
        debug!("Event not found, falling back to get_payment_info queries");
        for pool in &pool_commitments {
            let pool_hash: [u8; 32] = pool.poolHash.into();
            match self.get_payment_info(pool_hash).await {
                Ok(_info) => {
                    // Found the payment on-chain - get the amount via estimate
                    // (PaymentInfo doesn't store totalAmount, but estimate is deterministic)
                    let total_amount = self
                        .estimate_merkle_tree_cost(
                            depth,
                            pool_commitments.clone(),
                            merkle_payment_timestamp,
                        )
                        .await
                        .unwrap_or_default();

                    debug!(
                        "Found payment via get_payment_info fallback: pool_hash={}, amount={total_amount}",
                        hex::encode(pool_hash)
                    );
                    return Ok((pool_hash, total_amount));
                }
                Err(Error::PaymentNotFound(_)) => {
                    // This pool wasn't the winner, continue checking
                    continue;
                }
                Err(e) => {
                    // RPC error - log and continue trying other pools
                    warn!("Error checking pool {}: {e}", hex::encode(pool_hash));
                    continue;
                }
            }
        }

        // If we get here, the payment truly wasn't found
        Err(Error::Rpc(format!(
            "Payment not found for transaction {tx_hash}. \
            Event lookup and on-chain verification both failed. \
            This may indicate a chain reorganization."
        )))
    }

    /// Get calldata for payForMerkleTree
    fn pay_for_merkle_tree_calldata(
        &self,
        depth: u8,
        pool_commitments: Vec<IMerklePaymentVault::PoolCommitment>,
        merkle_payment_timestamp: u64,
    ) -> Result<(Calldata, Address), Error> {
        let calldata = self
            .contract
            .payForMerkleTree(depth, pool_commitments, merkle_payment_timestamp)
            .calldata()
            .to_owned();

        Ok((calldata, *self.contract.address()))
    }

    /// Estimate the cost of a Merkle tree payment without executing it
    ///
    /// This is a view function (0 gas) that runs the same pricing logic as
    /// pay_for_merkle_tree but returns only the estimated cost.
    ///
    /// # Arguments
    /// * `depth` - Merkle tree depth
    /// * `pool_commitments` - Pool commitments with metrics
    /// * `merkle_payment_timestamp` - Payment timestamp
    ///
    /// # Returns
    /// * `Amount` - Estimated total cost
    pub async fn estimate_merkle_tree_cost<I, T>(
        &self,
        depth: u8,
        pool_commitments: I,
        merkle_payment_timestamp: u64,
    ) -> Result<Amount, Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<IMerklePaymentVault::PoolCommitment>,
    {
        debug!("Estimating Merkle tree cost: depth={depth}, timestamp={merkle_payment_timestamp}",);

        let pool_commitments: Vec<IMerklePaymentVault::PoolCommitment> = pool_commitments
            .into_iter()
            .map(|item| item.into())
            .collect();

        let total_amount = self
            .contract
            .estimateMerkleTreeCost(depth, pool_commitments, merkle_payment_timestamp)
            .call()
            .await
            .map_err(Error::Contract)?;

        Ok(total_amount)
    }

    /// Get payment info for a winner pool hash
    pub async fn get_payment_info(
        &self,
        winner_pool_hash: PoolHash,
    ) -> Result<IMerklePaymentVault::PaymentInfo, Error> {
        debug!(
            "Getting payment info for pool hash: {}",
            hex::encode(winner_pool_hash)
        );

        let info = self
            .contract
            .getPaymentInfo(winner_pool_hash.into())
            .call()
            .await
            .map_err(Error::Contract)?;

        // Check if payment exists (depth == 0 means not found)
        if info.depth == 0 {
            return Err(Error::PaymentNotFound(hex::encode(winner_pool_hash)));
        }

        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::U256;
    use crate::merkle_batch_payment::{CandidateNode, PoolCommitment};
    use crate::testnet::{deploy_network_token_contract, start_node};
    use crate::transaction_config::TransactionConfig;
    use crate::wallet::wallet_address;
    use alloy::providers::WalletProvider;

    #[tokio::test]
    async fn test_smart_contract() {
        // Start local Anvil node
        let (_anvil, rpc_url) = start_node();

        // Deploy network token
        let network_token = deploy_network_token_contract(&rpc_url, &_anvil).await;
        let token_address = *network_token.contract.address();

        // Deploy Merkle payment vault using the same provider as network token
        let vault_address =
            implementation::deploy(network_token.contract.provider(), token_address).await;

        // Create handler with the same provider
        let vault_handler = MerklePaymentVaultHandler::new(
            vault_address,
            network_token.contract.provider().clone(),
        );

        // Get wallet address
        let wallet_addr = wallet_address(network_token.contract.provider().wallet());

        // Transaction config
        let tx_config = TransactionConfig::default();

        // Approve vault contract to spend tokens (wallet already has tokens from deployment)
        network_token
            .approve(vault_address, U256::MAX, &tx_config)
            .await
            .expect("Failed to approve tokens");

        // Create test pool commitments with candidates
        let depth = 4u8;
        let num_pools = 2usize.pow((depth / 2) as u32);
        let timestamp = 1234567890u64;

        let mut pool_commitments = Vec::new();
        for pool_idx in 0..num_pools {
            // Create 16 candidates per pool
            let mut candidates = Vec::new();
            for candidate_idx in 0..16 {
                candidates.push(CandidateNode {
                    rewards_address: wallet_addr, // Use wallet address for simplicity
                    metrics: QuotingMetrics {
                        data_type: 0,
                        data_size: 1024 * (candidate_idx + 1),
                        close_records_stored: 100 + candidate_idx,
                        max_records: 1000,
                        received_payment_count: 10,
                        live_time: 3600,
                        records_per_type: vec![(0, 50), (1, 30), (2, 20)],
                        network_density: Some([0u8; 32]),
                        network_size: Some(100),
                    },
                });
            }

            pool_commitments.push(PoolCommitment {
                pool_hash: [pool_idx as u8; 32],
                candidates: candidates
                    .try_into()
                    .expect("Should have exactly 16 candidates"),
            });
        }

        // Test 1: Estimate Merkle tree cost
        println!("Test 1: Estimating Merkle tree cost...");
        let estimated_cost = vault_handler
            .estimate_merkle_tree_cost(depth, pool_commitments.clone(), timestamp)
            .await
            .expect("Failed to estimate cost");

        println!("Estimated cost: {estimated_cost} tokens");
        assert!(
            estimated_cost > U256::ZERO,
            "Cost should be greater than zero"
        );

        // Test 2: Pay for Merkle tree
        println!("\nTest 2: Paying for Merkle tree...");
        let (winner_pool_hash, paid_amount) = vault_handler
            .pay_for_merkle_tree(depth, pool_commitments.clone(), timestamp, &tx_config)
            .await
            .expect("Failed to pay for Merkle tree");

        println!(
            "Payment successful: {} tokens paid for pool: {}",
            paid_amount,
            hex::encode(winner_pool_hash)
        );
        assert_eq!(
            estimated_cost, paid_amount,
            "Paid amount should match estimate"
        );

        // Test 3: Get payment info
        println!("\nTest 3: Retrieving payment info...");
        let payment_info = vault_handler
            .get_payment_info(winner_pool_hash)
            .await
            .expect("Failed to get payment info");

        println!("Payment info retrieved:");
        println!("  Depth: {}", payment_info.depth);
        println!("  Timestamp: {}", payment_info.merklePaymentTimestamp);
        println!("  Paid nodes: {}", payment_info.paidNodeAddresses.len());

        assert_eq!(
            payment_info.depth, depth,
            "Stored depth should match what we paid for"
        );
        assert_eq!(
            payment_info.merklePaymentTimestamp, timestamp,
            "Stored timestamp should match"
        );
        assert!(
            !payment_info.paidNodeAddresses.is_empty(),
            "Should have paid nodes"
        );

        // Test 4: Try to pay again for the same tree (should fail with PaymentAlreadyExists)
        println!("\nTest 4: Testing duplicate payment detection...");
        let duplicate_result = vault_handler
            .pay_for_merkle_tree(depth, pool_commitments, timestamp, &tx_config)
            .await;

        match duplicate_result {
            Err(Error::PaymentAlreadyExists(_)) => {
                println!("Correctly detected duplicate payment!");
            }
            Err(e) => panic!("Expected PaymentAlreadyExists error, got: {e:?}"),
            Ok(_) => panic!("Should not allow duplicate payment"),
        }

        println!("\n✅ All tests passed!");
    }
}
