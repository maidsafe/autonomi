// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::storage::DataTypes;

use crate::client::payment::PaymentOption;
use crate::client::{ClientEvent, GetError, PutError, UploadSummary};
use crate::Amount;
use crate::AttoTokens;
use crate::{self_encryption::encrypt, Client};

pub use crate::client::data_types::chunk::DataMapChunk;
pub use crate::Bytes;

impl Client {
    /// Fetch a blob of private data from the network.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use autonomi::{Client, Bytes};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init().await?;
    /// # let data_map = todo!();
    /// let data_fetched = client.data_get(&data_map).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn data_get(&self, data_map: &DataMapChunk) -> Result<Bytes, GetError> {
        info!(
            "Fetching private data from Data Map {:?}",
            data_map.0.address()
        );
        let data = self.fetch_from_data_map_chunk(data_map.0.value()).await?;

        debug!("Successfully fetched a blob of private data from the network");
        Ok(data)
    }

    /// Upload a piece of private data to the network. This data will be self-encrypted.
    /// The [`DataMapChunk`] is not uploaded to the network, keeping the data private.
    ///
    /// Returns the [`DataMapChunk`] containing the map to the encrypted chunks.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use autonomi::{Client, Bytes};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init().await?;
    /// # let wallet = todo!();
    /// let data = Bytes::from("Hello, World");
    /// let (total_cost, data_map) = client.data_put(data, wallet).await?;
    /// let data_fetched = client.data_get(&data_map).await?;
    /// assert_eq!(data, data_fetched);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn data_put(
        &self,
        data: Bytes,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, DataMapChunk), PutError> {
        let now = ant_networking::time::Instant::now();
        let (data_map_chunk, chunks) = encrypt(data)?;
        debug!("Encryption took: {:.2?}", now.elapsed());

        // Pay for all chunks
        let xor_names: Vec<_> = chunks
            .iter()
            .map(|chunk| (*chunk.name(), chunk.size()))
            .collect();
        info!("Paying for {} addresses", xor_names.len());
        let (receipt, skipped_payments) = self
            .pay_for_content_addrs(DataTypes::Chunk, xor_names.into_iter(), payment_option)
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

        let record_count = chunks.len().saturating_sub(skipped_payments);

        let tokens_spent = receipt
            .values()
            .map(|(_, cost)| cost.as_atto())
            .sum::<Amount>();
        let total_cost = AttoTokens::from_atto(tokens_spent);

        // Reporting
        if let Some(channel) = self.client_event_sender.as_ref() {
            let summary = UploadSummary {
                records_paid: record_count,
                records_already_paid: skipped_payments,
                tokens_spent,
            };
            if let Err(err) = channel.send(ClientEvent::UploadComplete(summary)).await {
                error!("Failed to send client event: {err:?}");
            }
        }

        Ok((total_cost, DataMapChunk(data_map_chunk)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::data_types::chunk::Chunk;

    #[test]
    fn test_hex() {
        let data_map = DataMapChunk(Chunk::new(Bytes::from_static(b"hello")));
        let hex = data_map.to_hex();
        let data_map2 = DataMapChunk::from_hex(&hex).expect("Failed to decode hex");
        assert_eq!(data_map, data_map2);
    }
}
