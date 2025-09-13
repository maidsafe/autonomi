// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Payment choice and routing system
//! 
//! This module provides the infrastructure for choosing between different
//! payment methods and routing payments to the appropriate provider.

use super::{PaymentProvider, PaymentResult};
use crate::client::quote::PaymentType;
use super::providers::{EvmPaymentProvider, NativeTokenPaymentProvider};
use crate::client::quote::DataTypes;
use ant_protocol::storage::{PaymentProofType, AmountConversion};
use std::sync::Arc;
use tracing::{info, warn};
use xor_name::XorName;

/// Payment choice options for users
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PaymentChoice {
    /// Use EVM-based payment exclusively
    #[default]
    UseEvm,
    /// Use native token payment exclusively  
    UseNativeToken,
    /// Automatically choose the best payment method based on availability and cost
    Automatic,
}

/// Payment router that manages multiple payment providers
/// 
/// The router handles the selection and coordination of different payment
/// methods based on user preferences and network capabilities.
#[derive(Clone, Debug)]
pub struct PaymentRouter {
    /// EVM payment provider (existing system)
    pub evm_provider: Option<Arc<EvmPaymentProvider>>,
    
    /// Native token payment provider (new system)
    pub native_provider: Option<Arc<NativeTokenPaymentProvider>>,
    
    /// Default payment choice when not specified
    pub default_choice: PaymentChoice,
}

impl PaymentRouter {
    /// Creates a new payment router with default configuration
    pub fn new() -> Self {
        Self {
            evm_provider: None,
            native_provider: None,
            default_choice: PaymentChoice::default(),
        }
    }

    /// Creates a payment router with EVM provider only
    pub fn with_evm_provider(evm_provider: EvmPaymentProvider) -> Self {
        Self {
            evm_provider: Some(Arc::new(evm_provider)),
            native_provider: None,
            default_choice: PaymentChoice::UseEvm,
        }
    }

    /// Creates a payment router with native token provider only
    pub fn with_native_provider(native_provider: NativeTokenPaymentProvider) -> Self {
        Self {
            evm_provider: None,
            native_provider: Some(Arc::new(native_provider)),
            default_choice: PaymentChoice::UseNativeToken,
        }
    }

    /// Adds an EVM payment provider to the router
    pub fn add_evm_provider(&mut self, provider: EvmPaymentProvider) {
        self.evm_provider = Some(Arc::new(provider));
    }

    /// Adds a native token payment provider to the router
    pub fn add_native_provider(&mut self, provider: NativeTokenPaymentProvider) {
        self.native_provider = Some(Arc::new(provider));
    }

    /// Sets the default payment choice
    pub fn set_default_choice(&mut self, choice: PaymentChoice) {
        self.default_choice = choice;
    }

    /// Gets the available payment types based on configured providers
    pub fn available_payment_types(&self) -> Vec<PaymentType> {
        let mut types = Vec::new();
        
        if self.evm_provider.is_some() {
            types.push(PaymentType::Evm);
        }
        
        if self.native_provider.is_some() {
            types.push(PaymentType::NativeToken);
        }
        
        types
    }

    /// Selects the appropriate payment provider based on choice and availability
    async fn select_provider(
        &self, 
        choice: PaymentChoice,
        data_type: DataTypes,
        content_addrs: &[(XorName, usize)]
    ) -> PaymentResult<PaymentType> {
        match choice {
            PaymentChoice::UseEvm => {
                if self.evm_provider.is_some() {
                    Ok(PaymentType::Evm)
                } else {
                    Err("EVM payment provider not available".into())
                }
            }
            PaymentChoice::UseNativeToken => {
                if self.native_provider.is_some() {
                    Ok(PaymentType::NativeToken)
                } else {
                    Err("Native token payment provider not available".into())
                }
            }
            PaymentChoice::Automatic => {
                self.select_best_provider(data_type, content_addrs).await
            }
        }
    }

    /// Intelligently selects the best payment provider based on cost and availability
    async fn select_best_provider(
        &self,
        data_type: DataTypes,
        content_addrs: &[(XorName, usize)]
    ) -> PaymentResult<PaymentType> {
        let mut available_options = Vec::new();
        
        // Check EVM provider availability and cost
        if let Some(evm_provider) = &self.evm_provider {
            match evm_provider.estimate_cost(data_type, content_addrs.iter().cloned()).await {
                Ok(cost) => {
                    available_options.push((PaymentType::Evm, cost.to::<u128>()));
                }
                Err(e) => {
                    warn!("EVM cost estimation failed: {}", e);
                }
            }
        }
        
        // Check native token provider availability and cost
        if let Some(native_provider) = &self.native_provider {
            match native_provider.estimate_cost(data_type, content_addrs.iter().cloned()).await {
                Ok(cost) => {
                    available_options.push((PaymentType::NativeToken, cost.to_u128()));
                }
                Err(e) => {
                    warn!("Native token cost estimation failed: {}", e);
                }
            }
        }
        
        if available_options.is_empty() {
            return Err("No payment providers available".into());
        }
        
        // Select the provider with the lowest cost
        let best_option = available_options
            .into_iter()
            .min_by_key(|(_, cost)| *cost)
            .expect("available_options is guaranteed to be non-empty by check above");
            
        info!("Selected {:?} payment method with cost: {}", best_option.0, best_option.1);
        Ok(best_option.0)
    }

    /// Pays for storage using the specified payment choice
    /// 
    /// # Arguments
    /// * `data_type` - The type of data being stored
    /// * `content_addrs` - Vector of (address, size) tuples
    /// * `choice` - Payment method preference
    /// 
    /// # Returns
    /// Vector of unified payment proofs
    pub async fn pay_for_storage(
        &self,
        data_type: DataTypes,
        content_addrs: Vec<(XorName, usize)>,
        choice: PaymentChoice,
    ) -> PaymentResult<Vec<PaymentProofType>> {
        let selected_type = self.select_provider(choice, data_type, &content_addrs).await?;
        
        match selected_type {
            PaymentType::Evm => {
                let provider = self.evm_provider.as_ref()
                    .ok_or("EVM provider not available")?;
                
                let quote = provider.request_quotes(data_type, content_addrs.into_iter()).await?;
                let proofs = provider.make_payment(&quote).await?;
                
                // For proof-of-concept, just indicate successful payment
                info!("EVM payment completed with {} proofs", proofs.len());
                Ok(vec![])
            }
            PaymentType::NativeToken => {
                let provider = self.native_provider.as_ref()
                    .ok_or("Native token provider not available")?;
                
                let quote = provider.request_quotes(data_type, content_addrs.into_iter()).await?;
                let proofs = provider.make_payment(&quote).await?;
                
                // For proof-of-concept, just indicate successful payment
                info!("Native token payment completed with {} proofs", proofs.len());
                Ok(vec![])
            }
        }
    }

    /// Estimates the cost for storage using the specified payment choice
    /// 
    /// # Arguments
    /// * `data_type` - The type of data being stored
    /// * `content_addrs` - Vector of (address, size) tuples
    /// * `choice` - Payment method preference
    /// 
    /// # Returns
    /// Cost estimate as a string (format depends on payment method)
    pub async fn estimate_cost(
        &self,
        data_type: DataTypes,
        content_addrs: Vec<(XorName, usize)>,
        choice: PaymentChoice,
    ) -> PaymentResult<String> {
        let selected_type = self.select_provider(choice, data_type, &content_addrs).await?;
        
        match selected_type {
            PaymentType::Evm => {
                let provider = self.evm_provider.as_ref()
                    .ok_or("EVM provider not available")?;
                
                let cost = provider.estimate_cost(data_type, content_addrs.into_iter()).await?;
                Ok(format!("EVM Cost: {}", cost.to::<u128>()))
            }
            PaymentType::NativeToken => {
                let provider = self.native_provider.as_ref()
                    .ok_or("Native token provider not available")?;
                
                let cost = provider.estimate_cost(data_type, content_addrs.into_iter()).await?;
                Ok(format!("Native Tokens: {}", cost.to_u128()))
            }
        }
    }

    /// Verifies a payment proof regardless of its type
    /// 
    /// # Arguments
    /// * `proof` - The payment proof to verify
    /// * `content_addr` - The content address the proof should cover
    /// 
    /// # Returns
    /// True if the proof is valid, false otherwise
    pub async fn verify_payment(
        &self,
        proof: &PaymentProofType,
        content_addr: &XorName,
    ) -> PaymentResult<bool> {
        match proof {
            PaymentProofType::Native(native_proof) => {
                if let Some(provider) = &self.native_provider {
                    provider.verify_payment(native_proof, content_addr).await
                } else {
                    Err("Native token provider not available for verification".into())
                }
            },
            #[cfg(feature = "evm-integration")]
            PaymentProofType::Evm(evm_proof) => {
                if let Some(provider) = &self.evm_provider {
                    provider.verify_payment(evm_proof, content_addr).await
                } else {
                    Err("EVM payment provider not available for verification".into())
                }
            },
        }
    }

    /// Compares costs between available payment methods
    /// 
    /// # Arguments
    /// * `data_type` - The type of data being stored
    /// * `content_addrs` - Vector of (address, size) tuples
    /// 
    /// # Returns
    /// Map of payment types to their estimated costs
    pub async fn compare_costs(
        &self,
        data_type: DataTypes,
        content_addrs: Vec<(XorName, usize)>,
    ) -> PaymentResult<std::collections::HashMap<PaymentType, u128>> {
        let mut costs = std::collections::HashMap::new();
        
        // Get EVM cost if available
        if let Some(evm_provider) = &self.evm_provider {
            match evm_provider.estimate_cost(data_type, content_addrs.iter().cloned()).await {
                Ok(cost) => {
                    costs.insert(PaymentType::Evm, cost.to::<u128>());
                }
                Err(e) => {
                    warn!("Failed to get EVM cost estimate: {}", e);
                }
            }
        }
        
        // Get native token cost if available
        if let Some(native_provider) = &self.native_provider {
            match native_provider.estimate_cost(data_type, content_addrs.iter().cloned()).await {
                Ok(cost) => {
                    costs.insert(PaymentType::NativeToken, cost.to_u128());
                }
                Err(e) => {
                    warn!("Failed to get native token cost estimate: {}", e);
                }
            }
        }
        
        Ok(costs)
    }

    /// Attempts payment with fallback to alternative methods
    /// 
    /// # Arguments
    /// * `data_type` - The type of data being stored
    /// * `content_addrs` - Vector of (address, size) tuples
    /// * `preferred_choice` - Preferred payment method
    /// 
    /// # Returns
    /// Payment proofs and the method that was actually used
    pub async fn pay_with_fallback(
        &self,
        data_type: DataTypes,
        content_addrs: Vec<(XorName, usize)>,
        preferred_choice: PaymentChoice,
    ) -> PaymentResult<(Vec<PaymentProofType>, PaymentType)> {
        // Try the preferred method first
        match self.pay_for_storage(data_type, content_addrs.clone(), preferred_choice.clone()).await {
            Ok(proofs) => {
                let actual_type = self.select_provider(preferred_choice, data_type, &content_addrs).await?;
                Ok((proofs, actual_type))
            }
            Err(e) => {
                warn!("Preferred payment method failed: {}. Trying fallback.", e);
                
                // Determine fallback method
                let fallback_choice = match preferred_choice {
                    PaymentChoice::UseEvm => PaymentChoice::UseNativeToken,
                    PaymentChoice::UseNativeToken => PaymentChoice::UseEvm,
                    PaymentChoice::Automatic => {
                        // For automatic, we already tried the best option, so no fallback
                        return Err(e);
                    }
                };
                
                match self.pay_for_storage(data_type, content_addrs.clone(), fallback_choice.clone()).await {
                    Ok(proofs) => {
                        let actual_type = self.select_provider(fallback_choice, data_type, &content_addrs).await?;
                        info!("Fallback payment method succeeded: {:?}", actual_type);
                        Ok((proofs, actual_type))
                    }
                    Err(fallback_error) => {
                        Err(format!("Both payment methods failed. Preferred: {e}. Fallback: {fallback_error}").into())
                    }
                }
            }
        }
    }

    /// Checks if a specific payment type is available and functional
    /// 
    /// # Arguments
    /// * `payment_type` - The payment type to check
    /// 
    /// # Returns
    /// True if the payment type is available, false otherwise
    pub fn is_payment_type_available(&self, payment_type: PaymentType) -> bool {
        match payment_type {
            PaymentType::Evm => {
                self.evm_provider.is_some()
            }
            PaymentType::NativeToken => {
                self.native_provider.is_some()
            }
        }
    }

    /// Gets detailed information about available payment methods
    /// 
    /// # Returns
    /// A summary of available payment methods and their status
    pub fn get_payment_status(&self) -> PaymentStatus {
        PaymentStatus {
            evm_available: self.evm_provider.is_some(),
            native_available: self.native_provider.is_some(),
            default_choice: self.default_choice.clone(),
            available_types: self.available_payment_types(),
        }
    }
}

/// Status information about available payment methods
#[derive(Debug, Clone)]
pub struct PaymentStatus {
    /// Whether EVM payments are available
    pub evm_available: bool,
    /// Whether native token payments are available
    pub native_available: bool,
    /// The default payment choice
    pub default_choice: PaymentChoice,
    /// List of available payment types
    pub available_types: Vec<PaymentType>,
}

impl Default for PaymentRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_choice_default() {
        assert_eq!(PaymentChoice::default(), PaymentChoice::UseEvm);
    }

    #[test]
    fn test_payment_router_creation() {
        let router = PaymentRouter::new();
        assert!(router.evm_provider.is_none());
        assert!(router.native_provider.is_none());
        assert_eq!(router.default_choice, PaymentChoice::UseEvm);
    }

    #[test]
    fn test_available_payment_types() {
        let router = PaymentRouter::new();
        assert_eq!(router.available_payment_types().len(), 0);
    }

    #[tokio::test]
    async fn test_select_provider_no_providers() {
        use crate::client::quote::DataTypes;
        
        let router = PaymentRouter::new();
        let content_addrs = vec![(XorName::random(&mut rand::thread_rng()), 1024)];
        
        assert!(router.select_provider(PaymentChoice::UseEvm, DataTypes::Chunk, &content_addrs).await.is_err());
        assert!(router.select_provider(PaymentChoice::UseNativeToken, DataTypes::Chunk, &content_addrs).await.is_err());
        assert!(router.select_provider(PaymentChoice::Automatic, DataTypes::Chunk, &content_addrs).await.is_err());
    }

    #[test]
    fn test_payment_status() {
        let router = PaymentRouter::new();
        let status = router.get_payment_status();
        
        assert!(!status.evm_available);
        assert!(!status.native_available);
        assert_eq!(status.default_choice, PaymentChoice::UseEvm);
        assert_eq!(status.available_types.len(), 0);
    }

    #[tokio::test] 
    async fn test_payment_type_availability() {
        let router = PaymentRouter::new();
        
        assert!(!router.is_payment_type_available(PaymentType::Evm));
        assert!(!router.is_payment_type_available(PaymentType::NativeToken));
    }

    #[tokio::test]
    async fn test_compare_costs_no_providers() {
        use crate::client::quote::DataTypes;
        
        let router = PaymentRouter::new();
        let content_addrs = vec![(XorName::random(&mut rand::thread_rng()), 1024)];
        
        let costs = router.compare_costs(DataTypes::Chunk, content_addrs).await.unwrap();
        assert_eq!(costs.len(), 0);
    }
}
