// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::archive_private::{PrivateArchive, PrivateArchiveDataMap};
use super::{DownloadError, UploadError};

use crate::client::data_types::chunk::DataMapChunk;
use crate::client::payment::PaymentOption;
use crate::{AttoTokens, Client};
use std::path::PathBuf;

use crate::self_encryption::encrypt_directory_files;

impl Client {
    /// Download private file directly to filesystem. Always uses streaming.
    pub async fn file_download(
        &self,
        data_map: &DataMapChunk,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        info!("Downloading private file to {to_dest:?}");

        // Create parent directories if needed
        if let Some(parent) = to_dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let datamap = self.restore_data_map_from_chunk(data_map).await?;
        self.stream_download_from_datamap(datamap, &to_dest)?;

        debug!("Successfully downloaded private file to {to_dest:?}");
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
    /// The datamaps of these (private) files are not uploaded but returned within the [`PrivateArchive`] return type.
    pub async fn dir_content_upload(
        &self,
        dir_path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PrivateArchive), UploadError> {
        info!("Uploading directory as private: {dir_path:?}");

        // encrypt
        let encryption_results = encrypt_directory_files(dir_path, false).await?;
        let mut chunk_iterators = vec![];
        for encryption_result in encryption_results {
            match encryption_result {
                Ok(file_chunk_iterator) => {
                    let file_path = file_chunk_iterator.file_path.clone();
                    info!("Successfully encrypted file: {file_path:?}");
                    #[cfg(feature = "loud")]
                    println!("Successfully encrypted file: {file_path:?}");

                    chunk_iterators.push(file_chunk_iterator);
                }
                Err(err_msg) => {
                    error!("Error during file encryption: {err_msg}");
                    #[cfg(feature = "loud")]
                    println!("Error during file encryption: {err_msg}");
                    return Err(UploadError::Encryption(err_msg));
                }
            }
        }

        // pay and upload
        let total_cost = self
            .pay_and_upload(payment_option, &mut chunk_iterators)
            .await?;

        // create an archive
        let mut private_archive = PrivateArchive::new();
        for file in chunk_iterators {
            let file_path = file.file_path.clone();
            let relative_path = file.relative_path.clone();
            let file_metadata = file.metadata.clone();
            let datamap = match file.data_map_chunk() {
                Some(datamap) => datamap,
                None => {
                    error!("Datamap chunk not found for file: {file_path:?}, this is a BUG");
                    continue;
                }
            };
            private_archive.add_file(relative_path, datamap, file_metadata);
        }

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
        let (data_map_chunk, processed_chunks, free_chunks, receipts) =
            self.stream_upload_file(path, payment_option, false).await?;
        let total_cost = self
            .calculate_total_cost(processed_chunks, receipts, free_chunks)
            .await;
        Ok((total_cost, data_map_chunk))
    }
}
