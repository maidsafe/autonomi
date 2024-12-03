use crate::client::data::PutError;
use crate::client::utils::extract_quote_payments;
use crate::self_encryption::encrypt;
use crate::Client;
use ant_evm::{PaymentQuote, QuotePayment};
use ant_protocol::storage::Chunk;
use bytes::Bytes;
use std::collections::HashMap;
use xor_name::XorName;

use crate::utils::cost_map_to_quotes;
#[allow(unused_imports)]
pub use ant_evm::external_signer::*;

impl Client {
    /// Get quotes for data.
    /// Returns a cost map, data payments to be executed and a list of free (already paid for) chunks.
    pub async fn get_quotes_for_content_addresses(
        &self,
        content_addrs: impl Iterator<Item = XorName>,
    ) -> Result<
        (
            HashMap<XorName, PaymentQuote>,
            Vec<QuotePayment>,
            Vec<XorName>,
        ),
        PutError,
    > {
        let cost_map = self.get_store_quotes(content_addrs).await?;
        let (quote_payments, free_chunks) = extract_quote_payments(&cost_map);
        let quotes = cost_map_to_quotes(cost_map);

        debug!(
            "Got the quotes , quote_payments and freechunks from the network {:?}",
            (quotes.clone(), quote_payments.clone(), free_chunks.clone())
        );
        Ok((quotes, quote_payments, free_chunks))
    }
}

/// Encrypts data as chunks.
///
/// Returns the data map chunk and file chunks.
pub fn encrypt_data(data: Bytes) -> Result<(Chunk, Vec<Chunk>), PutError> {
    let now = ant_networking::target_arch::Instant::now();
    let result = encrypt(data)?;

    debug!("Encryption took: {:.2?}", now.elapsed());

    Ok((result.0, result.1))
}
