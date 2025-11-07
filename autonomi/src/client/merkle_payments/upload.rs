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
use crate::networking::NetworkError;
use crate::self_encryption::EncryptionStream;
use ant_evm::merkle_payments::MerklePaymentProof;
use ant_protocol::NetworkAddress;
use ant_protocol::storage::{Chunk, ChunkAddress, DataTypes, RecordKind, try_serialize_record};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use libp2p::kad::Record;
use thiserror::Error;
use xor_name::XorName;

#[derive(Debug, Error)]
pub enum MerklePutError {
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    #[error("Serialization error: {0}")]
    Serialization(String),

    // Errors that should never happen:
    #[error(
        "Missing payment proof for xorname: {0}. This could be caused by a change in content between the payment and the upload. Please try again but make sure to avoid changing the files while they are being uploaded."
    )]
    MissingPaymentProofFor(XorName),
    #[error(
        "Stream should have a datamap, this is a bug: please report it and save your logs from ~/.autonomi/client/logs/"
    )]
    StreamShouldHaveDatamap,
}

impl Client {
    /// Upload a stream of chunks with Merkle batch payments
    /// # Arguments
    /// * `upload_stream` - EncryptionStream used for actual upload (will be fully consumed)
    /// * `receipt` - Merkle payment receipt for the stream of chunks
    ///
    /// # Returns
    /// * `EncryptionStream` - The exhausted upload_stream containing the datamap
    pub async fn chunk_stream_put_with_merkle_receipt(
        &self,
        mut upload_stream: EncryptionStream,
        total_chunks: usize,
        receipt: &MerklePaymentReceipt,
    ) -> Result<EncryptionStream, MerklePutError> {
        // Stream and upload chunks with Merkle proofs
        let stream_batch_size: usize = std::cmp::max(1, *CHUNK_UPLOAD_BATCH_SIZE);
        let mut tasks = FuturesUnordered::new();
        let mut completed = 0;

        // Helper to start an upload task for a chunk
        let start_upload = |chunk: ant_protocol::storage::Chunk| -> Result<_, MerklePutError> {
            let xor_name = *chunk.name();
            let proof = receipt
                .get(&xor_name)
                .ok_or(MerklePutError::MissingPaymentProofFor(xor_name))?
                .clone();

            let self_clone = self.clone();
            Ok(async move {
                self_clone
                    .upload_chunk_with_merkle_proof(&chunk, &proof)
                    .await
            })
        };

        // Process chunks in streaming batches
        while let Some(batch) = upload_stream.next_batch(stream_batch_size) {
            // Queue uploads from this batch
            for chunk in batch {
                // If we've hit max concurrent, wait for one to complete first
                while tasks.len() >= stream_batch_size {
                    if let Some(result) = tasks.next().await {
                        let addr = result?;
                        completed += 1;
                        #[cfg(feature = "loud")]
                        println!("{completed}/{total_chunks} chunks uploaded at: {addr:?}");
                        debug!("{completed}/{total_chunks} chunks uploaded at: {addr:?}");
                    }
                }

                // Start upload for this chunk
                let task = start_upload(chunk)?;
                tasks.push(task);
            }
        }

        // Wait for all remaining uploads to complete
        while let Some(result) = tasks.next().await {
            let addr = result?;
            completed += 1;
            #[cfg(feature = "loud")]
            println!("{completed}/{total_chunks} chunks uploaded at: {addr:?}");
            debug!("{completed}/{total_chunks} chunks uploaded at: {addr:?}");
        }

        Ok(upload_stream)
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
            .get_closest_peers(network_addr.clone(), None)
            .await?;

        debug!("Storing record: {record:?} to {:?}", storing_nodes);

        self.network
            .put_record_with_retries(record, storing_nodes.clone(), &self.config.chunks)
            .await?;

        Ok(())
    }
}
