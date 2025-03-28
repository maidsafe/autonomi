// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::HashMap;

use crate::client::high_level::files::archive_private::PrivateArchiveDataMap;
use crate::client::high_level::files::archive_public::ArchiveAddress;
use crate::client::payment::PaymentOption;
use crate::client::Client;
use crate::client::GetError;
use crate::register::RegisterAddress;
use ant_evm::AttoTokens;
use ant_protocol::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use super::{app_name_to_vault_content_type, VaultContentType, VaultError, VaultSecretKey};

/// Vault content type for UserDataVault
pub static USER_DATA_VAULT_CONTENT_IDENTIFIER: LazyLock<VaultContentType> =
    LazyLock::new(|| app_name_to_vault_content_type("UserData"));

/// UserData is stored in Vaults and contains most of a user's private data:
/// It allows users to keep track of only the key to their User Data Vault
/// while having the rest kept on the Network encrypted in a Vault for them
/// Using User Data Vault is optional, one can decide to keep all their data locally instead.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UserData {
    /// Owned file archive addresses, along with their names (can be empty)
    pub file_archives: HashMap<ArchiveAddress, String>,
    /// Owned private file archives, along with their names (can be empty)
    pub private_file_archives: HashMap<PrivateArchiveDataMap, String>,
    /// Owned register addresses, along with their names (can be empty)
    pub register_addresses: HashMap<RegisterAddress, String>,
}

/// Errors that can occur during the get operation.
#[derive(Debug, thiserror::Error)]
pub enum UserDataVaultError {
    #[error("Vault error: {0}")]
    Vault(#[from] VaultError),
    #[error("Unsupported vault content type: {0}")]
    UnsupportedVaultContentType(VaultContentType),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Get error: {0}")]
    GetError(#[from] GetError),
}

impl UserData {
    /// Create a new empty UserData
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a register. Returning `Option::Some` with the old name if the register was already in the set.
    pub fn add_register(&mut self, register: RegisterAddress, name: String) -> Option<String> {
        self.register_addresses.insert(register, name)
    }

    /// Add an archive. Returning `Option::Some` with the old name if the archive was already in the set.
    pub fn add_file_archive(&mut self, archive: ArchiveAddress) -> Option<String> {
        self.file_archives.insert(archive, "".into())
    }

    /// Add an archive. Returning `Option::Some` with the old name if the archive was already in the set.
    pub fn add_file_archive_with_name(
        &mut self,
        archive: ArchiveAddress,
        name: String,
    ) -> Option<String> {
        self.file_archives.insert(archive, name)
    }

    /// Add a private archive. Returning `Option::Some` with the old name if the archive was already in the set.
    pub fn add_private_file_archive(&mut self, archive: PrivateArchiveDataMap) -> Option<String> {
        self.private_file_archives.insert(archive, "".into())
    }

    /// Add a private archive with a name. Returning `Option::Some` with the old name if the archive was already in the set.
    pub fn add_private_file_archive_with_name(
        &mut self,
        archive: PrivateArchiveDataMap,
        name: String,
    ) -> Option<String> {
        self.private_file_archives.insert(archive, name)
    }

    /// Remove an archive. Returning `Option::Some` with the old name if the archive was already in the set.
    pub fn remove_file_archive(&mut self, archive: ArchiveAddress) -> Option<String> {
        self.file_archives.remove(&archive)
    }

    /// Remove a private archive. Returning `Option::Some` with the old name if the archive was already in the set.
    pub fn remove_private_file_archive(
        &mut self,
        archive: PrivateArchiveDataMap,
    ) -> Option<String> {
        self.private_file_archives.remove(&archive)
    }

    /// To bytes
    pub fn to_bytes(&self) -> Result<Bytes, rmp_serde::encode::Error> {
        let bytes = rmp_serde::to_vec(&self)?;
        Ok(Bytes::from(bytes))
    }

    /// From bytes
    pub fn from_bytes(bytes: Bytes) -> Result<Self, rmp_serde::decode::Error> {
        let vault_content = rmp_serde::from_slice(&bytes)?;
        Ok(vault_content)
    }
}

impl Client {
    /// Get the user data from the vault.
    pub async fn get_user_data_from_vault(
        &self,
        secret_key: &VaultSecretKey,
    ) -> Result<UserData, UserDataVaultError> {
        let (bytes, content_type) = self.fetch_and_decrypt_vault(secret_key).await?;

        if content_type != *USER_DATA_VAULT_CONTENT_IDENTIFIER {
            return Err(UserDataVaultError::UnsupportedVaultContentType(
                content_type,
            ));
        }

        let vault = UserData::from_bytes(bytes).map_err(|e| {
            UserDataVaultError::Serialization(format!("Failed to deserialize vault content: {e}"))
        })?;

        Ok(vault)
    }

    /// Put the user data to the vault.
    ///
    /// Returns the total cost of the put operation.
    pub async fn put_user_data_to_vault(
        &self,
        secret_key: &VaultSecretKey,
        payment_option: PaymentOption,
        user_data: UserData,
    ) -> Result<AttoTokens, UserDataVaultError> {
        let bytes = user_data.to_bytes().map_err(|e| {
            UserDataVaultError::Serialization(format!("Failed to serialize user data: {e}"))
        })?;
        let total_cost = self
            .write_bytes_to_vault(
                bytes,
                payment_option,
                secret_key,
                *USER_DATA_VAULT_CONTENT_IDENTIFIER,
            )
            .await?;
        Ok(total_cost)
    }
}
