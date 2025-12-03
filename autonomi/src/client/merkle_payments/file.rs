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
use ant_evm::merkle_payments::MAX_LEAVES;
use ant_evm::{AttoTokens, EvmWallet};
use ant_protocol::storage::DataTypes;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use xor_name::XorName;

/// Payment option for Merkle batch uploads
#[derive(Clone)]
pub enum MerklePaymentOption<'a> {
    /// Fresh upload - pays for all chunks
    Wallet(&'a EvmWallet),
    /// Upload with External Payment Flow - assumes all proofs present, fails if not
    Receipt(MerklePaymentReceipt),
    /// Continue/Retry upload with partial payment receipt - uses existing proofs, pays for any missing chunks
    ContinueWithReceipt(&'a EvmWallet, MerklePaymentReceipt),
}

/// Error with optional receipt attached
/// Receipt is only present if payments were made before the error occurred
#[derive(Debug, Error)]
#[error("{error}")]
pub struct MerkleUploadErrorWithReceipt {
    /// Receipt if any payments were made before failure (None = no payment happened)
    pub receipt: Option<MerklePaymentReceipt>,
    /// The actual error details
    #[source]
    pub error: MerkleUploadError,
}

impl MerkleUploadErrorWithReceipt {
    /// Create error, only including receipt if it contains actual payments
    fn new(receipt: MerklePaymentReceipt, kind: MerkleUploadError) -> Self {
        let receipt = if receipt.proofs.is_empty() {
            None // No payments made
        } else {
            Some(receipt) // Real payments - include proof
        };
        Self {
            receipt,
            error: kind,
        }
    }

    fn encryption(receipt: MerklePaymentReceipt, msg: String) -> Self {
        Self::new(receipt, MerkleUploadError::Encryption(msg))
    }

    fn payment(receipt: MerklePaymentReceipt, err: MerklePaymentError) -> Self {
        Self::new(receipt, MerkleUploadError::Payment(err))
    }

    fn upload(receipt: MerklePaymentReceipt, err: MerklePutError) -> Self {
        Self::new(receipt, MerkleUploadError::Upload(err))
    }
}

#[derive(Debug, Error)]
pub enum MerkleUploadError {
    #[error("Encryption error: {0}")]
    Encryption(String),
    #[error("Payment error: {0}")]
    Payment(MerklePaymentError),
    #[error("Upload error: {0}")]
    Upload(MerklePutError),
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
    ) -> Result<AttoTokens, MerkleUploadError> {
        debug!(
            "merkle payment: file_cost_merkle starting for path: {path:?}, is_public: {is_public}"
        );

        // Check if the wallet uses the same network as the client
        if wallet.network() != self.evm_network() {
            return Err(MerkleUploadError::Payment(
                MerklePaymentError::EvmWalletNetworkMismatch,
            ));
        }

        #[cfg(feature = "loud")]
        println!("Encrypting files to calculate cost...");

        // Collect all XorNames
        let (all_xor_names, _file_chunk_counts) = self
            .collect_xornames_from_dir(path, is_public)
            .await
            .map_err(MerkleUploadError::Encryption)?;

        let total_chunks = all_xor_names.len();
        debug!("merkle payment: file_cost_merkle total chunks: {total_chunks}");

        #[cfg(feature = "loud")]
        println!("Encrypted into {total_chunks} chunks");

        // Split into batches of MAX_LEAVES
        let batches: Vec<Vec<XorName>> = all_xor_names
            .chunks(MAX_LEAVES)
            .map(|c| c.to_vec())
            .collect();
        let num_batches = batches.len();

        #[cfg(feature = "loud")]
        println!("Estimating cost for {num_batches} batch(es)...");

        // Estimate cost for each batch and sum
        let mut total_cost = ant_evm::U256::ZERO;

        for (batch_idx, batch_xornames) in batches.into_iter().enumerate() {
            let batch_num = batch_idx + 1;
            debug!("Estimating batch {batch_num}/{num_batches}");

            // Prepare batch (build tree, query pools)
            let (tree, _candidate_pools, pool_commitments, merkle_payment_timestamp) = self
                .prepare_merkle_batch(DataTypes::Chunk, batch_xornames, MAX_CHUNK_SIZE)
                .await
                .map_err(MerkleUploadError::Payment)?;

            // Estimate cost for this batch
            let batch_cost = wallet
                .estimate_merkle_payment_cost(
                    tree.depth(),
                    &pool_commitments,
                    merkle_payment_timestamp,
                )
                .await
                .map_err(|e| MerkleUploadError::Payment(MerklePaymentError::EvmWalletError(e)))?;

            total_cost = total_cost.saturating_add(batch_cost);
        }

        let estimated_cost = AttoTokens::from_atto(total_cost);
        debug!("merkle payment: file_cost_merkle estimated total cost: {estimated_cost}");

        #[cfg(feature = "loud")]
        println!("Total estimated cost: {estimated_cost}");

        Ok(estimated_cost)
    }

    /// Helper function to pay for a directory of files with Merkle batch payments
    async fn files_put_with_merkle_payment_internal(
        &self,
        path: PathBuf,
        is_public: bool,
        wallet: Option<&EvmWallet>,
        mut receipt: MerklePaymentReceipt,
    ) -> Result<(AttoTokens, Vec<(PathBuf, DataMapChunk, Metadata)>), MerkleUploadErrorWithReceipt>
    {
        debug!("merkle payment: starting for path: {path:?}, is_public: {is_public}");

        // Check wallet network (if wallet provided)
        if let Some(w) = wallet
            && w.network() != self.evm_network()
        {
            return Err(MerkleUploadErrorWithReceipt::payment(
                receipt,
                MerklePaymentError::EvmWalletNetworkMismatch,
            ));
        }

        // Encrypt files to collect ALL XorNames
        #[cfg(feature = "loud")]
        println!("Encrypting files a first time to create the Merkle Tree...");
        let (all_xor_names, file_chunk_counts) = self
            .collect_xornames_from_dir(path.clone(), is_public)
            .await
            .map_err(|e| MerkleUploadErrorWithReceipt::encryption(receipt.clone(), e))?;
        receipt.file_chunk_counts = file_chunk_counts;
        let total_files = receipt.file_chunk_counts.len();

        let total_chunks = all_xor_names.len();
        info!("Collected {total_chunks} XorNames from {total_files} files");

        // Split into batches of MAX_LEAVES
        let batches: Vec<Vec<XorName>> = all_xor_names
            .chunks(MAX_LEAVES)
            .map(|c| c.to_vec())
            .collect();
        let num_batches = batches.len();
        info!("Split into {num_batches} batch(es) of up to {MAX_LEAVES} chunks each");

        // Start upload streams
        #[cfg(feature = "loud")]
        println!("Starting upload of {total_chunks} chunks in {num_batches} batch(es)...");
        let mut streams: Vec<EncryptionStream> = encrypt_directory_files(path, is_public)
            .await
            .map_err(|e| MerkleUploadErrorWithReceipt::encryption(receipt.clone(), e.to_string()))?
            .into_iter()
            .map(|stream| {
                stream.map_err(|e| MerkleUploadErrorWithReceipt::encryption(receipt.clone(), e))
            })
            .collect::<Result<Vec<EncryptionStream>, MerkleUploadErrorWithReceipt>>()?;

        let mut results: Vec<(PathBuf, DataMapChunk, Metadata)> = Vec::new();

        // Interleaved pay/upload for each batch
        for (batch_idx, batch_xornames) in batches.into_iter().enumerate() {
            let batch_num = batch_idx + 1;
            let batch_size = batch_xornames.len();
            info!("Processing batch {batch_num}/{num_batches} ({batch_size} chunks)");

            // Pay for this batch if needed
            let needs_payment = batch_xornames
                .iter()
                .any(|xn| !receipt.proofs.contains_key(xn));
            if needs_payment {
                receipt = self
                    .pay_for_batch(
                        wallet,
                        batch_xornames,
                        receipt.clone(),
                        batch_num,
                        num_batches,
                    )
                    .await
                    .map_err(|kind| MerkleUploadErrorWithReceipt::new(receipt.clone(), kind))?;
            }

            #[cfg(feature = "loud")]
            println!("Batch {batch_num}/{num_batches}: Uploading {batch_size} chunks...");

            // Upload this batch's chunks
            let (remaining_streams, completed_files) = self
                .upload_batch_with_merkle(streams, &receipt, batch_size)
                .await
                .map_err(|err| MerkleUploadErrorWithReceipt::upload(receipt.clone(), err))?;

            streams = remaining_streams;
            results.extend(completed_files);

            info!(
                "Batch {batch_num}/{num_batches} complete, {} files finished so far",
                results.len()
            );
        }

        // Handle any remaining streams (should be empty if all went well)
        for stream in streams {
            if let Some(datamap) = stream.data_map_chunk() {
                results.push((
                    stream.relative_path.clone(),
                    datamap,
                    stream.metadata.clone(),
                ));
            }
        }

        debug!("merkle payment: files_put_unified all files uploaded successfully");
        #[cfg(feature = "loud")]
        println!("✓ All {total_chunks} chunks uploaded successfully!");

        // Send upload completion event
        self.send_upload_complete_event(&receipt).await;

        Ok((receipt.amount_paid, results))
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
    ///
    /// # Errors
    /// On error, check `error.receipt` for any payments made before the failure.
    /// If `Some(receipt)`, payments were made and can be reused via [`MerklePaymentOption::ContinueWithReceipt`].
    pub async fn files_put_with_merkle_payment(
        &self,
        path: PathBuf,
        is_public: bool,
        payment: MerklePaymentOption<'_>,
    ) -> Result<(AttoTokens, Vec<(PathBuf, DataMapChunk, Metadata)>), MerkleUploadErrorWithReceipt>
    {
        debug!(
            "merkle payment: files_put starting upload for path: {path:?}, is_public: {is_public}"
        );

        match payment {
            MerklePaymentOption::Wallet(wallet) => {
                self.files_put_with_merkle_payment_internal(
                    path,
                    is_public,
                    Some(wallet),
                    MerklePaymentReceipt::default(),
                )
                .await
            }
            MerklePaymentOption::Receipt(receipt) => {
                self.files_put_with_merkle_payment_internal(path, is_public, None, receipt)
                    .await
            }
            MerklePaymentOption::ContinueWithReceipt(wallet, receipt) => {
                self.files_put_with_merkle_payment_internal(path, is_public, Some(wallet), receipt)
                    .await
            }
        }
    }

    /// Collect all XorNames from a directory, returning (all_xornames, file_chunk_counts)
    async fn collect_xornames_from_dir(
        &self,
        path: PathBuf,
        is_public: bool,
    ) -> Result<(Vec<XorName>, HashMap<String, usize>), String> {
        let streams: Vec<EncryptionStream> = encrypt_directory_files(path, is_public)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect::<Result<Vec<EncryptionStream>, String>>()?;

        let mut all_xor_names = Vec::new();
        let mut file_chunk_counts = HashMap::new();

        for stream in streams {
            let file_path = stream.file_path.clone();
            let xor_names = collect_xor_names_from_stream(stream);
            file_chunk_counts.insert(file_path, xor_names.len());
            all_xor_names.extend(xor_names);
        }

        Ok((all_xor_names, file_chunk_counts))
    }

    /// Pay for a batch, returning the merged receipt
    async fn pay_for_batch(
        &self,
        wallet: Option<&EvmWallet>,
        batch_xornames: Vec<XorName>,
        mut receipt: MerklePaymentReceipt,
        batch_num: usize,
        num_batches: usize,
    ) -> Result<MerklePaymentReceipt, MerkleUploadError> {
        // Need wallet to pay - error if Receipt variant (wallet is None)
        let w = wallet.ok_or_else(|| {
            let missing_xn = batch_xornames
                .iter()
                .find(|xn| !receipt.proofs.contains_key(xn))
                .copied()
                .unwrap_or_default();
            MerkleUploadError::Upload(MerklePutError::MissingPaymentProofFor(missing_xn))
        })?;

        let batch_size = batch_xornames.len();
        #[cfg(feature = "loud")]
        println!("Batch {batch_num}/{num_batches}: Paying for {batch_size} chunks...");

        let batch_receipt = self
            .pay_for_single_merkle_batch(DataTypes::Chunk, batch_xornames, MAX_CHUNK_SIZE, w)
            .await
            .map_err(MerkleUploadError::Payment)?;

        receipt.merge(batch_receipt);
        Ok(receipt)
    }

    /// Send upload completion event
    async fn send_upload_complete_event(&self, receipt: &MerklePaymentReceipt) {
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
