use crate::client::quote::{DataTypes, StoreQuote};
use crate::Client;
use ant_evm::{EncodedPeerId, EvmWallet, EvmWalletError, ProofOfPayment};
use std::collections::HashMap;
use xor_name::XorName;

use super::quote::CostError;

pub use crate::{Amount, AttoTokens};

/// Contains the proof of payments for each XOR address and the amount paid
pub type Receipt = HashMap<XorName, (ProofOfPayment, AttoTokens)>;

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

        let mut proof_of_payment = ProofOfPayment {
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
        }
    }

    /// Pay for the chunks and get the proof of payment.
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

        if !quotes.is_empty() {
            // Make sure nobody else can use the wallet while we are paying
            debug!("Waiting for wallet lock");
            let lock_guard = wallet.lock().await;
            debug!("Locked wallet");

            // TODO: the error might contain some succeeded quote payments as well. These should be returned on err, so that they can be skipped when retrying.
            // TODO: retry when it fails?
            // Execute chunk payments
            let _payments = wallet
                .pay_for_quotes(quotes.payments())
                .await
                .map_err(|err| PayError::from(err.0))?;

            // payment is done, unlock the wallet for other threads
            drop(lock_guard);
            debug!("Unlocked wallet");
        }

        let skipped_chunks = number_of_content_addrs - quotes.len();
        trace!(
            "Chunk payments of {} chunks completed. {} chunks were free / already paid for",
            quotes.len(),
            skipped_chunks
        );

        let receipt = receipt_from_store_quotes(quotes);

        Ok((receipt, skipped_chunks))
    }
}
