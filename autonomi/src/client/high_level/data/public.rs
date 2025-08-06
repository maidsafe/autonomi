// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::storage::DataTypes;
use bytes::Bytes;
use std::time::Instant;

use crate::client::encryption::EncryptionStream;
use crate::client::payment::PaymentOption;
use crate::client::quote::CostError;
use crate::client::{GetError, PutError};
use crate::{Client, chunk::ChunkAddress, self_encryption::encrypt};
use ant_evm::{Amount, AttoTokens};
use xor_name::XorName;

use super::DataAddress;

impl Client {
    /// Fetch a blob of data from the network
    pub async fn data_get_public(&self, addr: &DataAddress) -> Result<Bytes, GetError> {
        info!("Fetching data from Data Address: {addr:?}");
        let data_map_chunk = self.chunk_get(&ChunkAddress::new(*addr.xorname())).await?;
        let data = self
            .fetch_from_data_map_chunk(data_map_chunk.value())
            .await?;

        debug!("Successfully fetched a blob of data from the network");
        Ok(data)
    }

    /// Upload a piece of data to the network. This data is publicly accessible.
    ///
    /// Returns the Data Address at which the data was stored.
    pub async fn data_put_public(
        &self,
        data: Bytes,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, DataAddress), PutError> {
        let (chunk_stream, data_map_chunk) = EncryptionStream::new_in_memory(data, true)?;

        let data_map_addr = *data_map_chunk.0.address();
        info!("Uploading datamap chunk to the network at: {data_map_addr:?}");
        let data_address = DataAddress::new(*data_map_addr.xorname());

        // Note within the `pay_and_upload`, UploadSummary will be sent to client via event_channel.
        let mut chunk_streams = vec![chunk_stream];
        self.pay_and_upload(payment_option, &mut chunk_streams)
            .await
            .map(|total_cost| (total_cost, data_address))
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
        let (data_map_chunks, chunks) = encrypt(data)?;

        debug!("Encryption took: {:.2?}", now.elapsed());

        let map_xor_name = *data_map_chunks.address().xorname();
        let mut content_addrs = vec![(map_xor_name, data_map_chunks.size())];

        for chunk in &chunks {
            content_addrs.push((*chunk.name(), chunk.size()));
        }

        info!(
            "Calculating cost of storing {} chunks. Data map chunk at: {map_xor_name:?}",
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
