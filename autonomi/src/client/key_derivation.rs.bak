// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use bls::{serde_impl::SerdeSecret, PublicKey, SecretKey, PK_SIZE};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Errors that can occur when decoding a key from a hex string
#[derive(Error, Debug)]
pub enum KeyDecodeError {
    #[error("Failed to decode hex to key")]
    FailedToDecodeHexToKey,
    #[error("Failed to parse BLS key")]
    FailedToParseBlsKey,
    #[error("Invalid key length")]
    InvalidKeyLength,
}

/// This is used to generate a new DerivedPubkey
/// from a MainPubkey, and the corresponding
/// DerivedSecretKey from the MainSecretKey of that MainPubkey.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Hash)]
pub struct DerivationIndex([u8; 32]);

impl fmt::Debug for DerivationIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{:02x}{:02x}{:02x}..",
            self.0[0], self.0[1], self.0[2]
        )
    }
}

impl DerivationIndex {
    /// generates a random derivation index
    pub fn random(rng: &mut impl RngCore) -> DerivationIndex {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        DerivationIndex(bytes)
    }

    /// returns the inner bytes representation
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// returns the inner bytes
    pub fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Create a new DerivationIndex from a bytes array
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// A public key derived from a [`MainPubkey`] using a [`DerivationIndex`]
/// Its associated secret key is the [`DerivedSecretKey`]
/// This key is unlinkable to the original [`MainPubkey`]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DerivedPubkey(PublicKey);

impl DerivedPubkey {
    pub fn new<G: Into<PublicKey>>(public_key: G) -> Self {
        Self(public_key.into())
    }

    pub fn to_bytes(&self) -> [u8; bls::PK_SIZE] {
        self.0.to_bytes()
    }

    /// Returns `true` if the signature matches the message.
    pub fn verify<M: AsRef<[u8]>>(&self, sig: &bls::Signature, msg: M) -> bool {
        self.0.verify(sig, msg)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    pub fn from_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, KeyDecodeError> {
        let public_key = bls_public_from_hex(hex)?;
        Ok(Self::new(public_key))
    }
}

/// Custom implementation of Serialize and Deserialize for [`DerivedPubkey`] to make it an actionable
/// hex string that can be copy pasted in apps, instead of a useless array of numbers
impl Serialize for DerivedPubkey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for DerivedPubkey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex = String::deserialize(deserializer)?;
        DerivedPubkey::from_hex(hex).map_err(|e| {
            serde::de::Error::custom(format!("Failed to deserialize DerivedPubkey from hex: {e}",))
        })
    }
}

/// Actionable way to print a DerivedPubkey
/// This way to print it is lengthier but allows to copy/paste it into the cli or other apps
/// To use for verification purposes
impl std::fmt::Debug for DerivedPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl std::fmt::Display for DerivedPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// The secret key of a [`DerivedPubkey`]
/// It is derived from the [`MainSecretKey`] with the same [`DerivationIndex`]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSecretKey(SerdeSecret<SecretKey>);

impl DerivedSecretKey {
    pub fn new<S: Into<SecretKey>>(secret_key: S) -> Self {
        Self(SerdeSecret(secret_key.into()))
    }

    /// The [`DerivedPubkey`] of this [`DerivedSecretKey`]
    pub fn public_key(&self) -> DerivedPubkey {
        DerivedPubkey(self.0.public_key())
    }

    /// Sign a message with the secret key
    pub fn sign(&self, msg: &[u8]) -> bls::Signature {
        self.0.sign(msg)
    }
}

/// This is the public key of the [`MainSecretKey`]
/// One can derive [`DerivedPubkey`]s from this [`MainPubkey`]
#[derive(Copy, PartialEq, Eq, Ord, PartialOrd, Clone, Serialize, Deserialize, Hash)]
pub struct MainPubkey(pub PublicKey);

impl MainPubkey {
    /// Create a new [`MainPubkey`] from a bls [`PublicKey`]
    pub fn new(public_key: PublicKey) -> Self {
        Self(public_key)
    }

    /// Verify that the signature is valid for the message.
    pub fn verify(&self, sig: &bls::Signature, msg: &[u8]) -> bool {
        self.0.verify(sig, msg)
    }

    /// Generate a new [`DerivedPubkey`] from provided [`DerivationIndex`].
    pub fn derive_key(&self, index: &DerivationIndex) -> DerivedPubkey {
        DerivedPubkey(self.0.derive_child(&index.0))
    }

    /// Return the inner pubkey's bytes representation
    pub fn to_bytes(self) -> [u8; PK_SIZE] {
        self.0.to_bytes()
    }

    /// Return a hex representation of the [`MainPubkey`]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Create a new [`MainPubkey`] from a hex string
    pub fn from_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, KeyDecodeError> {
        let public_key = bls_public_from_hex(hex)?;
        Ok(Self::new(public_key))
    }
}

impl std::fmt::Debug for MainPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// The secret key of the [`MainPubkey`]
/// It is held privately and not shared with anyone
/// With this [`MainSecretKey`], new [`DerivedSecretKey`]:[`DerivedPubkey`] pairs can be generated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainSecretKey(SerdeSecret<SecretKey>);

impl MainSecretKey {
    /// Create a [`MainSecretKey`] from a bls [`SecretKey`].
    pub fn new(secret_key: SecretKey) -> Self {
        Self(SerdeSecret(secret_key))
    }

    /// Return the matching [`MainPubkey`]
    pub fn public_key(&self) -> MainPubkey {
        MainPubkey(self.0.public_key())
    }

    /// Signs the given message
    pub fn sign(&self, msg: &[u8]) -> bls::Signature {
        self.0.sign(msg)
    }

    /// Derive a [`DerivedSecretKey`] from a [`DerivationIndex`]
    /// This is used to create a new unlinkable key pair that cannot be linked back to the [`MainSecretKey`] without the [`DerivationIndex`]
    pub fn derive_key(&self, index: &DerivationIndex) -> DerivedSecretKey {
        DerivedSecretKey::new(self.0.inner().derive_child(&index.0))
    }

    /// Return the inner secret key's bytes representation
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }

    /// Generate a new random [`MainSecretKey`]
    pub fn random() -> Self {
        Self::new(SecretKey::random())
    }

    /// Generate a new random [`DerivedSecretKey`] from the [`MainSecretKey`]
    pub fn random_derived_key(&self, rng: &mut impl RngCore) -> DerivedSecretKey {
        self.derive_key(&DerivationIndex::random(rng))
    }
}

/// Construct a BLS public key from a hex-encoded string.
fn bls_public_from_hex<T: AsRef<[u8]>>(hex: T) -> Result<PublicKey, KeyDecodeError> {
    let bytes = hex::decode(hex).map_err(|_| KeyDecodeError::FailedToDecodeHexToKey)?;
    let bytes_fixed_len: [u8; bls::PK_SIZE] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| KeyDecodeError::InvalidKeyLength)?;
    let pk =
        PublicKey::from_bytes(bytes_fixed_len).map_err(|_| KeyDecodeError::FailedToParseBlsKey)?;
    Ok(pk)
}

// conversions to bls types
impl From<MainSecretKey> for SecretKey {
    fn from(main_secret_key: MainSecretKey) -> Self {
        main_secret_key.0.inner().to_owned()
    }
}
impl From<DerivedSecretKey> for SecretKey {
    fn from(derived_secret_key: DerivedSecretKey) -> Self {
        derived_secret_key.0.inner().to_owned()
    }
}
impl From<DerivedPubkey> for PublicKey {
    fn from(derived_pubkey: DerivedPubkey) -> Self {
        derived_pubkey.0
    }
}
impl From<MainPubkey> for PublicKey {
    fn from(main_pubkey: MainPubkey) -> Self {
        main_pubkey.0
    }
}

// conversions from bls types
impl From<SecretKey> for MainSecretKey {
    fn from(secret_key: SecretKey) -> Self {
        MainSecretKey::new(secret_key)
    }
}
impl From<SecretKey> for DerivedSecretKey {
    fn from(secret_key: SecretKey) -> Self {
        DerivedSecretKey::new(secret_key)
    }
}
impl From<PublicKey> for MainPubkey {
    fn from(public_key: PublicKey) -> Self {
        MainPubkey::new(public_key)
    }
}
impl From<PublicKey> for DerivedPubkey {
    fn from(public_key: PublicKey) -> Self {
        DerivedPubkey::new(public_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pubkeys_hex_conversion() -> eyre::Result<()> {
        let sk = bls::SecretKey::random();
        let pk = sk.public_key();
        let main_pubkey = MainPubkey::new(pk);
        let unique_pubkey =
            main_pubkey.derive_key(&DerivationIndex::random(&mut rand::thread_rng()));

        let main_pubkey_hex = main_pubkey.to_hex();
        let unique_pubkey_hex = unique_pubkey.to_hex();

        let main_pubkey_from_hex = MainPubkey::from_hex(main_pubkey_hex)?;
        let unique_pubkey_from_hex = DerivedPubkey::from_hex(unique_pubkey_hex)?;

        assert_eq!(main_pubkey, main_pubkey_from_hex);
        assert_eq!(unique_pubkey, unique_pubkey_from_hex);
        Ok(())
    }

    #[test]
    fn test_serialisation() -> eyre::Result<()> {
        let pk = SecretKey::random().public_key();
        let main_pubkey = MainPubkey::new(pk);
        let unique_pk = main_pubkey.derive_key(&DerivationIndex::random(&mut rand::thread_rng()));

        let str_serialised = rmp_serde::to_vec_named(&unique_pk)?;
        let str_deserialised: DerivedPubkey = rmp_serde::from_slice(&str_serialised)?;
        assert_eq!(str_deserialised, unique_pk);

        Ok(())
    }

    #[test]
    fn verification_using_child_key() -> eyre::Result<()> {
        let msg = "just a test string".as_bytes();
        let main_sk = MainSecretKey::random();
        let derived_sk = main_sk.random_derived_key(&mut rand::thread_rng());

        // Signature signed by parent key can not be verified by the child key.
        let signature = main_sk.sign(msg);
        assert!(main_sk.public_key().verify(&signature, msg));
        assert!(!derived_sk.public_key().verify(&signature, msg));

        // Signature signed by child key can not be verified by the parent key.
        let signature = derived_sk.sign(msg);
        assert!(derived_sk.public_key().verify(&signature, msg));
        assert!(!main_sk.public_key().verify(&signature, msg));

        Ok(())
    }
}
