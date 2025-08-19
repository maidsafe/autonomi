// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::archive_public::{ArchiveAddress, PublicArchive};
use super::{DownloadError, FileCostError, UploadError};
use crate::AttoTokens;
use crate::client::Client;
use crate::client::chunk::{ChunkAddress, DataMapChunk};
use crate::client::high_level::data::DataAddress;
use crate::client::payment::PaymentOption;
use crate::files::{get_relative_file_path_from_abs_file_and_folder_path, metadata_from_entry};
use bytes::Bytes;
use std::path::{Path, PathBuf};

impl Client {
    /// Download file from network to local file system
    pub async fn file_download_public(
        &self,
        data_addr: &DataAddress,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        info!("Downloading public file to {to_dest:?} from {data_addr:?}");

        // Create parent directories if needed
        if let Some(parent) = to_dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
            debug!("Created parent directories {parent:?} for {to_dest:?}");
        }

        // Get the datamap chunk from the public address
        let data_map_chunk = DataMapChunk(
            self.chunk_get(&ChunkAddress::new(*data_addr.xorname()))
                .await?,
        );

        self.stream_download_chunks_to_file(&data_map_chunk, &to_dest)
            .await?;

        debug!("Downloaded file to {to_dest:?} from the network address {data_addr:?}");
        Ok(())
    }

    /// Download directory from network to local file system
    pub async fn dir_download_public(
        &self,
        archive_addr: &ArchiveAddress,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        let archive = self.archive_get_public(archive_addr).await?;
        debug!("Downloaded archive for the directory from the network at {archive_addr:?}");
        for (path, addr, _meta) in archive.iter() {
            self.file_download_public(addr, to_dest.join(path)).await?;
        }
        debug!(
            "All files in the directory downloaded to {:?} from the network address {:?}",
            to_dest.parent(),
            archive_addr
        );
        Ok(())
    }

    /// Upload the content of all files in a directory to the network.
    /// The directory is recursively walked and each file is uploaded to the network.
    ///
    /// The datamaps of these files are uploaded on the network, making the individual files publicly available.
    ///
    /// This returns, but does not upload (!),the [`PublicArchive`] containing the datamaps of the uploaded files.
    pub async fn dir_content_upload_public(
        &self,
        dir_path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PublicArchive), UploadError> {
        info!("Uploading directory: {dir_path:?}");

        // encrypt
        let encryption_results = self
            .encrypt_directory_files_in_memory(dir_path.clone(), true)
            .await?;
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
                }
            }
        }

        // pay and upload
        let total_cost = self
            .pay_and_upload(payment_option, &mut chunk_iterators)
            .await?;

        // create an archive
        let mut public_archive = PublicArchive::new();
        for file_chunk_iterator in chunk_iterators {
            let file_path = Path::new(&file_chunk_iterator.file_path);
            let relative_path =
                get_relative_file_path_from_abs_file_and_folder_path(file_path, &dir_path);
            let file_metadata = metadata_from_entry(file_path);
            let data_address = match file_chunk_iterator.data_map_chunk() {
                Some(datamap) => DataAddress::new(*datamap.0.name()),
                None => {
                    error!("Datamap chunk not found for file: {file_path:?}, this is a BUG");
                    continue;
                }
            };
            public_archive.add_file(relative_path, data_address, file_metadata);
        }

        for (file_path, data_addr, _meta) in public_archive.iter() {
            info!("Uploaded file: {file_path:?} to: {data_addr}");
            #[cfg(feature = "loud")]
            println!("Uploaded file: {file_path:?} to: {data_addr}");
        }

        Ok((total_cost, public_archive))
    }

    /// Same as [`Client::dir_content_upload_public`] but also uploads the archive to the network.
    ///
    /// Returns the [`ArchiveAddress`] of the uploaded archive.
    pub async fn dir_upload_public(
        &self,
        dir_path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, ArchiveAddress), UploadError> {
        let (cost1, archive) = self
            .dir_content_upload_public(dir_path, payment_option.clone())
            .await?;
        let (cost2, archive_addr) = self.archive_put_public(&archive, payment_option).await?;
        let total_cost = cost1.checked_add(cost2).unwrap_or_else(|| {
            error!("Total cost overflowed: {cost1:?} + {cost2:?}");
            cost1
        });
        Ok((total_cost, archive_addr))
    }

    /// Upload the content of a file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns DataAddr (pointing to the datamap)
    pub async fn file_content_upload_public(
        &self,
        path: PathBuf,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, DataAddress), UploadError> {
        info!("Uploading file: {path:?}");
        #[cfg(feature = "loud")]
        println!("Uploading file: {path:?}");

        let data = tokio::fs::read(path.clone()).await?;
        let data = Bytes::from(data);
        let (cost, addr) = self.data_put_public(data, payment_option).await?;
        debug!("File {path:?} uploaded to the network at {addr:?}");
        Ok((cost, addr))
    }

    /// Get the cost to upload a file/dir to the network.
    /// quick and dirty implementation, please refactor once files are cleanly implemented
    pub async fn file_cost(&self, path: &PathBuf) -> Result<AttoTokens, FileCostError> {
        let mut archive = PublicArchive::new();
        let mut content_addrs = vec![];

        for entry in walkdir::WalkDir::new(path) {
            let entry = entry?;

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path().to_path_buf();
            tracing::info!("Cost for file: {path:?}");

            let data = tokio::fs::read(&path).await?;
            let file_bytes = Bytes::from(data);

            let addrs = self.get_content_addrs(file_bytes.clone())?;

            // The first addr is always the chunk_map_name
            let map_xor_name = addrs[0].0;

            content_addrs.extend(addrs);

            let metadata = metadata_from_entry(&path);

            archive.add_file(path, DataAddress::new(map_xor_name), metadata);
        }

        let serialized = archive.to_bytes()?;
        content_addrs.extend(self.get_content_addrs(serialized)?);

        let total_cost = self.get_cost_estimation(content_addrs).await?;
        debug!("Total cost for the directory: {total_cost:?}");
        Ok(total_cost)
    }
}
