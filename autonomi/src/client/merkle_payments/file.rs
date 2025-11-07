// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::payments::{MerklePaymentError, MerklePaymentReceipt};
use super::upload::MerklePutError;
use crate::Client;
use crate::client::config::CHUNK_UPLOAD_BATCH_SIZE;
use crate::client::data_types::chunk::DataMapChunk;
use crate::self_encryption::{EncryptionStream, MAX_CHUNK_SIZE, encrypt_directory_files};
use ant_evm::EvmWallet;
use ant_protocol::storage::DataTypes;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use xor_name::XorName;

#[derive(Debug, Error)]
pub enum MerkleFilePutError {
    #[error("Encryption error: {0}")]
    Encryption(String),
    #[error("Merkle payment error: {0}")]
    MerklePayment(#[from] MerklePaymentError),
    #[error("Upload error: {err}")]
    Upload {
        err: MerklePutError,
        receipt: MerklePaymentReceipt,
    },
}

impl Client {
    /// Upload a directory of files with Merkle batch payments
    /// It is very important that the files are not changed while they are being uploaded as it could invalidate the Merkle payment.
    ///
    /// # Arguments
    /// * `path` - The path to the directory to upload
    /// * `is_public` - Whether the files are uploaded as public
    /// * `wallet` - The wallet to use for the Merkle payment
    ///
    /// # Returns
    /// * `HashMap<String, DataMapChunk>` - A map of file paths to their datamaps
    pub async fn files_put_with_merkle_payment(
        &self,
        path: PathBuf,
        is_public: bool,
        wallet: &EvmWallet,
    ) -> Result<HashMap<String, DataMapChunk>, MerkleFilePutError> {
        // Get encryption streams for payment
        let pay_streams: Vec<EncryptionStream> = encrypt_directory_files(path.clone(), is_public)
            .await
            .map_err(|e| MerkleFilePutError::Encryption(e.to_string()))?
            .into_iter()
            .map(|stream| stream.map_err(MerkleFilePutError::Encryption))
            .collect::<Result<Vec<EncryptionStream>, MerkleFilePutError>>()?;

        // Pay for the all xornames, consuming the pay_streams
        let xor_names_for_each_file = pay_streams
            .into_iter()
            .map(|s| (s.file_path.clone(), collect_xor_names_from_stream(s)))
            .collect::<HashMap<String, Vec<XorName>>>();
        let all_file_chunk_counts = xor_names_for_each_file
            .iter()
            .map(|(path, xor_names)| (path.clone(), xor_names.len()))
            .collect::<HashMap<String, usize>>();
        let all_xor_names = xor_names_for_each_file
            .into_iter()
            .flat_map(|(_, xor_names)| xor_names)
            .collect::<Vec<XorName>>();
        let receipt = self
            .pay_for_merkle_batch(
                DataTypes::Chunk,
                all_xor_names.into_iter(),
                MAX_CHUNK_SIZE,
                wallet,
            )
            .await?;

        // Get encryption streams for upload
        let upload_streams: Vec<EncryptionStream> = encrypt_directory_files(path, is_public)
            .await
            .map_err(|e| MerkleFilePutError::Encryption(e.to_string()))?
            .into_iter()
            .map(|stream| stream.map_err(MerkleFilePutError::Encryption))
            .collect::<Result<Vec<EncryptionStream>, MerkleFilePutError>>()?;

        // For each files, upload the chunks
        let mut datamaps: HashMap<String, DataMapChunk> = HashMap::new();
        for file in upload_streams {
            // Chatter
            let file_path = file.file_path.clone();
            let file_chunk_count = *all_file_chunk_counts.get(&file_path).unwrap_or(&0);
            #[cfg(feature = "loud")]
            println!("Uploading {file_chunk_count} chunks from file: {file_path:?}");
            info!("Uploading {file_chunk_count} chunks from file: {file_path:?}");

            // Upload the chunks
            let exhausted_stream = self
                .chunk_stream_put_with_merkle_receipt(file, file_chunk_count, &receipt)
                .await
                .map_err(|err| MerkleFilePutError::Upload {
                    err,
                    receipt: receipt.clone(),
                })?;
            let datamap = exhausted_stream
                .data_map_chunk()
                .ok_or(MerkleFilePutError::Upload {
                    err: MerklePutError::StreamShouldHaveDatamap,
                    receipt: receipt.clone(),
                })?;
            datamaps.insert(file_path, datamap);
        }

        Ok(datamaps)
    }
}

/// Collect all XorNames from a stream
fn collect_xor_names_from_stream(mut encryption_stream: EncryptionStream) -> Vec<XorName> {
    let mut xor_names: Vec<XorName> = Vec::new();
    let xorname_collection_batch_size: usize = std::cmp::max(32, *CHUNK_UPLOAD_BATCH_SIZE);
    while let Some(batch) = encryption_stream.next_batch(xorname_collection_batch_size) {
        for chunk in batch {
            xor_names.push(*chunk.name());
        }
    }
    xor_names
}
