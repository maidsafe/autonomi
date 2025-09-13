// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::address::GraphEntryAddress;
use super::native_tokens::NativeTokens;
use bls::SecretKey;
use serde::{Deserialize, Serialize};

// re-exports
pub use bls::{PublicKey, Signature};

/// Content of a graph, limited to 32 bytes
pub type GraphContent = [u8; 32];

/// Payment details extracted from a descendant's content in native token transactions.
/// 
/// This structure represents the parsed payment information from a GraphEntry
/// descendant when used for native token payments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentDetails {
    /// First 4 bytes of the hash of the record key being paid for.
    /// This links the payment to the specific data being stored.
    pub record_key_hash: [u8; 4],
    
    /// BLS key derivation index used to generate the recipient's address.
    /// This allows the recipient to derive the private key needed to claim the payment.
    pub derivation_index: [u8; 16],
    
    /// The payment amount in native tokens.
    pub amount: NativeTokens,
}

/// A generic GraphEntry on the Network.
///
/// Graph entries are stored at the owner's public key. Note that there can only be one graph entry per owner.
/// Graph entries can be linked to other graph entries as parents or descendants.
/// Applications are free to define the meaning of these links, those are not enforced by the protocol.
/// The protocol only ensures that the graph entry is immutable once uploaded and that the signature is valid and matches the owner.
///
/// For convenience it is advised to make use of BLS key derivation to create multiple graph entries from a single key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Hash, Ord, PartialOrd)]
pub struct GraphEntry {
    /// The owner of the graph. Note that graph entries are stored at the owner's public key
    pub owner: PublicKey,
    /// Other graph entries that this graph entry refers to as parents
    pub parents: Vec<PublicKey>,
    /// The content of the graph entry
    pub content: GraphContent,
    /// Other graph entries that this graph entry refers to as descendants/outputs along with some data associated to each one
    pub descendants: Vec<(PublicKey, GraphContent)>,
    /// signs the above 4 fields with the owners key
    pub signature: Signature,
}

impl std::fmt::Debug for GraphEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEntry")
            .field("owner", &self.owner.to_hex())
            .field(
                "parents",
                &self.parents.iter().map(|p| p.to_hex()).collect::<Vec<_>>(),
            )
            .field("content", &hex::encode(self.content))
            .field(
                "descendants",
                &self
                    .descendants
                    .iter()
                    .map(|(p, c)| format!("{}: {}", p.to_hex(), hex::encode(c)))
                    .collect::<Vec<_>>(),
            )
            .field("signature", &hex::encode(self.signature.to_bytes()))
            .finish()
    }
}

impl GraphEntry {
    /// Maximum size of a graph entry: 100KB
    pub const MAX_SIZE: usize = 100 * 1024;

    /// Create a new graph entry, signing it with the provided secret key.
    pub fn new(
        owner: &SecretKey,
        parents: Vec<PublicKey>,
        content: GraphContent,
        descendants: Vec<(PublicKey, GraphContent)>,
    ) -> Self {
        let key = owner;
        let owner = key.public_key();
        let signature = key.sign(Self::bytes_to_sign(
            &owner,
            &parents,
            &content,
            &descendants,
        ));
        Self {
            owner,
            parents,
            content,
            descendants,
            signature,
        }
    }

    /// Create a new graph entry, with the signature already calculated.
    pub fn new_with_signature(
        owner: PublicKey,
        parents: Vec<PublicKey>,
        content: GraphContent,
        descendants: Vec<(PublicKey, GraphContent)>,
        signature: Signature,
    ) -> Self {
        Self {
            owner,
            parents,
            content,
            descendants,
            signature,
        }
    }

    /// Get the bytes that the signature is calculated from.
    pub fn bytes_to_sign(
        owner: &PublicKey,
        parents: &[PublicKey],
        content: &[u8],
        descendants: &[(PublicKey, GraphContent)],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&owner.to_bytes());
        bytes.extend_from_slice("parent".as_bytes());
        bytes.extend_from_slice(
            &parents
                .iter()
                .map(|p| p.to_bytes())
                .collect::<Vec<_>>()
                .concat(),
        );
        bytes.extend_from_slice("content".as_bytes());
        bytes.extend_from_slice(content);
        bytes.extend_from_slice("descendants".as_bytes());
        bytes.extend_from_slice(
            &descendants
                .iter()
                .flat_map(|(p, c)| [&p.to_bytes(), c.as_slice()].concat())
                .collect::<Vec<_>>(),
        );
        bytes
    }

    pub fn address(&self) -> GraphEntryAddress {
        GraphEntryAddress::new(self.owner)
    }

    /// Get the bytes that the signature is calculated from.
    pub fn bytes_for_signature(&self) -> Vec<u8> {
        Self::bytes_to_sign(&self.owner, &self.parents, &self.content, &self.descendants)
    }

    /// Verify the signature of the graph entry
    pub fn verify_signature(&self) -> bool {
        self.owner
            .verify(&self.signature, self.bytes_for_signature())
    }

    /// Size of the graph entry
    pub fn size(&self) -> usize {
        size_of::<GraphEntry>()
            + self
                .descendants
                .iter()
                .map(|(p, c)| p.to_bytes().len() + c.len())
                .sum::<usize>()
            + self
                .parents
                .iter()
                .map(|p| p.to_bytes().len())
                .sum::<usize>()
    }

    /// Returns true if the graph entry is too big
    pub fn is_too_big(&self) -> bool {
        self.size() > Self::MAX_SIZE
    }

    // ===== Native Token Payment Methods =====
    // These methods enable parsing GraphEntry content for native token payments

    /// Extracts the monetary ID from the GraphEntry content field.
    /// 
    /// According to the native token design:
    /// - Bytes 0-3: Monetary ID (u32 little-endian)
    /// - Value 0x00000000 is reserved for Autonomi native tokens
    /// - Other values represent different monetary schemes
    /// 
    /// # Returns
    /// The monetary scheme identifier, or 0 if parsing fails
    pub fn monetary_id(&self) -> u32 {
        u32::from_le_bytes(self.content[0..4].try_into().unwrap_or_default())
    }

    /// Extracts the total input amount from the GraphEntry content field.
    /// 
    /// According to the native token design:
    /// - Bytes 20-31: Total Input Amount (u96 little-endian, 12 bytes)
    /// - Represents the sum of all parent token values being consumed
    /// - Zero for genesis tokens (they create value from nothing)
    /// 
    /// # Returns
    /// The total input amount as NativeTokens, or zero if parsing fails
    pub fn total_input_amount(&self) -> NativeTokens {
        // Extract 12 bytes for u96 amount
        let mut amount_bytes = [0u8; 12];
        if self.content.len() >= 32 {
            amount_bytes.copy_from_slice(&self.content[20..32]);
        }
        
        NativeTokens::from_le_bytes(amount_bytes).unwrap_or(NativeTokens::ZERO)
    }

    /// Checks if this GraphEntry represents a native token transaction.
    /// 
    /// Returns true if the monetary ID is 0x00000000 (Autonomi native token).
    /// This identifies GraphEntries that use the native token payment system
    /// rather than external payment methods like EVM.
    /// 
    /// # Returns
    /// `true` if this is a native token GraphEntry, `false` otherwise
    pub fn is_native_token(&self) -> bool {
        self.monetary_id() == 0
    }

    /// Parses payment details from a descendant's content.
    /// 
    /// For native token payments, descendant content format is:
    /// - Bytes 0-3: Record Key Hash (first 4 bytes of data address hash)
    /// - Bytes 4-19: BLS Key Derivation Index (16 bytes)
    /// - Bytes 20-31: Payment Amount (u96 little-endian, 12 bytes)
    /// 
    /// # Arguments
    /// * `descendant` - The (PublicKey, GraphContent) tuple from descendants
    /// 
    /// # Returns
    /// * `Some(PaymentDetails)` if parsing succeeds
    /// * `None` if the content format is invalid
    pub fn parse_descendant_payment(&self, descendant: &(PublicKey, GraphContent)) -> Option<PaymentDetails> {
        let (_recipient_key, content) = descendant;
        
        // Ensure we have enough bytes
        if content.len() != 32 {
            return None;
        }
        
        // Extract record key hash (first 4 bytes)
        let mut record_key_hash = [0u8; 4];
        record_key_hash.copy_from_slice(&content[0..4]);
        
        // Extract derivation index (bytes 4-19)
        let mut derivation_index = [0u8; 16];
        derivation_index.copy_from_slice(&content[4..20]);
        
        // Extract payment amount (bytes 20-31)
        let mut amount_bytes = [0u8; 12];
        amount_bytes.copy_from_slice(&content[20..32]);
        
        let amount = NativeTokens::from_le_bytes(amount_bytes).ok()?;
        
        Some(PaymentDetails {
            record_key_hash,
            derivation_index,
            amount,
        })
    }

    /// Calculates the total amount paid to a specific recipient in this GraphEntry.
    /// 
    /// This method sums up all payments to the given recipient public key
    /// across all descendants. Used for parent validation to ensure
    /// claimed input amounts match actual payouts.
    /// 
    /// # Arguments
    /// * `recipient` - The public key of the recipient to calculate payments for
    /// 
    /// # Returns
    /// * `Ok(NativeTokens)` - Total amount paid to the recipient
    /// * `Err(String)` - If payment parsing fails
    pub fn descendant_payout(&self, recipient: PublicKey) -> Result<NativeTokens, String> {
        let mut total = NativeTokens::ZERO;
        
        for descendant in &self.descendants {
            if descendant.0 == recipient
                && let Some(payment) = self.parse_descendant_payment(descendant) {
                    total = total.checked_add(payment.amount)
                        .ok_or("Amount overflow in descendant payout calculation")?;
                }
        }
        
        Ok(total)
    }

    /// Extracts the total amount claimed as input from the content field.
    /// 
    /// This is a convenience method that returns the total input amount
    /// as parsed from bytes 20-31 of the content field.
    /// 
    /// # Returns
    /// The total input amount claimed in this GraphEntry
    pub fn input_amount(&self) -> NativeTokens {
        self.total_input_amount()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::native_tokens::NativeTokens;

    fn create_test_key() -> bls::SecretKey {
        bls::SecretKey::random()
    }

    fn create_native_token_content(monetary_id: u32, total_input: u128) -> GraphContent {
        let mut content = [0u8; 32];
        
        // Set monetary ID (bytes 0-3)
        content[0..4].copy_from_slice(&monetary_id.to_le_bytes());
        
        // Set total input amount (bytes 20-31, u96 as u128)
        let native_tokens = NativeTokens::new(total_input);
        let amount_bytes = native_tokens.to_le_bytes();
        content[20..32].copy_from_slice(&amount_bytes);
        
        content
    }

    fn create_payment_descendant_content(
        record_hash: [u8; 4],
        derivation_index: [u8; 16],
        amount: u128,
    ) -> GraphContent {
        let mut content = [0u8; 32];
        
        // Record key hash (bytes 0-3)
        content[0..4].copy_from_slice(&record_hash);
        
        // Derivation index (bytes 4-19)
        content[4..20].copy_from_slice(&derivation_index);
        
        // Payment amount (bytes 20-31)
        let native_tokens = NativeTokens::new(amount);
        let amount_bytes = native_tokens.to_le_bytes();
        content[20..32].copy_from_slice(&amount_bytes);
        
        content
    }

    #[test]
    fn test_monetary_id_parsing() {
        let key = create_test_key();
        
        // Test native token (monetary ID = 0)
        let content = create_native_token_content(0, 1000);
        let entry = GraphEntry::new(&key, vec![], content, vec![]);
        assert_eq!(entry.monetary_id(), 0);
        assert!(entry.is_native_token());
        
        // Test other monetary scheme
        let content = create_native_token_content(0x12345678, 1000);
        let entry = GraphEntry::new(&key, vec![], content, vec![]);
        assert_eq!(entry.monetary_id(), 0x12345678);
        assert!(!entry.is_native_token());
    }

    #[test]
    fn test_total_input_amount_parsing() {
        let key = create_test_key();
        
        // Test various amounts
        let test_amounts = [0, 1, 1000, 1_000_000, u64::MAX as u128];
        
        for amount in test_amounts {
            let content = create_native_token_content(0, amount);
            let entry = GraphEntry::new(&key, vec![], content, vec![]);
            assert_eq!(entry.total_input_amount(), NativeTokens::new(amount));
            assert_eq!(entry.input_amount(), NativeTokens::new(amount));
        }
    }

    #[test]
    fn test_parse_descendant_payment() {
        let key = create_test_key();
        let recipient_key = create_test_key().public_key();
        
        let record_hash = [0x12, 0x34, 0x56, 0x78];
        let derivation_index = [0xAB; 16];
        let amount = 5000u128;
        
        let descendant_content = create_payment_descendant_content(
            record_hash,
            derivation_index,
            amount,
        );
        
        let content = create_native_token_content(0, 10000);
        let descendants = vec![(recipient_key, descendant_content)];
        let entry = GraphEntry::new(&key, vec![], content, descendants);
        
        let payment = entry.parse_descendant_payment(&entry.descendants[0]).unwrap();
        
        assert_eq!(payment.record_key_hash, record_hash);
        assert_eq!(payment.derivation_index, derivation_index);
        assert_eq!(payment.amount, NativeTokens::new(amount));
    }

    #[test]
    fn test_descendant_payout_calculation() {
        let key = create_test_key();
        let recipient1 = create_test_key().public_key();
        let recipient2 = create_test_key().public_key();
        
        // Create multiple payments to recipient1 and one to recipient2
        let descendants = vec![
            (recipient1, create_payment_descendant_content([1, 2, 3, 4], [0; 16], 1000)),
            (recipient2, create_payment_descendant_content([5, 6, 7, 8], [1; 16], 500)),
            (recipient1, create_payment_descendant_content([9, 10, 11, 12], [2; 16], 2000)),
        ];
        
        let content = create_native_token_content(0, 3500);
        let entry = GraphEntry::new(&key, vec![], content, descendants);
        
        // Test recipient1 gets 1000 + 2000 = 3000
        let payout1 = entry.descendant_payout(recipient1).unwrap();
        assert_eq!(payout1, NativeTokens::new(3000));
        
        // Test recipient2 gets 500
        let payout2 = entry.descendant_payout(recipient2).unwrap();
        assert_eq!(payout2, NativeTokens::new(500));
        
        // Test unknown recipient gets 0
        let unknown_recipient = create_test_key().public_key();
        let payout3 = entry.descendant_payout(unknown_recipient).unwrap();
        assert_eq!(payout3, NativeTokens::ZERO);
    }

    #[test]
    fn test_parse_descendant_payment_edge_cases() {
        let key = create_test_key();
        let recipient_key = create_test_key().public_key();
        
        // Create a valid GraphEntry
        let valid_content = create_payment_descendant_content([1, 2, 3, 4], [0; 16], 1000);
        let descendants = vec![(recipient_key, valid_content)];
        let content = create_native_token_content(0, 1000);
        let entry = GraphEntry::new(&key, vec![], content, descendants);
        
        // Test successful parsing
        assert!(entry.parse_descendant_payment(&entry.descendants[0]).is_some());
        
        // Test with different content that should still work (all zeros)
        let zero_content = [0u8; 32];
        let zero_descendant = (recipient_key, zero_content);
        let zero_payment = entry.parse_descendant_payment(&zero_descendant);
        assert!(zero_payment.is_some());
        assert_eq!(zero_payment.unwrap().amount, NativeTokens::ZERO);
    }

    #[test]
    fn test_edge_cases() {
        let key = create_test_key();
        
        // Test with empty content (all zeros)
        let empty_content = [0u8; 32];
        let entry = GraphEntry::new(&key, vec![], empty_content, vec![]);
        
        assert_eq!(entry.monetary_id(), 0);
        assert!(entry.is_native_token());
        assert_eq!(entry.total_input_amount(), NativeTokens::ZERO);
        
        // Test descendant payout with no descendants
        let unknown_key = create_test_key().public_key();
        assert_eq!(entry.descendant_payout(unknown_key).unwrap(), NativeTokens::ZERO);
    }
}
