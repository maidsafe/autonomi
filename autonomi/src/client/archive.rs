// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use sn_networking::target_arch::{Duration, SystemTime, UNIX_EPOCH};

use super::{
    data::{CostError, DataAddr, GetError, PutError},
    Client,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sn_evm::{AttoTokens, EvmWallet};
use xor_name::XorName;

/// The address of an archive on the network. Points to an [`Archive`].
pub type ArchiveAddr = XorName;

use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum RenameError {
    #[error("File not found in archive: {0}")]
    FileNotFound(PathBuf),
}

/// An archive of files that containing file paths, their metadata and the files data addresses
/// Using archives is useful for uploading entire directories to the network, only needing to keep track of a single address.
/// Archives are public meaning anyone can read the data in the archive. For private archives use [`crate::client::archive_private::PrivateArchive`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Archive {
    map: HashMap<PathBuf, (DataAddr, Metadata)>,
}

/// Metadata for a file in an archive. Time values are UNIX timestamps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    /// When the file was (last) uploaded to the network.
    pub uploaded: u64,
    /// File creation time on local file system. See [`std::fs::Metadata::created`] for details per OS.
    pub created: u64,
    /// Last file modification time taken from local file system. See [`std::fs::Metadata::modified`] for details per OS.
    pub modified: u64,
    /// File size in bytes
    pub size: u64,
}

impl Metadata {
    /// Create a new metadata struct with the current time as uploaded, created and modified.
    pub fn new_with_size(size: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        Self {
            uploaded: now,
            created: now,
            modified: now,
            size,
        }
    }
}

impl Archive {
    /// Create a new emtpy local archive
    /// Note that this does not upload the archive to the network
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Rename a file in an archive
    /// Note that this does not upload the archive to the network
    pub fn rename_file(&mut self, old_path: &Path, new_path: &Path) -> Result<(), RenameError> {
        let (data_addr, mut meta) = self
            .map
            .remove(old_path)
            .ok_or(RenameError::FileNotFound(old_path.to_path_buf()))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();
        meta.modified = now;
        self.map.insert(new_path.to_path_buf(), (data_addr, meta));
        debug!("File renamed successfully in the archive");
        Ok(())
    }

    /// Add a file to a local archive
    /// Note that this does not upload the archive to the network
    pub fn add_file(&mut self, path: PathBuf, data_addr: DataAddr, meta: Metadata) {
        self.map.insert(path, (data_addr, meta));
        debug!("Added file successfully to the archive");
    }

    /// List all files in the archive
    pub fn files(&self) -> Vec<(PathBuf, Metadata)> {
        self.map
            .iter()
            .map(|(path, (_, meta))| (path.clone(), meta.clone()))
            .collect()
    }

    /// List all data addresses of the files in the archive
    pub fn addresses(&self) -> Vec<DataAddr> {
        self.map.values().map(|(addr, _)| *addr).collect()
    }

    /// Iterate over the archive items
    /// Returns an iterator over (PathBuf, DataAddr, Metadata)
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &DataAddr, &Metadata)> {
        self.map
            .iter()
            .map(|(path, (addr, meta))| (path, addr, meta))
    }

    /// Get the underlying map
    pub fn map(&self) -> &HashMap<PathBuf, (DataAddr, Metadata)> {
        &self.map
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: Bytes) -> Result<Archive, rmp_serde::decode::Error> {
        let root: Archive = rmp_serde::from_slice(&data[..])?;

        Ok(root)
    }

    /// Serialize to bytes.
    pub fn into_bytes(&self) -> Result<Bytes, rmp_serde::encode::Error> {
        let root_serialized = rmp_serde::to_vec(&self)?;
        let root_serialized = Bytes::from(root_serialized);

        Ok(root_serialized)
    }
}

impl Client {
    /// Fetch an archive from the network
    pub async fn archive_get(&self, addr: ArchiveAddr) -> Result<Archive, GetError> {
        let data = self.data_get(addr).await?;
        debug!("Fetched archive from network");
        Ok(Archive::from_bytes(data)?)
    }

    /// Upload an archive to the network
    pub async fn archive_put(
        &self,
        archive: Archive,
        wallet: &EvmWallet,
    ) -> Result<ArchiveAddr, PutError> {
        let bytes = archive
            .into_bytes()
            .map_err(|e| PutError::Serialization(format!("Failed to serialize archive: {e:?}")))?;
        let result_archive = self.data_put(bytes, wallet.into()).await;
        debug!("Uploaded archive to network");
        result_archive
    }

    /// Get the cost to upload an archive
    pub async fn archive_cost(&self, archive: Archive) -> Result<AttoTokens, CostError> {
        let bytes = archive
            .into_bytes()
            .map_err(|e| CostError::Serialization(format!("Failed to serialize archive: {e:?}")))?;
        let result_cost = self.data_cost(bytes).await;
        debug!("Got cost to upload archive");
        result_cost
    }
}
