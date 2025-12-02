// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::contract::merkle_payment_vault::error::Error;
use crate::contract::merkle_payment_vault::handler::MerklePaymentVaultHandler;
use crate::merkle_batch_payment::PoolHash;
use crate::utils::http_provider;

pub mod error;
pub mod handler;
pub mod implementation;
pub mod interface;

/// Helper function to get payment info for a Merkle payment verification
/// Returns the payment info if the payment exists on-chain
pub async fn get_merkle_payment_info(
    network: &crate::Network,
    winner_pool_hash: PoolHash,
) -> Result<interface::IMerklePaymentVault::PaymentInfo, Error> {
    let merkle_vault_address = network
        .merkle_payments_address()
        .ok_or(Error::MerklePaymentsAddressNotConfigured)?;

    let provider = http_provider(network.rpc_url().clone());
    let merkle_vault = MerklePaymentVaultHandler::new(*merkle_vault_address, provider);

    merkle_vault.get_payment_info(winner_pool_hash).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::U256;
    use crate::merkle_batch_payment::{CandidateNode, PoolCommitment};
    use crate::quoting_metrics::QuotingMetrics;
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
        let _tx_hash = vault_handler
            .pay_for_merkle_tree(depth, pool_commitments.clone(), timestamp, &tx_config)
            .await
            .expect("Failed to pay for Merkle tree");

        println!("Payment transaction sent successfully");

        // Find the winner pool by checking payment info
        let mut winner_pool_hash = None;
        for pool in &pool_commitments {
            let pool_hash: [u8; 32] = pool.pool_hash.into();
            if vault_handler.get_payment_info(pool_hash).await.is_ok() {
                winner_pool_hash = Some(pool_hash);
                break;
            }
        }
        let winner_pool_hash = winner_pool_hash.expect("Should find winner pool");

        println!(
            "Payment successful for pool: {}",
            hex::encode(winner_pool_hash)
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
            Err(error::Error::PaymentAlreadyExists(_)) => {
                println!("Correctly detected duplicate payment!");
            }
            Err(e) => panic!("Expected PaymentAlreadyExists error, got: {e:?}"),
            Ok(_) => panic!("Should not allow duplicate payment"),
        }

        println!("\n✅ All tests passed!");
    }
}
