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
use crate::transaction_config::TransactionConfig;
use alloy::network::Network;
use alloy::providers::Provider;

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
    /// * Transaction hash
    pub async fn pay_for_merkle_tree<I, T>(
        &self,
        depth: u8,
        pool_commitments: I,
        merkle_payment_timestamp: u64,
        transaction_config: &TransactionConfig,
    ) -> Result<crate::common::TxHash, Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<IMerklePaymentVault::PoolCommitment>,
    {
        debug!("Paying for Merkle tree: depth={depth}, timestamp={merkle_payment_timestamp}");
        let (calldata, to) = self.pay_for_merkle_tree_calldata(depth, pool_commitments, merkle_payment_timestamp)?;
        crate::retry::send_transaction_with_retries(
            self.contract.provider(),
            calldata,
            to,
            "pay for merkle tree",
            transaction_config,
        )
        .await
        .map_err(Error::from)
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
