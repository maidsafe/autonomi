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

use super::archive_private::{PrivateArchive, PrivateArchiveDataMap};
use super::{get_relative_file_path_from_abs_file_and_folder_path, FILE_ENCRYPT_BATCH_SIZE};
use super::{CombinedChunks, DownloadError, UploadError};

use crate::client::payment::PaymentOption;
use crate::client::{data_types::chunk::DataMapChunk, utils::process_tasks_with_max_concurrency};
use crate::self_encryption::encrypt;
use crate::{AttoTokens, Client};
use bytes::Bytes;
use std::path::PathBuf;

impl Client {
    /// Download a private file from network to local file system
    pub async fn file_download(
        &self,
        data_access: &DataMapChunk,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let data = self.data_get(data_access).await?;
        if let Some(parent) = to_dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
            debug!("Created parent directories for {to_dest:?}");
        }
        tokio::fs::write(to_dest.clone(), data).await?;
        debug!("Downloaded file to {to_dest:?}");
        Ok(())
    }

    /// Download a private directory from network to local file system
    pub async fn dir_download(
        &self,
        archive_access: &PrivateArchiveDataMap,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let archive = self.archive_get(archive_access).await?;
        for (path, addr, _meta) in archive.iter() {
            self.file_download(addr, to_dest.join(path)).await?;
        }
        debug!("Downloaded directory to {to_dest:?}");
        Ok(())
    }

    /// Upload the content of all files in a directory to the network.
    /// The directory is recursively walked and each file is uploaded to the network.
    ///
    /// The data maps of these (private) files are not uploaded but returned within the [`PrivateArchive`] return type.
    pub async fn dir_content_upload(
        &self,
        dir_path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PrivateArchive), UploadError> {
        info!("Uploading directory as private: {dir_path:?}");
        let mut encryption_tasks = vec![];

        for entry in walkdir::WalkDir::new(&dir_path) {
            let entry = entry?;

            if entry.file_type().is_dir() {
                continue;
            }

            let dir_path = dir_path.clone();

            encryption_tasks.push(async move {
                let file_path = entry.path().to_path_buf();

                info!("Encrypting file: {file_path:?}..");
                #[cfg(feature = "loud")]
                println!("Encrypting file: {file_path:?}..");

                let data = tokio::fs::read(&file_path)
                    .await
                    .map_err(|err| format!("Could not read file {file_path:?}: {err:?}"))?;
                let data = Bytes::from(data);

                if data.len() < 3 {
                    let err_msg =
                        format!("Skipping file {file_path:?}, as it is smaller than 3 bytes");
                    return Err(err_msg);
                }

                let now = ant_networking::time::Instant::now();

                let (data_map_chunk, chunks) = encrypt(data).map_err(|err| err.to_string())?;

                debug!("Encryption of {file_path:?} took: {:.2?}", now.elapsed());

                let metadata = super::fs_public::metadata_from_entry(&entry);

                let relative_path =
                    get_relative_file_path_from_abs_file_and_folder_path(&file_path, &dir_path);

                Ok((
                    file_path.to_string_lossy().to_string(),
                    chunks,
                    (relative_path, DataMapChunk::from(data_map_chunk), metadata),
                ))
            });
        }

        let mut combined_chunks: CombinedChunks = vec![];
        let mut private_archive = PrivateArchive::new();

        let encryption_results =
            process_tasks_with_max_concurrency(encryption_tasks, *FILE_ENCRYPT_BATCH_SIZE).await;

        for encryption_result in encryption_results {
            match encryption_result {
                Ok((file_path, chunked_file, file_data)) => {
                    info!("Successfully encrypted file: {file_path:?}");
                    #[cfg(feature = "loud")]
                    println!("Successfully encrypted file: {file_path:?}");

                    combined_chunks.push(((file_path, None), chunked_file));
                    let (relative_path, data_map_chunk, file_metadata) = file_data;
                    private_archive.add_file(relative_path, data_map_chunk, file_metadata);
                }
                Err(err_msg) => {
                    error!("Error during file encryption: {err_msg}");
                }
            }
        }

        let total_cost = self.pay_and_upload(payment_option, combined_chunks).await?;

        Ok((total_cost, private_archive))
    }

    /// Same as [`Client::dir_content_upload`] but also uploads the archive (privately) to the network.
    ///
    /// Returns the [`PrivateArchiveDataMap`] allowing the private archive to be downloaded from the network.
    pub async fn dir_upload(
        &self,
        dir_path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PrivateArchiveDataMap), UploadError> {
        let (cost1, archive) = self
            .dir_content_upload(dir_path, payment_option.clone())
            .await?;
        let (cost2, archive_addr) = self.archive_put(&archive, payment_option).await?;
        let total_cost = cost1.checked_add(cost2).unwrap_or_else(|| {
            error!("Total cost overflowed: {cost1:?} + {cost2:?}");
            cost1
        });
        Ok((total_cost, archive_addr))
    }

    /// Upload the content of a private file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns [`DataMapChunk`] (pointing to the datamap)
    pub async fn file_content_upload(
        &self,
        path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, DataMapChunk), UploadError> {
        info!("Uploading file: {path:?}");
        #[cfg(feature = "loud")]
        println!("Uploading file: {path:?}");

        let data = tokio::fs::read(path).await?;
        let data = Bytes::from(data);
        let (total_cost, addr) = self.data_put(data, payment_option).await?;
        debug!("Uploaded file successfully in the privateAchive: {addr:?}");
        Ok((total_cost, addr))
    }
}
