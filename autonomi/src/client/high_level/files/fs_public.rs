// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::archive_public::{ArchiveAddress, PublicArchive};
use super::{DownloadError, Metadata, UploadError, bulk_upload_internal, file_upload_internal};
use crate::AttoTokens;
use crate::client::data_types::chunk::{ChunkAddress, DataMapChunk};
use crate::client::high_level::data::DataAddress;
use crate::client::payment::{BulkPaymentOption, PaymentOption};
use crate::client::quote::add_costs;
use crate::client::{Client, PutError};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

impl Client {
    /// Download file from network to local file system
    pub async fn file_download_public(
        &self,
        data_addr: &DataAddress,
        to_dest: PathBuf,
    ) -> Result<(), DownloadError> {
        info!("Downloading public file to {to_dest:?} from {data_addr:?}");

        let data_map_chunk = DataMapChunk(
            self.chunk_get(&ChunkAddress::new(*data_addr.xorname()))
                .await
                .map_err(DownloadError::GetError)?,
        );

        self.file_download(&data_map_chunk, to_dest).await
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
    ///
    /// When using `BulkPaymentOption::Wallet`, the payment method is automatically selected:
    /// - For directories with >= 64 estimated chunks: uses merkle payments (more efficient)
    /// - For smaller directories: uses regular per-batch payments
    pub async fn dir_content_upload_public(
        &self,
        dir_path: PathBuf,
        payment_option: BulkPaymentOption,
    ) -> Result<(AttoTokens, PublicArchive), UploadError> {
        let (cost, archive) =
            bulk_upload_internal(self, dir_path, payment_option, true, |results| {
                let mut archive = PublicArchive::new();
                for (path, datamap, metadata) in results {
                    let data_address = DataAddress::new(*datamap.0.name());
                    archive.add_file(path, data_address, metadata);
                }
                archive
            })
            .await?;

        // Log uploaded files
        for (file_path, data_addr, _meta) in archive.iter() {
            crate::loud_info!("Uploaded file: {file_path:?} to: {data_addr}");
        }

        Ok((cost, archive))
    }

    /// Same as [`Client::dir_content_upload_public`] but also uploads the archive to the network.
    ///
    /// Returns the [`ArchiveAddress`] of the uploaded archive.
    ///
    /// # Atomic Operation
    ///
    /// This is an atomic operation that requires a fresh wallet payment for both content and archive.
    ///
    /// # Resume Support
    ///
    /// To resume failed uploads with payment receipts, use the two-step approach:
    /// 1. Upload content with receipt: `dir_content_upload_public(path, BulkPaymentOption::ContinueMerkle(wallet, receipt))`
    /// 2. Upload archive separately: `archive_put_public(&archive, PaymentOption::Wallet(wallet))`
    ///
    /// This allows you to preserve merkle payment receipts while still completing the full upload.
    pub async fn dir_upload_public(
        &self,
        dir_path: PathBuf,
        wallet: &ant_evm::EvmWallet,
    ) -> Result<(AttoTokens, ArchiveAddress), UploadError> {
        let (cost1, archive) = self
            .dir_content_upload_public(dir_path, BulkPaymentOption::Wallet(wallet.clone()))
            .await?;
        let (cost2, archive_addr) = self
            .archive_put_public(&archive, PaymentOption::Wallet(wallet.clone()))
            .await?;
        let total_cost = add_costs(cost1, cost2).map_err(PutError::from)?;
        Ok((total_cost, archive_addr))
    }

    /// Upload the content of a file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns DataAddr (pointing to the datamap)
    ///
    /// All `BulkPaymentOption` variants are supported:
    /// - `Wallet`: Fresh upload with auto-selection (merkle for >= 64 chunks, regular otherwise)
    /// - `Receipt`: Resume with regular receipt
    /// - `ContinueMerkle`: Uses merkle flow with wallet for unpaid chunks
    /// - `MerkleReceipt`: Uses merkle flow with existing proofs (fails if unpaid chunks exist)
    pub async fn file_content_upload_public(
        &self,
        path: PathBuf,
        payment_option: BulkPaymentOption,
    ) -> Result<(AttoTokens, DataAddress), UploadError> {
        let (total_cost, data_map_chunk) =
            file_upload_internal(self, path.clone(), payment_option, true).await?;
        let addr = DataAddress::new(*data_map_chunk.0.name());
        debug!("File {path:?} uploaded to the network at {addr:?}");
        Ok((total_cost, addr))
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
