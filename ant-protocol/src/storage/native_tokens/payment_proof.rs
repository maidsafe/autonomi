// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::amount::NativeTokens;
use crate::storage::graph::PublicKey;
use serde::{Deserialize, Serialize};

#[cfg(feature = "evm-integration")]
use ant_evm::ProofOfPayment;

/// Native payment proof structure for the Autonomi Network.
///
/// This structure provides proof that a payment was made for data storage
/// using native tokens through the GraphEntry payment system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePaymentProof {
    /// Address of the payment GraphEntry transaction on the network.
    /// This GraphEntry contains the payment details and can be fetched for verification.
    pub payment_transaction: PublicKey,

    /// BLS key derivation index used to generate the recipient's address.
    /// This allows the recipient to derive the private key needed to claim the payment.
    pub recipient_derivation_index: [u8; 16],

    /// Expected amount of native tokens in the payment.
    /// This is used by the recipient to verify they received the correct amount.
    pub expected_amount: NativeTokens,

    /// First 4 bytes of the hash of the record key being paid for.
    /// This links the payment to the specific data being stored.
    pub record_key_hash: [u8; 4],
}

impl NativePaymentProof {
    /// Creates a new native payment proof.
    ///
    /// # Arguments
    /// * `payment_transaction` - The public key address of the payment GraphEntry
    /// * `recipient_derivation_index` - BLS derivation index for the recipient
    /// * `expected_amount` - Amount of native tokens expected in the payment
    /// * `record_key_hash` - Hash of the record key being paid for (first 4 bytes)
    pub fn new(
        payment_transaction: PublicKey,
        recipient_derivation_index: [u8; 16],
        expected_amount: NativeTokens,
        record_key_hash: [u8; 4],
    ) -> Self {
        Self {
            payment_transaction,
            recipient_derivation_index,
            expected_amount,
            record_key_hash,
        }
    }

    /// Returns the payment transaction address.
    pub fn payment_address(&self) -> &PublicKey {
        &self.payment_transaction
    }

    /// Returns the derivation index for the recipient.
    pub fn derivation_index(&self) -> &[u8; 16] {
        &self.recipient_derivation_index
    }

    /// Returns the expected payment amount.
    pub fn amount(&self) -> NativeTokens {
        self.expected_amount
    }

    /// Returns the record key hash.
    pub fn record_hash(&self) -> &[u8; 4] {
        &self.record_key_hash
    }

    /// Validates that the payment proof contains valid data.
    ///
    /// This performs basic validation checks on the payment proof structure
    /// without network access. For full validation, use network-based validation.
    ///
    /// # Returns
    /// * `Ok(())` if the proof structure is valid
    /// * `Err(String)` describing the validation error
    pub fn validate_structure(&self) -> Result<(), String> {
        // Check that the amount is not zero
        if self.expected_amount.is_zero() {
            return Err("Expected amount cannot be zero".to_string());
        }

        // Check that the derivation index is not all zeros (this would be unusual)
        if self.recipient_derivation_index == [0u8; 16] {
            return Err("Derivation index should not be all zeros".to_string());
        }

        // Check that record key hash is not all zeros
        if self.record_key_hash == [0u8; 4] {
            return Err("Record key hash should not be all zeros".to_string());
        }

        Ok(())
    }

    /// Creates a payment proof for testing purposes.
    ///
    /// # Arguments
    /// * `payment_tx` - Payment transaction address
    /// * `amount` - Amount in u64 (will be converted to NativeTokens)
    ///
    /// # Returns
    /// A payment proof with default test values for other fields
    #[cfg(test)]
    pub fn create_test_proof(payment_tx: PublicKey, amount: u64) -> Self {
        Self::new(
            payment_tx,
            [0xAA; 16], // Test derivation index
            NativeTokens::from_u64(amount),
            [0x12, 0x34, 0x56, 0x78], // Test record hash
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::graph::PublicKey;

    fn create_test_public_key() -> PublicKey {
        // Create a test public key for testing
        bls::SecretKey::random().public_key()
    }

    #[test]
    fn test_native_payment_proof_creation() {
        let payment_tx = create_test_public_key();
        let derivation_index = [1u8; 16];
        let amount = NativeTokens::from_u64(1000);
        let record_hash = [0x12, 0x34, 0x56, 0x78];

        let proof = NativePaymentProof::new(payment_tx, derivation_index, amount, record_hash);

        assert_eq!(proof.payment_address(), &payment_tx);
        assert_eq!(proof.derivation_index(), &derivation_index);
        assert_eq!(proof.amount(), amount);
        assert_eq!(proof.record_hash(), &record_hash);
    }

    #[test]
    fn test_payment_proof_type() {
        let native_proof = NativePaymentProof::new(
            create_test_public_key(),
            [1u8; 16],
            NativeTokens::from_u64(1000),
            [0x12, 0x34, 0x56, 0x78],
        );

        let proof_type = PaymentProofType::from(native_proof.clone());

        assert!(proof_type.is_native());
        assert!(!proof_type.is_evm());
        assert_eq!(proof_type.as_native(), Some(&native_proof));
    }

    #[test]
    fn test_serialization() {
        let proof = NativePaymentProof::new(
            create_test_public_key(),
            [1u8; 16],
            NativeTokens::from_u64(1000),
            [0x12, 0x34, 0x56, 0x78],
        );

        // Test serialization and deserialization
        let serialized = bincode::serialize(&proof).expect("Serialization failed");
        let deserialized: NativePaymentProof =
            bincode::deserialize(&serialized).expect("Deserialization failed");

        assert_eq!(proof, deserialized);
    }

    #[test]
    fn test_payment_proof_validation() {
        // Test valid proof
        let valid_proof = NativePaymentProof::new(
            create_test_public_key(),
            [1u8; 16],
            NativeTokens::from_u64(1000),
            [0x12, 0x34, 0x56, 0x78],
        );
        assert!(valid_proof.validate_structure().is_ok());

        // Test invalid proof - zero amount
        let zero_amount_proof = NativePaymentProof::new(
            create_test_public_key(),
            [1u8; 16],
            NativeTokens::ZERO,
            [0x12, 0x34, 0x56, 0x78],
        );
        assert!(zero_amount_proof.validate_structure().is_err());

        // Test invalid proof - zero derivation index
        let zero_derivation_proof = NativePaymentProof::new(
            create_test_public_key(),
            [0u8; 16],
            NativeTokens::from_u64(1000),
            [0x12, 0x34, 0x56, 0x78],
        );
        assert!(zero_derivation_proof.validate_structure().is_err());

        // Test invalid proof - zero record hash
        let zero_hash_proof = NativePaymentProof::new(
            create_test_public_key(),
            [1u8; 16],
            NativeTokens::from_u64(1000),
            [0u8; 4],
        );
        assert!(zero_hash_proof.validate_structure().is_err());
    }

    #[test]
    fn test_payment_proof_type_functionality() {
        let native_proof = NativePaymentProof::create_test_proof(create_test_public_key(), 1000);
        let proof_type = PaymentProofType::from(native_proof.clone());

        // Test type identification
        assert!(proof_type.is_native());
        assert!(!proof_type.is_evm());
        assert_eq!(proof_type.type_name(), "Native");

        // Test extraction
        assert_eq!(proof_type.as_native(), Some(&native_proof));

        // Test validation
        assert!(proof_type.validate().is_ok());

        // Test serialization of PaymentProofType
        let serialized = bincode::serialize(&proof_type).expect("Serialization failed");
        let deserialized: PaymentProofType =
            bincode::deserialize(&serialized).expect("Deserialization failed");

        assert!(deserialized.is_native());
        assert_eq!(deserialized.as_native().unwrap(), &native_proof);
    }

    #[test]
    fn test_create_test_proof() {
        let payment_tx = create_test_public_key();
        let test_proof = NativePaymentProof::create_test_proof(payment_tx, 5000);

        assert_eq!(test_proof.payment_address(), &payment_tx);
        assert_eq!(test_proof.amount(), NativeTokens::from_u64(5000));
        assert_eq!(test_proof.derivation_index(), &[0xAA; 16]);
        assert_eq!(test_proof.record_hash(), &[0x12, 0x34, 0x56, 0x78]);
        assert!(test_proof.validate_structure().is_ok());
    }
}

/// Unified payment proof type that supports both EVM and native token payments
///
/// This enum allows the network to handle both traditional EVM-based payments
/// and new native token payments through a single interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentProofType {
    /// EVM-based payment proof (traditional payment method)
    #[cfg(feature = "evm-integration")]
    Evm(ProofOfPayment),

    /// Native token payment proof (new payment method)
    Native(NativePaymentProof),
}

impl PaymentProofType {
    /// Create a new EVM payment proof type
    #[cfg(feature = "evm-integration")]
    pub fn evm(proof: ProofOfPayment) -> Self {
        Self::Evm(proof)
    }

    /// Create a new native token payment proof type
    pub fn native(proof: NativePaymentProof) -> Self {
        Self::Native(proof)
    }

    /// Check if this is an EVM payment
    #[cfg(feature = "evm-integration")]
    pub fn is_evm(&self) -> bool {
        matches!(self, Self::Evm(_))
    }

    /// Check if this is a native token payment
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native(_))
    }

    /// Get the EVM payment proof if this is an EVM payment
    #[cfg(feature = "evm-integration")]
    pub fn as_evm(&self) -> Option<&ProofOfPayment> {
        match self {
            Self::Evm(proof) => Some(proof),
            _ => None,
        }
    }

    /// Get the native payment proof if this is a native payment
    pub fn as_native(&self) -> Option<&NativePaymentProof> {
        match self {
            Self::Native(proof) => Some(proof),
            #[cfg(feature = "evm-integration")]
            _ => None,
        }
    }

    /// Convert into the EVM payment proof if this is an EVM payment
    #[cfg(feature = "evm-integration")]
    pub fn into_evm(self) -> Option<ProofOfPayment> {
        match self {
            Self::Evm(proof) => Some(proof),
            _ => None,
        }
    }

    /// Convert into the native payment proof if this is a native payment
    pub fn into_native(self) -> Option<NativePaymentProof> {
        match self {
            Self::Native(proof) => Some(proof),
            #[cfg(feature = "evm-integration")]
            _ => None,
        }
    }

    /// Check if this is an EVM payment (non-conditional version)
    #[cfg(not(feature = "evm-integration"))]
    pub fn is_evm(&self) -> bool {
        false
    }

    /// Get a string representation of the payment proof type
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Native(_) => "Native",
            #[cfg(feature = "evm-integration")]
            Self::Evm(_) => "EVM",
        }
    }

    /// Validate the payment proof structure
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Native(proof) => proof.validate_structure(),
            #[cfg(feature = "evm-integration")]
            Self::Evm(_proof) => {
                // EVM proof validation would be implemented here
                // For now, assume EVM proofs are valid if they exist
                Ok(())
            }
        }
    }
}

#[cfg(feature = "evm-integration")]
impl From<ProofOfPayment> for PaymentProofType {
    fn from(proof: ProofOfPayment) -> Self {
        Self::Evm(proof)
    }
}

impl From<NativePaymentProof> for PaymentProofType {
    fn from(proof: NativePaymentProof) -> Self {
        Self::Native(proof)
    }
}
