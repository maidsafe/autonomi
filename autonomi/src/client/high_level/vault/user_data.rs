// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::HashMap;

use super::{VaultContentType, VaultError, VaultSecretKey, app_name_to_vault_content_type};
use crate::chunk::DataMapChunk;
use crate::client::Client;
use crate::client::GetError;
use crate::client::high_level::files::archive_private::PrivateArchiveDataMap;
use crate::client::high_level::files::archive_public::ArchiveAddress;
use crate::client::payment::PaymentOption;
use crate::data::DataAddress;
use crate::register::RegisterAddress;
use ant_evm::AttoTokens;
use ant_protocol::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Vault content type for UserDataVault
pub static USER_DATA_VAULT_CONTENT_IDENTIFIER: LazyLock<VaultContentType> =
    LazyLock::new(|| app_name_to_vault_content_type("UserData"));

pub type RegisterSecretKeyHex = String;
pub type ScratchpadSecretKeyHex = String;
pub type PointerSecretKeyHex = String;

/// UserData is stored in Vaults and contains most of a user's private data:
/// It allows users to keep track of only the key to their User Data Vault
/// while having the rest kept on the Network encrypted in a Vault for them
/// Using User Data Vault is optional, one can decide to keep all their data locally instead.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq, Deserialize)]
pub struct UserData {
    /// Owned file archive addresses, along with their names (can be empty)
    pub file_archives: HashMap<ArchiveAddress, String>,
    /// Owned private file archives, along with their names (can be empty)
    pub private_file_archives: HashMap<PrivateArchiveDataMap, String>,
    /// Owned register addresses, along with their names (can be empty)
    pub register_addresses: HashMap<RegisterAddress, String>,
    /// Register key
    #[serde(default)]
    // This makes the field optional to support old versions without that field
    pub register_key: Option<RegisterSecretKeyHex>,
    /// Scratchpads key
    #[serde(default)]
    // This makes the field optional to support old versions without that field
    pub scratchpad_key: Option<ScratchpadSecretKeyHex>,
    /// Pointer key
    #[serde(default)]
    // This makes the field optional to support old versions without that field
    pub pointer_key: Option<PointerSecretKeyHex>,
    /// Individual public files (non-archive), along with their names
    #[serde(default)]
    // This makes the field optional to support old versions without that field
    pub public_files: HashMap<DataAddress, String>,
    /// Individual private files (non-archive), along with their names
    #[serde(default)]
    // This makes the field optional to support old versions without that field
    pub private_files: HashMap<DataMapChunk, String>,
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

    /// Display content
    pub fn display_stats(&self) {
        let file_archives_len = self.file_archives.len();
        let private_file_archives_len = self.private_file_archives.len();
        let public_files_len = self.public_files.len();
        let private_files_len = self.private_files.len();
        let registers_len = self.register_addresses.len();
        let register_key = match self.register_key.is_some() {
            true => "1",
            false => "0",
        };
        let scratchpad_key = match self.scratchpad_key.is_some() {
            true => "1",
            false => "0",
        };
        let pointer_key = match self.pointer_key.is_some() {
            true => "1",
            false => "0",
        };

        println!("{file_archives_len} public file archive(s)");
        println!("{private_file_archives_len} private file archive(s)");
        println!("{public_files_len} public file(s)");
        println!("{private_files_len} private file(s)");
        println!("{registers_len} register(s)");
        println!("{register_key} register key");
        println!("{scratchpad_key} scratchpad key");
        println!("{pointer_key} pointer key");
    }
}

impl Client {
    /// Get the user data from the vault
    pub async fn vault_get_user_data(
        &self,
        secret_key: &VaultSecretKey,
    ) -> Result<UserData, UserDataVaultError> {
        let (bytes, content_type) = self.vault_get(secret_key).await?;

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

    /// Put the user data to the vault
    ///
    /// Returns the total cost of the put operation
    pub async fn vault_put_user_data(
        &self,
        secret_key: &VaultSecretKey,
        payment_option: PaymentOption,
        user_data: UserData,
    ) -> Result<AttoTokens, UserDataVaultError> {
        let bytes = user_data.to_bytes().map_err(|e| {
            UserDataVaultError::Serialization(format!("Failed to serialize user data: {e}"))
        })?;
        let total_cost = self
            .vault_put(
                bytes,
                payment_option,
                secret_key,
                *USER_DATA_VAULT_CONTENT_IDENTIFIER,
            )
            .await?;
        Ok(total_cost)
    }

    /// @deprecated Use `vault_get_user_data` instead. This function will be removed in a future version.
    #[deprecated(since = "0.2.0", note = "Use `vault_get_user_data` instead")]
    pub async fn get_user_data_from_vault(
        &self,
        secret_key: &VaultSecretKey,
    ) -> Result<UserData, UserDataVaultError> {
        self.vault_get_user_data(secret_key).await
    }

    /// @deprecated Use `vault_put_user_data` instead. This function will be removed in a future version.
    #[deprecated(since = "0.2.0", note = "Use `vault_put_user_data` instead")]
    pub async fn put_user_data_to_vault(
        &self,
        secret_key: &VaultSecretKey,
        payment_option: PaymentOption,
        user_data: UserData,
    ) -> Result<AttoTokens, UserDataVaultError> {
        self.vault_put_user_data(secret_key, payment_option, user_data)
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::XorName;
    use bls::SecretKey;

    use super::*;

    // simulate how the previous version of UserData looked like
    #[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
    struct UserDataV1 {
        pub file_archives: HashMap<ArchiveAddress, String>,
        pub private_file_archives: HashMap<PrivateArchiveDataMap, String>,
        pub register_addresses: HashMap<RegisterAddress, String>,
    }

    #[test]
    fn test_user_data_v1_deserialization() {
        // Create a V1 instance with some test data
        let v1_data = UserDataV1 {
            file_archives: HashMap::from([(ArchiveAddress::new(XorName::random(&mut rand::thread_rng())), "test_archive".to_string())]),
            private_file_archives: HashMap::from([(
                PrivateArchiveDataMap::from_hex("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap(),
                "test_private".to_string(),
            )]),
            register_addresses: HashMap::from([(RegisterAddress::new(SecretKey::random().public_key()), "test_register".to_string())]),
        };

        // Serialize V1 data
        let serialized = rmp_serde::to_vec(&v1_data).unwrap();

        // Deserialize into current UserData
        let deserialized: UserData = rmp_serde::from_slice(&serialized).unwrap();

        // Verify the conversion was successful
        assert_eq!(deserialized.file_archives, v1_data.file_archives);
        assert_eq!(
            deserialized.private_file_archives,
            v1_data.private_file_archives
        );
        assert_eq!(deserialized.register_addresses, v1_data.register_addresses);
        assert_eq!(deserialized.register_key, None);
        assert_eq!(deserialized.scratchpad_key, None);
        assert_eq!(deserialized.pointer_key, None);
        assert_eq!(deserialized.public_files, HashMap::new());
        assert_eq!(deserialized.private_files, HashMap::new());

        // Test current version serialization/deserialization
        let current_data = UserData {
            file_archives: v1_data.file_archives.clone(),
            private_file_archives: v1_data.private_file_archives.clone(),
            register_addresses: v1_data.register_addresses.clone(),
            register_key: Some("test_key".to_string()),
            scratchpad_key: Some("test_scratchpad_key".to_string()),
            pointer_key: Some("test_pointer_key".to_string()),
            public_files: HashMap::new(),
            private_files: HashMap::new(),
        };

        let serialized = rmp_serde::to_vec(&current_data).unwrap();
        let deserialized: UserData = rmp_serde::from_slice(&serialized).unwrap();

        // Verify current version maintains all fields
        assert_eq!(deserialized, current_data);
    }
}
