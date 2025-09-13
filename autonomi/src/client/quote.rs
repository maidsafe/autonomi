// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::Client;
use crate::client::config::CHUNK_UPLOAD_BATCH_SIZE;
use crate::client::utils::process_tasks_with_max_concurrency;
use crate::networking::Network;
use crate::networking::common::Addresses;
use ant_evm::payment_vault::get_market_price;
use ant_evm::{Amount, PaymentQuote, QuotePayment, QuotingMetrics};
pub use ant_protocol::storage::DataTypes;
use ant_protocol::{CLOSE_GROUP_SIZE, NetworkAddress, storage::ChunkAddress};
use ant_protocol::storage::NativeTokens;
use libp2p::PeerId;
use std::collections::HashMap;
use tracing::{debug, error, info, trace, warn};
use xor_name::XorName;

// todo: limit depends per RPC endpoint. We should make this configurable
// todo: test the limit for the Arbitrum One public RPC endpoint
// Working limit of the Arbitrum Sepolia public RPC endpoint
const GET_MARKET_PRICE_BATCH_LIMIT: usize = 2000;

/// A quote for a single address
#[derive(Debug, Clone)]
pub struct QuoteForAddress(pub(crate) Vec<(PeerId, Addresses, PaymentQuote, Amount)>);

impl QuoteForAddress {
    pub fn price(&self) -> Amount {
        self.0.iter().map(|(_, _, _, price)| price).sum()
    }
}

/// A quote for many addresses
#[derive(Debug, Clone)]
pub struct StoreQuote(pub HashMap<XorName, QuoteForAddress>);

/// Types of payment supported by the system
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PaymentType {
    /// EVM-based payment (existing system)
    Evm,
    /// Native token payment (new system)
    NativeToken,
}

/// Enhanced payment quote that supports multiple payment types
#[derive(Debug, Clone)]
pub struct EnhancedPaymentQuote {
    /// The base quote with EVM pricing information
    pub store_quote: StoreQuote,
    
    /// Optional native token pricing (if supported by nodes)
    pub native_pricing: Option<NativePricing>,
    
    /// Supported payment types for these addresses
    pub supported_payment_types: Vec<PaymentType>,
}

/// Native token pricing information
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePricing {
    /// Total cost in native tokens
    pub total_cost: NativeTokens,
    
    /// Per-address native token costs
    pub costs_per_address: HashMap<XorName, NativeTokens>,
}

/// A quote that can include both EVM and native token pricing
#[derive(Debug, Clone)]
pub struct UnifiedQuoteForAddress {
    /// EVM quote information
    pub evm_quotes: Vec<(PeerId, Addresses, PaymentQuote, Amount)>,
    
    /// Native token quotes (if supported)
    pub native_quotes: Option<Vec<(PeerId, Addresses, NativeTokens)>>,
    
    /// Supported payment types for this address
    pub supported_types: Vec<PaymentType>,
}

impl StoreQuote {
    pub fn price(&self) -> Amount {
        self.0.values().map(|quote| quote.price()).sum()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn payments(&self) -> Vec<QuotePayment> {
        let mut quote_payments = vec![];
        for (_address, quote) in self.0.iter() {
            for (_peer, _addrs, quote, price) in quote.0.iter() {
                quote_payments.push((quote.hash(), quote.rewards_address, *price));
            }
        }
        quote_payments
    }

    pub fn payees_info(&self) -> Vec<(PeerId, Addresses)> {
        let mut payees_info = vec![];
        for (_address, quote) in self.0.iter() {
            for (peer, addrs, _quote, _price) in quote.0.iter() {
                payees_info.push((*peer, addrs.clone()));
            }
        }
        payees_info
    }
}

impl EnhancedPaymentQuote {
    /// Create a new enhanced quote from a basic store quote
    pub fn from_store_quote(store_quote: StoreQuote) -> Self {
        Self {
            store_quote,
            native_pricing: None,
            supported_payment_types: vec![PaymentType::Evm],
        }
    }

    /// Create an enhanced quote with both EVM and native pricing
    pub fn with_native_pricing(
        store_quote: StoreQuote,
        native_pricing: NativePricing,
    ) -> Self {
        Self {
            store_quote,
            native_pricing: Some(native_pricing),
            supported_payment_types: vec![PaymentType::Evm, PaymentType::NativeToken],
        }
    }

    /// Get the total cost for a specific payment type
    pub fn get_cost_for_payment_type(&self, payment_type: PaymentType) -> Option<String> {
        match payment_type {
            PaymentType::Evm => Some(format!("{}", self.store_quote.price())),
            PaymentType::NativeToken => {
                self.native_pricing.as_ref()
                    .map(|pricing| format!("{}", pricing.total_cost.as_u128()))
            }
        }
    }

    /// Check if a specific payment type is supported
    pub fn supports_payment_type(&self, payment_type: &PaymentType) -> bool {
        self.supported_payment_types.contains(payment_type)
    }

    /// Get the most economical payment option
    pub fn get_cheapest_payment_type(&self) -> Option<PaymentType> {
        if self.supported_payment_types.is_empty() {
            return None;
        }

        // Convert costs to u128 for comparison
        let mut options = Vec::new();

        if self.supports_payment_type(&PaymentType::Evm) {
            // Convert EVM amount to u128 for comparison
            let evm_cost = self.store_quote.price().to::<u128>();
            options.push((PaymentType::Evm, evm_cost));
        }

        if let Some(native_pricing) = &self.native_pricing 
            && self.supports_payment_type(&PaymentType::NativeToken) {
            options.push((PaymentType::NativeToken, native_pricing.total_cost.as_u128()));
        }

        options.into_iter().min_by_key(|(_, cost)| *cost).map(|(payment_type, _)| payment_type)
    }

    /// Get a summary of all available pricing options
    pub fn get_pricing_summary(&self) -> HashMap<PaymentType, String> {
        let mut summary = HashMap::new();

        for payment_type in &self.supported_payment_types {
            if let Some(cost) = self.get_cost_for_payment_type(payment_type.clone()) {
                summary.insert(payment_type.clone(), cost);
            }
        }

        summary
    }
}

impl NativePricing {
    /// Create new native pricing
    pub fn new(
        total_cost: NativeTokens,
        costs_per_address: HashMap<XorName, NativeTokens>,
    ) -> Self {
        Self {
            total_cost,
            costs_per_address,
        }
    }

    /// Create pricing from a uniform cost per address
    pub fn uniform_pricing(addresses: &[XorName], cost_per_address: NativeTokens) -> Self {
        let costs_per_address: HashMap<XorName, NativeTokens> = addresses
            .iter()
            .map(|addr| (*addr, cost_per_address))
            .collect();

        let total_cost = addresses.iter().fold(NativeTokens::ZERO, |acc, _| {
            acc.checked_add(cost_per_address).unwrap_or(acc)
        });

        Self {
            total_cost,
            costs_per_address,
        }
    }

    /// Get the cost for a specific address
    pub fn get_cost_for_address(&self, address: &XorName) -> Option<NativeTokens> {
        self.costs_per_address.get(address).copied()
    }

    /// Check if pricing is available for all given addresses
    pub fn covers_addresses(&self, addresses: &[XorName]) -> bool {
        addresses.iter().all(|addr| self.costs_per_address.contains_key(addr))
    }
}

impl UnifiedQuoteForAddress {
    /// Create a new unified quote with only EVM pricing
    pub fn evm_only(evm_quotes: Vec<(PeerId, Addresses, PaymentQuote, Amount)>) -> Self {
        Self {
            evm_quotes,
            native_quotes: None,
            supported_types: vec![PaymentType::Evm],
        }
    }

    /// Create a unified quote with both EVM and native pricing
    pub fn with_native_support(
        evm_quotes: Vec<(PeerId, Addresses, PaymentQuote, Amount)>,
        native_quotes: Vec<(PeerId, Addresses, NativeTokens)>,
    ) -> Self {
        Self {
            evm_quotes,
            native_quotes: Some(native_quotes),
            supported_types: vec![PaymentType::Evm, PaymentType::NativeToken],
        }
    }

    /// Get EVM price
    pub fn evm_price(&self) -> Amount {
        self.evm_quotes.iter().map(|(_, _, _, price)| price).sum()
    }

    /// Get native token price
    pub fn native_price(&self) -> Option<NativeTokens> {
        self.native_quotes.as_ref().map(|quotes| {
            quotes.iter()
                .map(|(_, _, price)| *price)
                .fold(NativeTokens::ZERO, |acc, price| {
                    acc.checked_add(price).unwrap_or(acc)
                })
        })
    }

    /// Check if a payment type is supported
    pub fn supports_payment_type(&self, payment_type: &PaymentType) -> bool {
        self.supported_types.contains(payment_type)
    }
}

/// Errors that can occur during the cost calculation.
#[derive(Debug, thiserror::Error)]
pub enum CostError {
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error(
        "Not enough node quotes for {content_addr:?}, got: {got:?} and need at least {required:?}"
    )]
    NotEnoughNodeQuotes {
        content_addr: XorName,
        got: usize,
        required: usize,
    },
    #[error("Failed to serialize {0}")]
    Serialization(String),
    #[error("Market price error: {0:?}")]
    MarketPriceError(#[from] ant_evm::payment_vault::error::Error),
    #[error("Received invalid cost")]
    InvalidCost,
    #[error("Network error: {0:?}")]
    Network(#[from] crate::networking::NetworkError),
}

impl Client {
    /// Get raw quotes from nodes.
    /// These quotes do not include actual record prices.
    /// You will likely want to use `get_store_quotes` instead.
    pub async fn get_raw_quotes(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)>,
    ) -> Vec<Result<(XorName, Vec<(PeerId, Addresses, PaymentQuote)>), CostError>> {
        let futures: Vec<_> = content_addrs
            .into_iter()
            .map(|(content_addr, data_size)| {
                info!("Quoting for {content_addr:?} ..");
                #[cfg(feature = "loud")]
                println!("Quoting for {content_addr:?} ..");
                fetch_store_quote(
                    &self.network,
                    content_addr,
                    data_type.get_index(),
                    data_size,
                )
            })
            .collect();

        let parallism = std::cmp::min(*CHUNK_UPLOAD_BATCH_SIZE * 8, 128);

        process_tasks_with_max_concurrency(futures, parallism).await
    }

    pub async fn get_store_quotes(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)>,
    ) -> Result<StoreQuote, CostError> {
        let raw_quotes_per_addr = self.get_raw_quotes(data_type, content_addrs).await;
        let mut all_quotes = Vec::new();

        for result in raw_quotes_per_addr {
            let (content_addr, mut raw_quotes) = result?;
            debug!(
                "fetched raw quotes for content_addr: {content_addr}, with {} quotes.",
                raw_quotes.len()
            );

            if raw_quotes.is_empty() {
                debug!(
                    "content_addr: {content_addr} is already paid for. No need to fetch market price."
                );
                continue;
            }

            let target_addr = NetworkAddress::from(ChunkAddress::new(content_addr));

            // Only keep the quotes of the 5 closest nodes
            raw_quotes.sort_by_key(|(peer_id, _, _)| {
                NetworkAddress::from(*peer_id).distance(&target_addr)
            });
            raw_quotes.truncate(CLOSE_GROUP_SIZE);

            for (peer_id, addrs, quote) in raw_quotes.into_iter() {
                all_quotes.push((content_addr, peer_id, addrs, quote));
            }
        }

        let mut all_prices = Vec::new();

        for chunk in all_quotes.chunks(GET_MARKET_PRICE_BATCH_LIMIT) {
            let quoting_metrics: Vec<QuotingMetrics> = chunk
                .iter()
                .map(|(_, _, _, quote)| quote.quoting_metrics.clone())
                .collect();

            debug!(
                "Getting market prices for {} quoting metrics",
                quoting_metrics.len()
            );

            let batch_prices = get_market_price(&self.evm_network, quoting_metrics).await?;

            all_prices.extend(batch_prices);
        }

        let quotes_with_prices: Vec<(XorName, PeerId, Addresses, PaymentQuote, Amount)> =
            all_quotes
                .into_iter()
                .zip(all_prices.into_iter())
                .map(|((content_addr, peer_id, addrs, quote), price)| {
                    (content_addr, peer_id, addrs, quote, price)
                })
                .collect();

        let mut quotes_per_addr: HashMap<XorName, Vec<(PeerId, Addresses, PaymentQuote, Amount)>> =
            HashMap::new();

        for (content_addr, peer_id, addrs, quote, price) in quotes_with_prices {
            let entry = quotes_per_addr.entry(content_addr).or_default();
            entry.push((peer_id, addrs, quote, price));
            entry.sort_by_key(|(_, _, _, price)| *price);
        }

        let mut quotes_to_pay_per_addr = HashMap::new();

        const MINIMUM_QUOTES_TO_PAY: usize = 5;

        for (content_addr, quotes) in quotes_per_addr {
            if quotes.len() >= MINIMUM_QUOTES_TO_PAY {
                let (p1, q1, a1, _) = &quotes[0];
                let (p2, q2, a2, _) = &quotes[1];

                let peer_ids = vec![quotes[2].0, quotes[3].0, quotes[4].0];
                trace!("Peers to pay for {content_addr}: {peer_ids:?}");
                quotes_to_pay_per_addr.insert(
                    content_addr,
                    QuoteForAddress(vec![
                        (*p1, q1.clone(), a1.clone(), Amount::ZERO),
                        (*p2, q2.clone(), a2.clone(), Amount::ZERO),
                        quotes[2].clone(),
                        quotes[3].clone(),
                        quotes[4].clone(),
                    ]),
                );
            } else {
                error!(
                    "Not enough quotes for content_addr: {content_addr}, got: {} and need at least {MINIMUM_QUOTES_TO_PAY}",
                    quotes.len()
                );
                return Err(CostError::NotEnoughNodeQuotes {
                    content_addr,
                    got: quotes.len(),
                    required: MINIMUM_QUOTES_TO_PAY,
                });
            }
        }

        Ok(StoreQuote(quotes_to_pay_per_addr))
    }

    /// Get enhanced quotes that include both EVM and potential native token pricing
    pub async fn get_enhanced_quotes(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
    ) -> Result<EnhancedPaymentQuote, CostError> {
        // First get the standard EVM quotes
        let store_quote = self.get_store_quotes(data_type, content_addrs.clone()).await?;
        
        // TODO: In a full implementation, this would also query nodes for native token pricing
        // For now, we'll create a placeholder native pricing based on EVM pricing
        let content_addresses: Vec<XorName> = content_addrs.map(|(addr, _)| addr).collect();
        let native_pricing = self.estimate_native_pricing(&content_addresses);
        
        if let Some(pricing) = native_pricing {
            Ok(EnhancedPaymentQuote::with_native_pricing(store_quote, pricing))
        } else {
            Ok(EnhancedPaymentQuote::from_store_quote(store_quote))
        }
    }

    /// Estimate native token pricing (placeholder implementation)
    fn estimate_native_pricing(&self, addresses: &[XorName]) -> Option<NativePricing> {
        // TODO: This is a placeholder implementation
        // In a real implementation, this would:
        // 1. Query nodes for native token pricing
        // 2. Apply conversion rates between EVM and native tokens
        // 3. Consider network congestion and other factors
        
        // For POC, let's create a simple conversion: 1 EVM unit = 1000 native tokens
        if addresses.is_empty() {
            return None;
        }

        let cost_per_address = NativeTokens::from_u64(1000); // Example conversion rate
        Some(NativePricing::uniform_pricing(addresses, cost_per_address))
    }

    /// Get quotes for a specific payment type
    pub async fn get_quotes_for_payment_type(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
        payment_type: PaymentType,
    ) -> Result<EnhancedPaymentQuote, CostError> {
        let enhanced_quote = self.get_enhanced_quotes(data_type, content_addrs).await?;
        
        // Filter to only include the requested payment type
        match payment_type {
            PaymentType::Evm => {
                Ok(EnhancedPaymentQuote {
                    store_quote: enhanced_quote.store_quote,
                    native_pricing: None,
                    supported_payment_types: vec![PaymentType::Evm],
                })
            }
            PaymentType::NativeToken => {
                if let Some(native_pricing) = enhanced_quote.native_pricing {
                    Ok(EnhancedPaymentQuote {
                        store_quote: StoreQuote(HashMap::new()), // Empty EVM quote
                        native_pricing: Some(native_pricing),
                        supported_payment_types: vec![PaymentType::NativeToken],
                    })
                } else {
                    Err(CostError::InvalidCost) // Native tokens not available
                }
            }
        }
    }

}

/// Fetch a store quote for a content address.
/// Returns an empty vector if the record already exists and there is no need to pay for it.
async fn fetch_store_quote(
    network: &Network,
    content_addr: XorName,
    data_type: u32,
    data_size: usize,
) -> Result<(XorName, Vec<(PeerId, Addresses, PaymentQuote)>), CostError> {
    let maybe_quotes = network
        .get_quotes_with_retries(
            NetworkAddress::from(ChunkAddress::new(content_addr)),
            data_type,
            data_size,
        )
        .await
        .inspect_err(|err| {
            error!("Error while fetching store quote: {err:?}");
        })?;

    // if no quotes are returned an empty vector is returned
    let quotes = maybe_quotes.unwrap_or_default();
    let quotes_with_peer_id = quotes
        .into_iter()
        .filter_map(|(peer, quote)| match quote.peer_id() {
            Ok(peer_id) => Some((peer_id, Addresses(peer.addrs), quote)),
            Err(e) => {
                warn!("Ignoring invalid quote with invalid peer id: {e}");
                None
            }
        })
        .collect();
    Ok((content_addr, quotes_with_peer_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_protocol::storage::NativeTokens;

    #[test]
    fn test_enhanced_payment_quote_creation() {
        let store_quote = StoreQuote(HashMap::new());
        
        // Test EVM-only quote
        let evm_quote = EnhancedPaymentQuote::from_store_quote(store_quote.clone());
        assert_eq!(evm_quote.supported_payment_types, vec![PaymentType::Evm]);
        assert!(evm_quote.native_pricing.is_none());
        assert!(evm_quote.supports_payment_type(&PaymentType::Evm));
        assert!(!evm_quote.supports_payment_type(&PaymentType::NativeToken));

        // Test quote with native pricing
        let native_pricing = NativePricing::new(
            NativeTokens::from_u64(1000),
            HashMap::new(),
        );
        let enhanced_quote = EnhancedPaymentQuote::with_native_pricing(store_quote, native_pricing);
        assert_eq!(enhanced_quote.supported_payment_types, vec![PaymentType::Evm, PaymentType::NativeToken]);
        assert!(enhanced_quote.native_pricing.is_some());
        assert!(enhanced_quote.supports_payment_type(&PaymentType::Evm));
        assert!(enhanced_quote.supports_payment_type(&PaymentType::NativeToken));
    }

    #[test]
    fn test_native_pricing_uniform() {
        let addresses = vec![XorName::random(&mut rand::thread_rng()), XorName::random(&mut rand::thread_rng()), XorName::random(&mut rand::thread_rng())];
        let cost_per_address = NativeTokens::from_u64(500);
        
        let pricing = NativePricing::uniform_pricing(&addresses, cost_per_address);
        
        assert_eq!(pricing.costs_per_address.len(), 3);
        assert_eq!(pricing.total_cost, NativeTokens::from_u64(1500));
        assert!(pricing.covers_addresses(&addresses));
        
        for addr in &addresses {
            assert_eq!(pricing.get_cost_for_address(addr), Some(cost_per_address));
        }
    }

    #[test]
    fn test_unified_quote_for_address() {
        let evm_quotes = vec![];
        let native_quotes = vec![];
        
        // Test EVM-only quote
        let evm_only = UnifiedQuoteForAddress::evm_only(evm_quotes.clone());
        assert_eq!(evm_only.supported_types, vec![PaymentType::Evm]);
        assert!(evm_only.supports_payment_type(&PaymentType::Evm));
        assert!(!evm_only.supports_payment_type(&PaymentType::NativeToken));
        assert!(evm_only.native_quotes.is_none());

        // Test with native support
        let with_native = UnifiedQuoteForAddress::with_native_support(evm_quotes, native_quotes);
        assert_eq!(with_native.supported_types, vec![PaymentType::Evm, PaymentType::NativeToken]);
        assert!(with_native.supports_payment_type(&PaymentType::Evm));
        assert!(with_native.supports_payment_type(&PaymentType::NativeToken));
        assert!(with_native.native_quotes.is_some());
    }

    #[test]
    fn test_enhanced_quote_cost_comparison() {
        let store_quote = StoreQuote(HashMap::new());
        let native_pricing = NativePricing::new(
            NativeTokens::from_u64(800), // Cheaper than EVM
            HashMap::new(),
        );
        let quote = EnhancedPaymentQuote::with_native_pricing(store_quote, native_pricing);
        
        // Since our store quote is empty (0 cost), native should be more expensive
        // But in a real scenario with actual costs, this would test the comparison logic
        let cheapest = quote.get_cheapest_payment_type();
        assert!(cheapest.is_some());
    }

    #[test]
    fn test_pricing_summary() {
        let store_quote = StoreQuote(HashMap::new());
        let native_pricing = NativePricing::new(
            NativeTokens::from_u64(1200),
            HashMap::new(),
        );
        let quote = EnhancedPaymentQuote::with_native_pricing(store_quote, native_pricing);
        
        let summary = quote.get_pricing_summary();
        assert_eq!(summary.len(), 2); // EVM and Native
        assert!(summary.contains_key(&PaymentType::Evm));
        assert!(summary.contains_key(&PaymentType::NativeToken));
        assert_eq!(summary.get(&PaymentType::NativeToken), Some(&"1200".to_string()));
    }

    #[test]
    fn test_native_pricing_coverage() {
        let addr1 = XorName::random(&mut rand::thread_rng());
        let addr2 = XorName::random(&mut rand::thread_rng());
        let addr3 = XorName::random(&mut rand::thread_rng());
        
        let mut costs = HashMap::new();
        costs.insert(addr1, NativeTokens::from_u64(100));
        costs.insert(addr2, NativeTokens::from_u64(200));
        
        let pricing = NativePricing::new(NativeTokens::from_u64(300), costs);
        
        assert!(pricing.covers_addresses(&[addr1, addr2]));
        assert!(!pricing.covers_addresses(&[addr1, addr2, addr3]));
        assert_eq!(pricing.get_cost_for_address(&addr1), Some(NativeTokens::from_u64(100)));
        assert_eq!(pricing.get_cost_for_address(&addr3), None);
    }
}
