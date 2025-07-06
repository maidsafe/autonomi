// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::ScratchpadAddress;
use crate::error::{Error, Result};
use crate::Bytes;
use crate::NetworkAddress;
use bls::{Ciphertext, PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use xor_name::XorName;

/// Scratchpad, a mutable space for encrypted data on the Network
#[derive(Hash, Eq, PartialEq, PartialOrd, Ord, Clone, Serialize, Deserialize)]
pub struct Scratchpad {
    /// Network address. Omitted when serialising and
    /// calculated from the `encrypted_data` when deserialising.
    address: ScratchpadAddress,
    /// Data encoding: custom apps using scratchpad should use this so they can identify the type of data they are storing
    data_encoding: u64,
    /// Encrypted data stored in the scratchpad, it is encrypted automatically by the [`Scratchpad::new`] and [`Scratchpad::update`] methods
    encrypted_data: Bytes,
    /// Monotonically increasing counter to track the number of times this has been updated.
    /// When pushed to the network, the scratchpad with the highest counter is kept.
    counter: u64,
    /// Signature over the above fields
    signature: Signature,
}

impl std::fmt::Debug for Scratchpad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scratchpad")
            .field("address", &self.address)
            .field("data_encoding", &self.data_encoding)
            .field(
                "encrypted_data",
                &format!("({} bytes of encrypted data)", self.encrypted_data.len()),
            )
            .field("counter", &self.counter)
            .field("signature", &hex::encode(self.signature.to_bytes()))
            .finish()
    }
}

impl Scratchpad {
    /// Max Scratchpad size is 4MB including the metadata
    pub const MAX_SIZE: usize = 4 * 1024 * 1024;

    /// Creates a new instance of `Scratchpad`. Encrypts the data, and signs all the elements.
    pub fn new(
        owner: &SecretKey,
        data_encoding: u64,
        unencrypted_data: &Bytes,
        counter: u64,
    ) -> Self {
        let pk = owner.public_key();
        let encrypted_data = Bytes::from(pk.encrypt(unencrypted_data).to_bytes());
        let addr = ScratchpadAddress::new(pk);
        let signature = owner.sign(Self::bytes_for_signature(
            addr,
            data_encoding,
            &encrypted_data,
            counter,
        ));
        Self {
            address: addr,
            encrypted_data,
            data_encoding,
            counter,
            signature,
        }
    }

    /// Create a new Scratchpad without provding the secret key
    /// It is the caller's responsibility to ensure the signature is valid (signs [`Scratchpad::bytes_for_signature`]) and the data is encrypted
    /// It is recommended to use the [`Scratchpad::new`] method instead when possible
    pub fn new_with_signature(
        owner: PublicKey,
        data_encoding: u64,
        encrypted_data: Bytes,
        counter: u64,
        signature: Signature,
    ) -> Self {
        Self {
            address: ScratchpadAddress::new(owner),
            encrypted_data,
            data_encoding,
            counter,
            signature,
        }
    }

    /// Returns the bytes to sign for the signature
    pub fn bytes_for_signature(
        address: ScratchpadAddress,
        data_encoding: u64,
        encrypted_data: &Bytes,
        counter: u64,
    ) -> Vec<u8> {
        let mut bytes_to_sign = data_encoding.to_be_bytes().to_vec();
        bytes_to_sign.extend(address.to_hex().as_bytes());
        bytes_to_sign.extend(counter.to_be_bytes().to_vec());
        bytes_to_sign.extend(encrypted_data.to_vec());
        bytes_to_sign
    }

    /// Get the counter of the Scratchpad, the higher the counter, the more recent the Scratchpad is
    /// Similarly to counter CRDTs only the latest version (highest counter) of the Scratchpad is kept on the network
    pub fn counter(&self) -> u64 {
        self.counter
    }

    /// Return the current data encoding
    pub fn data_encoding(&self) -> u64 {
        self.data_encoding
    }

    /// Updates the content and encrypts it, increments the counter, re-signs the scratchpad
    pub fn update(&mut self, unencrypted_data: &Bytes, sk: &SecretKey) {
        self.counter += 1;
        let pk = self.owner();
        let address = ScratchpadAddress::new(*pk);
        self.encrypted_data = Bytes::from(pk.encrypt(unencrypted_data).to_bytes());

        let bytes_to_sign = Self::bytes_for_signature(
            address,
            self.data_encoding,
            &self.encrypted_data,
            self.counter,
        );
        self.signature = sk.sign(&bytes_to_sign);
        debug_assert!(self.verify_signature(), "Must be valid after being signed. This is a bug, please report it by opening an issue on our github");
    }

    /// Verifies that the Scratchpad signature is valid
    pub fn verify_signature(&self) -> bool {
        let signing_bytes = Self::bytes_for_signature(
            self.address,
            self.data_encoding,
            &self.encrypted_data,
            self.counter,
        );
        self.owner().verify(&self.signature, &signing_bytes)
    }

    /// Returns the encrypted_data.
    pub fn encrypted_data(&self) -> &Bytes {
        &self.encrypted_data
    }

    /// Returns the encrypted_data, decrypted via the passed SecretKey
    pub fn decrypt_data(&self, sk: &SecretKey) -> Result<Bytes> {
        let cipher = Ciphertext::from_bytes(&self.encrypted_data)
            .map_err(|_| Error::ScratchpadCipherTextFailed)?;
        let bytes = sk
            .decrypt(&cipher)
            .ok_or(Error::ScratchpadCipherTextInvalid)?;
        Ok(Bytes::from(bytes))
    }

    /// Returns the encrypted_data hash
    pub fn encrypted_data_hash(&self) -> XorName {
        XorName::from_content(&self.encrypted_data)
    }

    /// Returns the owner of the scratchpad
    pub fn owner(&self) -> &PublicKey {
        self.address.owner()
    }

    /// Returns the address of the scratchpad
    pub fn address(&self) -> &ScratchpadAddress {
        &self.address
    }

    /// Returns the NetworkAddress.
    pub fn network_address(&self) -> NetworkAddress {
        NetworkAddress::ScratchpadAddress(self.address)
    }

    /// Returns the xorname.
    pub fn xorname(&self) -> XorName {
        self.address.xorname()
    }

    /// Returns size of contained encrypted_data.
    pub fn payload_size(&self) -> usize {
        self.encrypted_data.len()
    }

    /// Size of the scratchpad
    pub fn size(&self) -> usize {
        size_of::<Scratchpad>() + self.payload_size()
    }

    /// Returns true if the scratchpad is too big
    pub fn is_too_big(&self) -> bool {
        self.size() > Self::MAX_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratchpad_sig_and_update() {
        let sk = SecretKey::random();
        let raw_data = Bytes::from_static(b"data to be encrypted");
        let mut scratchpad = Scratchpad::new(&sk, 42, &raw_data, 0);
        assert!(scratchpad.verify_signature());
        assert_eq!(scratchpad.counter(), 0);
        assert_ne!(scratchpad.encrypted_data(), &raw_data);

        let raw_data2 = Bytes::from_static(b"data to be encrypted v2");
        scratchpad.update(&raw_data2, &sk);
        assert!(scratchpad.verify_signature());
        assert_eq!(scratchpad.counter(), 1);
        assert_ne!(scratchpad.encrypted_data(), &raw_data);
        assert_ne!(scratchpad.encrypted_data(), &raw_data2);
    }

    #[test]
    fn test_scratchpad_encryption() {
        let sk = SecretKey::random();
        let raw_data = Bytes::from_static(b"data to be encrypted");
        let scratchpad = Scratchpad::new(&sk, 42, &raw_data, 0);

        let decrypted_data = scratchpad.decrypt_data(&sk).unwrap();
        assert_eq!(decrypted_data, raw_data);
    }
}
