// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::{Address, Amount, Calldata};
use crate::merkle_batch_payment::CANDIDATES_PER_POOL;
use crate::quoting_metrics::QuotingMetrics;
use crate::retry::send_transaction_with_retries;
use crate::transaction_config::TransactionConfig;
use crate::utils::http_provider;
use alloy::network::{Network, ReceiptResponse};
use alloy::primitives::U256;
use alloy::providers::Provider;
use alloy::sol;

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

fn data_type_conversion(data_type: u32) -> u8 {
    match data_type {
        0 => 2, // Chunk
        1 => 0, // GraphEntry
        2 => 3, // Pointer
        3 => 1, // Scratchpad
        _ => 4, // Does not exist
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

        // Get calldata
        let (calldata, to) =
            self.pay_for_merkle_tree_calldata(depth, pool_commitments, merkle_payment_timestamp)?;

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

        debug!("Merkle payment transaction mined in block {block_number}, querying events...");

        // Query events from the specific block where the transaction was mined
        let filter = self
            .contract
            .MerklePaymentMade_filter()
            .from_block(block_number)
            .to_block(block_number);

        let events = filter.query().await.map_err(Error::Contract)?;

        // Find the event matching our transaction
        let event = events
            .into_iter()
            .find(|(_evt, log)| log.transaction_hash == Some(tx_hash))
            .map(|(evt, _log)| evt)
            .ok_or_else(|| {
                Error::Rpc(format!(
                    "MerklePaymentMade event not found in block {block_number} for transaction {tx_hash}"
                ))
            })?;

        let winner_pool_hash: [u8; 32] = event.winnerPoolHash.into();
        let total_amount = event.totalAmount;

        debug!(
            "Extracted from event: pool_hash={}, amount={total_amount}",
            hex::encode(winner_pool_hash)
        );

        Ok((winner_pool_hash, total_amount))
    }

    /// Get calldata for payForMerkleTree
    fn pay_for_merkle_tree_calldata<I, T>(
        &self,
        depth: u8,
        pool_commitments: I,
        merkle_payment_timestamp: u64,
    ) -> Result<(Calldata, Address), Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<IMerklePaymentVault::PoolCommitment>,
    {
        let pool_commitments: Vec<IMerklePaymentVault::PoolCommitment> = pool_commitments
            .into_iter()
            .map(|item| item.into())
            .collect();

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
