// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ant_evm::AttoTokens;
use ant_networking::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    client::{
        data_types::chunk::DataMapChunk, high_level::files::RenameError, payment::PaymentOption,
        GetError, PutError,
    },
    Client,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use super::Metadata;

/// Private archive data map, allowing access to the [`PrivateArchive`] data.
pub type PrivateArchiveAccess = DataMapChunk;

/// Directory structure mapping filepaths to their data maps and metadata.
///
/// The data maps are stored within this structure instead of uploading them to the network, keeping the data private.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrivateArchive {
    map: BTreeMap<PathBuf, (DataMapChunk, Metadata)>,
}

/// This type essentially wraps archive in version marker. E.g. in JSON format:
/// `{ "V0": { "map": <xxx> } }`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrivateArchiveVersioned {
    V0(PrivateArchive),
}

impl PrivateArchive {
    /// Create a new emtpy local archive
    /// Note that this does not upload the archive to the network
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
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
        debug!("Renamed file successfully in the private archive, old path: {old_path:?} new_path: {new_path:?}");
        Ok(())
    }

    /// Add a file to a local archive. Note that this does not upload the archive to the network.
    pub fn add_file(&mut self, path: PathBuf, data_map: DataMapChunk, meta: Metadata) {
        self.map.insert(path.clone(), (data_map, meta));
        debug!("Added a new file to the archive, path: {:?}", path);
    }

    /// List all files in the archive
    pub fn files(&self) -> Vec<(PathBuf, Metadata)> {
        self.map
            .iter()
            .map(|(path, (_, meta))| (path.clone(), meta.clone()))
            .collect()
    }

    /// List all data [`DataMapChunk`]s of the files in the archive
    pub fn data_maps(&self) -> Vec<DataMapChunk> {
        self.map
            .values()
            .map(|(data_map, _)| data_map.clone())
            .collect()
    }

    /// Iterate over the archive items.
    ///
    /// Returns an iterator over ([`PathBuf`], [`DataMapChunk`], [`Metadata`])
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &DataMapChunk, &Metadata)> {
        self.map
            .iter()
            .map(|(path, (data_map, meta))| (path, data_map, meta))
    }

    /// Get the underlying map
    pub fn map(&self) -> &BTreeMap<PathBuf, (DataMapChunk, Metadata)> {
        &self.map
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: Bytes) -> Result<PrivateArchive, rmp_serde::decode::Error> {
        let root: PrivateArchiveVersioned = rmp_serde::from_slice(&data[..])?;
        // Currently we have only `V0`. If we add `V1`, then we need an upgrade/migration path here.
        let PrivateArchiveVersioned::V0(root) = root;

        Ok(root)
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Result<Bytes, rmp_serde::encode::Error> {
        let versioned = PrivateArchiveVersioned::V0(self.clone());
        let root_serialized = rmp_serde::to_vec_named(&versioned)?;
        let root_serialized = Bytes::from(root_serialized);

        Ok(root_serialized)
    }

    /// Merge with another archive
    ///
    /// Note that if there are duplicate entries for the same filename, the files from the other archive will be the ones that are kept.
    pub fn merge(&mut self, other: &PrivateArchive) {
        self.map.extend(other.map.clone());
    }
}

impl Client {
    /// Fetch a [`PrivateArchive`] from the network
    pub async fn archive_get(
        &self,
        addr: &PrivateArchiveAccess,
    ) -> Result<PrivateArchive, GetError> {
        let data = self.data_get(addr).await?;
        Ok(PrivateArchive::from_bytes(data)?)
    }

    /// Upload a [`PrivateArchive`] to the network
    pub async fn archive_put(
        &self,
        archive: &PrivateArchive,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PrivateArchiveAccess), PutError> {
        let bytes = archive
            .to_bytes()
            .map_err(|e| PutError::Serialization(format!("Failed to serialize archive: {e:?}")))?;

        #[cfg(feature = "loud")]
        println!(
            "Uploading private archive referencing {} files",
            archive.map().len()
        );

        let result = self.data_put(bytes, payment_option).await;
        debug!("Uploaded private archive {archive:?} to the network and address is {result:?}");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_private_archive_merge() {
        let mut arch = PrivateArchive::new();
        let file1 = PathBuf::from_str("file1").unwrap();
        let file2 = PathBuf::from_str("file2").unwrap();
        arch.add_file(
            file1.clone(),
            DataMapChunk::from_hex("1111").unwrap(),
            Metadata::new_with_size(1),
        );
        let mut other_arch = PrivateArchive::new();
        other_arch.add_file(
            file2.clone(),
            DataMapChunk::from_hex("AAAA").unwrap(),
            Metadata::new_with_size(2),
        );
        arch.merge(&other_arch);
        assert_eq!(arch.map().len(), 2);
        assert_eq!(arch.map().get(&file1).unwrap().1.size, 1);
        assert_eq!(arch.map().get(&file2).unwrap().1.size, 2);

        let mut arch_with_duplicate = PrivateArchive::new();
        arch_with_duplicate.add_file(
            file1.clone(),
            DataMapChunk::from_hex("BBBB").unwrap(),
            Metadata::new_with_size(5),
        );
        arch.merge(&arch_with_duplicate);
        assert_eq!(arch.map().len(), 2);
        assert_eq!(arch.map().get(&file1).unwrap().1.size, 5);
        assert_eq!(arch.map().get(&file2).unwrap().1.size, 2);
    }
}
