// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::path::PathBuf;

use bytes::Bytes;

use super::archive_private::PrivateArchive;
use super::archive_public::PublicArchive;
use super::fs_public::metadata_from_entry;
use super::{FileCostError, estimate_directory_chunks};
use crate::Client;
use crate::client::config::MERKLE_PAYMENT_THRESHOLD;
use crate::client::data_types::chunk::{Chunk, DataMapChunk};
use crate::client::high_level::data::DataAddress;
use crate::client::merkle_payments::MerklePaymentError;
use crate::client::quote::add_costs;
use crate::self_encryption::{MAX_CHUNK_SIZE, encrypt_directory_files};
use ant_evm::AttoTokens;
use ant_evm::merkle_payments::MAX_LEAVES;
use ant_protocol::storage::DataTypes;
use xor_name::XorName;

impl Client {
    /// Get the cost to upload a file/dir to the network.
    ///
    /// # Parameters
    ///
    /// - `path`: Path to the file or directory to estimate cost for
    /// - `is_public`: Whether the upload will be public (datamaps uploaded) or private (datamaps kept local)
    /// - `include_archive`: Whether to include archive metadata cost in the estimate
    ///
    /// # Cost Estimation Method
    ///
    /// Automatically selects the appropriate cost estimation method:
    /// - For directories with >= 64 estimated chunks: uses merkle payment estimation
    /// - For smaller directories: uses regular per-batch payment estimation
    ///
    /// # Archive Cost
    ///
    /// When `include_archive` is true and `path` is a directory:
    /// - For public uploads: includes the cost of uploading the `PublicArchive` (file paths + addresses + metadata)
    /// - For private uploads: includes the cost of uploading the `PrivateArchive` (file paths + datamaps + metadata)
    ///
    /// Note: Archive cost is only added for directories. Single file uploads don't create archives,
    /// so `include_archive` is ignored when `path` points to a file.
    pub async fn file_cost(
        &self,
        path: &PathBuf,
        is_public: bool,
        include_archive: bool,
    ) -> Result<AttoTokens, FileCostError> {
        // Estimate chunk count to choose cost estimation method
        let estimated_chunks = estimate_directory_chunks(path)?;

        // Get base content cost from either merkle or regular method
        let content_cost = if estimated_chunks >= MERKLE_PAYMENT_THRESHOLD {
            crate::loud_info!(
                "Using merkle cost estimation for ~{estimated_chunks} chunks (threshold: {MERKLE_PAYMENT_THRESHOLD})"
            );
            self.file_cost_merkle(path.clone(), is_public).await?
        } else {
            crate::loud_info!(
                "Using regular cost estimation for ~{estimated_chunks} chunks (threshold: {MERKLE_PAYMENT_THRESHOLD})"
            );
            self.file_cost_regular(path, is_public).await?
        };

        // Add archive cost if requested and path is a directory
        // Single file uploads don't create archives, so skip archive cost for files
        if include_archive && !path.is_file() {
            let archive_cost = self
                .estimate_archive_cost_from_path(path, is_public)
                .await?;
            let total_cost = add_costs(content_cost, archive_cost)?;
            debug!("Total cost (content + archive): {total_cost:?}");
            Ok(total_cost)
        } else {
            debug!("Total cost (content only): {content_cost:?}");
            Ok(content_cost)
        }
    }

    /// Get the cost to upload file/dir content using regular (non-merkle) payment estimation.
    ///
    /// This method only estimates the cost of the file content chunks, not the archive.
    /// Use `file_cost` if you need to include archive cost.
    ///
    /// For private uploads (`is_public == false`), the datamap chunk is excluded from the estimate
    /// since it is kept locally and not uploaded to the network.
    pub async fn file_cost_regular(
        &self,
        path: &PathBuf,
        is_public: bool,
    ) -> Result<AttoTokens, FileCostError> {
        let mut content_addrs = vec![];

        for entry in walkdir::WalkDir::new(path) {
            let entry = entry?;

            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path().to_path_buf();
            tracing::info!("Cost for file: {file_path:?}");

            let data = tokio::fs::read(&file_path).await?;
            let file_bytes = Bytes::from(data);

            let addrs = self.get_content_addrs(file_bytes)?;

            // For private uploads, skip the datamap chunk (first element) since it's kept locally
            if is_public {
                content_addrs.extend(addrs);
            } else {
                content_addrs.extend(addrs.into_iter().skip(1));
            }
        }

        let total_cost = self.get_cost_estimation(content_addrs).await?;
        debug!("Content cost for {path:?}: {total_cost:?}");
        Ok(total_cost)
    }

    /// Estimate the cost of uploading a directory of files with Merkle batch payments.
    ///
    /// This calls the smart contract's view function (0 gas) which runs the exact same
    /// pricing logic as the actual payment, ensuring accurate cost estimation.
    /// No wallet is required since this only queries the smart contract.
    ///
    /// This method only estimates the cost of the file content chunks, not the archive.
    /// Use `file_cost` if you need to include archive cost.
    ///
    /// # Arguments
    /// * `path` - The path to the directory
    /// * `is_public` - Whether the files will be uploaded as public
    ///
    /// # Returns
    /// * `AttoTokens` - Estimated total cost
    pub async fn file_cost_merkle(
        &self,
        path: PathBuf,
        is_public: bool,
    ) -> Result<AttoTokens, FileCostError> {
        debug!(
            "merkle payment: file_cost_merkle starting for path: {path:?}, is_public: {is_public}"
        );

        crate::loud_info!("Encrypting files to calculate cost...");

        // Collect all XorNames
        let all_xor_names = self.collect_xornames_for_cost(path, is_public).await?;

        let total_chunks = all_xor_names.len();
        crate::loud_info!("Encrypted into {total_chunks} chunks");

        // Split into batches of MAX_LEAVES
        let batches: Vec<Vec<XorName>> = all_xor_names
            .chunks(MAX_LEAVES)
            .map(|c| c.to_vec())
            .collect();
        let num_batches = batches.len();

        crate::loud_info!("Estimating cost for {num_batches} batch(es)...");

        // Estimate cost for each batch and sum
        let mut total_cost = ant_evm::U256::ZERO;

        for (batch_idx, batch_xornames) in batches.into_iter().enumerate() {
            let batch_num = batch_idx + 1;
            debug!("Estimating batch {batch_num}/{num_batches}");

            // Prepare batch (build tree, query pools)
            let (tree, _candidate_pools, pool_commitments, merkle_payment_timestamp) = self
                .prepare_merkle_batch(DataTypes::Chunk, batch_xornames, MAX_CHUNK_SIZE)
                .await?;

            // Estimate cost for this batch using the Network method (no wallet needed)
            let batch_cost = self
                .evm_network()
                .estimate_merkle_payment_cost(
                    tree.depth(),
                    &pool_commitments,
                    merkle_payment_timestamp,
                )
                .await
                .map_err(MerklePaymentError::MerklePaymentVault)?;

            total_cost = total_cost.saturating_add(batch_cost);
        }

        let estimated_cost = AttoTokens::from_atto(total_cost);
        crate::loud_info!("Total estimated cost: {estimated_cost}");

        Ok(estimated_cost)
    }

    /// Collect all XorNames from a directory for cost estimation.
    /// Returns only the xornames (no file results needed for cost calculation).
    async fn collect_xornames_for_cost(
        &self,
        path: PathBuf,
        is_public: bool,
    ) -> Result<Vec<XorName>, FileCostError> {
        let streams = encrypt_directory_files(path, is_public).await?;

        let mut all_xor_names = Vec::new();

        for stream_result in streams {
            let mut stream = stream_result.map_err(FileCostError::Encryption)?;
            let batch_size: usize = 32;

            while let Some(batch) = stream.next_batch(batch_size) {
                for chunk in batch {
                    all_xor_names.push(*chunk.name());
                }
            }
        }

        Ok(all_xor_names)
    }

    /// Estimate the cost of uploading an archive for the given directory path.
    ///
    /// For public archives: estimates based on file paths + addresses + metadata.
    /// For private archives: estimates based on file paths + placeholder datamaps + metadata.
    async fn estimate_archive_cost_from_path(
        &self,
        path: &PathBuf,
        is_public: bool,
    ) -> Result<AttoTokens, FileCostError> {
        let mut public_archive = PublicArchive::new();
        let mut private_archive = PrivateArchive::new();

        for entry in walkdir::WalkDir::new(path) {
            let entry = entry?;

            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path().to_path_buf();
            let metadata = metadata_from_entry(&entry);

            if is_public {
                // For public archives, we just need a placeholder address
                // The actual address would come from encryption, but for cost estimation
                // the size is the same regardless of the actual xorname
                let placeholder_addr = DataAddress::new(xor_name::XorName::default());
                public_archive.add_file(file_path, placeholder_addr, metadata);
            } else {
                // For private archives, we need to estimate datamap size
                // Read file and encrypt to get actual datamap size
                let data = tokio::fs::read(&file_path).await?;
                let file_bytes = Bytes::from(data);
                let addrs = self.get_content_addrs(file_bytes)?;

                // First addr is the datamap (xorname, size)
                // Note: unwrap_or(0) is safe - encrypt() always produces a datamap chunk,
                // so addrs is never empty. If somehow empty, 0 size is a reasonable fallback.
                let datamap_size = addrs.first().map(|(_, size)| *size).unwrap_or(0);

                // Create placeholder DataMapChunk of the correct size for estimation
                let placeholder_bytes = vec![0u8; datamap_size];
                let placeholder_chunk = DataMapChunk(Chunk::new(Bytes::from(placeholder_bytes)));
                private_archive.add_file(file_path, placeholder_chunk, metadata);
            }
        }

        let archive_bytes = if is_public {
            public_archive.to_bytes()?
        } else {
            private_archive.to_bytes()?
        };

        let archive_addrs = self.get_content_addrs(archive_bytes)?;

        // For private archives, skip the datamap chunk (first element) since it's kept locally
        let archive_addrs: Vec<_> = if is_public {
            archive_addrs
        } else {
            archive_addrs.into_iter().skip(1).collect()
        };

        let archive_cost = self.get_cost_estimation(archive_addrs).await?;
        debug!("Archive cost for {path:?}: {archive_cost:?}");
        Ok(archive_cost)
    }
}
