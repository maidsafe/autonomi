// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::time::Instant;

use crate::Client;
use crate::chunk::{ChunkAddress, DataMapChunk};
use crate::client::payment::PaymentOption;
use crate::client::quote::CostError;
use crate::client::{GetError, PutError};
use crate::data::DataStream;
use crate::self_encryption::encrypt;
use ant_evm::{Amount, AttoTokens};
use ant_protocol::storage::DataTypes;
use bytes::Bytes;
use xor_name::XorName;

use super::DataAddress;

impl Client {
    /// Fetch a blob of public data from the network. In-memory only - fails for large files.
    /// Use file_download_public for large files that need streaming.
    pub async fn data_get_public(&self, addr: &DataAddress) -> Result<Bytes, GetError> {
        info!("Fetching public data from Data Address: {addr:?}");
        let datamap_chunk =
            DataMapChunk(self.chunk_get(&ChunkAddress::new(*addr.xorname())).await?);
        self.data_get(&datamap_chunk).await
    }

    /// Stream a blob of public data from the network. Returns an Iterator that yields chunks progressively.
    /// Use this for large blobs of data like videos to avoid loading everything into memory.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use autonomi::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init().await?;
    /// # let addr = todo!();
    /// let stream = client.data_stream_public(&addr).await?;
    /// for chunk_result in stream {
    ///     let chunk = chunk_result?;
    ///     // Process chunk...
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn data_stream_public(&self, addr: &DataAddress) -> Result<DataStream, GetError> {
        info!("Starting streaming fetch of public data from Data Address: {addr:?}");
        let datamap_chunk =
            DataMapChunk(self.chunk_get(&ChunkAddress::new(*addr.xorname())).await?);
        let stream = self.data_stream(&datamap_chunk).await?;
        Ok(stream)
    }

    /// Upload a piece of data to the network. This data is publicly accessible.
    ///
    /// Returns the Data Address at which the data was stored.
    pub async fn data_put_public(
        &self,
        data: Bytes,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, DataAddress), PutError> {
        let (total_cost, data_map_chunk) =
            self.data_put_internal(data, payment_option, true).await?;
        let data_address = DataAddress::new(*data_map_chunk.0.address().xorname());
        Ok((total_cost, data_address))
    }

    /// Get the estimated cost of storing a piece of data.
    pub async fn data_cost(&self, data: Bytes) -> Result<AttoTokens, CostError> {
        let content_addrs = self.get_content_addrs(data)?;
        self.get_cost_estimation(content_addrs).await
    }

    /// Get the content addresses of the data.
    pub(crate) fn get_content_addrs(
        &self,
        data: Bytes,
    ) -> Result<Vec<(XorName, usize)>, CostError> {
        let now = Instant::now();
        let (data_map_chunk, chunks) = encrypt(data)?;

        debug!("Encryption took: {:.2?}", now.elapsed());

        let map_xor_name = *data_map_chunk.address().xorname();
        let mut content_addrs = vec![(map_xor_name, data_map_chunk.size())];

        for chunk in &chunks {
            content_addrs.push((*chunk.name(), chunk.size()));
        }

        info!(
            "Calculating cost of storing {} chunks. Datamap chunk at: {map_xor_name:?}",
            content_addrs.len()
        );

        Ok(content_addrs)
    }

    /// Get the estimated cost of content addresses.
    pub async fn get_cost_estimation(
        &self,
        content_addrs: Vec<(XorName, usize)>,
    ) -> Result<AttoTokens, CostError> {
        let store_quote = self
            .get_store_quotes(DataTypes::Chunk, content_addrs.into_iter())
            .await
            .inspect_err(|err| error!("Error getting store quotes: {err:?}"))?;

        let total_cost = AttoTokens::from_atto(
            store_quote
                .0
                .values()
                .map(|quote| quote.price())
                .sum::<Amount>(),
        );

        Ok(total_cost)
    }
}
