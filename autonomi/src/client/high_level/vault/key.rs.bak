// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use blst::min_pk::SecretKey as BlstSecretKey;
use sha2::{Digest, Sha256};

/// Secret key to decrypt vault content
pub type VaultSecretKey = bls::SecretKey;

#[derive(Debug, thiserror::Error)]
pub enum VaultKeyError {
    #[error("Failed to sign message: {0}")]
    FailedToSignMessage(#[from] ant_evm::cryptography::SignError),
    #[error("Failed to generate vault secret key: {0}")]
    FailedToGenerateVaultSecretKey(String),
    #[error("Failed to convert blst secret key to blsttc secret key: {0}")]
    BlsConversionError(#[from] bls::Error),
    #[error("Failed to generate blst secret key")]
    KeyGenerationError,
}

/// Message used to generate the vault secret key from the EVM secret key
const VAULT_SECRET_KEY_SEED: &[u8] = b"Massive Array of Internet Disks Secure Access For Everyone";

/// Derives the vault secret key from the EVM secret key hex string
/// The EVM secret key is used to sign a message and the signature is hashed to derive the vault secret key
/// Being able to derive the vault secret key from the EVM secret key allows users to only keep track of one key: the EVM secret key
pub fn derive_vault_key(evm_sk_hex: &str) -> Result<VaultSecretKey, VaultKeyError> {
    let signature = ant_evm::cryptography::sign_message(evm_sk_hex, VAULT_SECRET_KEY_SEED)
        .map_err(VaultKeyError::FailedToSignMessage)?;

    let blst_key = derive_secret_key_from_seed(&signature)?;
    let vault_sk = blst_to_blsttc(&blst_key)?;
    Ok(vault_sk)
}

/// Derives the vault secret key from a signature hex string
pub fn vault_key_from_signature_hex(signature_hex: &str) -> Result<VaultSecretKey, VaultKeyError> {
    let signature_bytes = hex::decode(signature_hex)
        .map_err(|e| VaultKeyError::FailedToGenerateVaultSecretKey(e.to_string()))?;

    let blst_key = derive_secret_key_from_seed(&signature_bytes)?;
    let vault_sk = blst_to_blsttc(&blst_key)?;

    Ok(vault_sk)
}

/// Convert a blst secret key to a blsttc secret key and pray that endianness is the same
pub(crate) fn blst_to_blsttc(sk: &BlstSecretKey) -> Result<bls::SecretKey, VaultKeyError> {
    let sk_bytes = sk.to_bytes();
    let sk = bls::SecretKey::from_bytes(sk_bytes).map_err(VaultKeyError::BlsConversionError)?;
    Ok(sk)
}

pub(crate) fn derive_secret_key_from_seed(seed: &[u8]) -> Result<BlstSecretKey, VaultKeyError> {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    let hashed_seed = hasher.finalize();
    let sk =
        BlstSecretKey::key_gen(&hashed_seed, &[]).map_err(|_| VaultKeyError::KeyGenerationError)?;
    Ok(sk)
}
