// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Native Token Transaction Builder
//! 
//! This module provides functionality to build GraphEntry-based native token
//! transactions for the Autonomi Network.

use ant_protocol::storage::{GraphEntry, GraphContent, NativeTokens};
use bls::{PublicKey, SecretKey};
use crate::networking::Network;
use super::{NativeWalletError, NativeWalletResult};
use tracing::{debug, info, warn};

/// Builder for creating native token transactions using GraphEntry structures
/// 
/// This builder creates GraphEntry objects that represent native token transactions
/// according to the native token design document specification.
#[derive(Debug, Clone)]
pub struct NativeTransactionBuilder {
    /// Optional network client for validating addresses and publishing transactions
    pub network_client: Option<Network>,
}

impl NativeTransactionBuilder {
    /// Create a new transaction builder without network client
    /// 
    /// # Returns
    /// A new NativeTransactionBuilder instance
    pub fn new() -> Self {
        Self {
            network_client: None,
        }
    }

    /// Create a new transaction builder with network client
    /// 
    /// # Arguments
    /// * `network` - The network client for validating and publishing transactions
    /// 
    /// # Returns
    /// A new NativeTransactionBuilder instance with network support
    pub fn with_network(network: Network) -> Self {
        Self {
            network_client: Some(network),
        }
    }

    /// Build a payment transaction GraphEntry
    /// 
    /// # Arguments
    /// * `payer` - The secret key of the payer (transaction signer)
    /// * `parent_tokens` - Vector of input GraphEntry addresses to spend
    /// * `recipients` - Vector of (recipient_key, amount, record_hash) tuples
    /// * `derivation_indices` - Vector of derivation indices for each recipient
    /// 
    /// # Returns
    /// A GraphEntry representing the payment transaction
    /// 
    /// # Transaction Structure
    /// The created GraphEntry follows this structure:
    /// - owner: payer public key
    /// - parents: parent_tokens (input GraphEntries)
    /// - content: [monetary_id(4) + tbd(16) + total_input(12)]
    /// - descendants: recipients with payment details
    /// - signature: signed with payer key
    pub fn build_payment_transaction(
        &self,
        payer: SecretKey,
        parent_tokens: Vec<PublicKey>,
        recipients: Vec<(PublicKey, NativeTokens, [u8; 4])>, // (node_key, amount, record_hash)
        derivation_indices: Vec<[u8; 16]>,
    ) -> NativeWalletResult<GraphEntry> {
        if recipients.len() != derivation_indices.len() {
            return Err(NativeWalletError::TransactionCreation(
                "Recipients and derivation indices length mismatch".to_string()
            ));
        }

        // Calculate total input amount (for POC, we'll estimate based on recipients)
        let total_output = recipients.iter()
            .map(|(_, amount, _)| *amount)
            .try_fold(NativeTokens::ZERO, |acc, amount| acc.checked_add(amount))
            .ok_or_else(|| NativeWalletError::TransactionCreation("Output amount overflow".to_string()))?;

        // For POC, assume input equals output (no fees)
        let total_input = total_output;

        // Build the GraphEntry content according to the native token specification
        let content = self.build_transaction_content(total_input)?;

        // Build descendants (payment outputs)
        let descendants = self.build_payment_descendants(&recipients, &derivation_indices)?;

        let graph_entry = GraphEntry::new(&payer, parent_tokens.clone(), content, descendants);

        // Validate the transaction
        self.validate_transaction(&graph_entry)?;

        info!(
            "Built payment transaction with {} inputs, {} outputs, total amount: {}",
            parent_tokens.len(),
            recipients.len(),
            total_input.as_u128()
        );

        Ok(graph_entry)
    }

    /// Build a simple transfer transaction between two parties
    /// 
    /// # Arguments
    /// * `payer` - The secret key of the payer
    /// * `parent_tokens` - Input GraphEntry addresses
    /// * `recipient` - The recipient's public key
    /// * `amount` - The amount to transfer
    /// * `record_hash` - Hash of the record being paid for
    /// 
    /// # Returns
    /// A GraphEntry representing the transfer transaction
    pub fn build_simple_transfer(
        &self,
        payer: SecretKey,
        parent_tokens: Vec<PublicKey>,
        recipient: PublicKey,
        amount: NativeTokens,
        record_hash: [u8; 4],
    ) -> NativeWalletResult<GraphEntry> {
        let recipients = vec![(recipient, amount, record_hash)];
        let derivation_indices = vec![[1u8; 16]]; // Simple derivation index

        self.build_payment_transaction(payer, parent_tokens, recipients, derivation_indices)
    }

    /// Build a batch payment transaction to multiple recipients
    /// 
    /// # Arguments
    /// * `payer` - The secret key of the payer
    /// * `parent_tokens` - Input GraphEntry addresses
    /// * `payments` - Vector of (recipient, amount, record_hash) for each payment
    /// 
    /// # Returns
    /// A GraphEntry representing the batch payment transaction
    pub fn build_batch_payment(
        &self,
        payer: SecretKey,
        parent_tokens: Vec<PublicKey>,
        payments: Vec<(PublicKey, NativeTokens, [u8; 4])>,
    ) -> NativeWalletResult<GraphEntry> {
        // Generate unique derivation indices for each payment
        let derivation_indices: Vec<[u8; 16]> = (0..payments.len())
            .map(|i| {
                let mut index = [0u8; 16];
                index[0..8].copy_from_slice(&(i as u64).to_le_bytes());
                index[8..16].copy_from_slice(&std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_le_bytes());
                index
            })
            .collect();

        self.build_payment_transaction(payer, parent_tokens, payments, derivation_indices)
    }

    // Private helper methods

    /// Build the transaction content according to native token specification
    /// 
    /// Content structure: [monetary_id(4) + tbd(16) + total_input(12)]
    /// - Bytes 0-3: Monetary ID (0x00000000 for Autonomi native token)
    /// - Bytes 4-19: TBD (reserved for future use)
    /// - Bytes 20-31: Total input amount (u96 in little-endian)
    fn build_transaction_content(&self, total_input: NativeTokens) -> NativeWalletResult<GraphContent> {
        let mut content = [0u8; 32];

        // Bytes 0-3: Monetary ID (0x00000000 for Autonomi native token)
        let monetary_id = 0u32;
        content[0..4].copy_from_slice(&monetary_id.to_le_bytes());

        // Bytes 4-19: TBD (reserved for future use, set to zero for now)
        // Already zeroed by default

        // Bytes 20-31: Total input amount as u96 (12 bytes)
        let amount_bytes = total_input.to_le_bytes();
        content[20..32].copy_from_slice(&amount_bytes);

        debug!("Built transaction content with monetary_id={}, total_input={}", 
               monetary_id, total_input.as_u128());

        Ok(content)
    }

    /// Build payment descendants for the transaction
    /// 
    /// Each descendant represents a payment output with the structure:
    /// [record_key_hash(4) + derivation_index(16) + amount(12)]
    fn build_payment_descendants(
        &self,
        recipients: &[(PublicKey, NativeTokens, [u8; 4])],
        derivation_indices: &[[u8; 16]],
    ) -> NativeWalletResult<Vec<(PublicKey, GraphContent)>> {
        let mut descendants = Vec::new();

        for (i, (recipient_key, amount, record_hash)) in recipients.iter().enumerate() {
            let derivation_index = derivation_indices.get(i)
                .ok_or_else(|| NativeWalletError::TransactionCreation(
                    format!("Missing derivation index for recipient {i}")
                ))?;

            let mut descendant_content = [0u8; 32];

            // Bytes 0-3: Record key hash
            descendant_content[0..4].copy_from_slice(record_hash);

            // Bytes 4-19: Derivation index
            descendant_content[4..20].copy_from_slice(derivation_index);

            // Bytes 20-31: Payment amount as u96 (12 bytes)
            let amount_bytes = amount.to_le_bytes();
            descendant_content[20..32].copy_from_slice(&amount_bytes);

            descendants.push((*recipient_key, descendant_content));

            debug!("Built payment descendant: recipient={:?}, amount={}, record_hash={:?}",
                   recipient_key, amount.as_u128(), record_hash);
        }

        Ok(descendants)
    }

    /// Validate the transaction for consistency and correctness
    fn validate_transaction(&self, graph_entry: &GraphEntry) -> NativeWalletResult<()> {
        // Check that the GraphEntry has a valid signature
        if !graph_entry.verify_signature() {
            return Err(NativeWalletError::GraphEntryValidation(
                "Invalid signature".to_string()
            ));
        }

        // Validate content structure
        if graph_entry.content.len() != 32 {
            return Err(NativeWalletError::GraphEntryValidation(
                format!("Invalid content length: expected 32, got {}", graph_entry.content.len())
            ));
        }

        // Check monetary ID
        let monetary_id = u32::from_le_bytes(
            graph_entry.content[0..4].try_into()
                .map_err(|_| NativeWalletError::GraphEntryValidation("Invalid monetary ID".to_string()))?
        );
        
        if monetary_id != 0 {
            return Err(NativeWalletError::GraphEntryValidation(
                format!("Invalid monetary ID: expected 0, got {monetary_id}")
            ));
        }

        // Validate descendant content structure
        for (i, (_, descendant_content)) in graph_entry.descendants.iter().enumerate() {
            if descendant_content.len() != 32 {
                return Err(NativeWalletError::GraphEntryValidation(
                    format!("Invalid descendant {} content length: expected 32, got {}", 
                           i, descendant_content.len())
                ));
            }
        }

        // Validate that we have at least one parent or this is a genesis transaction
        if graph_entry.parents.is_empty() && !graph_entry.descendants.is_empty() {
            warn!("Transaction has no parents but has descendants - assuming genesis transaction");
        }

        debug!("Transaction validation passed: {} parents, {} descendants", 
               graph_entry.parents.len(), graph_entry.descendants.len());

        Ok(())
    }
}

impl Default for NativeTransactionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_keys() -> (SecretKey, PublicKey) {
        let secret = SecretKey::random();
        let public = secret.public_key();
        (secret, public)
    }

    #[test]
    fn test_transaction_builder_creation() {
        let builder = NativeTransactionBuilder::new();
        assert!(builder.network_client.is_none());
        
        // Test default creation
        let default_builder = NativeTransactionBuilder::default();
        assert!(default_builder.network_client.is_none());
    }

    #[test]
    fn test_content_building() {
        let builder = NativeTransactionBuilder::new();
        let total_input = NativeTokens::from_u64(1000);
        
        let content = builder.build_transaction_content(total_input).unwrap();
        
        assert_eq!(content.len(), 32);
        
        // Check monetary ID (bytes 0-3)
        let monetary_id = u32::from_le_bytes(content[0..4].try_into().unwrap());
        assert_eq!(monetary_id, 0);
        
        // Check total input amount (bytes 20-31)
        let amount_bytes: [u8; 12] = content[20..32].try_into().unwrap();
        let recovered_amount = NativeTokens::from_le_bytes(amount_bytes).unwrap();
        assert_eq!(recovered_amount, total_input);
    }

    #[test]
    fn test_descendant_building() {
        let builder = NativeTransactionBuilder::new();
        let (_, payer_public) = create_test_keys();
        let recipients = vec![
            (payer_public, NativeTokens::from_u64(500), [0x12, 0x34, 0x56, 0x78]),
            (payer_public, NativeTokens::from_u64(300), [0xAB, 0xCD, 0xEF, 0x12]),
        ];
        let derivation_indices = vec![
            [0xAA; 16],
            [0xBB; 16],
        ];
        
        let descendants = builder.build_payment_descendants(&recipients, &derivation_indices).unwrap();
        
        assert_eq!(descendants.len(), 2);
        
        // Check first descendant
        let (key1, content1) = &descendants[0];
        assert_eq!(*key1, recipients[0].0);
        assert_eq!(content1[0..4], [0x12, 0x34, 0x56, 0x78]); // Record hash
        assert_eq!(content1[4..20], [0xAA; 16]); // Derivation index
        
        // Check amount in first descendant
        let amount_bytes: [u8; 12] = content1[20..32].try_into().unwrap();
        let recovered_amount = NativeTokens::from_le_bytes(amount_bytes).unwrap();
        assert_eq!(recovered_amount, recipients[0].1);
    }

    #[tokio::test]
    async fn test_simple_transfer() {
        let builder = NativeTransactionBuilder::new();
        let (payer_secret, _) = create_test_keys();
        let recipient = SecretKey::random().public_key();
        let parent_tokens = vec![SecretKey::random().public_key()];
        let amount = NativeTokens::from_u64(1000);
        let record_hash = [0x12, 0x34, 0x56, 0x78];
        
        let graph_entry = builder.build_simple_transfer(
            payer_secret,
            parent_tokens.clone(),
            recipient,
            amount,
            record_hash,
        ).unwrap();
        
        assert_eq!(graph_entry.parents, parent_tokens);
        assert_eq!(graph_entry.descendants.len(), 1);
        // Signature is no longer Optional, so just check it exists
        // assert!(graph_entry.signature.is_some());
        
        // Validate monetary ID
        let monetary_id = u32::from_le_bytes(graph_entry.content[0..4].try_into().unwrap());
        assert_eq!(monetary_id, 0);
    }

    #[tokio::test]
    async fn test_batch_payment() {
        let builder = NativeTransactionBuilder::new();
        let (payer_secret, _) = create_test_keys();
        let parent_tokens = vec![SecretKey::random().public_key()];
        let payments = vec![
            (SecretKey::random().public_key(), NativeTokens::from_u64(500), [0x11, 0x22, 0x33, 0x44]),
            (SecretKey::random().public_key(), NativeTokens::from_u64(300), [0x55, 0x66, 0x77, 0x88]),
            (SecretKey::random().public_key(), NativeTokens::from_u64(200), [0x99, 0xAA, 0xBB, 0xCC]),
        ];
        
        let graph_entry = builder.build_batch_payment(
            payer_secret,
            parent_tokens.clone(),
            payments.clone(),
        ).unwrap();
        
        assert_eq!(graph_entry.parents, parent_tokens);
        assert_eq!(graph_entry.descendants.len(), payments.len());
        // Signature is no longer Optional, so just check it exists
        // assert!(graph_entry.signature.is_some());
        
        // Verify each payment in descendants
        for (i, (expected_recipient, expected_amount, expected_hash)) in payments.iter().enumerate() {
            let (actual_recipient, descendant_content) = &graph_entry.descendants[i];
            assert_eq!(actual_recipient, expected_recipient);
            
            // Check record hash
            assert_eq!(descendant_content[0..4], *expected_hash);
            
            // Check amount
            let amount_bytes: [u8; 12] = descendant_content[20..32].try_into().unwrap();
            let actual_amount = NativeTokens::from_le_bytes(amount_bytes).unwrap();
            assert_eq!(actual_amount, *expected_amount);
        }
    }
}
