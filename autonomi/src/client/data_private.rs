// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::hash::{DefaultHasher, Hash, Hasher};

use ant_evm::Amount;
use ant_protocol::storage::Chunk;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use super::data::{GetError, PutError};
use crate::client::payment::PaymentOption;
use crate::client::{ClientEvent, UploadSummary};
use crate::{self_encryption::encrypt, Client};

/// Private data on the network can be accessed with this
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrivateDataAccess(Chunk);

impl PrivateDataAccess {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.value())
    }

    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let data = hex::decode(hex)?;
        Ok(Self(Chunk::new(Bytes::from(data))))
    }

    /// Get a private address for [`PrivateDataAccess`]. Note that this is not a network address, it is only used for refering to private data client side.
    pub fn address(&self) -> String {
        hash_to_short_string(&self.to_hex())
    }
}

fn hash_to_short_string(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash_value = hasher.finish();
    hash_value.to_string()
}

impl Client {
    /// Fetch a blob of private data from the network
    pub async fn private_data_get(&self, data_map: PrivateDataAccess) -> Result<Bytes, GetError> {
        info!(
            "Fetching private data from Data Map {:?}",
            data_map.0.address()
        );
        let data = self.fetch_from_data_map_chunk(data_map.0.value()).await?;

        debug!("Successfully fetched a blob of private data from the network");
        Ok(data)
    }

    /// Upload a piece of private data to the network. This data will be self-encrypted.
    /// Returns the [`PrivateDataAccess`] containing the map to the encrypted chunks.
    /// This data is private and only accessible with the [`PrivateDataAccess`].
    pub async fn private_data_put(
        &self,
        data: Bytes,
        payment_option: PaymentOption,
    ) -> Result<PrivateDataAccess, PutError> {
        let now = ant_networking::target_arch::Instant::now();
        let (data_map_chunk, chunks) = encrypt(data)?;
        debug!("Encryption took: {:.2?}", now.elapsed());

        // Pay for all chunks
        let xor_names: Vec<_> = chunks.iter().map(|chunk| *chunk.name()).collect();
        info!("Paying for {} addresses", xor_names.len());
        let receipt = self
            .pay_for_content_addrs(xor_names.into_iter(), payment_option)
            .await
            .inspect_err(|err| error!("Error paying for data: {err:?}"))?;

        // Upload the chunks with the payments
        debug!("Uploading {} chunks", chunks.len());

        let mut failed_uploads = self
            .upload_chunks_with_retries(chunks.iter().collect(), &receipt)
            .await;

        // Return the last chunk upload error
        if let Some(last_chunk_fail) = failed_uploads.pop() {
            tracing::error!(
                "Error uploading chunk ({:?}): {:?}",
                last_chunk_fail.0.address(),
                last_chunk_fail.1
            );
            return Err(last_chunk_fail.1);
        }

        let record_count = chunks.len();

        // Reporting
        if let Some(channel) = self.client_event_sender.as_ref() {
            let tokens_spent = receipt
                .values()
                .map(|proof| proof.quote.cost.as_atto())
                .sum::<Amount>();

            let summary = UploadSummary {
                record_count,
                tokens_spent,
            };
            if let Err(err) = channel.send(ClientEvent::UploadComplete(summary)).await {
                error!("Failed to send client event: {err:?}");
            }
        }

        Ok(PrivateDataAccess(data_map_chunk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex() {
        let data_map = PrivateDataAccess(Chunk::new(Bytes::from_static(b"hello")));
        let hex = data_map.to_hex();
        let data_map2 = PrivateDataAccess::from_hex(&hex).expect("Failed to decode hex");
        assert_eq!(data_map, data_map2);
    }
}
