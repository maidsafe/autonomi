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
use libp2p::PeerId;
use std::collections::HashMap;
use xor_name::XorName;

/// Payment strategy for uploads
#[derive(Debug, Clone, Copy, Default)]
pub enum PaymentMode {
    /// Default mode: Pay 3 nodes
    #[default]
    Standard,
    /// Alternative mode: Pay only the highest priced node with 10x the quoted amount
    SingleNode,
}

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

        let quotes_to_pay_per_addr = self.process_quotes_by_payment_mode(quotes_per_addr)?;

        Ok(StoreQuote(quotes_to_pay_per_addr))
    }

    /// Process quotes according to the payment mode
    fn process_quotes_by_payment_mode(
        &self,
        quotes_per_addr: HashMap<XorName, Vec<(PeerId, Addresses, PaymentQuote, Amount)>>,
    ) -> Result<HashMap<XorName, QuoteForAddress>, CostError> {
        match self.payment_mode {
            PaymentMode::Standard => self.process_standard_payment_quotes(quotes_per_addr),
            PaymentMode::SingleNode => self.process_single_node_payment_quotes(quotes_per_addr),
        }
    }

    /// Process quotes for standard payment mode (pay 3 nodes)
    fn process_standard_payment_quotes(
        &self,
        quotes_per_addr: HashMap<XorName, Vec<(PeerId, Addresses, PaymentQuote, Amount)>>,
    ) -> Result<HashMap<XorName, QuoteForAddress>, CostError> {
        const MINIMUM_QUOTES_TO_PAY: usize = 5;
        let mut quotes_to_pay_per_addr = HashMap::new();

        for (content_addr, quotes) in quotes_per_addr {
            if quotes.len() >= MINIMUM_QUOTES_TO_PAY {
                let quote_for_addr = self.create_standard_quote_payment(&quotes, content_addr);
                quotes_to_pay_per_addr.insert(content_addr, quote_for_addr);
            } else {
                return Err(self.create_insufficient_quotes_error(content_addr, quotes.len(), MINIMUM_QUOTES_TO_PAY));
            }
        }

        Ok(quotes_to_pay_per_addr)
    }

    /// Process quotes for single node payment mode (pay only highest priced node with 10x amount)
    fn process_single_node_payment_quotes(
        &self,
        quotes_per_addr: HashMap<XorName, Vec<(PeerId, Addresses, PaymentQuote, Amount)>>,
    ) -> Result<HashMap<XorName, QuoteForAddress>, CostError> {
        const MINIMUM_QUOTES_TO_PAY: usize = 5;
        let mut quotes_to_pay_per_addr = HashMap::new();

        for (content_addr, mut quotes) in quotes_per_addr {
            if quotes.len() >= MINIMUM_QUOTES_TO_PAY {
                let quote_for_addr = self.create_single_node_quote_payment(&mut quotes, content_addr);
                quotes_to_pay_per_addr.insert(content_addr, quote_for_addr);
            } else {
                return Err(self.create_insufficient_quotes_error(content_addr, quotes.len(), MINIMUM_QUOTES_TO_PAY));
            }
        }

        Ok(quotes_to_pay_per_addr)
    }

    /// Create payment structure for standard mode (pay nodes at indices 2, 3, 4)
    fn create_standard_quote_payment(
        &self,
        quotes: &[(PeerId, Addresses, PaymentQuote, Amount)],
        content_addr: XorName,
    ) -> QuoteForAddress {
        let (p1, a1, q1, _) = &quotes[0];
        let (p2, a2, q2, _) = &quotes[1];

        let peer_ids = vec![quotes[2].0, quotes[3].0, quotes[4].0];
        trace!("Peers to pay for {content_addr}: {peer_ids:?}");

        QuoteForAddress(vec![
            (*p1, a1.clone(), q1.clone(), Amount::ZERO),
            (*p2, a2.clone(), q2.clone(), Amount::ZERO),
            quotes[2].clone(),
            quotes[3].clone(),
            quotes[4].clone(),
        ])
    }

    /// Create payment structure for single node mode (pay only highest priced node with 10x)
    fn create_single_node_quote_payment(
        &self,
        quotes: &mut [(PeerId, Addresses, PaymentQuote, Amount)],
        content_addr: XorName,
    ) -> QuoteForAddress {
        // Find the highest price and select node to pay
        let highest_price = quotes
            .iter()
            .map(|(_, _, _, price)| *price)
            .max()
            .expect("quotes should not be empty as we checked len >= MINIMUM_QUOTES_TO_PAY");

        let node_to_pay = self.select_highest_priced_node(quotes, highest_price);
        let enhanced_price = highest_price * Amount::from(10u64);

        // Sort by distance to maintain the closest 5 nodes
        self.sort_quotes_by_distance(quotes, content_addr);

        // Build payment list
        self.build_single_node_payment_list(quotes, node_to_pay, enhanced_price, highest_price, content_addr)
    }

    /// Select the first node with the highest price (deterministic selection)
    fn select_highest_priced_node(
        &self,
        quotes: &[(PeerId, Addresses, PaymentQuote, Amount)],
        highest_price: Amount,
    ) -> PeerId {
        quotes
            .iter()
            .find(|(_, _, _, price)| *price == highest_price)
            .map(|(peer_id, _, _, _)| *peer_id)
            .expect("At least one node should have the highest price")
    }

    /// Sort quotes by distance to the content address
    fn sort_quotes_by_distance(&self, quotes: &mut [(PeerId, Addresses, PaymentQuote, Amount)], content_addr: XorName) {
        quotes.sort_by_key(|(peer_id, _, _, _)| {
            NetworkAddress::from(*peer_id)
                .distance(&NetworkAddress::from(ChunkAddress::new(content_addr)))
        });
    }

    /// Build payment list for single node mode
    fn build_single_node_payment_list(
        &self,
        quotes: &[(PeerId, Addresses, PaymentQuote, Amount)],
        node_to_pay: PeerId,
        enhanced_price: Amount,
        highest_price: Amount,
        content_addr: XorName,
    ) -> QuoteForAddress {
        let mut payment_list = Vec::new();
        let mut paid_node = false;

        for (i, (peer_id, addrs, quote, original_price)) in quotes.iter().take(5).enumerate() {
            if *peer_id == node_to_pay && !paid_node {
                // Pay the selected highest priced node 10x
                payment_list.push((*peer_id, addrs.clone(), quote.clone(), enhanced_price));
                trace!(
                    "Single peer to pay for {content_addr}: {peer_id:?} (position {}) with price {enhanced_price} (10x of {highest_price})",
                    i + 1
                );
                paid_node = true;
            } else {
                // Other nodes store but don't get paid
                payment_list.push((*peer_id, addrs.clone(), quote.clone(), Amount::ZERO));
                if *original_price == highest_price && *peer_id != node_to_pay {
                    trace!(
                        "Node {peer_id:?} also has highest price {highest_price} but won't be paid"
                    );
                }
            }
        }

        QuoteForAddress(payment_list)
    }

    /// Create error for insufficient quotes
    fn create_insufficient_quotes_error(&self, content_addr: XorName, got: usize, required: usize) -> CostError {
        error!(
            "Not enough quotes for content_addr: {content_addr}, got: {got} and need at least {required}"
        );
        CostError::NotEnoughNodeQuotes {
            content_addr,
            got,
            required,
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
