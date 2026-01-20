// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use self_encryption::DataMap;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tracing::info;

use crate::client::config::MERKLE_PAYMENT_THRESHOLD;
use crate::client::data_types::chunk::ChunkAddress;
use crate::client::merkle_payments::{
    MerklePaymentError, MerklePaymentOption, MerkleUploadErrorWithReceipt,
};
use crate::client::{GetError, PutError, quote::CostError};
use crate::self_encryption::{EncryptionStream, MAX_CHUNK_SIZE};
use crate::utils::process_tasks_with_max_concurrency;
use crate::{
    Client,
    chunk::DataMapChunk,
    client::payment::{BulkPaymentOption, PaymentOption},
};
use ant_evm::AttoTokens;
use bytes::Bytes;
use self_encryption::streaming_decrypt_from_storage;
use xor_name::XorName;

pub mod archive_private;
pub mod archive_public;
mod cost;
pub mod fs_private;
pub mod fs_public;

pub use archive_private::PrivateArchive;
pub use archive_public::PublicArchive;

/// Estimate chunk count for a directory or file.
/// Returns at least 3 chunks per file (self-encryption minimum).
pub(crate) fn estimate_directory_chunks(dir_path: &PathBuf) -> Result<usize, std::io::Error> {
    let mut total_chunks = 0;

    // Handle single file case
    if dir_path.is_file() {
        let size = std::fs::metadata(dir_path)?.len() as usize;
        return Ok(std::cmp::max(3, size.div_ceil(MAX_CHUNK_SIZE)));
    }

    // Walk directory
    for entry in walkdir::WalkDir::new(dir_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let size = entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
            // Each file produces at least 3 chunks (self-encryption minimum)
            total_chunks += std::cmp::max(3, size.div_ceil(MAX_CHUNK_SIZE));
        }
    }

    Ok(total_chunks)
}

/// Convert encryption streams to file results (path, datamap, metadata).
pub(crate) fn streams_to_file_results(
    streams: Vec<EncryptionStream>,
) -> Result<Vec<(PathBuf, DataMapChunk, Metadata)>, UploadError> {
    let mut results = Vec::with_capacity(streams.len());
    for stream in streams {
        let datamap = stream.data_map_chunk().ok_or_else(|| {
            UploadError::Encryption(format!(
                "Datamap chunk not found for file: {:?}",
                stream.file_path
            ))
        })?;
        results.push((stream.relative_path, datamap, stream.metadata));
    }
    Ok(results)
}

/// Internal bulk upload handler - routes to regular or merkle flow based on payment option.
/// Returns (cost, archive) from either regular or merkle flow.
///
/// The `build_archive` function converts file results into the appropriate archive type.
pub(crate) async fn bulk_upload_internal<A, F>(
    client: &Client,
    dir_path: PathBuf,
    payment_option: BulkPaymentOption,
    is_public: bool,
    build_archive: F,
) -> Result<(AttoTokens, A), UploadError>
where
    F: FnOnce(Vec<(PathBuf, DataMapChunk, Metadata)>) -> A,
{
    match payment_option {
        BulkPaymentOption::Wallet(wallet) => {
            let estimated_chunks = estimate_directory_chunks(&dir_path)?;
            if estimated_chunks >= MERKLE_PAYMENT_THRESHOLD {
                crate::loud_info!(
                    "Using merkle payments for ~{estimated_chunks} chunks (threshold: {MERKLE_PAYMENT_THRESHOLD})"
                );
                let (cost, results) = client
                    .files_put_with_merkle_payment(
                        dir_path,
                        is_public,
                        MerklePaymentOption::Wallet(&wallet),
                    )
                    .await?;
                Ok((cost, build_archive(results)))
            } else {
                crate::loud_info!(
                    "Using regular payments for ~{estimated_chunks} chunks (threshold: {MERKLE_PAYMENT_THRESHOLD})"
                );
                let (cost, streams) = client
                    .dir_content_upload_internal(dir_path, PaymentOption::Wallet(wallet), is_public)
                    .await?;
                let results = streams_to_file_results(streams)?;
                Ok((cost, build_archive(results)))
            }
        }
        BulkPaymentOption::Receipt(receipt) => {
            let (cost, streams) = client
                .dir_content_upload_internal(dir_path, PaymentOption::Receipt(receipt), is_public)
                .await?;
            let results = streams_to_file_results(streams)?;
            Ok((cost, build_archive(results)))
        }
        BulkPaymentOption::MerkleReceipt(receipt) => {
            let (cost, results) = client
                .files_put_with_merkle_payment(
                    dir_path,
                    is_public,
                    MerklePaymentOption::Receipt(receipt),
                )
                .await?;
            Ok((cost, build_archive(results)))
        }
        BulkPaymentOption::ContinueMerkle(wallet, receipt) => {
            let (cost, results) = client
                .files_put_with_merkle_payment(
                    dir_path,
                    is_public,
                    MerklePaymentOption::ContinueWithReceipt(&wallet, receipt),
                )
                .await?;
            Ok((cost, build_archive(results)))
        }
    }
}

/// Internal single file upload handler - routes to regular or merkle flow based on payment option.
/// Returns (cost, datamap) from either regular or merkle flow.
pub(crate) async fn file_upload_internal(
    client: &Client,
    path: PathBuf,
    payment_option: BulkPaymentOption,
    is_public: bool,
) -> Result<(AttoTokens, DataMapChunk), UploadError> {
    let (cost, results) =
        bulk_upload_internal(client, path, payment_option, is_public, |r| r).await?;

    let datamap = results
        .into_iter()
        .next()
        .map(|(_, dm, _)| dm)
        .ok_or_else(|| UploadError::Encryption("No file results from upload".to_string()))?;

    Ok((cost, datamap))
}

/// Metadata for a file in an archive. Time values are UNIX timestamps (UTC).
///
/// The recommended way to create a new [`Metadata`] is to use [`Metadata::new_with_size`].
///
/// The [`Metadata::default`] method creates a new [`Metadata`] with 0 as size and the current time for created and modified.
///
/// The [`Metadata::empty`] method creates a new [`Metadata`] filled with 0s. Use this if you don't want to reveal any metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    /// File creation time on local file system as UTC. See [`std::fs::Metadata::created`] for details per OS.
    pub created: u64,
    /// Last file modification time taken from local file system as UTC. See [`std::fs::Metadata::modified`] for details per OS.
    pub modified: u64,
    /// File size in bytes
    pub size: u64,

    /// Optional extra metadata with undefined structure, e.g. JSON.
    pub extra: Option<String>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self::new_with_size(0)
    }
}

impl Metadata {
    /// Create a new metadata struct with the current time as uploaded, created and modified.
    pub fn new_with_size(size: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        Self {
            created: now,
            modified: now,
            size,
            extra: None,
        }
    }

    /// Create a new empty metadata struct
    pub fn empty() -> Self {
        Self {
            created: 0,
            modified: 0,
            size: 0,
            extra: None,
        }
    }
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum RenameError {
    #[error("File not found in archive: {0}")]
    FileNotFound(PathBuf),
}

/// Errors that can occur during the file upload operation.
///
/// # Receipt Preservation
///
/// When merkle uploads fail, the `MerkleUpload` variant preserves any payment receipts
/// that were created before the error occurred. This allows you to resume uploads without
/// losing funds:
///
/// ```ignore
/// match client.dir_content_upload(path, payment).await {
///     Ok((cost, archive)) => { /* success */ }
///     Err(UploadError::MerkleUpload(err)) => {
///         if let Some(receipt) = err.receipt {
///             // Save the receipt - it contains valid merkle proofs
///             // Resume with: BulkPaymentOption::ContinueMerkle(wallet, receipt)
///         }
///     }
///     Err(e) => { /* other error */ }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("Failed to recursively traverse directory")]
    WalkDir(#[from] walkdir::Error),
    #[error("Input/output failure")]
    IoError(#[from] std::io::Error),
    #[error("Failed to upload file")]
    PutError(#[from] PutError),
    #[error("Encryption error")]
    Encryption(String),
    #[error("Merkle upload error: {0}")]
    MerkleUpload(#[from] MerkleUploadErrorWithReceipt),
}

/// Errors that can occur during the download operation.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("Failed to download file")]
    GetError(#[from] GetError),
    #[error("IO failure")]
    IoError(#[from] std::io::Error),
}

/// Errors that can occur during the file cost calculation.
#[derive(Debug, thiserror::Error)]
pub enum FileCostError {
    #[error("Cost error: {0}")]
    Cost(#[from] CostError),
    #[error("IO failure")]
    IoError(#[from] std::io::Error),
    #[error("Serialization error")]
    Serialization(#[from] rmp_serde::encode::Error),
    #[error("Walkdir error")]
    WalkDir(#[from] walkdir::Error),
    #[error("Merkle payment error: {0}")]
    MerklePayment(#[from] MerklePaymentError),
    #[error("Encryption error: {0}")]
    Encryption(String),
}

/// Normalize a path to use forward slashes, regardless of the operating system.
/// This is used to ensure that paths stored in archives always use forward slashes,
/// which is important for cross-platform compatibility.
pub(crate) fn normalize_path(path: PathBuf) -> PathBuf {
    // Convert backslashes to forward slashes (Windows..)
    // Also collapse any double slashes that result from joining components
    let normalized = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
        .replace('\\', "/")
        .replace("//", "/");

    PathBuf::from(normalized)
}

impl Client {
    pub(crate) fn stream_download_from_datamap(
        &self,
        data_map: DataMap,
        to_dest: &Path,
    ) -> Result<(), DownloadError> {
        // Verify that the destination path can be used to create a file.
        if let Err(e) = std::fs::File::create(to_dest) {
            crate::loud_info!(
                "Input destination path {to_dest:?} cannot be used for streaming disk flushing: {e}"
            );
            crate::loud_info!(
                "This file may have been uploaded without a metadata archive. A file name must be provided to download and save it."
            );
            return Err(DownloadError::IoError(e));
        }

        // Clean up the temporary verification file
        if let Err(cleanup_err) = std::fs::remove_file(to_dest) {
            crate::loud_info!(
                "Warning: Failed to clean up temporary verification file {to_dest:?}: {cleanup_err}"
            );
            return Err(DownloadError::IoError(cleanup_err));
        }

        let total_chunks = data_map.infos().len();

        crate::loud_info!("Streaming fetching {total_chunks} chunks to {to_dest:?} ...");

        // Create parallel chunk fetcher for streaming decryption
        let client_clone = self.clone();
        let parallel_chunk_fetcher = move |chunk_names: &[(usize, XorName)]| -> Result<
            Vec<(usize, Bytes)>,
            self_encryption::Error,
        > {
            let chunk_addresses: Vec<(usize, ChunkAddress)> = chunk_names
                .iter()
                .map(|(i, name)| (*i, ChunkAddress::new(*name)))
                .collect();

            // Use tokio::task::block_in_place to handle async in sync context
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    client_clone
                        .fetch_chunks_parallel(&chunk_addresses, total_chunks)
                        .await
                })
            })
        };

        // Stream decrypt directly to file
        streaming_decrypt_from_storage(&data_map, to_dest, parallel_chunk_fetcher).map_err(
            |e| {
                DownloadError::GetError(crate::client::GetError::Decryption(
                    crate::self_encryption::Error::SelfEncryption(e),
                ))
            },
        )?;

        // Cleanup the chunk_cache
        let chunk_addrs: Vec<ChunkAddress> = data_map
            .infos()
            .iter()
            .map(|info| ChunkAddress::new(info.dst_hash))
            .collect();
        self.cleanup_cached_chunks(&chunk_addrs);

        Ok(())
    }

    /// Fetch multiple chunks in parallel from the network
    pub(super) async fn fetch_chunks_parallel(
        &self,
        chunk_addresses: &[(usize, ChunkAddress)],
        total_chunks: usize,
    ) -> Result<Vec<(usize, Bytes)>, self_encryption::Error> {
        let mut download_tasks = vec![];

        for (i, chunk_addr) in chunk_addresses {
            let client_clone = self.clone();
            let addr_clone = *chunk_addr;

            download_tasks.push(async move {
                crate::loud_debug!("Fetching chunk {i}/{total_chunks}({addr_clone:?})");
                let result = client_clone
                    .chunk_get(&addr_clone)
                    .await
                    .map(|chunk| (*i, chunk.value))
                    .map_err(|e| {
                        self_encryption::Error::Generic(format!(
                            "Failed to fetch chunk {addr_clone:?}: {e:?}"
                        ))
                    });
                crate::loud_debug!("Fetching chunk {i}/{total_chunks}({addr_clone:?}) [DONE]");
                result
            });
        }

        let chunks = process_tasks_with_max_concurrency(
            download_tasks,
            *crate::client::config::CHUNK_DOWNLOAD_BATCH_SIZE,
        )
        .await
        .into_iter()
        .collect::<Result<Vec<(usize, Bytes)>, self_encryption::Error>>()?;

        Ok(chunks)
    }

    /// Internal helper for uploading directory contents.
    /// Used by both `dir_content_upload` (private) and `dir_content_upload_public`.
    pub(crate) async fn dir_content_upload_internal(
        &self,
        dir_path: PathBuf,
        payment_option: PaymentOption,
        is_public: bool,
    ) -> Result<(AttoTokens, Vec<EncryptionStream>), UploadError> {
        info!("Uploading directory: {dir_path:?}, public: {is_public}");

        let encryption_results =
            crate::self_encryption::encrypt_directory_files(dir_path, is_public).await?;
        let mut chunk_iterators = vec![];

        for encryption_result in encryption_results {
            match encryption_result {
                Ok(stream) => {
                    crate::loud_info!("Successfully encrypted file: {:?}", stream.file_path);
                    chunk_iterators.push(stream);
                }
                Err(err_msg) => {
                    crate::loud_error!("Error during file encryption: {err_msg}");
                    return Err(UploadError::Encryption(err_msg));
                }
            }
        }

        let total_cost = self
            .pay_and_upload(payment_option, &mut chunk_iterators)
            .await?;

        Ok((total_cost, chunk_iterators))
    }
}

pub(crate) fn get_relative_file_path_from_abs_file_and_folder_path(
    abs_file_path: &Path,
    abs_folder_path: &Path,
) -> Result<PathBuf, String> {
    // check if the dir is a file
    let is_file = abs_folder_path.is_file();

    // could also be the file name
    let dir_name = abs_folder_path
        .file_name()
        .ok_or_else(|| format!("Failed to get file/dir name from path: {abs_folder_path:?}"))
        .map(PathBuf::from)?;

    if is_file {
        Ok(dir_name)
    } else {
        let folder_prefix = abs_folder_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        abs_file_path
            .strip_prefix(&folder_prefix)
            .map_err(|e| {
                format!("Could not strip prefix {folder_prefix:?} from path {abs_file_path:?}: {e}")
            })
            .map(|p| p.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_path;
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn test_normalize_path_to_forward_slashes() {
        let windows_path = PathBuf::from(r"folder\test\file.txt");
        let normalized = normalize_path(windows_path);
        assert_eq!(normalized, PathBuf::from("folder/test/file.txt"));
    }

    #[test]
    fn test_normalize_path_preserves_leading_slash() {
        // Test that paths with leading slashes don't get double slashes (issue #3260)
        // Use string comparison because PathBuf::eq normalizes paths (so "//x" == "/x")
        let path = PathBuf::from("/folder/test/file.txt");
        let normalized = normalize_path(path);
        assert_eq!(
            normalized.to_string_lossy(),
            "/folder/test/file.txt",
            "Leading slash should not produce double slash"
        );
    }
}
