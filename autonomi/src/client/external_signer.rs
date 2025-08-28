// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use crate::client::{PutError, quote::DataTypes};
use crate::self_encryption::encrypt;
use ant_evm::QuotePayment;
use ant_protocol::storage::Chunk;
use bytes::Bytes;
use std::collections::HashMap;
use std::time::Instant;
use xor_name::XorName;

#[allow(unused_imports)]
pub use ant_evm::external_signer::*;

use super::quote::QuoteForAddress;

impl Client {
    /// Get quotes for data.
    /// Returns a cost map, data payments to be executed and a list of free (already paid for) chunks.
    pub async fn get_quotes_for_content_addresses(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)> + Clone,
    ) -> Result<
        (
            HashMap<XorName, QuoteForAddress>,
            Vec<QuotePayment>,
            Vec<XorName>,
        ),
        PutError,
    > {
        let quote = self
            .core_client
            .get_store_quotes(data_type, content_addrs.clone())
            .await
            .map_err(PutError::CostError)?;
        let payments = quote.payments();
        let free_chunks: Vec<_> = content_addrs
            .filter(|(addr, _)| !quote.0.contains_key(addr))
            .collect();
        let quotes_per_addr: HashMap<_, _> = quote.0.into_iter().collect();

        Ok((
            quotes_per_addr,
            payments,
            free_chunks.iter().map(|(addr, _)| *addr).collect(),
        ))
    }
}

/// Encrypts data as chunks.
///
/// Returns the datamap chunk and file chunks.
pub fn encrypt_data(data: Bytes) -> Result<(Chunk, Vec<Chunk>), crate::self_encryption::Error> {
    let now = Instant::now();
    let result = encrypt(data)?;

    debug!("Encryption took: {:.2?}", now.elapsed());

    Ok((result.0, result.1))
}
