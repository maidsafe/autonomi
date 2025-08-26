// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

use crate::Client;
use crate::chunk::DataMapChunk;
use crate::client::data_types::chunk::ChunkAddress;
use crate::client::utils::process_tasks_with_max_concurrency;
use crate::client::{GetError, PutError, quote::CostError};
use bytes::Bytes;
use self_encryption::streaming_decrypt_from_storage;
use xor_name::XorName;

pub mod archive_private;
pub mod archive_public;
pub mod fs_private;
pub mod fs_public;

pub use archive_private::PrivateArchive;
pub use archive_public::PublicArchive;

/// Number of files to upload in parallel.
///
/// Can be overridden by the `FILE_UPLOAD_BATCH_SIZE` environment variable.
pub static FILE_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("FILE_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    info!("File upload batch size: {}", batch_size);
    batch_size
});

/// Number of files to encrypt in parallel.
///
/// Can be overridden by the `FILE_ENCRYPT_BATCH_SIZE` environment variable.
pub static FILE_ENCRYPT_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("FILE_ENCRYPT_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                * 8,
        );
    info!("File encryption batch size: {}", batch_size);
    batch_size
});

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
#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("Failed to recursively traverse directory")]
    WalkDir(#[from] walkdir::Error),
    #[error("Input/output failure")]
    IoError(#[from] std::io::Error),
    #[error("Failed to upload file")]
    PutError(#[from] PutError),
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
    #[error("Self encryption error")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error("Walkdir error")]
    WalkDir(#[from] walkdir::Error),
}

/// Normalize a path to use forward slashes, regardless of the operating system.
/// This is used to ensure that paths stored in archives always use forward slashes,
/// which is important for cross-platform compatibility.
pub(crate) fn normalize_path(path: PathBuf) -> PathBuf {
    // Convert backslashes to forward slashes (Windows..)
    let normalized = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");

    PathBuf::from(normalized)
}

impl Client {
    /// Fetch
    pub(super) async fn stream_download_chunks_to_file(
        &self,
        data_map_chunk: &DataMapChunk,
        to_dest: &Path,
    ) -> Result<(), DownloadError> {
        let data_map = self.restore_data_map_from_chunk(data_map_chunk).await?;
        let total_chunks = data_map.infos().len();

        #[cfg(feature = "loud")]
        println!("Streaming fetching {total_chunks} chunks to {to_dest:?} ...");
        info!("Streaming fetching {total_chunks} chunks to {to_dest:?} ...");

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
                #[cfg(feature = "loud")]
                println!("Fetching chunk {i}/{total_chunks} ...");
                info!("Fetching chunk {i}/{total_chunks}({addr_clone:?})");
                let result = client_clone
                    .chunk_get(&addr_clone)
                    .await
                    .map(|chunk| (*i, chunk.value))
                    .map_err(|e| {
                        self_encryption::Error::Generic(format!(
                            "Failed to fetch chunk {addr_clone:?}: {e:?}"
                        ))
                    });
                #[cfg(feature = "loud")]
                println!("Fetching chunk {i}/{total_chunks} [DONE]");
                info!("Fetching chunk {i}/{total_chunks}({addr_clone:?}) [DONE]");
                result
            });
        }

        let chunks = process_tasks_with_max_concurrency(
            download_tasks,
            *crate::client::data_types::chunk::CHUNK_DOWNLOAD_BATCH_SIZE,
        )
        .await
        .into_iter()
        .collect::<Result<Vec<(usize, Bytes)>, self_encryption::Error>>()?;

        Ok(chunks)
    }
}

pub(crate) fn get_relative_file_path_from_abs_file_and_folder_path(
    abs_file_pah: &Path,
    abs_folder_path: &Path,
) -> PathBuf {
    // check if the dir is a file
    let is_file = abs_folder_path.is_file();

    // could also be the file name
    let dir_name = PathBuf::from(
        abs_folder_path
            .file_name()
            .expect("Failed to get file/dir name"),
    );

    if is_file {
        dir_name
    } else {
        let folder_prefix = abs_folder_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        abs_file_pah
            .strip_prefix(folder_prefix)
            .expect("Could not strip prefix path")
            .to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::normalize_path;
    #[cfg(windows)]
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn test_normalize_path_to_forward_slashes() {
        let windows_path = PathBuf::from(r"folder\test\file.txt");
        let normalized = normalize_path(windows_path);
        assert_eq!(normalized, PathBuf::from("folder/test/file.txt"));
    }
}
