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
use std::path::PathBuf;
use thiserror::Error;
use tracing::debug;
use xor_name::XorName;

#[derive(Debug, Error)]
pub enum MerklePutError {
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    #[error("Serialization error: {0}")]
    Serialization(String),

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

impl Client {
    /// Upload streams of chunks with Merkle batch payments
    ///
    /// Upload up to `limit` chunks from streams, returning remaining streams and completed file results
    ///
    /// This processes streams in order, uploading chunks until `limit` is reached or all streams exhausted.
    /// When a stream is exhausted, its datamap is harvested and added to the results.
    ///
    /// # Arguments
    /// * `streams` - Vector of encryption streams to process
    /// * `receipt` - Merkle payment receipt containing proofs for chunks
    /// * `limit` - Maximum number of chunks to upload in this batch
    ///
    /// # Returns
    /// * Tuple of (remaining_streams, completed_file_results)
    pub async fn upload_batch_with_merkle(
        &self,
        mut streams: Vec<EncryptionStream>,
        receipt: &MerklePaymentReceipt,
        limit: usize,
    ) -> Result<
        (
            Vec<EncryptionStream>,
            Vec<(PathBuf, DataMapChunk, Metadata)>,
        ),
        MerklePutError,
    > {
        let mut completed_files: Vec<(PathBuf, DataMapChunk, Metadata)> = Vec::new();
        let mut chunks_uploaded = 0;
        let total_files = receipt.file_chunk_counts.len();
        let upload_batch_size = std::cmp::max(1, *CHUNK_UPLOAD_BATCH_SIZE);

        while chunks_uploaded < limit {
            let Some(stream) = streams.first_mut() else {
                break;
            };

            // Try to get next batch of chunks from current stream
            let remaining_in_batch = limit - chunks_uploaded;
            let batch_size = std::cmp::min(upload_batch_size, remaining_in_batch);

            match stream.next_batch(batch_size) {
                Some(chunks) if !chunks.is_empty() => {
                    // Upload this batch of chunks
                    let mut tasks = Vec::with_capacity(chunks.len());
                    for chunk in chunks {
                        let xor_name = *chunk.name();
                        let proof = receipt
                            .proofs
                            .get(&xor_name)
                            .ok_or(MerklePutError::MissingPaymentProofFor(xor_name))?
                            .clone();
                        let client = self.clone();
                        tasks.push(async move {
                            client.upload_chunk_with_merkle_proof(&chunk, &proof).await
                        });
                    }

                    let results = process_tasks_with_max_concurrency(tasks, batch_size).await;

                    // Check each result for errors - propagate first error encountered
                    for result in results {
                        let addr = result?;
                        chunks_uploaded += 1;
                        debug!("Uploaded chunk {chunks_uploaded}/{limit}: {addr:?}");
                        #[cfg(feature = "loud")]
                        println!("({chunks_uploaded}/{limit}) Chunk stored at: {addr:?}");
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
                        debug!("[File {f}/{total_files}] ({path:?}) is now available at: {a:?}");
                        #[cfg(feature = "loud")]
                        println!("[File {f}/{total_files}] ({path:?}) is now available at: {a:?}");
                    }
                }
            }
        }

        Ok((streams, completed_files))
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
    pub async fn upload_record_with_merkle_proof<T: serde::Serialize>(
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

        let storing_nodes = self
            .network
            .get_closest_peers_with_retries(network_addr.clone(), None)
            .await?;

        self.network
            .put_record_with_retries(record, storing_nodes.clone(), &self.config.chunks)
            .await?;

        Ok(())
    }
}
