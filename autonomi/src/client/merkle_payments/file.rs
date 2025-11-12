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
use crate::client::files::Metadata;
use crate::self_encryption::{EncryptionStream, MAX_CHUNK_SIZE, encrypt_directory_files};
use ant_evm::EvmWallet;
use ant_protocol::storage::DataTypes;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use xor_name::XorName;

/// Payment option for Merkle batch uploads
#[derive(Clone)]
pub enum MerklePaymentOption<'a> {
    /// Pay with a wallet - will create a new Merkle payment
    Wallet(&'a EvmWallet),
    /// Use a cached receipt from a previous payment
    Receipt(MerklePaymentReceipt),
}

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
    /// Pay for a directory of files with Merkle batch payments
    pub async fn file_pay_with_merkle_payment(
        &self,
        path: PathBuf,
        is_public: bool,
        wallet: &EvmWallet,
    ) -> Result<MerklePaymentReceipt, MerkleFilePutError> {
        debug!("merkle payment: file_pay starting for path: {path:?}, is_public: {is_public}");

        // Get encryption streams for payment
        let pay_streams: Vec<EncryptionStream> = encrypt_directory_files(path.clone(), is_public)
            .await
            .map_err(|e| MerkleFilePutError::Encryption(e.to_string()))?
            .into_iter()
            .map(|stream| stream.map_err(MerkleFilePutError::Encryption))
            .collect::<Result<Vec<EncryptionStream>, MerkleFilePutError>>()?;
        debug!(
            "merkle payment: file_pay got {} encryption streams",
            pay_streams.len()
        );

        // Pay for the all xornames, consuming the pay_streams
        let xor_names_for_each_file = pay_streams
            .into_iter()
            .map(|s| (s.file_path.clone(), collect_xor_names_from_stream(s)))
            .collect::<HashMap<String, Vec<XorName>>>();
        let file_chunk_counts = xor_names_for_each_file
            .iter()
            .map(|(path, xor_names)| (path.clone(), xor_names.len()))
            .collect::<HashMap<String, usize>>();
        debug!(
            "merkle payment: file_pay collected chunks from {} files",
            file_chunk_counts.len()
        );
        for (file_path, count) in &file_chunk_counts {
            debug!("merkle payment: file_pay   - {file_path}: {count} chunks");
        }

        let all_xor_names = xor_names_for_each_file
            .into_iter()
            .flat_map(|(_, xor_names)| xor_names)
            .collect::<Vec<XorName>>();
        debug!(
            "merkle payment: file_pay total chunks to pay for: {}",
            all_xor_names.len()
        );

        let mut receipt = self
            .pay_for_merkle_batch(
                DataTypes::Chunk,
                all_xor_names.into_iter(),
                MAX_CHUNK_SIZE,
                wallet,
            )
            .await?;
        debug!(
            "merkle payment: file_pay received receipt with {} proofs",
            receipt.proofs.len()
        );

        // Add file chunk counts to receipt
        receipt.file_chunk_counts = file_chunk_counts;
        debug!("merkle payment: file_pay completed successfully");

        Ok(receipt)
    }

    /// Upload a directory of files with Merkle batch payments
    /// It is very important that the files are not changed while they are being uploaded as it could invalidate the Merkle payment.
    ///
    /// # Arguments
    /// * `path` - The path to the directory to upload
    /// * `is_public` - Whether the files are uploaded as public
    /// * `payment` - The payment option (wallet or cached receipt)
    ///
    /// # Returns
    /// * `Vec<(PathBuf, DataMapChunk, Metadata)>` - A vector of (relative_path, datamap, metadata) tuples for each uploaded file
    pub async fn files_put_with_merkle_payment(
        &self,
        path: PathBuf,
        is_public: bool,
        payment: MerklePaymentOption<'_>,
    ) -> Result<Vec<(PathBuf, DataMapChunk, Metadata)>, MerkleFilePutError> {
        debug!(
            "merkle payment: files_put starting upload for path: {path:?}, is_public: {is_public}"
        );

        // Get receipt based on payment option
        let receipt = match payment {
            MerklePaymentOption::Wallet(wallet) => {
                debug!("merkle payment: files_put using wallet payment option");
                self.file_pay_with_merkle_payment(path.clone(), is_public, wallet)
                    .await?
            }
            MerklePaymentOption::Receipt(receipt) => {
                debug!(
                    "merkle payment: files_put using cached receipt with {} proofs",
                    receipt.proofs.len()
                );
                receipt
            }
        };

        // Get encryption streams for upload
        debug!("merkle payment: files_put encrypting directory files for upload");
        let upload_streams: Vec<EncryptionStream> = encrypt_directory_files(path, is_public)
            .await
            .map_err(|e| MerkleFilePutError::Encryption(e.to_string()))?
            .into_iter()
            .map(|stream| stream.map_err(MerkleFilePutError::Encryption))
            .collect::<Result<Vec<EncryptionStream>, MerkleFilePutError>>()?;
        let total_files = upload_streams.len();
        debug!("merkle payment: files_put got {total_files} upload streams");

        // For each files, upload the chunks
        let mut results: Vec<(PathBuf, DataMapChunk, Metadata)> = Vec::new();
        for (file_index, file) in upload_streams.into_iter().enumerate() {
            // Extract metadata before upload
            let relative_path = file.relative_path.clone();
            let metadata = file.metadata.clone();
            let file_path = file.file_path.clone();
            let file_chunk_count = *receipt.file_chunk_counts.get(&file_path).unwrap_or(&0);

            debug!(
                "merkle payment: files_put uploading file {}/{total_files}: {file_path:?} ({file_chunk_count} chunks)",
                file_index + 1
            );

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

            let file_num = file_index + 1;
            debug!("merkle payment: files_put successfully uploaded file {file_num}");
            results.push((relative_path, datamap, metadata));
        }

        debug!("merkle payment: files_put all files uploaded successfully");
        Ok(results)
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
