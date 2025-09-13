// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Payment provider abstraction for the Autonomi Network
//! 
//! This module provides a unified interface for different payment methods,
//! allowing both EVM-based payments and native token payments to coexist.

use async_trait::async_trait;
// Removed unused imports: AmountConversion, PaymentProofType
use crate::client::quote::{DataTypes, EnhancedPaymentQuote, PaymentType, NativePricing};
use std::error::Error as StdError;
use xor_name::XorName;

mod providers;
mod choice;

pub use providers::{EvmPaymentProvider, NativeTokenPaymentProvider};
pub use crate::client::native_wallet::{InMemoryNativeWallet, NativeTokenBalance};
pub use choice::{PaymentChoice, PaymentRouter, PaymentStatus};

/// Result type for payment operations
pub type PaymentResult<T> = Result<T, Box<dyn StdError + Send + Sync>>;

// Types already imported above at line 16, no need to re-export duplicates

/// Unified payment provider trait
/// 
/// This trait abstracts over different payment methods, allowing the client
/// to use EVM payments, native tokens, or any future payment method through
/// a common interface.
#[async_trait]
pub trait PaymentProvider: Send + Sync {
    /// The type of payment proof this provider generates
    type PaymentProof: Send + Sync + Clone;
    
    /// The amount type used by this provider
    type Amount: Send + Sync + Clone;

    /// Requests quotes for storing content at the given addresses
    /// 
    /// # Arguments
    /// * `data_type` - The type of data being stored
    /// * `content_addrs` - Iterator of (address, size) tuples
    /// 
    /// # Returns
    /// Enhanced payment quotes that include pricing for this payment method
    async fn request_quotes(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Send,
    ) -> PaymentResult<EnhancedPaymentQuote>;

    /// Makes payment for the given quotes
    /// 
    /// # Arguments
    /// * `quote` - The enhanced payment quote to pay for
    /// 
    /// # Returns
    /// Vector of payment proofs, one for each address in the quote
    async fn make_payment(
        &self,
        quote: &EnhancedPaymentQuote,
    ) -> PaymentResult<Vec<Self::PaymentProof>>;

    /// Verifies that a payment proof is valid
    /// 
    /// # Arguments
    /// * `proof` - The payment proof to verify
    /// * `content_addr` - The content address the proof should cover
    /// 
    /// # Returns
    /// True if the proof is valid, false otherwise
    async fn verify_payment(
        &self,
        proof: &Self::PaymentProof,
        content_addr: &XorName,
    ) -> PaymentResult<bool>;

    /// Returns the payment type this provider supports
    fn payment_type(&self) -> PaymentType;

    /// Estimates the cost for storing data without making payment
    /// 
    /// # Arguments
    /// * `data_type` - The type of data being stored
    /// * `content_addrs` - Iterator of (address, size) tuples
    /// 
    /// # Returns
    /// Estimated cost in the provider's amount type
    async fn estimate_cost(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Send,
    ) -> PaymentResult<Self::Amount>;
}

// Note: PaymentProofType conversion would need to be handled differently
// to avoid orphan rule violations. For proof-of-concept, we'll use concrete types.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_type_enum() {
        assert_eq!(PaymentType::Evm, PaymentType::Evm);
        assert_eq!(PaymentType::NativeToken, PaymentType::NativeToken);
        assert_ne!(PaymentType::Evm, PaymentType::NativeToken);
    }

    #[test]
    fn test_enhanced_payment_quote() {
        use std::collections::HashMap;
        use crate::client::quote::StoreQuote;
        
        let store_quote = StoreQuote(HashMap::new());
        let quote = EnhancedPaymentQuote {
            store_quote,
            native_pricing: None,
            supported_payment_types: vec![PaymentType::Evm],
        };
        
        assert!(quote.native_pricing.is_none());
        assert_eq!(quote.supported_payment_types.len(), 1);
        assert_eq!(quote.supported_payment_types[0], PaymentType::Evm);
    }
}
