// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::archive::Metadata;
use crate::client::data::CostError;
use crate::client::utils::process_tasks_with_max_concurrency;
use crate::client::Client;
use ant_evm::EvmWallet;
use ant_networking::target_arch::{Duration, SystemTime};
use bytes::Bytes;
use std::path::PathBuf;
use std::sync::LazyLock;

use super::archive::{Archive, ArchiveAddr};
use super::data::{DataAddr, GetError, PutError};

/// Number of files to upload in parallel.
/// Can be overridden by the `FILE_UPLOAD_BATCH_SIZE` environment variable.
pub static FILE_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("FILE_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                * 8,
        );
    info!("File upload batch size: {}", batch_size);
    batch_size
});

/// Errors that can occur during the file upload operation.
#[cfg(feature = "fs")]
#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("Failed to recursively traverse directory")]
    WalkDir(#[from] walkdir::Error),
    #[error("Input/output failure")]
    IoError(#[from] std::io::Error),
    #[error("Failed to upload file")]
    PutError(#[from] PutError),
    #[error("Failed to fetch file")]
    GetError(#[from] GetError),
    #[error("Failed to serialize")]
    Serialization(#[from] rmp_serde::encode::Error),
    #[error("Failed to deserialize")]
    Deserialization(#[from] rmp_serde::decode::Error),
}

#[cfg(feature = "fs")]
/// Errors that can occur during the download operation.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("Failed to download file")]
    GetError(#[from] GetError),
    #[error("IO failure")]
    IoError(#[from] std::io::Error),
}

#[cfg(feature = "fs")]
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

impl Client {
    /// Download file from network to local file system
    pub async fn file_download(
        &self,
        data_addr: DataAddr,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let data = self.data_get(data_addr).await?;
        if let Some(parent) = to_dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
            debug!("Created parent directories {parent:?} for {to_dest:?}");
        }
        tokio::fs::write(to_dest.clone(), data).await?;
        debug!("Downloaded file to {to_dest:?} from the network address {data_addr:?}");
        Ok(())
    }

    /// Download directory from network to local file system
    pub async fn dir_download(
        &self,
        archive_addr: ArchiveAddr,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let archive = self.archive_get(archive_addr).await?;
        debug!("Downloaded archive for the directory from the network at {archive_addr:?}");
        for (path, addr, _meta) in archive.iter() {
            self.file_download(*addr, to_dest.join(path)).await?;
        }
        debug!(
            "All files in the directory downloaded to {:?} from the network address {:?}",
            to_dest.parent(),
            archive_addr
        );
        Ok(())
    }

    /// Upload a directory to the network. The directory is recursively walked.
    /// Reads all files, splits into chunks, uploads chunks, uploads datamaps, uploads archive, returns ArchiveAddr (pointing to the archive)
    pub async fn dir_upload(
        &self,
        dir_path: PathBuf,
        wallet: &EvmWallet,
    ) -> Result<ArchiveAddr, UploadError> {
        info!("Uploading directory: {dir_path:?}");
        let start = tokio::time::Instant::now();

        // start upload of files in parallel
        let mut upload_tasks = Vec::new();
        for entry in walkdir::WalkDir::new(dir_path) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }

            let metadata = metadata_from_entry(&entry);
            let path = entry.path().to_path_buf();
            upload_tasks.push(async move {
                let file = self.file_upload(path.clone(), wallet).await;
                (path, metadata, file)
            });
        }

        // wait for all files to be uploaded
        let uploads =
            process_tasks_with_max_concurrency(upload_tasks, *FILE_UPLOAD_BATCH_SIZE).await;
        info!(
            "Upload of {} files completed in {:?}",
            uploads.len(),
            start.elapsed()
        );
        let mut archive = Archive::new();
        for (path, metadata, maybe_file) in uploads.into_iter() {
            match maybe_file {
                Ok(file) => archive.add_file(path, file, metadata),
                Err(err) => {
                    error!("Failed to upload file: {path:?}: {err:?}");
                    return Err(err);
                }
            }
        }

        // upload archive
        let archive_serialized = archive.into_bytes()?;
        let arch_addr = self.data_put(archive_serialized, wallet.into()).await?;

        info!("Complete archive upload completed in {:?}", start.elapsed());
        #[cfg(feature = "loud")]
        println!("Upload completed in {:?}", start.elapsed());
        debug!("Directory uploaded to the network at {arch_addr:?}");
        Ok(arch_addr)
    }

    /// Upload a file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns DataAddr (pointing to the datamap)
    async fn file_upload(
        &self,
        path: PathBuf,
        wallet: &EvmWallet,
    ) -> Result<DataAddr, UploadError> {
        info!("Uploading file: {path:?}");
        #[cfg(feature = "loud")]
        println!("Uploading file: {path:?}");

        let data = tokio::fs::read(path.clone()).await?;
        let data = Bytes::from(data);
        let addr = self.data_put(data, wallet.into()).await?;
        debug!("File {path:?} uploaded to the network at {addr:?}");
        Ok(addr)
    }

    /// Get the cost to upload a file/dir to the network.
    /// quick and dirty implementation, please refactor once files are cleanly implemented
    pub async fn file_cost(&self, path: &PathBuf) -> Result<ant_evm::AttoTokens, FileCostError> {
        let mut archive = Archive::new();
        let mut total_cost = ant_evm::Amount::ZERO;

        for entry in walkdir::WalkDir::new(path) {
            let entry = entry?;

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path().to_path_buf();
            tracing::info!("Cost for file: {path:?}");

            let data = tokio::fs::read(&path).await?;
            let file_bytes = Bytes::from(data);
            let file_cost = self.data_cost(file_bytes.clone()).await?;

            total_cost += file_cost.as_atto();

            // re-do encryption to get the correct map xorname here
            // this code needs refactor
            let now = ant_networking::target_arch::Instant::now();
            let (data_map_chunk, _) = crate::self_encryption::encrypt(file_bytes)?;
            tracing::debug!("Encryption took: {:.2?}", now.elapsed());
            let map_xor_name = *data_map_chunk.address().xorname();

            let metadata = metadata_from_entry(&entry);
            archive.add_file(path, map_xor_name, metadata);
        }

        let root_serialized = rmp_serde::to_vec(&archive)?;

        let archive_cost = self.data_cost(Bytes::from(root_serialized)).await?;

        total_cost += archive_cost.as_atto();
        debug!("Total cost for the directory: {total_cost:?}");
        Ok(total_cost.into())
    }
}

// Get metadata from directory entry. Defaults to `0` for creation and modification times if
// any error is encountered. Logs errors upon error.
pub(crate) fn metadata_from_entry(entry: &walkdir::DirEntry) -> Metadata {
    let fs_metadata = match entry.metadata() {
        Ok(metadata) => metadata,
        Err(err) => {
            tracing::warn!(
                "Failed to get metadata for `{}`: {err}",
                entry.path().display()
            );
            return Metadata {
                uploaded: 0,
                created: 0,
                modified: 0,
                size: 0,
            };
        }
    };

    let unix_time = |property: &'static str, time: std::io::Result<SystemTime>| {
        time.inspect_err(|err| {
            tracing::warn!(
                "Failed to get '{property}' metadata for `{}`: {err}",
                entry.path().display()
            );
        })
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .inspect_err(|err| {
            tracing::warn!(
                "'{property}' metadata of `{}` is before UNIX epoch: {err}",
                entry.path().display()
            );
        })
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
    };
    let created = unix_time("created", fs_metadata.created());
    let modified = unix_time("modified", fs_metadata.modified());

    Metadata {
        uploaded: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs(),
        created,
        modified,
        size: fs_metadata.len(),
    }
}
