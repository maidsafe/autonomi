// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Upload and data retrieval implementation for the Autonomi core client
//!
//! This module provides data map handling, chunk fetching, and streaming download capabilities

use super::{Client, Error};
use crate::CHUNK_UPLOAD_BATCH_SIZE;
use crate::client::utils::process_tasks_with_max_concurrency;
use crate::networking::PeerInfo;
use crate::{DataContent, DataMapChunk, PaymentOption, PutError};

use ant_protocol::NetworkAddress;
use ant_protocol::storage::{Chunk, ChunkAddress};
use bytes::Bytes;
use self_encryption::{DataMap, MAX_CHUNK_SIZE};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Instant;
use tokio::sync::oneshot;
use tracing::{debug, error, info};

/// Maximum size of a file to be encrypted in memory.
///
/// Can be overridden by the [`IN_MEMORY_ENCRYPTION_MAX_SIZE`] environment variable.
/// The default is 100MB.
pub static IN_MEMORY_ENCRYPTION_MAX_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let max_size = std::env::var("IN_MEMORY_ENCRYPTION_MAX_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000_000);
    info!(
        "IN_MEMORY_ENCRYPTION_MAX_SIZE (from that threshold, the file will be encrypted in a stream): {}",
        max_size
    );
    max_size
});

const STREAM_CHUNK_CHANNEL_CAPACITY: usize = 100;

enum EncryptionState {
    InMemory(Vec<Chunk>, DataMap),
    StreamInProgress(StreamProgressState),
    /// StreamDone(DataMap, total_chunk_count)
    StreamDone((DataMap, usize)),
}

pub struct EncryptionStream {
    #[allow(dead_code)]
    pub file_path: String,
    #[allow(dead_code)]
    pub is_public: bool,
    state: EncryptionState,
}

struct StreamProgressState {
    /// Receiver for chunks
    chunk_receiver: std::sync::mpsc::Receiver<Chunk>,
    /// Receiver for the datamap once the stream is done
    datamap_receiver: oneshot::Receiver<DataMap>,
    /// Number of chunks received so far
    chunk_count: usize,
    /// Total number of chunks estimated to be received
    #[allow(dead_code)]
    total_estimated_chunks: usize,
}

impl EncryptionStream {
    #[allow(dead_code)]
    pub fn total_chunks(&self) -> usize {
        match &self.state {
            EncryptionState::InMemory(chunks, _) => chunks.len(),
            EncryptionState::StreamInProgress(state) => state.total_estimated_chunks,
            EncryptionState::StreamDone((_, total_chunk_count)) => *total_chunk_count,
        }
    }

    pub fn next_batch(&mut self, batch_size: usize) -> Option<Vec<Chunk>> {
        if batch_size == 0 {
            return Some(vec![]);
        }

        let mut state_change: Option<EncryptionState> = None;

        let result = match &mut self.state {
            EncryptionState::InMemory(chunks, _) => {
                let batch: Vec<Chunk> = chunks
                    .drain(0..std::cmp::min(batch_size, chunks.len()))
                    .collect();
                if batch.is_empty() {
                    return None;
                }
                Some(batch)
            }
            EncryptionState::StreamInProgress(progress) => {
                let chunk_receiver = &mut progress.chunk_receiver;
                let datamap_receiver = &mut progress.datamap_receiver;
                let mut batch = Vec::with_capacity(batch_size);

                // Try to receive chunks up to batch_size
                for _ in 0..batch_size {
                    match chunk_receiver.recv() {
                        Ok(chunk) => batch.push(chunk),
                        Err(_) => {
                            // Chunk stream is done, check if we have the datamap
                            match datamap_receiver.try_recv() {
                                Ok(datamap_chunk) => {
                                    // Transition to StreamDone state
                                    state_change = Some(EncryptionState::StreamDone((
                                        datamap_chunk,
                                        progress.chunk_count,
                                    )));
                                }
                                Err(oneshot::error::TryRecvError::Empty) => {
                                    error!("DataMap not available when chunk receiver was closed");
                                }
                                Err(oneshot::error::TryRecvError::Closed) => {
                                    error!("DataMap sender was dropped without sending data");
                                }
                            }
                            break;
                        }
                    }
                }

                progress.chunk_count += batch.len();
                if batch.is_empty() { None } else { Some(batch) }
            }
            EncryptionState::StreamDone(_) => None,
        };

        // Apply the state change if any
        if let Some(next_state) = state_change {
            self.state = next_state;
        }

        result
    }

    pub fn data_map(&self) -> Option<DataMap> {
        match &self.state {
            EncryptionState::InMemory(_, datamap) => Some(datamap.clone()),
            EncryptionState::StreamInProgress(_) => None,
            EncryptionState::StreamDone((datamap, _)) => Some(datamap.clone()),
        }
    }

    pub fn data_map_chunk(&self) -> Option<DataMapChunk> {
        if let Some(datamap) = self.data_map()
            && let Ok(datamap_bytes) = rmp_serde::to_vec(&datamap)
        {
            return Some(DataMapChunk(Chunk::new(Bytes::from(datamap_bytes))));
        }
        None
    }

    /// Returns the data address of the file if the file is public and the stream is done.
    pub fn data_address(&self) -> Option<ChunkAddress> {
        if let Some(data_map_chunk) = self.data_map_chunk()
            && self.is_public
        {
            return Some(*data_map_chunk.0.address());
        }
        None
    }

    pub fn new_in_memory(
        file_path: Option<PathBuf>,
        bytes: Bytes,
        is_public: bool,
    ) -> Result<(Self, DataMap), self_encryption::Error> {
        let start = Instant::now();
        let (data_map, chunks) = self_encryption::encrypt(bytes)?;

        // Convert EncryptedChunks to Chunks
        let mut content_chunks: Vec<Chunk> = chunks
            .into_iter()
            .map(|encrypted_chunk| Chunk::new(encrypted_chunk.content))
            .collect();

        if is_public {
            let datamap_bytes = rmp_serde::to_vec(&data_map).map_err(|e| {
                self_encryption::Error::Generic(format!("Failed to serialize DataMap: {e}"))
            })?;
            content_chunks.push(Chunk::new(Bytes::from(datamap_bytes)));
        }

        let file_path = match file_path {
            Some(path) => path.to_string_lossy().to_string(),
            None => "".to_string(),
        };

        let stream = EncryptionStream {
            file_path,
            is_public,
            state: EncryptionState::InMemory(content_chunks, data_map.clone()),
        };

        debug!("Encryption took: {:.2?}", start.elapsed());
        Ok((stream, data_map))
    }

    pub fn new_stream_from_file(
        file_path: PathBuf,
        is_public: bool,
        file_size: usize,
    ) -> Result<Self, String> {
        let start = Instant::now();
        let (chunk_sender, chunk_receiver) =
            std::sync::mpsc::sync_channel(STREAM_CHUNK_CHANNEL_CAPACITY);
        let (datamap_sender, datamap_receiver) = oneshot::channel();
        let file_path_clone = file_path.clone();

        // Spawn a task to handle streaming encryption
        tokio::spawn(async move {
            // encrypt the file and send chunks in a chunk channel
            let result = self_encryption::streaming_encrypt_from_file(
                &file_path_clone,
                |_xorname, bytes| {
                    let chunk = Chunk::new(bytes);
                    chunk_sender.send(chunk).map_err(|err| {
                        error!("Error sending chunk: {err:?}");
                        self_encryption::Error::Io(std::io::Error::other(format!(
                            "Channel send error in encryption stream for {file_path_clone:?}: {err}"
                        )))
                    })?;
                    Ok(())
                },
            );

            // once we're done, send the datamap to the datamap channel
            match result {
                Ok(datamap) => {
                    // If public, send the datamap_chunk for upload first.
                    if is_public {
                        if let Ok(datamap_bytes) = rmp_serde::to_vec(&datamap) {
                            let chunk = Chunk::new(Bytes::from(datamap_bytes));
                            if let Err(err) = chunk_sender.send(chunk) {
                                error!("Error sending datamap chunk: {err:?}");
                            }
                        } else {
                            error!("Failed to serialize DataMap for {file_path_clone:?}");
                        }
                    }
                    // Send the DataMap result
                    if let Err(_err) = datamap_sender.send(datamap) {
                        error!(
                            "Streaming encryption error sending datamap for {file_path_clone:?}"
                        );
                    };
                }
                Err(err) => {
                    error!("Streaming encryption failed for {file_path_clone:?}: {err}");
                }
            }

            // then close the chunk sender to signal completion and datamap availability
            drop(chunk_sender);
        });

        let stream = EncryptionStream {
            file_path: file_path.to_string_lossy().to_string(),
            is_public,
            state: EncryptionState::StreamInProgress(StreamProgressState {
                chunk_receiver,
                datamap_receiver,
                chunk_count: 0,
                total_estimated_chunks: std::cmp::max(3, file_size / MAX_CHUNK_SIZE),
            }),
        };

        debug!(
            "Started streaming encryption for file (size: {} bytes) in: {:.2?}",
            file_size,
            start.elapsed()
        );
        Ok(stream)
    }
}

impl Client {
    /// Upload a record to the network. Returns the cost and address of the uploaded record.
    /// When `payment_option.is_some()` is `true`, upload with payment.
    pub async fn record_put(
        &self,
        content: DataContent,
        payment_option: Option<PaymentOption>,
    ) -> Result<(crate::AttoTokens, NetworkAddress), Error> {
        // Use the helper function for batch upload with a single item
        let (total_cost, addresses) = self
            .upload_data_batch(vec![content], payment_option)
            .await?;

        // Extract the single address from the result
        let network_address = addresses.into_iter().next().ok_or_else(|| {
            Error::PutError(PutError::Serialization(
                "No address returned from upload".to_string(),
            ))
        })?;

        Ok((total_cost, network_address))
    }

    /// Upload multiple chunks in a batch. Returns the total cost and addresses of all uploaded chunks.
    pub async fn chunk_batch_upload(
        &self,
        chunks: Vec<Chunk>,
        payment_option: PaymentOption,
    ) -> Result<(crate::AttoTokens, Vec<NetworkAddress>), Error> {
        // Convert chunks to DataContent for the helper function
        let data_contents: Vec<DataContent> = chunks.into_iter().map(DataContent::Chunk).collect();

        // Use the helper function for batch upload
        self.upload_data_batch(data_contents, Some(payment_option))
            .await
    }

    /// Helper function to upload multiple data items in batch following the general workflow:
    /// 1. Handle payment for all items at once if PaymentOption provided
    /// 2. Prepare records for upload
    /// 3. Determine target nodes
    /// 4. Upload in parallel using process_tasks_with_max_concurrency
    async fn upload_data_batch(
        &self,
        data_items: Vec<DataContent>,
        payment_option: Option<PaymentOption>,
    ) -> Result<(crate::AttoTokens, Vec<NetworkAddress>), Error> {
        if data_items.is_empty() {
            return Ok((crate::AttoTokens::zero(), vec![]));
        }
        let total_items = data_items.len();

        // Handle payment for all items at once if PaymentOption provided
        let payment_proofs = if let Some(payment_opt) = &payment_option {
            debug!("Paying for {} data items in batch", total_items);

            // Collect all content info for batch payment
            let content_info: Vec<_> = data_items
                .iter()
                .map(|content| {
                    let (xor_name, size, _) = content.get_content_info();
                    (xor_name, size)
                })
                .collect();

            // Pay for all items at once
            let (proofs, _skipped_payments) = self
                .pay_for_content_addrs(
                    data_items[0].data_types(), // Use first item's data type for batch payment
                    content_info.into_iter(),
                    payment_opt.clone(),
                )
                .await
                .inspect_err(|err| {
                    error!("Error paying for batch of {} items: {err:?}", total_items)
                })?;

            Some(proofs)
        } else {
            None
        };

        let mut batch_upload_state = crate::client::put_error_state::ChunkBatchUploadState {
            payment: payment_proofs.clone(),
            ..Default::default()
        };
        let mut upload_tasks = vec![];

        for (i, content) in data_items.iter().enumerate() {
            let self_clone = self.clone();
            let content_clone = content.clone();
            let payment_proofs_clone = payment_proofs.clone();

            // Extract content information for upload
            let (xor_name, _size, network_address) = content.get_content_info();

            // Get payment proof and price for this item
            let (proof, price) = if let Some(ref proofs) = payment_proofs_clone {
                match proofs.get(&xor_name) {
                    Some((proof, price)) => (Some(proof.clone()), *price),
                    None => {
                        debug!(
                            "({}/{total_items}) Data at address {network_address:?} was already paid for so skipping",
                            i + 1
                        );
                        #[cfg(feature = "loud")]
                        println!(
                            "({}/{total_items}) data stored at: {network_address:?} (skipping, already exists)",
                            i + 1
                        );
                        continue;
                    }
                }
            } else {
                (None, crate::AttoTokens::zero())
            };

            upload_tasks.push(async move {
                let data_type = content.data_types();
                let mut error_result = "".to_string();

                // Prepare the record for upload
                let record = match content_clone.prepare_record(proof.clone()) {
                    Ok(record) => record,
                    Err(_err) => {
                        error_result = "{_err:?}".to_string(); 
                        return (network_address, price, error_result);
                    }
                };

                // Determine target nodes for upload
                let target_nodes = if let Some(receipt) = &proof {
                    // Use payment proof payees
                    receipt
                        .payees()
                        .iter()
                        .map(|(peer_id, addrs)| PeerInfo {
                            peer_id: *peer_id,
                            addrs: addrs.clone(),
                        })
                        .collect()
                } else {
                    // Get closest peers for free upload
                    match self_clone.network
                        .get_closest_peers_with_retries(network_address.clone())
                        .await {
                            Ok(peers) => peers,
                            Err(_err) => {
                                error_result = "{_err:?}".to_string(); 
                                return (network_address, price, error_result);
                            }
                        }
                };

                // Upload the record to the network
                debug!("Storing {:?} at address {:?} to the network", data_type, network_address);

                let strategy = self_clone.get_strategy(data_type);

                let res = self_clone.network
                    .put_record_with_retries(record, target_nodes, strategy)
                    .await;

                #[cfg(feature = "loud")]
                match &res {
                    Ok(_) => {
                        println!(
                            "({}/{total_items}) data stored",
                            i + 1,
                        );
                    }
                    Err(err) => {
                        println!(
                            "({}/{total_items}) data failed to be stored at: {network_address:?} ({err})",
                            i + 1,
                        );
                    }
                }

                if let Err(err) = res {
                        error!("Failed to put record - {data_type:?} {network_address:?} to the network: {err}");
                        error_result = "{err:?}".to_string(); 
                        return (network_address, price, error_result);
                    }

                (network_address, price, error_result)
            });
        }

        if upload_tasks.is_empty() {
            return Ok((crate::AttoTokens::zero(), vec![]));
        }

        let uploads: Vec<_> =
            process_tasks_with_max_concurrency(upload_tasks, *CHUNK_UPLOAD_BATCH_SIZE).await;

        // Collect results and calculate total cost
        let mut successful_addresses = vec![];
        let mut total_cost = crate::Amount::ZERO;
        let mut has_errors = false;

        for (address, cost, err_str) in uploads {
            let chunk_addr = ChunkAddress::new(address.xorname());
            if err_str.is_empty() {
                successful_addresses.push(address.clone());
                total_cost += cost.as_atto();
                batch_upload_state.successful.push(chunk_addr);
            } else {
                has_errors = true;
                error!("Failed to upload data at {address:?}: {err_str:?}");
                batch_upload_state.push_error(chunk_addr, err_str);
            }
        }

        if has_errors {
            return Err(Error::PutError(PutError::Batch(batch_upload_state)));
        }

        Ok((
            crate::AttoTokens::from_atto(total_cost),
            successful_addresses,
        ))
    }

    /// Using the streaming_encryption to upload the content of the dest_file.
    /// Returns the total cost and the DataMapChunk.
    /// Note: the DataMapChunk is only uploaded to the network when `is_public` is `true`.
    pub async fn streamingly_upload(
        &self,
        from_dest: PathBuf,
        payment_option: PaymentOption,
        is_public: bool,
    ) -> Result<(DataMap, crate::AttoTokens), Error> {
        info!("Starting streaming upload for file: {:?}", from_dest);

        // Check if file exists and get its size
        let file_metadata = tokio::fs::metadata(&from_dest).await.map_err(|e| {
            Error::PutError(PutError::Serialization(format!(
                "Failed to read file metadata: {e}"
            )))
        })?;
        let file_size = file_metadata.len() as usize;

        if file_size < 3 {
            return Err(Error::PutError(PutError::Serialization(
                "File is too small (less than 3 bytes)".to_string(),
            )));
        }

        // Decide between in-memory and streaming encryption based on file size
        let mut encryption_stream = if file_size > *IN_MEMORY_ENCRYPTION_MAX_SIZE {
            info!(
                "Using streaming encryption for large file (size: {} bytes)",
                file_size
            );
            EncryptionStream::new_stream_from_file(from_dest, is_public, file_size)
                .map_err(|e| Error::PutError(PutError::Serialization(e)))?
        } else {
            info!(
                "Using in-memory encryption for small file (size: {} bytes)",
                file_size
            );
            let file_data = tokio::fs::read(&from_dest).await.map_err(|e| {
                Error::PutError(PutError::Serialization(format!("Failed to read file: {e}")))
            })?;
            let (stream, _) =
                EncryptionStream::new_in_memory(Some(from_dest), Bytes::from(file_data), is_public)
                    .map_err(|e| Error::PutError(PutError::SelfEncryption(e)))?;
            stream
        };

        let mut total_cost = crate::AttoTokens::zero();
        let batch_size = *CHUNK_UPLOAD_BATCH_SIZE;

        // Upload chunks in batches
        while let Some(chunk_batch) = encryption_stream.next_batch(batch_size) {
            if !chunk_batch.is_empty() {
                info!("Uploading batch of {} chunks", chunk_batch.len());
                let (batch_cost, _addresses) = self
                    .chunk_batch_upload(chunk_batch, payment_option.clone())
                    .await?;
                total_cost =
                    crate::AttoTokens::from_atto(total_cost.as_atto() + batch_cost.as_atto());
            }
        }

        info!("Streaming upload completed. Total cost: {:?}", total_cost);
        let Some(datamap) = encryption_stream.data_map() else {
            return Err(Error::PutError(PutError::Serialization(
                "No DataMap generated".to_string(),
            )));
        };
        Ok((datamap, total_cost))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_streaming_state_transitions() {
        // Create a temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"Small test data";
        temp_file.write_all(test_data).unwrap();
        temp_file.flush().unwrap();

        let file_path = temp_file.path().to_path_buf();
        let is_public = false;
        let file_size = test_data.len();

        let mut stream =
            EncryptionStream::new_stream_from_file(file_path, is_public, file_size).unwrap();

        // Should start in StreamInProgress
        assert!(matches!(stream.state, EncryptionState::StreamInProgress(_)));

        // Give some time for the background task to potentially complete
        // (though it won't actually work due to the todo!() placeholder)
        sleep(Duration::from_millis(10)).await;

        // we should expect 3 chunks
        let total_chunks = stream.total_chunks();
        assert_eq!(total_chunks, 3);

        // the datamap should not be available yet
        assert!(stream.data_map().is_none());

        // Try to get a batch - this should handle the streaming logic
        let batch = stream.next_batch(5);

        // We expect 3 chunks
        match batch {
            Some(chunks) => assert_eq!(chunks.len(), 3),
            None => panic!("No chunks available when we expected 3"),
        }

        // we should have no more chunks
        let next_batch = stream.next_batch(5);
        assert_eq!(next_batch, None);

        // State should be StreamDone
        assert!(matches!(stream.state, EncryptionState::StreamDone(_)));

        // we should have the datamap now
        let data_map = stream.data_map();
        assert!(data_map.is_some());
    }
}
