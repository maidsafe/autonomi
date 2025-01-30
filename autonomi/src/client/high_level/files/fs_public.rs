// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::archive_public::{ArchiveAddr, PublicArchive};
use super::{DownloadError, FileCostError, Metadata, UploadError};
use crate::client::high_level::files::{
    get_relative_file_path_from_abs_file_and_folder_path, FILE_UPLOAD_BATCH_SIZE,
};
use crate::client::Client;
use crate::client::{high_level::data::DataAddr, utils::process_tasks_with_max_concurrency};
use ant_evm::EvmWallet;
use ant_networking::time::{Duration, SystemTime};
use bytes::Bytes;
use std::path::PathBuf;

impl Client {
    /// Download file from network to local file system
    pub async fn file_download_public(
        &self,
        data_addr: DataAddr,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let data = self.data_get_public(data_addr).await?;
        if let Some(parent) = to_dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
            debug!("Created parent directories {parent:?} for {to_dest:?}");
        }
        tokio::fs::write(to_dest.clone(), data).await?;
        debug!("Downloaded file to {to_dest:?} from the network address {data_addr:?}");
        Ok(())
    }

    /// Download directory from network to local file system
    pub async fn dir_download_public(
        &self,
        archive_addr: ArchiveAddr,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let archive = self.archive_get_public(archive_addr).await?;
        debug!("Downloaded archive for the directory from the network at {archive_addr:?}");
        for (path, addr, _meta) in archive.iter() {
            self.file_download_public(*addr, to_dest.join(path)).await?;
        }
        debug!(
            "All files in the directory downloaded to {:?} from the network address {:?}",
            to_dest.parent(),
            archive_addr
        );
        Ok(())
    }

    /// Upload a directory to the network. The directory is recursively walked and each file is uploaded to the network.
    ///
    /// The data maps of these files are uploaded on the network, making the individual files publicly available.
    ///
    /// This returns, but does not upload (!),the [`PublicArchive`] containing the data maps of the uploaded files.
    pub async fn dir_upload_public(
        &self,
        dir_path: PathBuf,
        wallet: &EvmWallet,
    ) -> Result<PublicArchive, UploadError> {
        info!("Uploading directory: {dir_path:?}");
        let start = tokio::time::Instant::now();

        // start upload of files in parallel
        let mut upload_tasks = Vec::new();
        for entry in walkdir::WalkDir::new(dir_path.clone()) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }

            let metadata = metadata_from_entry(&entry);
            let path = entry.path().to_path_buf();
            upload_tasks.push(async move {
                let file = self.file_upload_public(path.clone(), wallet).await;
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
        let mut archive = PublicArchive::new();
        for (path, metadata, maybe_file) in uploads.into_iter() {
            let rel_path = get_relative_file_path_from_abs_file_and_folder_path(&path, &dir_path);

            match maybe_file {
                Ok(file) => archive.add_file(rel_path, file, metadata),
                Err(err) => {
                    error!("Failed to upload file: {path:?}: {err:?}");
                    return Err(err);
                }
            }
        }

        #[cfg(feature = "loud")]
        println!("Upload completed in {:?}", start.elapsed());
        Ok(archive)
    }

    /// Same as [`Client::dir_upload_public`] but also uploads the archive to the network.
    ///
    /// Returns the [`ArchiveAddr`] of the uploaded archive.
    pub async fn dir_and_archive_upload_public(
        &self,
        dir_path: PathBuf,
        wallet: &EvmWallet,
    ) -> Result<ArchiveAddr, UploadError> {
        let archive = self.dir_upload_public(dir_path, wallet).await?;
        let archive_addr = self.archive_put_public(&archive, wallet).await?;
        Ok(archive_addr)
    }

    /// Upload a file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns DataAddr (pointing to the datamap)
    async fn file_upload_public(
        &self,
        path: PathBuf,
        wallet: &EvmWallet,
    ) -> Result<DataAddr, UploadError> {
        info!("Uploading file: {path:?}");
        #[cfg(feature = "loud")]
        println!("Uploading file: {path:?}");

        let data = tokio::fs::read(path.clone()).await?;
        let data = Bytes::from(data);
        let addr = self.data_put_public(data, wallet.into()).await?;
        debug!("File {path:?} uploaded to the network at {addr:?}");
        Ok(addr)
    }

    /// Get the cost to upload a file/dir to the network.
    /// quick and dirty implementation, please refactor once files are cleanly implemented
    pub async fn file_cost(&self, path: &PathBuf) -> Result<ant_evm::AttoTokens, FileCostError> {
        let mut archive = PublicArchive::new();
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
            let now = ant_networking::time::Instant::now();
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
                created: 0,
                modified: 0,
                size: 0,
                extra: None,
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
        created,
        modified,
        size: fs_metadata.len(),
        extra: None,
    }
}
