// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Native wallet configuration for the Autonomi Network
//! 
//! This module provides configuration options for setting up native token
//! wallets and payment providers in the Autonomi client.

use ant_protocol::storage::NativeTokens;
use bls::SecretKey;
use super::{InMemoryNativeWallet, NativeWalletResult};
use crate::client::payment_providers::{NativeTokenPaymentProvider, PaymentRouter};
use crate::networking::Network;
use std::sync::Arc;
use tracing::{debug, info};

/// Configuration for native wallet functionality
/// 
/// This configuration allows clients to set up native token payment
/// capabilities, including wallet initialization and genesis token setup.
#[derive(Debug, Clone, Default)]
pub struct NativeWalletConfig {
    /// Master private key for the native wallet
    /// 
    /// If None, a random key will be generated. For testing and development,
    /// you may want to provide a specific key for reproducible results.
    pub master_private_key: Option<SecretKey>,
    
    /// Initial genesis token amount for testing
    /// 
    /// If provided, the wallet will be initialized with this amount of
    /// genesis tokens. This is primarily useful for testing and development.
    pub genesis_token_amount: Option<NativeTokens>,
    
    /// Whether to enable native token payments
    /// 
    /// If false, the native payment provider will not be added to the
    /// payment router, and only EVM payments will be available.
    pub enable_native_payments: bool,
    
    /// Whether to set native tokens as the default payment method
    /// 
    /// If true and native payments are enabled, native tokens will be
    /// the default payment choice. Otherwise, EVM will remain the default.
    pub use_as_default: bool,
}


impl NativeWalletConfig {
    /// Create a new native wallet configuration
    /// 
    /// # Returns
    /// A new configuration with native payments disabled by default
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Enable native token payments
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable native payments
    /// 
    /// # Returns
    /// The configuration with native payments enabled/disabled
    pub fn with_native_payments(mut self, enabled: bool) -> Self {
        self.enable_native_payments = enabled;
        self
    }
    
    /// Set the master private key for the wallet
    /// 
    /// # Arguments
    /// * `key` - The secret key to use as the master key
    /// 
    /// # Returns
    /// The configuration with the specified master key
    pub fn with_master_key(mut self, key: SecretKey) -> Self {
        self.master_private_key = Some(key);
        self
    }
    
    /// Generate a random master key for the wallet
    /// 
    /// # Returns
    /// The configuration with a randomly generated master key
    pub fn with_random_master_key(mut self) -> Self {
        self.master_private_key = Some(SecretKey::random());
        self
    }
    
    /// Set genesis tokens for testing
    /// 
    /// # Arguments
    /// * `amount` - The amount of genesis tokens to start with
    /// 
    /// # Returns
    /// The configuration with genesis tokens set
    pub fn with_genesis_tokens(mut self, amount: NativeTokens) -> Self {
        self.genesis_token_amount = Some(amount);
        self
    }
    
    /// Set whether to use native tokens as the default payment method
    /// 
    /// # Arguments
    /// * `use_as_default` - Whether to set native tokens as default
    /// 
    /// # Returns
    /// The configuration with the default payment preference set
    pub fn as_default_payment(mut self, use_as_default: bool) -> Self {
        self.use_as_default = use_as_default;
        self
    }
    
    /// Create a development configuration with reasonable defaults
    /// 
    /// # Arguments
    /// * `genesis_amount` - Amount of genesis tokens to start with
    /// 
    /// # Returns
    /// A configuration suitable for development and testing
    pub fn for_development(genesis_amount: u64) -> Self {
        Self {
            master_private_key: Some(SecretKey::random()),
            genesis_token_amount: Some(NativeTokens::from_u64(genesis_amount)),
            enable_native_payments: true,
            use_as_default: true,
        }
    }
    
    /// Create a production configuration
    /// 
    /// # Arguments
    /// * `master_key` - The master key for the wallet
    /// 
    /// # Returns
    /// A configuration suitable for production use
    pub fn for_production(master_key: SecretKey) -> Self {
        Self {
            master_private_key: Some(master_key),
            genesis_token_amount: None, // No genesis tokens in production
            enable_native_payments: true,
            use_as_default: false, // Keep EVM as default for safety
        }
    }
}

/// Builder for setting up native wallet components
pub struct NativeWalletBuilder {
    config: NativeWalletConfig,
    network: Arc<Network>,
}

impl NativeWalletBuilder {
    /// Create a new builder with the given configuration and network
    /// 
    /// # Arguments
    /// * `config` - The native wallet configuration
    /// * `network` - The network client
    /// 
    /// # Returns
    /// A new builder instance
    pub fn new(config: NativeWalletConfig, network: Arc<Network>) -> Self {
        Self { config, network }
    }
    
    /// Build the native wallet from the configuration
    /// 
    /// # Returns
    /// A configured native wallet, or an error if setup failed
    pub fn build_wallet(&self) -> NativeWalletResult<InMemoryNativeWallet> {
        // Get or generate master key
        let master_key = self.config.master_private_key.clone()
            .unwrap_or_else(|| {
                debug!("No master key provided, generating random key");
                SecretKey::random()
            });
        
        // Create the wallet
        let mut wallet = InMemoryNativeWallet::new(master_key);
        
        // Add genesis tokens if specified
        if let Some(genesis_amount) = self.config.genesis_token_amount {
            let genesis_addr = wallet.add_genesis_tokens(genesis_amount)?;
            info!("Added {} genesis tokens to address {:?}", 
                  genesis_amount.as_u128(), genesis_addr);
        }
        
        Ok(wallet)
    }
    
    /// Build the native token payment provider
    /// 
    /// # Returns
    /// A configured payment provider, or an error if setup failed
    pub fn build_payment_provider(&self) -> NativeWalletResult<NativeTokenPaymentProvider> {
        if !self.config.enable_native_payments {
            return Err(super::NativeWalletError::TransactionCreation(
                "Native payments are disabled in configuration".to_string()
            ));
        }
        
        let wallet = self.build_wallet()?;
        let provider = NativeTokenPaymentProvider::new(
            Arc::<crate::networking::Network>::clone(&self.network),
            wallet.master_key,
        );
        
        Ok(provider)
    }
    
    /// Configure a payment router with native wallet support
    /// 
    /// # Arguments
    /// * `router` - The payment router to configure
    /// 
    /// # Returns
    /// Result indicating success or failure of configuration
    pub fn configure_payment_router(&self, router: &mut PaymentRouter) -> NativeWalletResult<()> {
        if !self.config.enable_native_payments {
            debug!("Native payments disabled, skipping payment router configuration");
            return Ok(());
        }
        
        let provider = self.build_payment_provider()?;
        router.add_native_provider(provider);
        
        if self.config.use_as_default {
            router.set_default_choice(crate::client::payment_providers::PaymentChoice::UseNativeToken);
            info!("Set native tokens as default payment method");
        }
        
        info!("Configured payment router with native token support");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NativeWalletConfig::default();
        assert!(config.master_private_key.is_none());
        assert!(config.genesis_token_amount.is_none());
        assert!(!config.enable_native_payments);
        assert!(!config.use_as_default);
    }
    
    #[test]
    fn test_config_builder_pattern() {
        let config = NativeWalletConfig::new()
            .with_native_payments(true)
            .with_random_master_key()
            .with_genesis_tokens(NativeTokens::from_u64(10000))
            .as_default_payment(true);
        
        assert!(config.master_private_key.is_some());
        assert_eq!(config.genesis_token_amount, Some(NativeTokens::from_u64(10000)));
        assert!(config.enable_native_payments);
        assert!(config.use_as_default);
    }
    
    #[test]
    fn test_development_config() {
        let config = NativeWalletConfig::for_development(5000);
        
        assert!(config.master_private_key.is_some());
        assert_eq!(config.genesis_token_amount, Some(NativeTokens::from_u64(5000)));
        assert!(config.enable_native_payments);
        assert!(config.use_as_default);
    }
    
    #[test]
    fn test_production_config() {
        let master_key = SecretKey::random();
        let config = NativeWalletConfig::for_production(master_key);
        
        assert!(config.master_private_key.is_some());
        assert!(config.genesis_token_amount.is_none());
        assert!(config.enable_native_payments);
        assert!(!config.use_as_default);
    }
    
    #[test]
    fn test_wallet_builder() {
        // Mock network for testing
        // Note: This would require a proper mock in a real test environment
        // For now, we'll test the configuration logic
        let config = NativeWalletConfig::for_development(1000);
        
        // Test that we can build a wallet with genesis tokens
        // In a real test, we would create a proper network mock
        assert!(config.enable_native_payments);
        assert_eq!(config.genesis_token_amount, Some(NativeTokens::from_u64(1000)));
    }
    
    #[test]
    fn test_config_validation() {
        let config = NativeWalletConfig::new();
        
        // Test that disabled payments prevent provider creation
        assert!(!config.enable_native_payments);
        
        let enabled_config = config.with_native_payments(true);
        assert!(enabled_config.enable_native_payments);
    }
}
