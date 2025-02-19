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

use ant_networking::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{client::payment::PaymentOption, AttoTokens};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use xor_name::XorName;

use crate::{
    client::{
        high_level::{data::DataAddr, files::RenameError},
        quote::CostError,
        GetError, PutError,
    },
    Client,
};

use super::Metadata;

/// The address of a public archive on the network. Points to an [`PublicArchive`].
pub type ArchiveAddr = XorName;

/// Public variant of [`crate::client::files::archive_private::PrivateArchive`]. Differs in that data maps of files are uploaded
/// to the network, of which the addresses are stored in this archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PublicArchive {
    ///           Path of the file in the directory
    ///           |         Data address of the content of the file (points to a DataMap)
    ///           |         |         Metadata of the file
    ///           |         |         |
    ///           V         V         V
    map: BTreeMap<PathBuf, (DataAddr, Metadata)>,
}

/// This type essentially wraps archive in version marker. E.g. in JSON format:
/// `{ "V0": { "map": <xxx> } }`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublicArchiveVersioned {
    V0(PublicArchive),
}

impl PublicArchive {
    /// Create a new emtpy local archive
    /// Note that this does not upload the archive to the network
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    /// Rename a file in an archive.
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
        debug!("Renamed file successfully in the archive, old path: {old_path:?} new_path: {new_path:?}");
        Ok(())
    }

    /// Add a file to a local archive
    /// Note that this does not upload the archive to the network
    pub fn add_file(&mut self, path: PathBuf, data_addr: DataAddr, meta: Metadata) {
        self.map.insert(path.clone(), (data_addr, meta));
        debug!("Added a new file to the archive, path: {:?}", path);
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
    pub fn map(&self) -> &BTreeMap<PathBuf, (DataAddr, Metadata)> {
        &self.map
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: Bytes) -> Result<PublicArchive, rmp_serde::decode::Error> {
        let root: PublicArchiveVersioned = rmp_serde::from_slice(&data[..])?;
        // Currently we have only `V0`. If we add `V1`, then we need an upgrade/migration path here.
        let PublicArchiveVersioned::V0(root) = root;

        Ok(root)
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Result<Bytes, rmp_serde::encode::Error> {
        let versioned = PublicArchiveVersioned::V0(self.clone());
        let root_serialized = rmp_serde::to_vec_named(&versioned)?;
        let root_serialized = Bytes::from(root_serialized);

        Ok(root_serialized)
    }

    /// Merge with another archive
    ///
    /// Note that if there are duplicate entries for the same filename, the files from the other archive will be the ones that are kept.
    pub fn merge(&mut self, other: &PublicArchive) {
        self.map.extend(other.map.clone());
    }
}

impl Client {
    /// Fetch an archive from the network
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use autonomi::{Client, client::files::archive_public::ArchiveAddr};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::init().await?;
    /// let archive = client.archive_get_public(&ArchiveAddr::random(&mut rand::thread_rng())).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn archive_get_public(&self, addr: &ArchiveAddr) -> Result<PublicArchive, GetError> {
        let data = self.data_get_public(addr).await?;
        Ok(PublicArchive::from_bytes(data)?)
    }

    /// Upload an archive to the network
    ///
    /// # Example
    ///
    /// Create simple archive containing `file.txt` pointing to random XOR name.
    ///
    /// ```no_run
    /// # use autonomi::{Client, client::{data::DataAddr, files::{Metadata, archive_public::{PublicArchive, ArchiveAddr}}}};
    /// # use autonomi::client::payment::PaymentOption;
    /// # use std::path::PathBuf;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init().await?;
    /// # let wallet = todo!();
    /// # let payment = PaymentOption::Wallet(wallet);
    /// let mut archive = PublicArchive::new();
    /// archive.add_file(PathBuf::from("file.txt"), DataAddr::random(&mut rand::thread_rng()), Metadata::new_with_size(0));
    /// let (cost, address) = client.archive_put_public(&archive, payment).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn archive_put_public(
        &self,
        archive: &PublicArchive,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, ArchiveAddr), PutError> {
        let bytes = archive
            .to_bytes()
            .map_err(|e| PutError::Serialization(format!("Failed to serialize archive: {e:?}")))?;

        #[cfg(feature = "loud")]
        println!(
            "Uploading public archive referencing {} files",
            archive.map().len()
        );

        let result = self.data_put_public(bytes, payment_option).await;
        debug!("Uploaded archive {archive:?} to the network and the address is {result:?}");
        result
    }

    /// Get the cost to upload an archive
    pub async fn archive_cost(&self, archive: &PublicArchive) -> Result<AttoTokens, CostError> {
        let bytes = archive
            .to_bytes()
            .map_err(|e| CostError::Serialization(format!("Failed to serialize archive: {e:?}")))?;
        let result = self.data_cost(bytes).await;
        debug!("Calculated the cost to upload archive {archive:?} is {result:?}");
        result
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn compatibility() {
        // In the future we'll have an extra variant.
        #[derive(Serialize, Deserialize)]
        #[non_exhaustive]
        pub enum FuturePublicArchiveVersioned {
            V0(PublicArchive),
            V1(PublicArchive),
            #[serde(other)]
            Unsupported,
        }

        let mut arch = PublicArchive::new();
        arch.add_file(
            PathBuf::from_str("hello_world").unwrap(),
            DataAddr::random(&mut rand::thread_rng()),
            Metadata::new_with_size(1),
        );
        let arch_serialized = arch.to_bytes().unwrap();

        // Create archive, forward compatible (still the same V0 version).
        let future_arch = FuturePublicArchiveVersioned::V0(arch.clone());
        let future_arch_serialized = rmp_serde::to_vec_named(&future_arch).unwrap();

        // Let's see if we can deserialize a (forward compatible) archive arriving to us from the future
        let _ = PublicArchive::from_bytes(Bytes::from(future_arch_serialized)).unwrap();

        // Let's see if we can deserialize an old archive from the future
        let _: FuturePublicArchiveVersioned = rmp_serde::from_slice(&arch_serialized[..]).unwrap();

        // Now we break forward compatibility by introducing a new version not supported by the old code.
        let future_arch = FuturePublicArchiveVersioned::V1(arch.clone());
        let future_arch_serialized = rmp_serde::to_vec_named(&future_arch).unwrap();
        // The old archive will not be able to decode this.
        assert!(PublicArchive::from_bytes(Bytes::from(future_arch_serialized)).is_err());

        // Now we prove backwards compatibility. Our old V0 archive will still be decoded by our new archive wrapper as V0.
        let versioned_arch = PublicArchiveVersioned::V0(arch.clone()); // 'Old' archive wrapper
        let versioned_arch_serialized = rmp_serde::to_vec_named(&versioned_arch).unwrap();
        let _: FuturePublicArchiveVersioned = // Into 'new' wrapper
            rmp_serde::from_slice(&versioned_arch_serialized[..]).unwrap();
    }

    #[test]
    fn forward_compatibility() {
        // What we do here is we create a new `Metadata` and use that in the `Archive` structs.

        /// A version `1.1` which is non-breaking (`1.0` is forward compatible with `1.1`).
        #[derive(Debug, Default, Serialize, Deserialize)]
        pub struct MetadataV1p1 {
            created: u64,
            modified: u64,
            size: u64,
            extra: Option<String>,
            accessed: Option<u64>, // Added field
        }
        #[derive(Debug, Default, Serialize, Deserialize)]
        pub struct PublicArchiveV1p1 {
            map: BTreeMap<PathBuf, (DataAddr, MetadataV1p1)>,
        }
        #[derive(Debug, Serialize, Deserialize)]
        pub enum PublicArchiveVersionedV1p1 {
            V0(PublicArchiveV1p1),
        }

        let mut arch_p1 = PublicArchiveV1p1::default();
        arch_p1.map.insert(
            PathBuf::from_str("hello_world").unwrap(),
            (
                DataAddr::random(&mut rand::thread_rng()),
                MetadataV1p1 {
                    accessed: Some(1),
                    ..Default::default()
                },
            ),
        );
        let arch_p1_ser =
            rmp_serde::to_vec_named(&PublicArchiveVersionedV1p1::V0(arch_p1)).unwrap();

        // Our old data structure should be forward compatible with the new one.
        assert!(PublicArchive::from_bytes(Bytes::from(arch_p1_ser)).is_ok());
    }
}
