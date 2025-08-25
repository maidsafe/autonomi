// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Chunk operations for the Autonomi client.
//! This module provides chunk upload, download, and cost estimation.
//! All operations delegate to autonomi_core::Client through the wrapper.

use crate::{
    Client,
    client::{
        GetError, PutError,
        payment::{PaymentOption, Receipt},
        quote::CostError,
    },
};
use ant_evm::AttoTokens;
use ant_protocol::{
    NetworkAddress,
    storage::{DataTypes, RecordKind},
};
use autonomi_core::DataContent;
use bytes::Bytes;
use self_encryption::DataMap;

// Re-export types from autonomi_core
pub use autonomi_core::{
    CHUNK_DOWNLOAD_BATCH_SIZE, CHUNK_UPLOAD_BATCH_SIZE, Chunk, ChunkAddress, DataMapChunk,
};

impl Client {
    /// Get a chunk from the network.
    pub async fn chunk_get(&self, addr: &ChunkAddress) -> Result<Chunk, GetError> {
        info!("Getting chunk: {addr:?}");
        let network_addr = NetworkAddress::from(*addr);

        match self.core_client.record_get(&network_addr).await {
            Ok(content) => match content {
                DataContent::Chunk(chunk) => Ok(chunk),
                _ => Err(GetError::RecordKindMismatch(RecordKind::DataOnly(
                    DataTypes::Chunk,
                ))),
            },
            Err(e) => Err(GetError::from_error(&e)),
        }
    }

    /// Manually upload a chunk to the network.
    /// It is recommended to use the [`Client::data_put`] method instead to upload data.
    pub async fn chunk_put(
        &self,
        chunk: &Chunk,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, ChunkAddress), PutError> {
        let address = chunk.address();
        debug!("storing chunk at address: {address:?}");
        let content = DataContent::Chunk(chunk.clone());

        self.core_client
            .record_put(content, Some(payment_option))
            .await
            .map(|(cost, _addr)| (cost, *address))
            .map_err(|e| PutError::from_error(&e))
    }

    /// Get the cost for storing a chunk.
    pub async fn chunk_cost(&self, addr: &ChunkAddress) -> Result<AttoTokens, CostError> {
        trace!("Getting cost for chunk of {addr:?}");
        let network_addr = NetworkAddress::from(*addr);
        self.core_client
            .get_cost_estimation(vec![(network_addr, 0)]) // 0 size means use max size
            .await
            .map_err(|e| CostError::from_error(&e))
    }

    /// Upload chunks in batches to the network. This is useful for pre-calculated payment proofs,
    /// in case of manual encryption or re-uploading certain chunks that were already paid for.
    ///
    /// This method requires a vector of chunks to be uploaded and the payment receipt. It returns a `PutError` for
    /// failures and `Ok(())` for successful uploads.
    ///
    /// # Example
    /// ```no_run
    /// # use ant_protocol::storage::DataTypes;
    /// # use autonomi::{Client, Wallet};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init_local().await?;
    /// # let wallet = Wallet::new_from_private_key(
    /// #     client.evm_network().clone(),
    /// #     "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    /// # )?;
    ///
    /// // Step 1: Encrypt your data using self-encryption
    /// let (data_map, chunks) = autonomi::self_encryption::encrypt("Hello, World!".into())?;
    ///
    /// // Step 2: Collect all chunks (datamap + content chunks)
    /// let mut all_chunks = vec![data_map];
    /// all_chunks.extend(chunks);
    ///
    /// // Step 3: Get storage quotes for all chunks
    /// let quote = client.get_store_quotes(
    ///     DataTypes::Chunk,
    ///     all_chunks.iter().map(|chunk| (*chunk.address().xorname(), chunk.size())),
    /// ).await?;
    ///
    /// // Step 4: Pay for all chunks at once and get receipt
    /// wallet.pay_for_quotes(quote.payments()).await.map_err(|err| err.0)?;
    /// let receipt = autonomi::client::payment::receipt_from_store_quotes(quote);
    ///
    /// // Step 5: Upload all chunks with the payment receipt
    /// client.chunk_batch_upload(all_chunks, &receipt).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chunk_batch_upload(
        &self,
        chunks: Vec<Chunk>,
        receipt: &Receipt,
    ) -> Result<(), PutError> {
        let payment_option = PaymentOption::Receipt(receipt.clone());

        let _ = self
            .core_client
            .chunk_batch_upload(chunks, payment_option)
            .await
            .map_err(|e| PutError::from_error(&e))?;
        Ok(())
    }

    /// Generic function to unpack a wrapped datamap and fetch all bytes using self-encryption.
    /// This function automatically detects whether the datamap is in the old format (DataMapLevel)
    /// or new format (DataMap) and calls the appropriate handler for backward compatibility.
    pub async fn fetch_from_data_map_chunk(
        &self,
        data_map_chunk: &DataMapChunk,
    ) -> Result<Bytes, GetError> {
        let mut data_map = self.restore_data_map_from_chunk(data_map_chunk).await?;
        // To be backward compatible
        data_map.child = None;
        self.fetch_from_data_map(&data_map).await
    }

    /// Fetch and decrypt all chunks in the datamap.
    pub async fn fetch_from_data_map(&self, data_map: &DataMap) -> Result<Bytes, GetError> {
        info!("Fetching from data_map of : \n{data_map:?}");
        match self
            .core_client
            .fetch_from_data_map(data_map, None, false)
            .await
        {
            Ok(Some(bytes)) => Ok(bytes),
            // If not using streaming download, a Bytes must be returned.
            Ok(None) => Err(GetError::RecordNotFound),
            Err(e) => Err(GetError::from_error(&e)),
        }
    }
}
