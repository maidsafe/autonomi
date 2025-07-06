// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::Hash;
use alloy::primitives::keccak256;
use alloy::signers::k256::ecdsa::{signature, RecoveryId, Signature, SigningKey};
use alloy::signers::local::PrivateKeySigner;

/// Hash data using Keccak256.
pub fn hash<T: AsRef<[u8]>>(data: T) -> Hash {
    keccak256(data.as_ref())
}

/// Sign error
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("Failed to parse EVM secret key: {0}")]
    InvalidEvmSecretKey(String),
    #[error("Failed to sign message: {0}")]
    Signature(#[from] signature::Error),
}

/// Sign a message with an EVM secret key.
pub fn sign_message(evm_secret_key_str: &str, message: &[u8]) -> Result<Vec<u8>, SignError> {
    let signer: PrivateKeySigner =
        evm_secret_key_str
            .parse::<PrivateKeySigner>()
            .map_err(|err| {
                error!("Error parsing EVM secret key: {err}");
                SignError::InvalidEvmSecretKey(err.to_string())
            })?;

    let message_hash = to_eth_signed_message_hash(message);
    let (signature, _) = sign_message_recoverable(&signer.into_credential(), message_hash)?;
    debug!("Message signed successfully with {message_hash:?} and {signature:?}");

    Ok(signature.to_vec())
}

/// Hash a message using Keccak256, then add the Ethereum prefix and hash it again.
fn to_eth_signed_message_hash<T: AsRef<[u8]>>(message: T) -> [u8; 32] {
    const PREFIX: &str = "\x19Ethereum Signed Message:\n32";

    let hashed_message = hash(message);

    let mut eth_message = Vec::with_capacity(PREFIX.len() + 32);
    eth_message.extend_from_slice(PREFIX.as_bytes());
    eth_message.extend_from_slice(hashed_message.as_slice());

    hash(eth_message).into()
}

/// Sign a message with a recoverable public key.
fn sign_message_recoverable<T: AsRef<[u8]>>(
    secret_key: &SigningKey,
    message: T,
) -> Result<(Signature, RecoveryId), signature::Error> {
    let hash = to_eth_signed_message_hash(message);
    secret_key.sign_prehash_recoverable(&hash)
}
