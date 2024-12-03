// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use bytes::Bytes;
use libp2p::kad::Quorum;

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use xor_name::XorName;

use crate::client::payment::PaymentOption;
use crate::client::utils::process_tasks_with_max_concurrency;
use crate::client::{ClientEvent, UploadSummary};
use crate::{self_encryption::encrypt, Client};
use ant_evm::{Amount, AttoTokens};
use ant_evm::{EvmWalletError, ProofOfPayment};
use ant_networking::{GetRecordCfg, NetworkError};
use ant_protocol::{
    storage::{try_deserialize_record, Chunk, ChunkAddress, RecordHeader, RecordKind},
    NetworkAddress,
};

/// Number of chunks to upload in parallel.
/// Can be overridden by the `CHUNK_UPLOAD_BATCH_SIZE` environment variable.
pub static CHUNK_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("CHUNK_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                * 8,
        );
    info!("Chunk upload batch size: {}", batch_size);
    batch_size
});

/// Number of retries to upload chunks.
pub const RETRY_ATTEMPTS: usize = 3;

/// Number of chunks to download in parallel.
/// Can be overridden by the `CHUNK_DOWNLOAD_BATCH_SIZE` environment variable.
pub static CHUNK_DOWNLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("CHUNK_DOWNLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                * 8,
        );
    info!("Chunk download batch size: {}", batch_size);
    batch_size
});

/// Raw Data Address (points to a DataMap)
pub type DataAddr = XorName;
/// Raw Chunk Address (points to a [`Chunk`])
pub type ChunkAddr = XorName;

/// Errors that can occur during the put operation.
#[derive(Debug, thiserror::Error)]
pub enum PutError {
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error("A network error occurred.")]
    Network(#[from] NetworkError),
    #[error("Error occurred during cost estimation.")]
    CostError(#[from] CostError),
    #[error("Error occurred during payment.")]
    PayError(#[from] PayError),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("A wallet error occurred.")]
    Wallet(#[from] ant_evm::EvmError),
    #[error("The vault owner key does not match the client's public key")]
    VaultBadOwner,
    #[error("Payment unexpectedly invalid for {0:?}")]
    PaymentUnexpectedlyInvalid(NetworkAddress),
}

/// Errors that can occur during the pay operation.
#[derive(Debug, thiserror::Error)]
pub enum PayError {
    #[error("Wallet error: {0:?}")]
    EvmWalletError(#[from] EvmWalletError),
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error("Cost error: {0:?}")]
    Cost(#[from] CostError),
}

/// Errors that can occur during the get operation.
#[derive(Debug, thiserror::Error)]
pub enum GetError {
    #[error("Could not deserialize data map.")]
    InvalidDataMap(rmp_serde::decode::Error),
    #[error("Failed to decrypt data.")]
    Decryption(crate::self_encryption::Error),
    #[error("Failed to deserialize")]
    Deserialization(#[from] rmp_serde::decode::Error),
    #[error("General networking error: {0:?}")]
    Network(#[from] NetworkError),
    #[error("General protocol error: {0:?}")]
    Protocol(#[from] ant_protocol::Error),
}

/// Errors that can occur during the cost calculation.
#[derive(Debug, thiserror::Error)]
pub enum CostError {
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error("Could not get store quote for: {0:?} after several retries")]
    CouldNotGetStoreQuote(XorName),
    #[error("Could not get store costs: {0:?}")]
    CouldNotGetStoreCosts(NetworkError),
    #[error("Failed to serialize {0}")]
    Serialization(String),
}

impl Client {
    /// Fetch a blob of data from the network
    pub async fn data_get(&self, addr: DataAddr) -> Result<Bytes, GetError> {
        info!("Fetching data from Data Address: {addr:?}");
        let data_map_chunk = self.chunk_get(addr).await?;
        let data = self
            .fetch_from_data_map_chunk(data_map_chunk.value())
            .await?;

        debug!("Successfully fetched a blob of data from the network");
        Ok(data)
    }

    /// Upload a piece of data to the network.
    /// Returns the Data Address at which the data was stored.
    /// This data is publicly accessible.
    pub async fn data_put(
        &self,
        data: Bytes,
        payment_option: PaymentOption,
    ) -> Result<DataAddr, PutError> {
        let now = ant_networking::target_arch::Instant::now();
        let (data_map_chunk, chunks) = encrypt(data)?;
        let data_map_addr = data_map_chunk.address();
        debug!("Encryption took: {:.2?}", now.elapsed());
        info!("Uploading datamap chunk to the network at: {data_map_addr:?}");

        let map_xor_name = *data_map_chunk.address().xorname();
        let mut xor_names = vec![map_xor_name];

        for chunk in &chunks {
            xor_names.push(*chunk.name());
        }

        // Pay for all chunks + data map chunk
        info!("Paying for {} addresses", xor_names.len());
        let receipt = self
            .pay_for_content_addrs(xor_names.into_iter(), payment_option)
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

        let record_count = chunks.len() + 1;

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

        Ok(map_xor_name)
    }

    /// Get a raw chunk from the network.
    pub async fn chunk_get(&self, addr: ChunkAddr) -> Result<Chunk, GetError> {
        info!("Getting chunk: {addr:?}");

        let key = NetworkAddress::from_chunk_address(ChunkAddress::new(addr)).to_record_key();
        debug!("Fetching chunk from network at: {key:?}");
        let get_cfg = GetRecordCfg {
            get_quorum: Quorum::One,
            retry_strategy: None,
            target_record: None,
            expected_holders: HashSet::new(),
            is_register: false,
        };

        let record = self
            .network
            .get_record_from_network(key, &get_cfg)
            .await
            .inspect_err(|err| error!("Error fetching chunk: {err:?}"))?;
        let header = RecordHeader::from_record(&record)?;

        if let RecordKind::Chunk = header.kind {
            let chunk: Chunk = try_deserialize_record(&record)?;
            debug!("Chunk deserialized successfully: {:?}", chunk);
            Ok(chunk)
        } else {
            error!(
                "Record kind mismatch: expected Chunk, got {:?}",
                header.kind
            );
            Err(NetworkError::RecordKindMismatch(RecordKind::Chunk).into())
        }
    }

    /// Get the estimated cost of storing a piece of data.
    pub async fn data_cost(&self, data: Bytes) -> Result<AttoTokens, CostError> {
        let now = ant_networking::target_arch::Instant::now();
        let (data_map_chunk, chunks) = encrypt(data)?;

        debug!("Encryption took: {:.2?}", now.elapsed());

        let map_xor_name = *data_map_chunk.address().xorname();
        let mut content_addrs = vec![map_xor_name];

        for chunk in &chunks {
            content_addrs.push(*chunk.name());
        }

        info!(
            "Calculating cost of storing {} chunks. Data map chunk at: {map_xor_name:?}",
            content_addrs.len()
        );

        let cost_map = self
            .get_store_quotes(content_addrs.into_iter())
            .await
            .inspect_err(|err| error!("Error getting store quotes: {err:?}"))?;
        let total_cost = AttoTokens::from_atto(
            cost_map
                .values()
                .map(|quote| quote.2.cost.as_atto())
                .sum::<Amount>(),
        );
        debug!("Total cost calculated: {:?}", total_cost);
        Ok(total_cost)
    }

    // Upload chunks and retry failed uploads up to `RETRY_ATTEMPTS` times.
    pub(crate) async fn upload_chunks_with_retries<'a>(
        &self,
        mut chunks: Vec<&'a Chunk>,
        receipt: &HashMap<XorName, ProofOfPayment>,
    ) -> Vec<(&'a Chunk, PutError)> {
        let mut current_attempt: usize = 1;

        loop {
            let mut upload_tasks = vec![];
            for chunk in chunks {
                let self_clone = self.clone();
                let address = *chunk.address();

                let Some(proof) = receipt.get(chunk.name()) else {
                    debug!("Chunk at {address:?} was already paid for so skipping");
                    continue;
                };

                upload_tasks.push(async move {
                    self_clone
                        .chunk_upload_with_payment(chunk, proof.clone())
                        .await
                        .inspect_err(|err| error!("Error uploading chunk {address:?} :{err:?}"))
                        // Return chunk reference too, to re-use it next attempt/iteration
                        .map_err(|err| (chunk, err))
                });
            }
            let uploads =
                process_tasks_with_max_concurrency(upload_tasks, *CHUNK_UPLOAD_BATCH_SIZE).await;

            // Check for errors.
            let total_uploads = uploads.len();
            let uploads_failed: Vec<_> = uploads.into_iter().filter_map(|up| up.err()).collect();
            info!(
                "Uploaded {} chunks out of {total_uploads}",
                total_uploads - uploads_failed.len()
            );

            // All uploads succeeded.
            if uploads_failed.is_empty() {
                return vec![];
            }

            // Max retries reached.
            if current_attempt > RETRY_ATTEMPTS {
                return uploads_failed;
            }

            tracing::info!(
                "Retrying putting {} failed chunks (attempt {current_attempt}/3)",
                uploads_failed.len()
            );

            // Re-iterate over the failed chunks
            chunks = uploads_failed.into_iter().map(|(chunk, _)| chunk).collect();
            current_attempt += 1;
        }
    }
}
