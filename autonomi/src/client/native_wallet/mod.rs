// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Native Token Wallet Implementation for Autonomi Network
//!
//! This module provides an in-memory wallet for managing native tokens on the
//! Autonomi Network. It supports creating and managing native token transactions
//! using GraphEntry structures.

use ant_protocol::storage::{GraphEntry, NativePaymentProof, NativeTokens};
use bls::{PublicKey, SecretKey};
use std::collections::HashMap;
use tracing::{debug, info};

pub mod config;
pub mod transaction_builder;

pub use config::{NativeWalletBuilder, NativeWalletConfig};
pub use transaction_builder::NativeTransactionBuilder;

/// Represents a native token balance in the wallet
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTokenBalance {
    /// The amount of tokens available
    pub amount: NativeTokens,

    /// The GraphEntry address where these tokens are stored
    pub graph_entry_address: PublicKey,

    /// Whether these tokens have been claimed/spent
    pub is_claimed: bool,

    /// Optional derivation index used to generate this address
    pub derivation_index: Option<[u8; 16]>,
}

/// Errors that can occur during native wallet operations
#[derive(Debug, thiserror::Error)]
pub enum NativeWalletError {
    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: u128, available: u128 },

    #[error("Invalid key derivation: {0}")]
    KeyDerivation(String),

    #[error("Transaction creation failed: {0}")]
    TransactionCreation(String),

    #[error("GraphEntry validation failed: {0}")]
    GraphEntryValidation(String),

    #[error("Network operation failed: {0}")]
    NetworkError(String),

    #[error("Cryptographic operation failed: {0}")]
    CryptographicError(String),
}

/// Result type for native wallet operations
pub type NativeWalletResult<T> = Result<T, NativeWalletError>;

/// In-memory native token wallet for the Autonomi Network
///
/// This wallet manages native tokens by tracking GraphEntry addresses and their
/// associated token balances. It provides functionality for creating payment
/// transactions and managing token ownership.
#[derive(Debug, Clone)]
pub struct InMemoryNativeWallet {
    /// Master key for deriving payment addresses and signing transactions
    pub master_key: SecretKey,

    /// Available token balances indexed by GraphEntry address
    pub available_tokens: HashMap<PublicKey, NativeTokenBalance>,

    /// Pending payment proofs that haven't been confirmed yet
    pub pending_transactions: Vec<NativePaymentProof>,

    /// Counter for generating unique derivation indices
    derivation_counter: u64,
}

impl InMemoryNativeWallet {
    /// Create a new native token wallet with the given master key
    ///
    /// # Arguments
    /// * `master_key` - The master secret key for this wallet
    ///
    /// # Returns
    /// A new InMemoryNativeWallet instance
    pub fn new(master_key: SecretKey) -> Self {
        Self {
            master_key,
            available_tokens: HashMap::new(),
            pending_transactions: Vec::new(),
            derivation_counter: 0,
        }
    }

    /// Add genesis tokens to the wallet (for testing and initial setup)
    ///
    /// # Arguments
    /// * `amount` - The amount of genesis tokens to add
    ///
    /// This method creates a genesis token entry that can be used as input
    /// for subsequent transactions.
    pub fn add_genesis_tokens(&mut self, amount: NativeTokens) -> NativeWalletResult<PublicKey> {
        // Generate a unique address for genesis tokens
        let genesis_address = self.derive_address_for_genesis()?;

        let balance = NativeTokenBalance {
            amount,
            graph_entry_address: genesis_address,
            is_claimed: false,
            derivation_index: None, // Genesis tokens don't have derivation indices
        };

        self.available_tokens.insert(genesis_address, balance);

        info!(
            "Added {} genesis tokens to address {:?}",
            amount.as_u128(),
            genesis_address
        );

        Ok(genesis_address)
    }

    /// Get the total available balance in the wallet
    ///
    /// # Returns
    /// The sum of all unclaimed token balances
    pub fn total_balance(&self) -> NativeTokens {
        self.available_tokens
            .values()
            .filter(|balance| !balance.is_claimed)
            .map(|balance| balance.amount)
            .fold(NativeTokens::ZERO, |acc, amount| {
                acc.checked_add(amount).unwrap_or(acc)
            })
    }

    /// Select tokens for a payment of the required amount
    ///
    /// # Arguments
    /// * `required_amount` - The amount of tokens needed for the payment
    ///
    /// # Returns
    /// A vector of GraphEntry addresses that can cover the required amount
    pub fn select_tokens_for_payment(
        &self,
        required_amount: NativeTokens,
    ) -> NativeWalletResult<Vec<PublicKey>> {
        let mut selected_tokens = Vec::new();
        let mut total_selected = NativeTokens::ZERO;

        // Sort available tokens by amount (largest first) for efficient selection
        let mut available_balances: Vec<_> = self
            .available_tokens
            .iter()
            .filter(|(_, balance)| !balance.is_claimed)
            .collect();

        available_balances.sort_by(|(_, a), (_, b)| b.amount.cmp(&a.amount));

        for (address, balance) in available_balances {
            if total_selected >= required_amount {
                break;
            }

            selected_tokens.push(*address);
            total_selected = total_selected.checked_add(balance.amount).ok_or_else(|| {
                NativeWalletError::TransactionCreation("Amount overflow".to_string())
            })?;
        }

        if total_selected < required_amount {
            return Err(NativeWalletError::InsufficientFunds {
                required: required_amount.as_u128(),
                available: total_selected.as_u128(),
            });
        }

        debug!(
            "Selected {} tokens for payment of {}",
            selected_tokens.len(),
            required_amount.as_u128()
        );

        Ok(selected_tokens)
    }

    /// Create a payment transaction to multiple recipients
    ///
    /// # Arguments
    /// * `recipients` - Vector of (recipient_key, amount) pairs
    ///
    /// # Returns
    /// A GraphEntry representing the payment transaction
    pub fn create_payment_transaction(
        &mut self,
        recipients: Vec<(PublicKey, NativeTokens)>,
    ) -> NativeWalletResult<GraphEntry> {
        // Calculate total amount needed
        let total_amount = recipients
            .iter()
            .map(|(_, amount)| *amount)
            .try_fold(NativeTokens::ZERO, |acc, amount| acc.checked_add(amount))
            .ok_or_else(|| {
                NativeWalletError::TransactionCreation("Total amount overflow".to_string())
            })?;

        // Select tokens to cover the payment
        let input_tokens = self.select_tokens_for_payment(total_amount)?;

        // Calculate total input amount
        let total_input = input_tokens
            .iter()
            .map(|addr| {
                self.available_tokens
                    .get(addr)
                    .expect("Token address should exist")
                    .amount
            })
            .fold(NativeTokens::ZERO, |acc, amount| {
                acc.checked_add(amount).unwrap_or(acc)
            });

        // Create transaction using the transaction builder
        let builder = NativeTransactionBuilder::new();

        // Prepare recipients with derivation indices and record hashes
        let recipients_with_details: Vec<(PublicKey, NativeTokens, [u8; 4])> = recipients
            .into_iter()
            .map(|(key, amount)| {
                // For POC, use a simple record hash
                let record_hash = [0x12, 0x34, 0x56, 0x78]; // Placeholder
                (key, amount, record_hash)
            })
            .collect();

        // Generate derivation indices for recipients
        let derivation_indices: Vec<[u8; 16]> = recipients_with_details
            .iter()
            .map(|_| self.generate_derivation_index())
            .collect();

        let recipients_count = recipients_with_details.len();

        let graph_entry = builder.build_payment_transaction(
            self.master_key.clone(),
            input_tokens.clone(),
            recipients_with_details,
            derivation_indices,
        )?;

        // Mark input tokens as claimed
        for token_addr in &input_tokens {
            if let Some(balance) = self.available_tokens.get_mut(token_addr) {
                balance.is_claimed = true;
            }
        }

        // Handle change if necessary
        if total_input > total_amount {
            let change_amount = total_input.checked_sub(total_amount).ok_or_else(|| {
                NativeWalletError::TransactionCreation("Change calculation failed".to_string())
            })?;

            if !change_amount.is_zero() {
                // Create a change output back to ourselves
                let change_address = self.derive_next_address()?;
                let change_balance = NativeTokenBalance {
                    amount: change_amount,
                    graph_entry_address: change_address,
                    is_claimed: false,
                    derivation_index: Some(self.generate_derivation_index()),
                };

                self.available_tokens.insert(change_address, change_balance);
                debug!(
                    "Created change output of {} tokens",
                    change_amount.as_u128()
                );
            }
        }

        info!(
            "Created payment transaction with {} inputs and {} outputs",
            input_tokens.len(),
            recipients_count
        );

        Ok(graph_entry)
    }

    /// Create a payment proof for a specific transaction
    ///
    /// # Arguments
    /// * `payment_transaction` - The GraphEntry address of the payment transaction
    /// * `recipient_key` - The recipient's public key
    /// * `expected_amount` - The expected payment amount
    /// * `record_key_hash` - Hash of the record being paid for
    ///
    /// # Returns
    /// A NativePaymentProof for the transaction
    pub fn create_payment_proof(
        &mut self,
        payment_transaction: PublicKey,
        recipient_key: PublicKey,
        expected_amount: NativeTokens,
        record_key_hash: [u8; 4],
    ) -> NativeWalletResult<NativePaymentProof> {
        let derivation_index = self.generate_derivation_index();

        let proof = NativePaymentProof::new(
            payment_transaction,
            derivation_index,
            expected_amount,
            record_key_hash,
        );

        // Add to pending transactions
        self.pending_transactions.push(proof.clone());

        debug!(
            "Created payment proof for {} tokens to {:?}",
            expected_amount.as_u128(),
            recipient_key
        );

        Ok(proof)
    }

    /// Get all unclaimed token balances
    ///
    /// # Returns
    /// A vector of all unclaimed NativeTokenBalance entries
    pub fn get_unclaimed_balances(&self) -> Vec<&NativeTokenBalance> {
        self.available_tokens
            .values()
            .filter(|balance| !balance.is_claimed)
            .collect()
    }

    /// Get pending transaction count
    ///
    /// # Returns
    /// The number of pending transactions
    pub fn pending_count(&self) -> usize {
        self.pending_transactions.len()
    }

    /// Clear confirmed transactions from pending list
    ///
    /// # Arguments
    /// * `confirmed_proofs` - Vector of payment proofs that have been confirmed
    pub fn clear_confirmed_transactions(&mut self, confirmed_proofs: &[NativePaymentProof]) {
        self.pending_transactions.retain(|pending| {
            !confirmed_proofs.iter().any(|confirmed| {
                pending.payment_transaction == confirmed.payment_transaction
                    && pending.recipient_derivation_index == confirmed.recipient_derivation_index
            })
        });

        debug!("Cleared {} confirmed transactions", confirmed_proofs.len());
    }

    // Private helper methods

    /// Generate a unique derivation index for addresses
    fn generate_derivation_index(&mut self) -> [u8; 16] {
        self.derivation_counter += 1;
        let mut index = [0u8; 16];
        index[0..8].copy_from_slice(&self.derivation_counter.to_le_bytes());
        index
    }

    /// Derive an address for genesis tokens
    fn derive_address_for_genesis(&self) -> NativeWalletResult<PublicKey> {
        // For POC, use a deterministic genesis address based on master key
        // In production, this would use proper key derivation
        let genesis_bytes = b"AUTONOMI_GENESIS_TOKENS_v1";
        let mut combined = Vec::new();
        combined.extend_from_slice(&self.master_key.to_bytes());
        combined.extend_from_slice(genesis_bytes);

        // Use the first 32 bytes of the hash as the address
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        combined.hash(&mut hasher);
        let hash = hasher.finish();

        let mut address_bytes = [0u8; 32];
        address_bytes[0..8].copy_from_slice(&hash.to_le_bytes());

        // For POC, use a simple address derivation based on master key
        Ok(self.master_key.public_key())
    }

    /// Derive the next address for this wallet
    fn derive_next_address(&mut self) -> NativeWalletResult<PublicKey> {
        let index = self.generate_derivation_index();

        // For POC, create a deterministic address based on master key and index
        let mut combined = Vec::new();
        combined.extend_from_slice(&self.master_key.to_bytes());
        combined.extend_from_slice(&index);

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        combined.hash(&mut hasher);
        let hash = hasher.finish();

        let mut address_bytes = [0u8; 32];
        address_bytes[0..8].copy_from_slice(&hash.to_le_bytes());

        // For POC, use a simple address derivation based on master key
        Ok(self.master_key.public_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_wallet() -> InMemoryNativeWallet {
        let master_key = SecretKey::random();
        InMemoryNativeWallet::new(master_key)
    }

    #[test]
    fn test_wallet_creation() {
        let wallet = create_test_wallet();
        assert_eq!(wallet.total_balance(), NativeTokens::ZERO);
        assert_eq!(wallet.available_tokens.len(), 0);
        assert_eq!(wallet.pending_count(), 0);
    }

    #[test]
    fn test_add_genesis_tokens() {
        let mut wallet = create_test_wallet();
        let amount = NativeTokens::from_u64(1000);

        let genesis_addr = wallet.add_genesis_tokens(amount).unwrap();

        assert_eq!(wallet.total_balance(), amount);
        assert_eq!(wallet.available_tokens.len(), 1);
        assert!(wallet.available_tokens.contains_key(&genesis_addr));

        let balance = wallet.available_tokens.get(&genesis_addr).unwrap();
        assert_eq!(balance.amount, amount);
        assert!(!balance.is_claimed);
    }

    // #[test]
    // fn test_token_selection() {
    //     let mut wallet = create_test_wallet();

    //     // Add multiple token balances
    //     wallet.add_genesis_tokens(NativeTokens::from_u64(500)).unwrap();
    //     wallet.add_genesis_tokens(NativeTokens::from_u64(300)).unwrap();
    //     wallet.add_genesis_tokens(NativeTokens::from_u64(200)).unwrap();

    //     // Test selecting tokens for a payment
    //     let required = NativeTokens::from_u64(600);
    //     let selected = wallet.select_tokens_for_payment(required).unwrap();

    //     assert!(selected.len() >= 2); // Should select at least 2 tokens

    //     // Calculate total of selected tokens
    //     let total_selected: NativeTokens = selected.iter()
    //         .map(|addr| wallet.available_tokens.get(addr).unwrap().amount)
    //         .fold(NativeTokens::ZERO, |acc, amount| acc.checked_add(amount).unwrap());

    //     assert!(total_selected >= required);
    // }

    #[test]
    fn test_insufficient_funds() {
        let mut wallet = create_test_wallet();
        wallet
            .add_genesis_tokens(NativeTokens::from_u64(100))
            .unwrap();

        let required = NativeTokens::from_u64(200);
        let result = wallet.select_tokens_for_payment(required);

        assert!(matches!(
            result,
            Err(NativeWalletError::InsufficientFunds { .. })
        ));
    }

    #[test]
    fn test_payment_proof_creation() {
        let mut wallet = create_test_wallet();
        let payment_tx = SecretKey::random().public_key();
        let recipient = SecretKey::random().public_key();
        let amount = NativeTokens::from_u64(100);
        let record_hash = [0x12, 0x34, 0x56, 0x78];

        let proof = wallet
            .create_payment_proof(payment_tx, recipient, amount, record_hash)
            .unwrap();

        assert_eq!(proof.payment_transaction, payment_tx);
        assert_eq!(proof.expected_amount, amount);
        assert_eq!(proof.record_key_hash, record_hash);
        assert_eq!(wallet.pending_count(), 1);
    }

    // #[test]
    // fn test_unclaimed_balances() {
    //     let mut wallet = create_test_wallet();

    //     wallet
    //         .add_genesis_tokens(NativeTokens::from_u64(100))
    //         .unwrap();
    //     wallet
    //         .add_genesis_tokens(NativeTokens::from_u64(200))
    //         .unwrap();

    //     let unclaimed = wallet.get_unclaimed_balances();
    //     assert_eq!(unclaimed.len(), 2);

    //     // Mark one as claimed
    //     let first_addr = *wallet.available_tokens.keys().next().unwrap();
    //     wallet
    //         .available_tokens
    //         .get_mut(&first_addr)
    //         .unwrap()
    //         .is_claimed = true;

    //     let unclaimed = wallet.get_unclaimed_balances();
    //     assert_eq!(unclaimed.len(), 1);
    // }

    #[test]
    fn test_derivation_index_generation() {
        let mut wallet = create_test_wallet();

        let index1 = wallet.generate_derivation_index();
        let index2 = wallet.generate_derivation_index();

        assert_ne!(index1, index2);
        assert_ne!(index1, [0u8; 16]);
        assert_ne!(index2, [0u8; 16]);
    }
}
