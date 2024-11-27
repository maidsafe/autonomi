// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::utils::process_tasks_with_max_concurrency;
use crate::client::Client;
use bytes::Bytes;
use sn_evm::EvmWallet;
use std::path::PathBuf;

use super::archive_private::{PrivateArchive, PrivateArchiveAccess};
use super::data_private::PrivateDataAccess;
use super::fs::{DownloadError, UploadError};

use super::fs::FILE_UPLOAD_BATCH_SIZE;

impl Client {
    /// Download a private file from network to local file system
    pub async fn private_file_download(
        &self,
        data_access: PrivateDataAccess,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let data = self.private_data_get(data_access).await?;
        if let Some(parent) = to_dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
            debug!("Created parent directories for {to_dest:?}");
        }
        tokio::fs::write(to_dest.clone(), data).await?;
        debug!("Downloaded private file to {to_dest:?}");
        Ok(())
    }

    /// Download a private directory from network to local file system
    pub async fn private_dir_download(
        &self,
        archive_access: PrivateArchiveAccess,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let archive = self.private_archive_get(archive_access).await?;
        for (path, addr, _meta) in archive.iter() {
            self.private_file_download(addr.clone(), to_dest.join(path))
                .await?;
        }
        debug!("Downloaded private directory to {to_dest:?}");
        Ok(())
    }

    /// Upload a private directory to the network. The directory is recursively walked.
    /// Reads all files, splits into chunks, uploads chunks, uploads private archive, returns [`PrivateArchiveAccess`] (pointing to the private archive)
    pub async fn private_dir_upload(
        &self,
        dir_path: PathBuf,
        wallet: &EvmWallet,
    ) -> Result<PrivateArchiveAccess, UploadError> {
        info!("Uploading directory as private: {dir_path:?}");
        let start = tokio::time::Instant::now();

        // start upload of file in parallel
        let mut upload_tasks = Vec::new();
        for entry in walkdir::WalkDir::new(dir_path) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }

            let metadata = super::fs::metadata_from_entry(&entry);
            let path = entry.path().to_path_buf();
            upload_tasks.push(async move {
                let file = self.private_file_upload(path.clone(), wallet).await;
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
        let mut archive = PrivateArchive::new();
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
        let arch_addr = self
            .private_data_put(archive_serialized, wallet.into())
            .await?;

        info!(
            "Complete private archive upload completed in {:?}",
            start.elapsed()
        );
        #[cfg(feature = "loud")]
        println!("Upload completed in {:?}", start.elapsed());
        Ok(arch_addr)
    }

    /// Upload a private file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns [`PrivateDataAccess`] (pointing to the datamap)
    async fn private_file_upload(
        &self,
        path: PathBuf,
        wallet: &EvmWallet,
    ) -> Result<PrivateDataAccess, UploadError> {
        info!("Uploading file: {path:?}");
        #[cfg(feature = "loud")]
        println!("Uploading file: {path:?}");

        let data = tokio::fs::read(path).await?;
        let data = Bytes::from(data);
        let addr = self.private_data_put(data, wallet.into()).await?;
        debug!("Private file uploaded to the network successfully");
        Ok(addr)
    }
}
