// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::{Address, Amount, Calldata};
use crate::contract::merkle_payment_vault::error::Error;
use crate::contract::merkle_payment_vault::interface::IMerklePaymentVault;
use crate::contract::merkle_payment_vault::interface::IMerklePaymentVault::IMerklePaymentVaultInstance;
use crate::merkle_batch_payment::PoolHash;
use crate::retry::TransactionError;
use crate::transaction_config::TransactionConfig;
use alloy::network::{Network, TransactionResponse};
use alloy::providers::Provider;
use exponential_backoff::Backoff;
use std::time::Duration;

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

    /// Get the MerklePaymentMade event from a transaction hash with retry logic
    ///
    /// This function retries up to 2 times with exponential backoff if the event
    /// is not found immediately. This handles cases where the transaction may not
    /// be fully indexed yet.
    ///
    /// # Arguments
    /// * `tx_hash` - The transaction hash to query
    ///
    /// # Returns
    /// * The MerklePaymentMade event from the transaction
    async fn get_merkle_payment_event(
        &self,
        tx_hash: crate::common::TxHash,
    ) -> Result<IMerklePaymentVault::MerklePaymentMade, Error> {
        const MAX_ATTEMPTS: u32 = 3;
        const INITIAL_DELAY_MS: u64 = 500;
        const MAX_DELAY_MS: u64 = 8000;

        // Configure backoff with exponential delays between attempts
        let backoff = Backoff::new(
            MAX_ATTEMPTS,
            Duration::from_millis(INITIAL_DELAY_MS),
            Some(Duration::from_millis(MAX_DELAY_MS)),
        );

        let mut last_error = None;
        let mut attempt = 1;

        for duration_opt in backoff {
            match self.try_get_merkle_payment_event(tx_hash).await {
                Ok(event) => return Ok(event),
                Err(e) => {
                    last_error = Some(e);

                    // Sleep before next attempt if duration is provided
                    if let Some(duration) = duration_opt {
                        debug!(
                            "Failed to get MerklePaymentMade event (attempt {}/{}), retrying in {}ms",
                            attempt,
                            MAX_ATTEMPTS,
                            duration.as_millis()
                        );
                        tokio::time::sleep(duration).await;
                    }
                    attempt += 1;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Rpc("Failed to get MerklePaymentMade event after retries".to_string())
        }))
    }

    /// Try to get the MerklePaymentMade event from a transaction hash (single attempt)
    async fn try_get_merkle_payment_event(
        &self,
        tx_hash: crate::common::TxHash,
    ) -> Result<IMerklePaymentVault::MerklePaymentMade, Error> {
        // Get the transaction to find its block number
        let tx = self
            .contract
            .provider()
            .get_transaction_by_hash(tx_hash)
            .await
            .map_err(|e| Error::Rpc(format!("Failed to get transaction: {e}")))?
            .ok_or_else(|| Error::Rpc("Transaction not found".to_string()))?;

        let block_number = tx
            .block_number()
            .ok_or_else(|| Error::Rpc("Transaction has no block number".to_string()))?;

        // Get the MerklePaymentMade event from that block
        let events = self
            .contract
            .MerklePaymentMade_filter()
            .from_block(block_number)
            .to_block(block_number)
            .query()
            .await
            .map_err(|e| Error::Rpc(format!("Failed to query MerklePaymentMade events: {e}")))?;

        events
            .into_iter()
            .find(|(_, log)| log.transaction_hash == Some(tx_hash))
            .map(|(event, _)| event)
            .ok_or_else(|| {
                Error::Rpc("MerklePaymentMade event not found in transaction".to_string())
            })
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
    /// * Tuple of (winner pool hash, total amount paid)
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
        debug!("Paying for Merkle tree: depth={depth}, timestamp={merkle_payment_timestamp}");

        let (calldata, to) =
            self.pay_for_merkle_tree_calldata(depth, pool_commitments, merkle_payment_timestamp)?;

        let tx_hash = self
            .send_transaction_and_handle_errors(calldata, to, transaction_config)
            .await?;

        let event = self.get_merkle_payment_event(tx_hash).await?;

        let winner_pool_hash = event.winnerPoolHash.0;
        let total_amount = event.totalAmount;

        debug!(
            "MerklePaymentMade event: winnerPoolHash={}, depth={}, totalAmount={}, timestamp={}",
            hex::encode(winner_pool_hash),
            event.depth,
            total_amount,
            event.merklePaymentTimestamp
        );

        Ok((winner_pool_hash, total_amount))
    }

    /// Send transaction with retries and handle revert errors
    async fn send_transaction_and_handle_errors(
        &self,
        calldata: Calldata,
        to: Address,
        transaction_config: &TransactionConfig,
    ) -> Result<crate::common::TxHash, Error> {
        let tx_result = crate::retry::send_transaction_with_retries(
            self.contract.provider(),
            calldata,
            to,
            "pay for merkle tree",
            transaction_config,
        )
        .await;

        match tx_result {
            Ok(hash) => Ok(hash),
            Err(TransactionError::TransactionReverted {
                message,
                revert_data,
                nonce,
            }) => {
                let error = self.decode_revert_error(message, revert_data, nonce);
                Err(error)
            }
            Err(other_err) => Err(Error::from(other_err)),
        }
    }

    /// Decode revert data or return generic transaction error
    fn decode_revert_error(
        &self,
        message: String,
        revert_data: Option<alloy::primitives::Bytes>,
        nonce: Option<u64>,
    ) -> Error {
        if let Some(revert_data_bytes) = &revert_data
            && let Some(decoded_err) = Error::try_decode_revert(revert_data_bytes)
        {
            return decoded_err;
        }

        Error::Transaction(TransactionError::TransactionReverted {
            message,
            revert_data,
            nonce,
        })
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
