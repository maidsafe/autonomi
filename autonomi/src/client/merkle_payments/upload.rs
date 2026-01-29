// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::payments::MerklePaymentReceipt;
use crate::Client;
use crate::client::config::CHUNK_UPLOAD_BATCH_SIZE;
use crate::client::data_types::chunk::DataMapChunk;
use crate::client::files::Metadata;
use crate::networking::NetworkError;
use crate::self_encryption::EncryptionStream;
use crate::utils::process_tasks_with_max_concurrency;
use ant_evm::merkle_payments::MerklePaymentProof;
use ant_protocol::NetworkAddress;
use ant_protocol::storage::{Chunk, ChunkAddress, DataTypes, RecordKind, try_serialize_record};
use libp2p::kad::Record;
use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;
use thiserror::Error;
use tokio::time::{Duration, sleep};
use xor_name::XorName;

#[derive(Debug, Error)]
pub enum MerklePutError {
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("{0}")]
    Batch(MerkleBatchUploadState),

    // Errors that should not happen unless there is a bug:
    #[error(
        "Missing payment proof for xorname: {0}. This could be caused by a change in content between the payment and the upload. Please try again but make sure the uploaded files are the same as the ones used for the payment."
    )]
    MissingPaymentProofFor(XorName),
    #[error(
        "Stream should have a datamap, this is a bug: please report it and save your logs from ~/.autonomi/client/logs/"
    )]
    StreamShouldHaveDatamap,
}

/// Tracks failed chunks from a merkle batch upload.
/// Failed chunks include the chunk data so they can be retried without re-encryption.
#[derive(Debug, Clone, Default)]
pub struct MerkleBatchUploadState {
    pub failed: Vec<(Chunk, String)>,
}

impl fmt::Display for MerkleBatchUploadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let failures = self.failed.len();
        writeln!(f, "{failures} uploads failed")?;

        // Print first 3 errors
        for (chunk, err) in self.failed.iter().take(3) {
            writeln!(f, "{:?}: {err}", chunk.address())?;
        }
        if failures > 3 {
            writeln!(f, "and {} more...", failures - 3)?;
        }
        Ok(())
    }
}

/// Result from a merkle batch upload containing streams, completed files, and any failed chunks
pub struct MerkleBatchUploadResult {
    /// Remaining streams that haven't been fully processed yet
    pub streams: Vec<EncryptionStream>,
    /// Files that have been completed (all chunks uploaded)
    pub completed_files: Vec<(PathBuf, DataMapChunk, Metadata)>,
    /// Chunks that failed to upload with their error messages
    pub failed_chunks: Vec<(Chunk, String)>,
}

impl Client {
    /// Upload streams of chunks with Merkle batch payments
    ///
    /// Upload up to `limit` chunks from streams, returning remaining streams and completed file results.
    /// Any failed chunks are returned for potential retry.
    ///
    /// This processes streams in order, uploading chunks until `limit` is reached or all streams exhausted.
    /// When a stream is exhausted, its datamap is harvested and added to the results.
    ///
    /// # Arguments
    /// * `streams` - Vector of encryption streams to process
    /// * `receipt` - Merkle payment receipt containing proofs for chunks
    /// * `dont_reupload` - Set of XorNames to skip (already exist on network), to which chunks are added as they are uploaded
    /// * `limit` - Maximum number of chunks to upload in this batch (not including skipped chunks)
    ///
    /// # Returns
    /// * `MerkleBatchUploadResult` containing remaining streams, completed files, and any failed chunks
    pub async fn upload_batch_with_merkle(
        &self,
        mut streams: Vec<EncryptionStream>,
        receipt: &MerklePaymentReceipt,
        dont_reupload: &mut HashSet<XorName>,
        limit: usize,
    ) -> Result<MerkleBatchUploadResult, MerklePutError> {
        let mut completed_files: Vec<(PathBuf, DataMapChunk, Metadata)> = Vec::new();
        let mut all_failed_chunks: Vec<(Chunk, String)> = Vec::new();
        let mut chunks_uploaded = 0;
        let mut chunks_attempted = 0;
        let total_files = receipt.file_chunk_counts.len();
        let upload_batch_size = std::cmp::max(1, *CHUNK_UPLOAD_BATCH_SIZE);

        while chunks_attempted < limit {
            let Some(stream) = streams.first_mut() else {
                break;
            };

            // Try to get next batch of chunks from current stream
            let remaining_in_batch = limit - chunks_attempted;
            let batch_size = std::cmp::min(upload_batch_size, remaining_in_batch);

            match stream.next_batch(batch_size) {
                Some(chunks) if !chunks.is_empty() => {
                    // Upload this batch of chunks, keeping chunk data for potential retry
                    let mut tasks = Vec::with_capacity(chunks.len());
                    for chunk in chunks {
                        let xor_name = *chunk.name();
                        if dont_reupload.contains(&xor_name) {
                            continue;
                        }

                        let proof = receipt
                            .proofs
                            .get(&xor_name)
                            .ok_or(MerklePutError::MissingPaymentProofFor(xor_name))?
                            .clone();
                        let client = self.clone();
                        // Keep chunk for potential retry on failure
                        tasks.push(async move {
                            let result =
                                client.upload_chunk_with_merkle_proof(&chunk, &proof).await;
                            (chunk, result)
                        });
                    }

                    let task_count = tasks.len();
                    let results = process_tasks_with_max_concurrency(tasks, batch_size).await;

                    // Count all attempted chunks (success or failure)
                    chunks_attempted += task_count;

                    // Collect successes and failures
                    for (chunk, result) in results {
                        match result {
                            Ok(addr) => {
                                dont_reupload.insert(*addr.xorname());
                                chunks_uploaded += 1;
                                crate::loud_debug!(
                                    "({chunks_uploaded}/{limit}) Chunk stored at: {addr:?}"
                                );
                            }
                            Err(err) => {
                                crate::loud_error!(
                                    "Chunk failed to be stored at: {:?} ({err})",
                                    chunk.address()
                                );
                                all_failed_chunks.push((chunk, err.to_string()));
                            }
                        }
                    }
                }
                _ => {
                    // Stream exhausted - harvest datamap and remove stream
                    let exhausted_stream = streams.remove(0);
                    let path = exhausted_stream.relative_path.clone();
                    let metadata = exhausted_stream.metadata.clone();
                    let datamap = exhausted_stream
                        .data_map_chunk()
                        .ok_or(MerklePutError::StreamShouldHaveDatamap)?;
                    completed_files.push((path.clone(), datamap, metadata));

                    // report progress
                    let f = total_files - streams.len();
                    if let Some(a) = exhausted_stream.data_address() {
                        crate::loud_info!(
                            "[File {f}/{total_files}] ({path:?}) is now available at: {a:?}"
                        );
                    }
                }
            }
        }

        Ok(MerkleBatchUploadResult {
            streams,
            completed_files,
            failed_chunks: all_failed_chunks,
        })
    }

    /// Upload a single chunk with its Merkle payment proof
    ///
    /// # Arguments
    /// * `chunk` - The chunk to upload
    /// * `proof` - The Merkle payment proof for this chunk
    ///
    /// # Returns
    /// * ChunkAddress on success
    pub async fn upload_chunk_with_merkle_proof(
        &self,
        chunk: &Chunk,
        proof: &MerklePaymentProof,
    ) -> Result<ChunkAddress, MerklePutError> {
        let address = *chunk.address();
        let network_addr = NetworkAddress::from(address);
        self.upload_record_with_merkle_proof(network_addr, DataTypes::Chunk, chunk, proof)
            .await?;
        Ok(address)
    }

    /// Upload a record with a Merkle payment proof
    ///
    /// This method first attempts upload using verified closest peers.
    /// If that fails, it falls back to using direct Kad query peers for retry.
    pub async fn upload_record_with_merkle_proof<T: serde::Serialize + Clone>(
        &self,
        network_addr: NetworkAddress,
        data_type: DataTypes,
        data: T,
        proof: &MerklePaymentProof,
    ) -> Result<(), MerklePutError> {
        let record_kind = RecordKind::DataWithMerklePayment(data_type);
        let record = Record {
            key: network_addr.to_record_key(),
            value: try_serialize_record(&(proof.clone(), data), record_kind)
                .map_err(|e| {
                    MerklePutError::Serialization(format!(
                        "Failed to serialize chunk with Merkle proof: {e:?}"
                    ))
                })?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        // First attempt: use verified closest peers
        let storing_nodes = match self
            .network
            .get_closest_peers_with_retries(network_addr.clone(), None)
            .await
        {
            Ok(peers) => peers,
            Err(e) => {
                warn!(
                    "Failed to get verified closest peers for {network_addr:?}: {e:?}, trying Kad-only fallback"
                );
                // Fallback to Kad-only query if verification fails
                self.network
                    .get_closest_peers_kad_only(network_addr.clone(), None)
                    .await?
            }
        };

        match self
            .network
            .put_record_with_retries(record.clone(), storing_nodes.clone(), &self.config.chunks)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                // Fallback: use direct Kad query to get a different set of peers and retry
                warn!(
                    "Merkle upload failed for {network_addr:?}: {e:?}, attempting Kad-only fallback"
                );

                let fallback_nodes = self
                    .network
                    .get_closest_peers_kad_only(network_addr.clone(), None)
                    .await?;

                debug!(
                    "Retrying merkle upload to {} Kad-only peers for {network_addr:?}",
                    fallback_nodes.len()
                );

                self.network
                    .put_record_with_retries(record, fallback_nodes, &self.config.chunks)
                    .await?;

                Ok(())
            }
        }
    }

    /// Retry uploading failed chunks with pause between attempts.
    ///
    /// Returns remaining failed chunks after all retry attempts (empty if all succeeded).
    pub async fn retry_failed_merkle_chunks(
        &self,
        mut failed_chunks: Vec<(Chunk, String)>,
        receipt: &MerklePaymentReceipt,
        already_exist: &mut HashSet<XorName>,
        max_retries: usize,
        retry_pause_secs: u64,
    ) -> Result<Vec<(Chunk, String)>, MerklePutError> {
        let mut retry_attempt = 0;
        let upload_batch_size = std::cmp::max(1, *CHUNK_UPLOAD_BATCH_SIZE);

        while !failed_chunks.is_empty() && retry_attempt < max_retries {
            retry_attempt += 1;
            let failed_count = failed_chunks.len();

            crate::loud_info!(
                "⚠️ Upload batch failed: {failed_count} chunks failed. Retrying scheduled"
            );
            crate::loud_info!(
                "Retry attempt {retry_attempt}/{max_retries}: {failed_count} chunks remaining. Pausing for {retry_pause_secs} seconds..."
            );

            sleep(Duration::from_secs(retry_pause_secs)).await;

            crate::loud_info!("🔄 Retrying {failed_count} chunks...");

            // Build upload tasks
            let chunks_to_retry: Vec<Chunk> =
                failed_chunks.into_iter().map(|(chunk, _)| chunk).collect();
            let mut tasks = Vec::with_capacity(chunks_to_retry.len());

            for chunk in chunks_to_retry {
                let xor_name = *chunk.name();
                if already_exist.contains(&xor_name) {
                    continue;
                }

                let Some(proof) = receipt.proofs.get(&xor_name).cloned() else {
                    return Err(MerklePutError::MissingPaymentProofFor(xor_name));
                };

                let client = self.clone();
                tasks.push(async move {
                    let result = client.upload_chunk_with_merkle_proof(&chunk, &proof).await;
                    (chunk, result)
                });
            }

            let results = process_tasks_with_max_concurrency(tasks, upload_batch_size).await;

            // Collect new failures
            failed_chunks = Vec::new();
            for (chunk, result) in results {
                match result {
                    Ok(addr) => {
                        already_exist.insert(*addr.xorname());
                        crate::loud_debug!("✓ Retry succeeded for chunk: {addr:?}");
                    }
                    Err(err) => {
                        crate::loud_error!("✗ Retry failed for chunk {:?}: {err}", chunk.address());
                        failed_chunks.push((chunk, err.to_string()));
                    }
                }
            }
        }

        Ok(failed_chunks)
    }
}
