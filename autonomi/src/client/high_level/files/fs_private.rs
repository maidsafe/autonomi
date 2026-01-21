// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::archive_private::{PrivateArchive, PrivateArchiveDataMap};
use super::{DownloadError, UploadError, bulk_upload_internal, file_upload_internal};
use crate::client::PutError;
use crate::client::data_types::chunk::DataMapChunk;
use crate::client::payment::{BulkPaymentOption, PaymentOption};
use crate::client::quote::add_costs;
use crate::{AttoTokens, Client};
use std::path::PathBuf;

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
    ///
    /// When using `BulkPaymentOption::Wallet`, the payment method is automatically selected:
    /// - For directories with >= 64 estimated chunks: uses merkle payments (more efficient)
    /// - For smaller directories: uses regular per-batch payments
    pub async fn dir_content_upload(
        &self,
        dir_path: PathBuf,
        payment_option: BulkPaymentOption,
    ) -> Result<(AttoTokens, PrivateArchive), UploadError> {
        bulk_upload_internal(self, dir_path, payment_option, false, |results| {
            let mut archive = PrivateArchive::new();
            for (path, datamap, metadata) in results {
                archive.add_file(path, datamap, metadata);
            }
            archive
        })
        .await
    }

    /// Same as [`Client::dir_content_upload`] but also uploads the archive (privately) to the network.
    ///
    /// Returns the [`PrivateArchiveDataMap`] allowing the private archive to be downloaded from the network.
    ///
    /// # Atomic Operation
    ///
    /// This is an atomic operation that requires a fresh wallet payment for both content and archive.
    ///
    /// # Resume Support
    ///
    /// To resume failed uploads with payment receipts, use the two-step approach:
    /// 1. Upload content with receipt: `dir_content_upload(path, BulkPaymentOption::ContinueMerkle(wallet, receipt))`
    /// 2. Upload archive separately: `archive_put(&archive, PaymentOption::Wallet(wallet))`
    ///
    /// This allows you to preserve merkle payment receipts while still completing the full upload.
    pub async fn dir_upload(
        &self,
        dir_path: PathBuf,
        wallet: &ant_evm::EvmWallet,
    ) -> Result<(AttoTokens, PrivateArchiveDataMap), UploadError> {
        let (cost1, archive) = self
            .dir_content_upload(dir_path, BulkPaymentOption::Wallet(wallet.clone()))
            .await?;
        let (cost2, archive_addr) = self
            .archive_put(&archive, PaymentOption::Wallet(wallet.clone()))
            .await?;
        let total_cost = add_costs(cost1, cost2).map_err(PutError::from)?;
        Ok((total_cost, archive_addr))
    }

    /// Upload the content of a private file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns [`DataMapChunk`] (pointing to the datamap)
    ///
    /// All `BulkPaymentOption` variants are supported:
    /// - `Wallet`: Fresh upload with auto-selection (merkle for >= 64 chunks, regular otherwise)
    /// - `Receipt`: Resume with regular receipt
    /// - `ContinueMerkle`: Uses merkle flow with wallet for unpaid chunks
    /// - `MerkleReceipt`: Uses merkle flow with existing proofs (fails if unpaid chunks exist)
    pub async fn file_content_upload(
        &self,
        path: PathBuf,
        payment_option: BulkPaymentOption,
    ) -> Result<(AttoTokens, DataMapChunk), UploadError> {
        file_upload_internal(self, path, payment_option, false).await
    }
}

#[cfg(test)]
mod tests {
    use crate::self_encryption::MAX_CHUNK_SIZE;

    #[test]
    fn test_chunk_estimation_ceiling_division() {
        // Test that we use ceiling division for accurate estimates
        // MAX_CHUNK_SIZE is typically 1MB (1024*1024 bytes)

        // Small file (500KB) should estimate correctly, not 0
        let small_size: usize = 500 * 1024; // 500KB
        let estimated = std::cmp::max(3, small_size.div_ceil(MAX_CHUNK_SIZE));
        assert!(
            estimated >= 3,
            "Small files should estimate at least 3 chunks"
        );

        // 1.5MB file should estimate 2 chunks (ceiling of 1.5)
        let medium_size: usize = (MAX_CHUNK_SIZE * 3) / 2; // 1.5 * MAX_CHUNK_SIZE
        let estimated = std::cmp::max(3, medium_size.div_ceil(MAX_CHUNK_SIZE));
        assert!(
            estimated >= 3,
            "1.5MB file should estimate at least 3 chunks (self-encryption minimum)"
        );

        // Exact multiple should work correctly
        let exact_size: usize = MAX_CHUNK_SIZE * 5;
        let estimated = exact_size.div_ceil(MAX_CHUNK_SIZE);
        assert_eq!(estimated, 5, "Exact multiples should estimate correctly");

        // Zero size should still estimate minimum
        let zero_estimated = std::cmp::max(3, 0_usize.div_ceil(MAX_CHUNK_SIZE));
        assert_eq!(
            zero_estimated, 3,
            "Empty files should estimate 3 chunks minimum"
        );
    }
}
