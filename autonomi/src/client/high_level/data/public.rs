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

use crate::client::payment::PaymentOption;
use crate::client::quote::CostError;
use crate::client::{ClientEvent, GetError, PutError, UploadSummary};
use crate::{chunk::ChunkAddress, self_encryption::encrypt, Client};
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
        let now = Instant::now();
        let (data_map_chunk, chunks) = encrypt(data)?;
        let data_map_addr = data_map_chunk.address();
        debug!("Encryption took: {:.2?}", now.elapsed());
        info!("Uploading datamap chunk to the network at: {data_map_addr:?}");

        let map_xor_name = *data_map_chunk.address().xorname();
        let mut xor_names = vec![(map_xor_name, data_map_chunk.size())];

        for chunk in &chunks {
            xor_names.push((*chunk.name(), chunk.size()));
        }

        // Pay for all chunks + data map chunk
        info!("Paying for {} addresses", xor_names.len());
        let (receipt, skipped_payments) = self
            .pay_for_content_addrs(DataTypes::Chunk, xor_names.into_iter(), payment_option)
            .await
            .inspect_err(|err| error!("Error paying for data: {err:?}"))?;

        // Upload all the chunks in parallel including the data map chunk
        debug!("Uploading {} chunks", chunks.len());

        let mut failed_uploads = self
            .upload_chunks_with_retries(
                chunks
                    .iter()
                    .chain(std::iter::once(&data_map_chunk))
                    .collect(),
                &receipt,
            )
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

        let record_count = (chunks.len() + 1) - skipped_payments;

        let tokens_spent = receipt
            .values()
            .map(|(_proof, price)| price.as_atto())
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

        Ok((total_cost, DataAddress::new(map_xor_name)))
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
