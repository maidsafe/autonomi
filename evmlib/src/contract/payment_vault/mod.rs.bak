use crate::common::{Address, Amount, QuoteHash};
use crate::contract::payment_vault::handler::PaymentVaultHandler;
use crate::quoting_metrics::QuotingMetrics;
use crate::utils::http_provider;
use crate::Network;

pub mod error;
pub mod handler;
pub mod implementation;
pub mod interface;

pub const MAX_TRANSFERS_PER_TRANSACTION: usize = 256;

/// Helper function to return a quote for the given quoting metrics.
pub async fn get_market_price(
    network: &Network,
    quoting_metrics: Vec<QuotingMetrics>,
) -> Result<Vec<Amount>, error::Error> {
    let provider = http_provider(network.rpc_url().clone());
    let payment_vault = PaymentVaultHandler::new(*network.data_payments_address(), provider);
    payment_vault.get_quote(quoting_metrics).await
}

/// Helper function to verify whether a data payment is valid.
/// Returns the amount paid to the owned quote hashes.
pub async fn verify_data_payment(
    network: &Network,
    owned_quote_hashes: Vec<QuoteHash>,
    payment: Vec<(QuoteHash, QuotingMetrics, Address)>,
) -> Result<Amount, error::Error> {
    let provider = http_provider(network.rpc_url().clone());
    let payment_vault = PaymentVaultHandler::new(*network.data_payments_address(), provider);

    let mut amount = Amount::ZERO;

    let payment_verifications: Vec<_> = payment
        .into_iter()
        .map(interface::IPaymentVault::PaymentVerification::from)
        .collect();

    let payment_verification_results = payment_vault.verify_payment(payment_verifications).await?;

    for payment_verification_result in payment_verification_results {
        // TODO we currently fail on a single invalid payment, maybe we should deal with this in a different way
        if !payment_verification_result.isValid {
            return Err(error::Error::PaymentInvalid);
        }

        if owned_quote_hashes.contains(&payment_verification_result.quoteHash) {
            amount += payment_verification_result.amountPaid;
        }
    }

    Ok(amount)
}
