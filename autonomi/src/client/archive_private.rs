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
    archive::{Metadata, RenameError},
    data::{GetError, PutError},
    data_private::PrivateDataAccess,
    Client,
};
use crate::client::payment::PaymentOption;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// The address of a private archive
/// Contains the [`PrivateDataAccess`] leading to the [`PrivateArchive`] data
pub type PrivateArchiveAccess = PrivateDataAccess;

/// A private archive of files that containing file paths, their metadata and the files data maps
/// Using archives is useful for uploading entire directories to the network, only needing to keep track of a single address.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrivateArchive {
    map: HashMap<PathBuf, (PrivateDataAccess, Metadata)>,
}

impl PrivateArchive {
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
    pub fn add_file(&mut self, path: PathBuf, data_map: PrivateDataAccess, meta: Metadata) {
        self.map.insert(path, (data_map, meta));
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
    pub fn addresses(&self) -> Vec<PrivateDataAccess> {
        self.map
            .values()
            .map(|(data_map, _)| data_map.clone())
            .collect()
    }

    /// Iterate over the archive items
    /// Returns an iterator over (PathBuf, SecretDataMap, Metadata)
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &PrivateDataAccess, &Metadata)> {
        self.map
            .iter()
            .map(|(path, (data_map, meta))| (path, data_map, meta))
    }

    /// Get the underlying map
    pub fn map(&self) -> &HashMap<PathBuf, (PrivateDataAccess, Metadata)> {
        &self.map
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: Bytes) -> Result<PrivateArchive, rmp_serde::decode::Error> {
        let root: PrivateArchive = rmp_serde::from_slice(&data[..])?;

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
    /// Fetch a private archive from the network
    pub async fn private_archive_get(
        &self,
        addr: PrivateArchiveAccess,
    ) -> Result<PrivateArchive, GetError> {
        let data = self.private_data_get(addr).await?;
        debug!("Private archive successfully fetched from the network");
        Ok(PrivateArchive::from_bytes(data)?)
    }

    /// Upload a private archive to the network
    pub async fn private_archive_put(
        &self,
        archive: PrivateArchive,
        payment_option: PaymentOption,
    ) -> Result<PrivateArchiveAccess, PutError> {
        let bytes = archive
            .into_bytes()
            .map_err(|e| PutError::Serialization(format!("Failed to serialize archive: {e:?}")))?;
        let private_archive_access_result = self.private_data_put(bytes, payment_option).await;
        debug!("Private archive successfully uploaded to the network");
        private_archive_access_result
    }
}
