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
use crate::client::{ClientEvent, UploadSummary};
use crate::self_encryption::{EncryptionStream, MAX_CHUNK_SIZE, encrypt_directory_files};
use ant_evm::{AttoTokens, EvmWallet};
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
    /// Estimate the cost of uploading a directory of files with Merkle batch payments
    ///
    /// This calls the smart contract's view function (0 gas) which runs the exact same
    /// pricing logic as the actual payment, ensuring accurate cost estimation.
    ///
    /// # Arguments
    /// * `path` - The path to the directory
    /// * `is_public` - Whether the files will be uploaded as public
    /// * `wallet` - The EVM wallet (needed for network checking)
    ///
    /// # Returns
    /// * `AttoTokens` - Estimated total cost
    pub async fn file_cost_merkle(
        &self,
        path: PathBuf,
        is_public: bool,
        wallet: &EvmWallet,
    ) -> Result<AttoTokens, MerkleFilePutError> {
        debug!(
            "merkle payment: file_cost_merkle starting for path: {path:?}, is_public: {is_public}"
        );

        // Check if the wallet uses the same network as the client
        if wallet.network() != self.evm_network() {
            return Err(MerkleFilePutError::MerklePayment(
                MerklePaymentError::EvmWalletNetworkMismatch,
            ));
        }

        #[cfg(feature = "loud")]
        println!("Encrypting files to calculate cost...");
        // Get encryption streams to determine chunk count and addresses
        let pay_streams: Vec<EncryptionStream> = encrypt_directory_files(path.clone(), is_public)
            .await
            .map_err(|e| MerkleFilePutError::Encryption(e.to_string()))?
            .into_iter()
            .map(|stream| stream.map_err(MerkleFilePutError::Encryption))
            .collect::<Result<Vec<EncryptionStream>, MerkleFilePutError>>()?;
        debug!(
            "merkle payment: file_cost_merkle got {} encryption streams",
            pay_streams.len()
        );

        // Collect all XorNames from streams
        let all_xor_names = pay_streams
            .into_iter()
            .flat_map(collect_xor_names_from_stream)
            .collect::<Vec<XorName>>();
        debug!(
            "merkle payment: file_cost_merkle total chunks: {}",
            all_xor_names.len()
        );

        #[cfg(feature = "loud")]
        println!("Encrypted into {} chunks", all_xor_names.len());

        // Build Merkle tree and get candidate pools (same as actual payment)
        use ant_evm::merkle_payments::MerkleTree;
        use std::time::{SystemTime, UNIX_EPOCH};

        #[cfg(feature = "loud")]
        println!("Building Merkle tree...");
        let tree = MerkleTree::from_xornames(all_xor_names.clone())
            .map_err(|e| MerkleFilePutError::MerklePayment(MerklePaymentError::MerkleTree(e)))?;
        let depth = tree.depth();
        debug!("merkle payment: file_cost_merkle built tree with depth={depth}");

        let merkle_payment_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| MerkleFilePutError::MerklePayment(MerklePaymentError::TimestampError(e)))?
            .as_secs();
        let midpoint_proofs = tree
            .reward_candidates(merkle_payment_timestamp)
            .map_err(|e| MerkleFilePutError::MerklePayment(MerklePaymentError::MerkleTree(e)))?;
        debug!(
            "merkle payment: file_cost_merkle generated {} candidate pools",
            midpoint_proofs.len()
        );

        #[cfg(feature = "loud")]
        println!("Creating reward candidate pools...");
        // Query network for candidate pools with signature validation
        let candidate_pools = self
            .build_candidate_pools(midpoint_proofs, DataTypes::Chunk, MAX_CHUNK_SIZE)
            .await?;
        debug!(
            "merkle payment: file_cost_merkle collected and validated {} pools",
            candidate_pools.len()
        );

        // Convert to pool commitments
        use evmlib::merkle_batch_payment::PoolCommitment;
        let pool_commitments: Vec<PoolCommitment> = candidate_pools
            .iter()
            .map(|pool| pool.to_commitment())
            .collect();

        #[cfg(feature = "loud")]
        println!("Estimating cost...");
        // Call wallet's estimate function which calls the contract's view function
        let estimated_cost_raw = wallet
            .estimate_merkle_payment_cost(depth, &pool_commitments, merkle_payment_timestamp)
            .await
            .map_err(|e| {
                MerkleFilePutError::MerklePayment(MerklePaymentError::EvmWalletError(e))
            })?;

        // Convert U256 to AttoTokens
        let estimated_cost = AttoTokens::from_atto(estimated_cost_raw);

        debug!("merkle payment: file_cost_merkle estimated cost: {estimated_cost}");
        Ok(estimated_cost)
    }

    /// Pay for a directory of files with Merkle batch payments
    pub async fn file_pay_with_merkle_payment(
        &self,
        path: PathBuf,
        is_public: bool,
        wallet: &EvmWallet,
    ) -> Result<MerklePaymentReceipt, MerkleFilePutError> {
        debug!("merkle payment: file_pay starting for path: {path:?}, is_public: {is_public}");
        #[cfg(feature = "loud")]
        println!("Paying for {path:?}...");

        #[cfg(feature = "loud")]
        println!("Encrypting files for payment...");
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

        #[cfg(feature = "loud")]
        println!("Performing first encryption pass to build merkle tree...");
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

        #[cfg(feature = "loud")]
        println!(
            "Submitting payment for {} chunks in {} files...",
            all_xor_names.len(),
            file_chunk_counts.len()
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
        #[cfg(feature = "loud")]
        println!("✓ Payment successful: {} paid", receipt.amount_paid);

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
    /// * Tuple of (amount_paid, results) where:
    ///   - `amount_paid` - Total amount paid for the Merkle batch (in AttoTokens)
    ///   - `results` - Vector of (relative_path, datamap, metadata) tuples for each uploaded file
    pub async fn files_put_with_merkle_payment(
        &self,
        path: PathBuf,
        is_public: bool,
        payment: MerklePaymentOption<'_>,
    ) -> Result<(AttoTokens, Vec<(PathBuf, DataMapChunk, Metadata)>), MerkleFilePutError> {
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
                #[cfg(feature = "loud")]
                println!(
                    "Using cached payment receipt with {} proofs",
                    receipt.proofs.len()
                );
                receipt
            }
        };

        #[cfg(feature = "loud")]
        println!("Starting upload phase...");
        // Get encryption streams for upload
        debug!("merkle payment: files_put encrypting directory files for upload");
        #[cfg(feature = "loud")]
        println!("Encrypting files for upload...");
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
            let file_num = file_index + 1;
            let relative_path = file.relative_path.clone();
            let metadata = file.metadata.clone();
            let file_path = file.file_path.clone();
            let file_chunk_count = *receipt.file_chunk_counts.get(&file_path).unwrap_or(&0);

            debug!(
                "merkle payment: files_put uploading file {file_num}/{total_files}: {file_path:?} ({file_chunk_count} chunks)"
            );

            #[cfg(feature = "loud")]
            println!(
                "[File {file_num}/{total_files}] Uploading {file_chunk_count} chunks from: {file_path}"
            );
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

            debug!("merkle payment: files_put successfully uploaded file {file_num}");
            #[cfg(feature = "loud")]
            println!("[File {file_num}/{total_files}] uploaded successfully");
            results.push((relative_path, datamap, metadata));
        }

        debug!("merkle payment: files_put all files uploaded successfully");
        #[cfg(feature = "loud")]
        println!("✓ All {total_files} files uploaded successfully!");

        // Send upload completion event
        let total_chunks: usize = receipt.file_chunk_counts.values().sum();
        if let Some(sender) = &self.client_event_sender {
            let summary = UploadSummary {
                records_paid: total_chunks,
                records_already_paid: 0,
                tokens_spent: receipt.amount_paid.as_atto(),
            };

            if let Err(err) = sender.send(ClientEvent::UploadComplete(summary)).await {
                error!("Failed to send upload completion event: {err:?}");
            }
        }

        Ok((receipt.amount_paid, results))
    }
}

/// Collect all XorNames from a stream
fn collect_xor_names_from_stream(mut encryption_stream: EncryptionStream) -> Vec<XorName> {
    let mut xor_names: Vec<XorName> = Vec::new();
    let xorname_collection_batch_size: usize = std::cmp::max(32, *CHUNK_UPLOAD_BATCH_SIZE);
    let mut total = 0;
    let estimated_total = encryption_stream.total_chunks();
    let file_path = &encryption_stream.file_path;
    #[cfg(feature = "loud")]
    let start = std::time::Instant::now();
    #[cfg(feature = "loud")]
    println!("Begin encrypting ~{estimated_total} chunks from {file_path}...");
    while let Some(batch) = encryption_stream.next_batch(xorname_collection_batch_size) {
        let batch_len = batch.len();
        total += batch_len;
        for chunk in batch {
            xor_names.push(*chunk.name());
        }
        #[cfg(feature = "loud")]
        println!(
            "Encrypted {total}/{estimated_total} chunks in {:?}",
            start.elapsed()
        );
    }
    xor_names
}
