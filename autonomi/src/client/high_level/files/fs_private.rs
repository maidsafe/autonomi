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
use std::time::Instant;
use std::collections::HashMap;

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


    /// Uploads a directory of files to the network as private data.
    ///
    /// # Arguments
    /// * `dir_path` - Path to the directory to upload
    /// * `payment_option` - Payment option for the upload
    ///
    /// # Returns
    /// * `Result<(HashMap<PathBuf, DataMapChunk>, PrivateArchive), UploadError>` - The private data addresses and private archive
    ///
    /// # Example
    /// ```no_run
    /// # use autonomi::{Client, PaymentOption};
    /// # use std::path::PathBuf;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init().await?;
    /// let dir_path = PathBuf::from("/path/to/directory");
    /// let payment = PaymentOption::default();
    /// let (addresses, archive) = client.dir_upload(dir_path, payment).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn dir_upload(
        &self,
        dir_path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(HashMap<PathBuf, DataMapChunk>, PrivateArchive), UploadError> {
        info!("Uploading directory as private data: {dir_path:?}");
        let start = Instant::now();

        // Generate chunks from directory
        let (chunks, private_archive) = self.dir_to_private_archive(dir_path.clone()).await?;

        // Upload chunks
        let tokens_spent = self.pay_and_upload(payment_option, chunks).await?;

        // Create data addresses map
        let mut data_addrs = HashMap::new();
        for (path, addr, _meta) in private_archive.iter() {
            data_addrs.insert(path.clone(), addr.clone());
        }

        debug!(
            "Uploaded directory in {:?}: {} files, {} tokens spent",
            start.elapsed(),
            data_addrs.len(),
            tokens_spent
        );
        Ok((data_addrs, private_archive))
    }

    /// Convert a directory to chunks and create a private archive
    async fn dir_to_private_archive(
        &self,
        dir_path: PathBuf,
    ) -> Result<(CombinedChunks, PrivateArchive), UploadError> {
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

                let now = Instant::now();

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

        Ok((combined_chunks, private_archive))
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
