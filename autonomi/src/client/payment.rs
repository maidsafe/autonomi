// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use crate::client::merkle_payments::MerklePaymentReceipt;
use crate::client::quote::{DataTypes, StoreQuote};
use ant_evm::{ClientProofOfPayment, EncodedPeerId, EvmWallet, EvmWalletError};
use std::collections::HashMap;
use xor_name::XorName;

use super::quote::CostError;

pub use crate::{Amount, AttoTokens};

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
    receipt_from_store_quotes_filtered(&quotes, None)
}

/// Create a receipt from store quotes, optionally filtering to only include paid quote hashes.
///
/// If `paid_quote_hashes` is Some, only includes quotes whose hash is in the set.
/// If `paid_quote_hashes` is None, includes all quotes.
pub fn receipt_from_store_quotes_filtered(
    quotes: &StoreQuote,
    paid_quote_hashes: Option<&std::collections::BTreeSet<ant_evm::QuoteHash>>,
) -> Receipt {
    let mut receipt = Receipt::new();

    for (content_addr, quote_for_address) in &quotes.0 {
        let mut proof_of_payment = ClientProofOfPayment {
            peer_quotes: vec![],
        };
        let mut price_sum = ant_evm::Amount::ZERO;

        for (peer_id, addrs, quote, amount) in &quote_for_address.0 {
            // If filtering, only include quotes that were paid
            if let Some(paid_hashes) = paid_quote_hashes
                && !paid_hashes.contains(&quote.hash())
            {
                continue;
            }
            proof_of_payment.peer_quotes.push((
                EncodedPeerId::from(*peer_id),
                addrs.0.clone(),
                quote.clone(),
            ));
            price_sum += *amount;
        }

        // skip empty proofs
        if proof_of_payment.peer_quotes.is_empty() {
            continue;
        }

        let price = AttoTokens::from_atto(price_sum);
        receipt.insert(*content_addr, (proof_of_payment, price));
    }

    receipt
}

/// Payment options for single-item data payments (pointer, scratchpad, graph, chunk).
#[derive(Clone)]
pub enum PaymentOption {
    /// Pay using an EVM wallet
    Wallet(EvmWallet),
    /// Resume upload with existing payment receipt
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

/// Payment options for bulk/file uploads (directories, files).
///
/// # Auto-Selection Behavior
///
/// When using `Wallet`, the payment method is **automatically selected** based on estimated chunk count:
/// - `< 64` chunks (MERKLE_PAYMENT_THRESHOLD): uses regular per-batch payments
/// - `>= 64` chunks: uses merkle tree payments (single tree payment, more efficient for large uploads)
///
/// This auto-selection applies to both file and directory uploads when using `file_content_upload`,
/// `file_content_upload_public`, `dir_content_upload`, and `dir_content_upload_public`.
///
/// # Forced Method Selection
///
/// Use `ForceMerkle` or `ForceRegular` to override auto-selection:
/// - `ForceMerkle`: Always use merkle tree payments regardless of chunk count
/// - `ForceRegular`: Always use regular per-batch payments regardless of chunk count
///
/// # Resume Support
///
/// All variants support resuming failed uploads:
/// - `Receipt`: Resume regular payment upload with existing receipt
/// - `MerkleReceipt`: Resume merkle upload with existing proofs (fails if unpaid chunks remain)
/// - `ContinueMerkle`: Resume merkle upload, paying for any remaining chunks with wallet
///
/// When a merkle upload fails, check `UploadError::MerkleUpload` for a receipt containing
/// valid payment proofs that can be reused.
#[derive(Clone)]
pub enum BulkPaymentOption {
    /// Pay using an EVM wallet - auto-selects merkle vs regular based on chunk count threshold (64)
    Wallet(EvmWallet),
    /// Force merkle tree payments regardless of chunk count
    ForceMerkle(EvmWallet),
    /// Force regular per-batch payments regardless of chunk count
    ForceRegular(EvmWallet),
    /// Resume upload with existing regular payment receipt (from non-merkle upload)
    Receipt(Receipt),
    /// Resume upload with existing merkle payment receipt - assumes all chunks paid (fails if not)
    MerkleReceipt(MerklePaymentReceipt),
    /// Continue merkle upload - uses existing proofs from receipt, pays for any unpaid chunks with wallet
    ContinueMerkle(EvmWallet, MerklePaymentReceipt),
}

impl From<EvmWallet> for BulkPaymentOption {
    fn from(value: EvmWallet) -> Self {
        BulkPaymentOption::Wallet(value)
    }
}

impl From<&EvmWallet> for BulkPaymentOption {
    fn from(value: &EvmWallet) -> Self {
        BulkPaymentOption::Wallet(value.clone())
    }
}

impl From<Receipt> for BulkPaymentOption {
    fn from(value: Receipt) -> Self {
        BulkPaymentOption::Receipt(value)
    }
}

impl From<MerklePaymentReceipt> for BulkPaymentOption {
    fn from(value: MerklePaymentReceipt) -> Self {
        BulkPaymentOption::MerkleReceipt(value)
    }
}

impl From<PaymentOption> for BulkPaymentOption {
    fn from(value: PaymentOption) -> Self {
        match value {
            PaymentOption::Wallet(w) => BulkPaymentOption::Wallet(w),
            PaymentOption::Receipt(r) => BulkPaymentOption::Receipt(r),
        }
    }
}

impl BulkPaymentOption {
    /// Get wallet reference for single-item uploads (archives).
    ///
    /// This extracts the wallet from any variant. For Receipt variant,
    /// this will panic since we can't create a wallet from a receipt.
    /// Callers should ensure they have a wallet available for archive uploads.
    pub fn wallet(&self) -> Option<&EvmWallet> {
        match self {
            BulkPaymentOption::Wallet(w) => Some(w),
            BulkPaymentOption::ForceMerkle(w) => Some(w),
            BulkPaymentOption::ForceRegular(w) => Some(w),
            BulkPaymentOption::Receipt(_) => None,
            BulkPaymentOption::MerkleReceipt(_) => None,
            BulkPaymentOption::ContinueMerkle(w, _) => Some(w),
        }
    }

    /// Convert to PaymentOption for single-item uploads (archives).
    ///
    /// For Receipt/MerkleReceipt variants, returns None since archives
    /// need a wallet for payment.
    pub fn to_payment_option(&self) -> Option<PaymentOption> {
        match self {
            BulkPaymentOption::Wallet(w) => Some(PaymentOption::Wallet(w.clone())),
            BulkPaymentOption::ForceMerkle(w) => Some(PaymentOption::Wallet(w.clone())),
            BulkPaymentOption::ForceRegular(w) => Some(PaymentOption::Wallet(w.clone())),
            BulkPaymentOption::Receipt(_) => None,
            BulkPaymentOption::MerkleReceipt(_) => None,
            BulkPaymentOption::ContinueMerkle(w, _) => Some(PaymentOption::Wallet(w.clone())),
        }
    }

    /// Returns true if this option forces merkle payment.
    pub fn is_force_merkle(&self) -> bool {
        matches!(self, BulkPaymentOption::ForceMerkle(_))
    }

    /// Returns true if this option forces regular payment.
    pub fn is_force_regular(&self) -> bool {
        matches!(self, BulkPaymentOption::ForceRegular(_))
    }
}

impl Client {
    /// Pay for content addresses using regular (non-merkle) payment flow.
    pub(crate) async fn pay_for_content_addrs(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
        payment_option: PaymentOption,
    ) -> Result<(Receipt, AlreadyPaidAddressesCount), PayError> {
        match payment_option {
            PaymentOption::Wallet(wallet) => self.pay(data_type, content_addrs, &wallet).await,
            PaymentOption::Receipt(receipt) => Ok((receipt, 0)),
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

        crate::loud_info!("Paying for {} addresses..", quotes.len());

        if !quotes.is_empty() {
            // Make sure nobody else can use the wallet while we are paying
            debug!("Waiting for wallet lock");
            let lock_guard = wallet.lock().await;
            debug!("Locked wallet");

            // Execute payments
            if let Err(pay_err) = wallet.pay_for_quotes(quotes.payments()).await {
                // payment failed, unlock the wallet for other threads
                drop(lock_guard);
                debug!("Unlocked wallet after payment error");
                return Err(PayError::from(pay_err.0));
            }

            // payment is done, unlock the wallet for other threads
            drop(lock_guard);
            debug!("Unlocked wallet");
        }

        let skipped_chunks = number_of_content_addrs - quotes.len();
        crate::loud_info!(
            "Payments of {} address completed. {} address were free / already paid for",
            quotes.len(),
            skipped_chunks
        );

        let receipt = receipt_from_store_quotes(quotes);

        Ok((receipt, skipped_chunks))
    }
}
