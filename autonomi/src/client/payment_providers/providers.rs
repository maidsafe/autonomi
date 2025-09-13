// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Payment provider implementations
//! 
//! This module contains the concrete implementations of payment providers
//! for both EVM-based payments and native token payments.

use super::{PaymentProvider, PaymentResult};
use crate::client::quote::{PaymentType, EnhancedPaymentQuote};
use crate::client::native_wallet::InMemoryNativeWallet;
use async_trait::async_trait;
use crate::client::quote::{DataTypes, StoreQuote};
use crate::networking::Network;
use ant_evm::{EvmWallet, Amount as EvmAmount, ProofOfPayment as EvmProofOfPayment};
use ant_protocol::storage::{NativeTokens, NativePaymentProof};
use std::sync::Arc;
use xor_name::XorName;

// Note: AmountConversion and PaymentProofType trait implementations
// would need to be in the ant-protocol crate to avoid orphan rule violations.
// For proof-of-concept, we'll work around this by using concrete types directly.

/// EVM-based payment provider
/// 
/// This provider uses the existing EVM payment system with smart contracts
/// and external blockchain infrastructure.
#[derive(Clone)]
#[derive(Debug)]
pub struct EvmPaymentProvider {
    /// The EVM wallet for making payments
    wallet: Arc<EvmWallet>,
    
    /// Network client for fetching quotes (future use)
    _network: Arc<Network>,
    
    /// EVM network configuration
    evm_network: ant_evm::EvmNetwork,
}

impl EvmPaymentProvider {
    /// Creates a new EVM payment provider
    /// 
    /// # Arguments
    /// * `wallet` - EVM wallet for making payments
    /// * `network` - Network client for communication
    /// * `evm_network` - EVM network configuration
    pub fn new(wallet: EvmWallet, network: Arc<Network>, evm_network: ant_evm::EvmNetwork) -> Self {
        Self {
            wallet: Arc::new(wallet),
            _network: network,
            evm_network,
        }
    }
    
    /// Gets the underlying EVM wallet
    pub fn wallet(&self) -> &EvmWallet {
        &self.wallet
    }
    
    /// Gets the EVM network configuration
    pub fn evm_network(&self) -> &ant_evm::EvmNetwork {
        &self.evm_network
    }
}

#[async_trait]
impl PaymentProvider for EvmPaymentProvider {
    type PaymentProof = EvmProofOfPayment;
    type Amount = EvmAmount;

    async fn request_quotes(
        &self,
        _data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Send,
    ) -> PaymentResult<EnhancedPaymentQuote> {
        // Use existing quote system from the client
        // This is a simplified implementation - in practice, this would integrate
        // with the full Client quote system
        let _addrs: Vec<_> = content_addrs.collect();
        
        // For now, create an empty store quote as placeholder
        // In a full implementation, this would call the network's quote system
        let store_quote = StoreQuote(std::collections::HashMap::new());
        
        Ok(EnhancedPaymentQuote {
            store_quote,
            native_pricing: None,
            supported_payment_types: vec![PaymentType::Evm],
        })
    }

    async fn make_payment(
        &self,
        quote: &EnhancedPaymentQuote,
    ) -> PaymentResult<Vec<Self::PaymentProof>> {
        // Use existing EVM payment logic
        if quote.store_quote.is_empty() {
            return Ok(vec![]);
        }
        
        // Lock wallet and make payment
        let _lock_guard = self.wallet.lock().await;
        
        // Execute EVM payments using existing wallet logic
        let _payments = self.wallet
            .pay_for_quotes(quote.store_quote.payments())
            .await
            .map_err(|err| format!("EVM payment failed: {:?}", err.0))?;

        // Convert payments to proof format
        // This is simplified - the actual implementation would properly convert
        let proofs = vec![]; // Placeholder
        
        Ok(proofs)
    }

    async fn verify_payment(
        &self,
        proof: &Self::PaymentProof,
        _content_addr: &XorName,
    ) -> PaymentResult<bool> {
        // Use existing EVM verification logic
        // This would check the proof against the blockchain
        Ok(!proof.peer_quotes.is_empty())
    }

    fn payment_type(&self) -> PaymentType {
        PaymentType::Evm
    }

    async fn estimate_cost(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Send,
    ) -> PaymentResult<Self::Amount> {
        let quote = self.request_quotes(data_type, content_addrs).await?;
        Ok(quote.store_quote.price())
    }
}

/// Native token payment provider
/// 
/// This provider uses the new native token system with GraphEntry-based
/// payments that don't require external blockchain infrastructure.
#[derive(Clone)]
#[derive(Debug)]
pub struct NativeTokenPaymentProvider {
    /// Network client for communication (future use)
    _network: Arc<Network>,
    
    /// In-memory wallet for native tokens (proof-of-concept)
    wallet: Arc<tokio::sync::Mutex<InMemoryNativeWallet>>,
}

// Native wallet implementation is now in crate::client::native_wallet module

impl NativeTokenPaymentProvider {
    /// Creates a new native token payment provider
    /// 
    /// # Arguments
    /// * `network` - Network client for communication
    /// * `master_key` - Master key for the native wallet
    pub fn new(network: Arc<Network>, master_key: bls::SecretKey) -> Self {
        let wallet = InMemoryNativeWallet::new(master_key);
        
        Self {
            _network: network,
            wallet: Arc::new(tokio::sync::Mutex::new(wallet)),
        }
    }
    
    /// Creates a provider with genesis tokens for testing
    pub fn new_with_genesis_tokens(
        network: Arc<Network>, 
        master_key: bls::SecretKey, 
        genesis_amount: NativeTokens
    ) -> Self {
        let mut wallet = InMemoryNativeWallet::new(master_key);
        let _ = wallet.add_genesis_tokens(genesis_amount);
        
        Self {
            _network: network,
            wallet: Arc::new(tokio::sync::Mutex::new(wallet)),
        }
    }
    
    /// Gets the total available balance
    pub async fn get_balance(&self) -> NativeTokens {
        let wallet = self.wallet.lock().await;
        wallet.total_balance()
    }
}

#[async_trait]
impl PaymentProvider for NativeTokenPaymentProvider {
    type PaymentProof = NativePaymentProof;
    type Amount = NativeTokens;

    async fn request_quotes(
        &self,
        _data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Send,
    ) -> PaymentResult<EnhancedPaymentQuote> {
        // For the proof-of-concept, we'll use a simple flat rate pricing
        // In practice, this would query nodes for native token quotes
        let addrs: Vec<_> = content_addrs.collect();
        let base_cost = NativeTokens::from_u64(1000); // 1000 native tokens per item
        
        let mut costs_per_address = std::collections::HashMap::new();
        let mut total_cost = NativeTokens::ZERO;
        
        for (addr, _size) in addrs {
            costs_per_address.insert(addr, base_cost);
            total_cost = total_cost.checked_add(base_cost)
                .ok_or("Cost calculation overflow")?;
        }
        
        let native_pricing = super::NativePricing {
            total_cost,
            costs_per_address,
        };
        
        Ok(EnhancedPaymentQuote {
            store_quote: StoreQuote(std::collections::HashMap::new()),
            native_pricing: Some(native_pricing),
            supported_payment_types: vec![PaymentType::NativeToken],
        })
    }

    async fn make_payment(
        &self,
        quote: &EnhancedPaymentQuote,
    ) -> PaymentResult<Vec<Self::PaymentProof>> {
        let native_pricing = quote.native_pricing.as_ref()
            .ok_or("No native pricing in quote")?;
        
        let mut wallet = self.wallet.lock().await;
        
        // Check if we have sufficient funds
        let available_balance = wallet.total_balance();
        if available_balance.as_u128() < native_pricing.total_cost.as_u128() {
            return Err("Insufficient native token balance".into());
        }
        
        // Select tokens for payment
        let selected_tokens = wallet.select_tokens_for_payment(native_pricing.total_cost)
            .map_err(|e| format!("Token selection failed: {e}"))?;
        
        // Create payment proofs for each address
        let mut proofs = Vec::new();
        
        for cost in native_pricing.costs_per_address.values() {
            // Create a simple payment proof (in practice, this would create GraphEntry transactions)
            let proof = NativePaymentProof::new(
                wallet.master_key.public_key(), // Payment transaction address (placeholder)
                [0u8; 16], // Derivation index (would be properly derived)
                *cost,
                [0u8; 4], // Record key hash (would be actual hash of addr)
            );
            
            proofs.push(proof);
        }
        
        // Mark tokens as pending (in practice, would submit GraphEntry transactions)
        for token_key in selected_tokens {
            if let Some(balance) = wallet.available_tokens.get_mut(&token_key) {
                balance.is_claimed = true;
            }
        }
        
        Ok(proofs)
    }

    async fn verify_payment(
        &self,
        proof: &Self::PaymentProof,
        _content_addr: &XorName,
    ) -> PaymentResult<bool> {
        // Basic validation of the payment proof structure
        proof.validate_structure()
            .map(|_| true)
            .map_err(|e| format!("Payment proof validation failed: {e}").into())
    }

    fn payment_type(&self) -> PaymentType {
        PaymentType::NativeToken
    }

    async fn estimate_cost(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Send,
    ) -> PaymentResult<Self::Amount> {
        let quote = self.request_quotes(data_type, content_addrs).await?;
        Ok(quote.native_pricing
            .map(|pricing| pricing.total_cost)
            .unwrap_or(NativeTokens::ZERO))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_wallet_creation() {
        let master_key = bls::SecretKey::random();
        let wallet = InMemoryNativeWallet::new(master_key);
        
        assert_eq!(wallet.total_balance(), NativeTokens::ZERO);
        assert_eq!(wallet.available_tokens.len(), 0);
        assert_eq!(wallet.pending_transactions.len(), 0);
    }

    #[test]
    fn test_native_wallet_genesis_tokens() {
        let master_key = bls::SecretKey::random();
        let mut wallet = InMemoryNativeWallet::new(master_key);
        
        let genesis_amount = NativeTokens::from_u64(1000000);
        let _ = wallet.add_genesis_tokens(genesis_amount);
        
        assert_eq!(wallet.total_balance(), genesis_amount);
        assert_eq!(wallet.available_tokens.len(), 1);
    }

    // Note: These tests require proper Network initialization with bootstrap peers
    // For proof-of-concept, they are commented out to avoid compilation issues
    
    // #[tokio::test]
    // async fn test_native_payment_provider_creation() {
    //     // Would need: let network = Arc::new(Network::new(bootstrap_peers, config));
    //     let master_key = bls::SecretKey::random();
    //     let genesis_amount = NativeTokens::from_u64(1000000);
    //     
    //     // let provider = NativeTokenPaymentProvider::new_with_genesis_tokens(
    //     //     network,
    //     //     master_key,
    //     //     genesis_amount,
    //     // );
    //     
    //     // let balance = provider.get_balance().await;
    //     // assert_eq!(balance, genesis_amount);
    // }

    // #[test]
    // fn test_payment_type_identification() {
    //     // Would need proper Network initialization
    //     let master_key = bls::SecretKey::random();
    //     
    //     // let native_provider = NativeTokenPaymentProvider::new(network, master_key);
    //     // assert_eq!(native_provider.payment_type(), PaymentType::NativeToken);
    // }
}
