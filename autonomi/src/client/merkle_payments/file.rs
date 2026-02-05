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
use crate::client::config::{CHUNK_UPLOAD_BATCH_SIZE, UPLOAD_MAX_RETRIES, UPLOAD_RETRY_PAUSE_SECS};
use crate::client::data_types::chunk::DataMapChunk;
use crate::client::files::Metadata;
use crate::self_encryption::{EncryptionStream, MAX_CHUNK_SIZE, encrypt_directory_files};
use ant_evm::merkle_payments::MAX_LEAVES;
use ant_evm::{AttoTokens, EvmWallet};
use ant_protocol::NetworkAddress;
use ant_protocol::storage::{ChunkAddress, DataTypes};
use std::collections::{HashMap, HashSet};
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
        crate::loud_info!("Encrypting files a first time to create the Merkle Tree(s)...");
        let (all_xor_names, file_chunk_counts, first_pass_results) = self
            .collect_xornames_from_dir(path.clone(), is_public)
            .await
            .map_err(|e| MerkleUploadErrorWithReceipt::encryption(receipt.clone(), e))?;
        receipt.file_chunk_counts = file_chunk_counts;
        let total_files = receipt.file_chunk_counts.len();

        let total_chunks = all_xor_names.len();
        info!("Collected {total_chunks} XorNames from {total_files} files");

        // Check which chunks already exist on the network and split into existing/new
        // Use the receipt's already_existed set to avoid re-querying known chunks
        let (mut already_exist, mut xor_to_pay_ordered) = self
            .split_existing_and_new_chunks(all_xor_names, &receipt)
            .await;
        let to_pay_len = xor_to_pay_ordered.len();
        let already_paid_count = already_exist.len();

        // Update receipt with newly discovered existing chunks
        receipt.add_already_existed(already_exist.iter().copied());

        // Emit event to save updated already_existed state (helps with resume)
        // Only emit if receipt has proofs (indicating we're resuming) or already_existed changed
        if !receipt.proofs.is_empty() || !receipt.already_existed.is_empty() {
            self.send_merkle_batch_payment_complete(&receipt).await;
        }

        // Early return if all chunks already exist - no need for second encryption
        if to_pay_len == 0 {
            crate::loud_info!(
                "✓ All {total_chunks} chunks already exist on the network, nothing to upload"
            );

            // Send upload completion event
            self.send_upload_complete(
                receipt.proofs.len(),
                already_paid_count,
                receipt.amount_paid.as_atto(),
            )
            .await;

            return Ok((receipt.amount_paid, first_pass_results));
        } else {
            std::mem::drop(first_pass_results);
        }

        // Split into batches of MAX_LEAVES - using drain to avoid cloning
        let num_batches = to_pay_len.div_ceil(MAX_LEAVES);
        let mut batches: Vec<Vec<XorName>> = Vec::with_capacity(num_batches);
        while !xor_to_pay_ordered.is_empty() {
            let drain_count = std::cmp::min(MAX_LEAVES, xor_to_pay_ordered.len());
            batches.push(xor_to_pay_ordered.drain(..drain_count).collect());
        }
        info!("Split into {num_batches} Merkle Tree(s) of up to {MAX_LEAVES} chunks each");

        // Start upload streams (second encryption pass - needed to get actual chunk data)
        crate::loud_info!(
            "🚀 Starting upload of {to_pay_len} chunks in {num_batches} Merkle Tree(s)..."
        );
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
                    .pay_for_merkle_tree_batch(
                        wallet,
                        batch_xornames,
                        receipt.clone(),
                        batch_num,
                        num_batches,
                    )
                    .await
                    .map_err(|kind| MerkleUploadErrorWithReceipt::new(receipt.clone(), kind))?;

                // Emit event so CLI can progressively save receipt to disk for upload resume
                self.send_merkle_batch_payment_complete(&receipt).await;
            }

            crate::loud_info!(
                "🌳 Merkle Tree {batch_num}/{num_batches}: Uploading {batch_size} chunks..."
            );

            // Upload this batch's chunks (skip chunks that already exist)
            let upload_result = self
                .upload_batch_with_merkle(streams, &receipt, &mut already_exist, batch_size)
                .await
                .map_err(|err| MerkleUploadErrorWithReceipt::upload(receipt.clone(), err))?;

            streams = upload_result.streams;
            results.extend(upload_result.completed_files);

            // Retry failed chunks if any
            if !upload_result.failed_chunks.is_empty() {
                let remaining_failures = self
                    .retry_failed_merkle_chunks(
                        upload_result.failed_chunks,
                        &receipt,
                        &mut already_exist,
                        UPLOAD_MAX_RETRIES,
                        UPLOAD_RETRY_PAUSE_SECS,
                    )
                    .await
                    .map_err(|err| MerkleUploadErrorWithReceipt::upload(receipt.clone(), err))?;

                if !remaining_failures.is_empty() {
                    let failed_count = remaining_failures.len();
                    error!("{failed_count} chunks failed after {UPLOAD_MAX_RETRIES} retries");
                    return Err(MerkleUploadErrorWithReceipt::upload(
                        receipt,
                        MerklePutError::Batch(super::upload::MerkleBatchUploadState {
                            failed: remaining_failures,
                        }),
                    ));
                }
            }

            info!(
                "Batch {batch_num}/{num_batches} complete, {} files finished so far",
                results.len()
            );
        }

        // Handle any remaining streams
        for mut stream in streams {
            let datamap = if let Some(datamap) = stream.data_map_chunk() {
                datamap
            } else {
                // flush the stream of remaining duplicate chunks to force transition to StreamDone state and try again
                while stream.next_batch(16).is_some() {}

                stream
                    .data_map_chunk()
                    .ok_or(MerkleUploadErrorWithReceipt::upload(
                        receipt.clone(),
                        MerklePutError::StreamShouldHaveDatamap,
                    ))?
            };

            // add the datamap to the results
            results.push((
                stream.relative_path.clone(),
                datamap,
                stream.metadata.clone(),
            ));

            // report progress
            if let Some(public_addr) = stream.data_address() {
                let path = &stream.relative_path;
                let f = results.len();
                crate::loud_info!(
                    "[File {f}/{total_files}] ({path:?}) is now available at: {public_addr:?}"
                );
            }
        }

        crate::loud_info!("✓ All {total_chunks} chunks uploaded successfully!");

        // Send upload completion event
        self.send_upload_complete(
            receipt.proofs.len(),
            already_paid_count,
            receipt.amount_paid.as_atto(),
        )
        .await;

        Ok((receipt.amount_paid, results))
    }

    /// Upload a directory of files with Merkle batch payments (internal API).
    ///
    /// **Note**: This is an internal API. Public users should use `dir_content_upload()` or
    /// `file_content_upload()` which automatically select the optimal payment method.
    ///
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
    pub(crate) async fn files_put_with_merkle_payment(
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

    /// Collect all XorNames from a directory, returning (all_xornames, file_chunk_counts, file_results)
    /// The file_results contain (relative_path, datamap, metadata) for each file, useful when all chunks already exist.
    async fn collect_xornames_from_dir(
        &self,
        path: PathBuf,
        is_public: bool,
    ) -> Result<
        (
            Vec<XorName>,
            HashMap<String, usize>,
            Vec<(PathBuf, DataMapChunk, Metadata)>,
        ),
        String,
    > {
        let streams: Vec<EncryptionStream> = encrypt_directory_files(path, is_public)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect::<Result<Vec<EncryptionStream>, String>>()?;

        let mut all_xor_names = Vec::new();
        let mut file_chunk_counts = HashMap::new();
        let mut file_results = Vec::new();

        for stream in streams {
            let file_path = stream.file_path.clone();
            let (xor_names, relative_path, datamap, metadata) =
                collect_xor_names_from_stream(stream)?;
            file_chunk_counts.insert(file_path, xor_names.len());
            all_xor_names.extend(xor_names);
            file_results.push((relative_path, datamap, metadata));
        }

        Ok((all_xor_names, file_chunk_counts, file_results))
    }

    /// Split chunks into existing and new, checking the network for existence.
    ///
    /// Performs a fast parallel existence check and splits the input into:
    /// - set of existing chunks (already on network, no payment needed)
    /// - vec of new chunks (need payment), in original order with duplicates removed
    ///
    /// When resuming with a cached receipt, chunks that are:
    /// - In `receipt.already_existed` - skip network check (known to exist)
    /// - In `receipt.proofs` - skip network check (already paid, will be uploaded)
    ///
    /// This avoids unnecessary network queries on resume.
    ///
    /// Takes ownership of the input to avoid unnecessary copies.
    async fn split_existing_and_new_chunks(
        &self,
        xornames: Vec<XorName>,
        receipt: &MerklePaymentReceipt,
    ) -> (HashSet<XorName>, Vec<XorName>) {
        // Dedupe while preserving order (keep first occurrence only)
        let mut seen: HashSet<XorName> = HashSet::new();
        let unique_ordered: Vec<XorName> =
            xornames.into_iter().filter(|xn| seen.insert(*xn)).collect();
        let total = unique_ordered.len();

        // Separate chunks into:
        // 1. Known to exist (from receipt.already_existed) - no network check needed
        // 2. Already paid (from receipt.proofs) - no network check needed, will upload
        // 3. Unknown - need network existence check
        let mut known_existing: HashSet<XorName> = HashSet::new();
        let mut already_paid: HashSet<XorName> = HashSet::new();
        let mut need_check: Vec<XorName> = Vec::new();

        for xn in &unique_ordered {
            if receipt.already_existed.contains(xn) {
                known_existing.insert(*xn);
            } else if receipt.proofs.contains_key(xn) {
                already_paid.insert(*xn);
            } else {
                need_check.push(*xn);
            }
        }

        let known_count = known_existing.len();
        let paid_count = already_paid.len();
        let check_count = need_check.len();

        if known_count > 0 || paid_count > 0 {
            crate::loud_info!(
                "Resuming: {known_count} chunks known to exist, {paid_count} already paid, {check_count} need checking"
            );
        }

        // Only check network for unknown chunks
        if need_check.is_empty() {
            crate::loud_info!(
                "All {total} chunks accounted for (no network check needed)"
            );
            // Return known_existing and chunks that need upload (already_paid)
            // Note: already_paid chunks still need to be uploaded, so they go in the "new" list
            let new_chunks: Vec<XorName> = unique_ordered
                .into_iter()
                .filter(|xn| !known_existing.contains(xn))
                .collect();
            return (known_existing, new_chunks);
        }

        crate::loud_info!("Checking {check_count} chunks for existence on the network...");

        // Check existence on network only for unknown chunks
        let addresses: Vec<NetworkAddress> = need_check
            .iter()
            .map(|xn| NetworkAddress::from(ChunkAddress::new(*xn)))
            .collect();
        let batch_size = std::cmp::max(16, *CHUNK_UPLOAD_BATCH_SIZE);
        let existing_addrs = self.check_records_exist_batch(&addresses, batch_size).await;

        // Convert to XorName set and merge with known existing
        let newly_found_existing: HashSet<XorName> = existing_addrs
            .into_iter()
            .filter_map(|addr| addr.xorname())
            .collect();

        // Merge all existing chunks
        let mut all_existing = known_existing;
        all_existing.extend(newly_found_existing.iter().copied());

        let existing_count = all_existing.len();
        let newly_found_count = newly_found_existing.len();
        crate::loud_info!(
            "Found {newly_found_count} more chunks on network. Total existing: {existing_count}/{total}"
        );

        // Filter out existing, keeping original order
        // Note: already_paid chunks are NOT filtered out - they need to be uploaded
        let new_chunks: Vec<XorName> = unique_ordered
            .into_iter()
            .filter(|xn| !all_existing.contains(xn))
            .collect();

        (all_existing, new_chunks)
    }

    /// Pay for a Merkle Tree batch, returning the merged receipt
    async fn pay_for_merkle_tree_batch(
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
        crate::loud_info!(
            "💸 Merkle Tree {batch_num}/{num_batches}: Paying for {batch_size} chunks..."
        );

        let batch_receipt = self
            .pay_for_single_merkle_batch(DataTypes::Chunk, batch_xornames, MAX_CHUNK_SIZE, w)
            .await
            .map_err(MerkleUploadError::Payment)?;

        receipt.merge(batch_receipt);
        Ok(receipt)
    }
}

/// Collect all XorNames from a stream, returning (xornames, relative_path, datamap, metadata)
fn collect_xor_names_from_stream(
    mut encryption_stream: EncryptionStream,
) -> Result<(Vec<XorName>, PathBuf, DataMapChunk, Metadata), String> {
    let mut xor_names: Vec<XorName> = Vec::new();
    let xorname_collection_batch_size: usize = std::cmp::max(32, *CHUNK_UPLOAD_BATCH_SIZE);
    let mut total = 0;
    let estimated_total = encryption_stream.total_chunks();
    let file_path = encryption_stream.file_path.clone();
    let start = std::time::Instant::now();
    crate::loud_debug!("Begin encrypting ~{estimated_total} chunks from {file_path}...");
    while let Some(batch) = encryption_stream.next_batch(xorname_collection_batch_size) {
        let batch_len = batch.len();
        total += batch_len;
        for chunk in batch {
            xor_names.push(*chunk.name());
        }
        crate::loud_debug!(
            "Encrypted {total}/{estimated_total} chunks in {:?}",
            start.elapsed()
        );
    }

    // Extract the datamap now that the stream is drained (in StreamDone or InMemory state)
    let datamap = encryption_stream
        .data_map_chunk()
        .ok_or_else(|| format!("No datamap available for {file_path}"))?;

    Ok((
        xor_names,
        encryption_stream.relative_path,
        datamap,
        encryption_stream.metadata,
    ))
}
