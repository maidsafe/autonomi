// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use crate::client::quote::{DataTypes, StoreQuote, PaymentType};
use ant_evm::{ClientProofOfPayment, EncodedPeerId, EvmWallet, EvmWalletError};
use std::collections::HashMap;
use xor_name::XorName;
use tracing::{debug, info};

use super::quote::CostError;

pub use crate::{Amount, AttoTokens};

// Types are already imported above, so these re-exports are duplicates and should be removed

/// Contains the proof of payments for each XOR address and the amount paid
pub type Receipt = HashMap<XorName, (ClientProofOfPayment, AttoTokens)>;

pub type AlreadyPaidAddressesCount = usize;

/// Errors that can occur during the pay operation.
#[derive(Debug, thiserror::Error)]
pub enum PayError {
    #[error(
        "EVM wallet and client use different EVM networks. Please use the same network for both."
    )]
    EvmWalletNetworkMismatch,
    #[error("Wallet error: {0:?}")]
    EvmWalletError(#[from] EvmWalletError),
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error("Cost error: {0:?}")]
    Cost(#[from] CostError),
}

pub fn receipt_from_store_quotes(quotes: StoreQuote) -> Receipt {
    let mut receipt = Receipt::new();

    for (content_addr, quote_for_address) in quotes.0 {
        let price = AttoTokens::from_atto(quote_for_address.price());

        let mut proof_of_payment = ClientProofOfPayment {
            peer_quotes: vec![],
        };

        for (peer_id, addrs, quote, _amount) in quote_for_address.0 {
            proof_of_payment
                .peer_quotes
                .push((EncodedPeerId::from(peer_id), addrs.0, quote));
        }

        // skip empty proofs
        if proof_of_payment.peer_quotes.is_empty() {
            continue;
        }

        receipt.insert(content_addr, (proof_of_payment, price));
    }

    receipt
}

/// Payment options for data payments.
#[derive(Clone)]
pub enum PaymentOption {
    /// Pay using an evm wallet
    Wallet(EvmWallet),
    /// When data was already paid for, use the receipt
    Receipt(Receipt),
    /// Use enhanced payment with specific payment type preference
    Enhanced {
        wallet: EvmWallet,
        preferred_type: PaymentType,
    },
    /// Use automatic payment method selection (chooses cheapest)
    Automatic(EvmWallet),
}

impl From<EvmWallet> for PaymentOption {
    fn from(value: EvmWallet) -> Self {
        PaymentOption::Wallet(value)
    }
}

impl From<&EvmWallet> for PaymentOption {
    fn from(value: &EvmWallet) -> Self {
        PaymentOption::Wallet(value.clone())
    }
}

impl From<Receipt> for PaymentOption {
    fn from(value: Receipt) -> Self {
        PaymentOption::Receipt(value)
    }
}

impl Client {
    pub(crate) async fn pay_for_content_addrs(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
        payment_option: PaymentOption,
    ) -> Result<(Receipt, AlreadyPaidAddressesCount), PayError> {
        match payment_option {
            PaymentOption::Wallet(wallet) => {
                let (receipt, skipped) = self.pay(data_type, content_addrs, &wallet).await?;
                Ok((receipt, skipped))
            }
            PaymentOption::Receipt(receipt) => Ok((receipt, 0)),
            PaymentOption::Enhanced { wallet, preferred_type } => {
                self.pay_with_enhanced_quotes(data_type, content_addrs, &wallet, Some(preferred_type)).await
            }
            PaymentOption::Automatic(wallet) => {
                self.pay_with_enhanced_quotes(data_type, content_addrs, &wallet, None).await
            }
        }
    }

    /// Pay for the content addrs and get the proof of payment.
    pub(crate) async fn pay(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
        wallet: &EvmWallet,
    ) -> Result<(Receipt, AlreadyPaidAddressesCount), PayError> {
        // Check if the wallet uses the same network as the client
        if wallet.network() != self.evm_network() {
            return Err(PayError::EvmWalletNetworkMismatch);
        }

        let number_of_content_addrs = content_addrs.clone().count();
        let quotes = self.get_store_quotes(data_type, content_addrs).await?;

        info!("Paying for {} addresses..", quotes.len());
        #[cfg(feature = "loud")]
        println!("Paying for {} addresses..", quotes.len());

        if !quotes.is_empty() {
            // Make sure nobody else can use the wallet while we are paying
            debug!("Waiting for wallet lock");
            let lock_guard = wallet.lock().await;
            debug!("Locked wallet");

            // TODO: the error might contain some succeeded quote payments as well. These should be returned on err, so that they can be skipped when retrying.
            // TODO: retry when it fails?
            // Execute payments
            let _payments = wallet
                .pay_for_quotes(quotes.payments())
                .await
                .map_err(|err| PayError::from(err.0))?;

            // payment is done, unlock the wallet for other threads
            drop(lock_guard);
            debug!("Unlocked wallet");
        }

        let skipped_chunks = number_of_content_addrs - quotes.len();
        info!(
            "Payments of {} address completed. {} address were free / already paid for",
            quotes.len(),
            skipped_chunks
        );
        #[cfg(feature = "loud")]
        println!(
            "Payments of {} address completed. {} address were free / already paid for",
            quotes.len(),
            skipped_chunks
        );

        let receipt = receipt_from_store_quotes(quotes);

        Ok((receipt, skipped_chunks))
    }

    /// Pay for content using enhanced quotes with payment type selection
    async fn pay_with_enhanced_quotes(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
        wallet: &EvmWallet,
        preferred_type: Option<PaymentType>,
    ) -> Result<(Receipt, AlreadyPaidAddressesCount), PayError> {
        // Check if the wallet uses the same network as the client
        if wallet.network() != self.evm_network() {
            return Err(PayError::EvmWalletNetworkMismatch);
        }

        let number_of_content_addrs = content_addrs.clone().count();
        
        // Get enhanced quotes that include both EVM and native pricing
        let enhanced_quote = self.get_enhanced_quotes(data_type, content_addrs).await?;
        
        // Determine which payment method to use
        let payment_type = match preferred_type {
            Some(ptype) => {
                if enhanced_quote.supports_payment_type(&ptype) {
                    ptype
                } else {
                    info!("Preferred payment type {:?} not available, falling back to EVM", ptype);
                    PaymentType::Evm
                }
            }
            None => {
                // Automatic selection - choose the cheapest option
                enhanced_quote.get_cheapest_payment_type().unwrap_or(PaymentType::Evm)
            }
        };

        info!("Using payment type: {:?}", payment_type);

        // For POC, we only support EVM payments for now
        // TODO: Add native token payment implementation
        match payment_type {
            PaymentType::Evm => {
                info!("Paying for {} addresses using EVM..", enhanced_quote.store_quote.len());
                
                if !enhanced_quote.store_quote.is_empty() {
                    let lock_guard = wallet.lock().await;
                    debug!("Locked wallet for EVM payment");

                    let _payments = wallet
                        .pay_for_quotes(enhanced_quote.store_quote.payments())
                        .await
                        .map_err(|err| PayError::from(err.0))?;

                    drop(lock_guard);
                    debug!("Unlocked wallet");
                }

                let skipped_chunks = number_of_content_addrs - enhanced_quote.store_quote.len();
                let receipt = receipt_from_store_quotes(enhanced_quote.store_quote);
                
                Ok((receipt, skipped_chunks))
            }
            PaymentType::NativeToken => {
                // TODO: Implement native token payment
                info!("Native token payment requested but not yet implemented, falling back to EVM");
                
                if !enhanced_quote.store_quote.is_empty() {
                    let lock_guard = wallet.lock().await;
                    debug!("Locked wallet for fallback EVM payment");

                    let _payments = wallet
                        .pay_for_quotes(enhanced_quote.store_quote.payments())
                        .await
                        .map_err(|err| PayError::from(err.0))?;

                    drop(lock_guard);
                    debug!("Unlocked wallet");
                }

                let skipped_chunks = number_of_content_addrs - enhanced_quote.store_quote.len();
                let receipt = receipt_from_store_quotes(enhanced_quote.store_quote);
                
                Ok((receipt, skipped_chunks))
            }
        }
    }

    // Note: compare_payment_costs and get_cheapest_payment_option methods are implemented in quote.rs
}
