// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::time::Instant;

use crate::AttoTokens;
use crate::Client;
use crate::client::encryption::EncryptionStream;
use crate::client::payment::PaymentOption;
use crate::client::{GetError, PutError};

pub use crate::Bytes;
pub use crate::client::data_types::chunk::DataMapChunk;
pub use crate::client::high_level::data::stream::DataStream;

impl Client {
    /// Fetch a blob of (private) data from the network. In-memory only - fails for large files.
    /// Use file_download for large files that need streaming.
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
            "Fetching private data from datamap {:?}",
            data_map.0.address()
        );

        let mut datamap = self.restore_data_map_from_chunk(data_map).await?;
        let chunk_count = datamap.infos().len();

        if chunk_count > *crate::client::config::MAX_IN_MEMORY_DOWNLOAD_SIZE {
            return Err(GetError::TooLargeForMemory);
        }

        datamap.child = None;
        let data = self.fetch_from_data_map(&datamap).await?;
        debug!(
            "Successfully fetched private data ({} chunks) in-memory",
            chunk_count
        );
        Ok(data)
    }

    /// Stream a blob of (private) data from the network. Returns an Iterator that yields chunks progressively.
    /// Use this for large files to avoid loading everything into memory.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use autonomi::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init().await?;
    /// # let data_map = todo!();
    /// let stream = client.data_stream(&data_map).await?;
    /// for chunk_result in stream {
    ///     let chunk = chunk_result?;
    ///     // Process chunk...
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn data_stream(
        &self,
        data_map: &DataMapChunk,
    ) -> Result<impl Iterator<Item = Result<Bytes, GetError>> + use<>, GetError> {
        info!(
            "Starting streaming fetch of private data from datamap {:?}",
            data_map.0.address()
        );

        let datamap = self.restore_data_map_from_chunk(data_map).await?;
        let chunk_count = datamap.infos().len();

        debug!(
            "Starting streaming fetch of private data ({} chunks)",
            chunk_count
        );

        DataStream::new(self.clone(), datamap)
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
        let now = Instant::now();

        let (chunk_stream, data_map_chunk) = EncryptionStream::new_in_memory(data, false)?;
        debug!("Encryption took: {:.2?}", now.elapsed());

        // Note within the `pay_and_upload`, UploadSummary will be sent to client via event_channel.
        let mut chunk_streams = vec![chunk_stream];
        self.pay_and_upload(payment_option, &mut chunk_streams)
            .await
            .map(|total_cost| (total_cost, data_map_chunk))
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
