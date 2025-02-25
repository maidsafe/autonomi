// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use bls::PublicKey;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use xor_name::XorName;

use super::AddressParseError;

/// Address of a [`crate::storage::scratchpad::Scratchpad`]
/// It is derived from the owner's public key
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
pub struct ScratchpadAddress(PublicKey);

impl ScratchpadAddress {
    /// Create a new [`ScratchpadAddress`]
    pub fn new(owner: PublicKey) -> Self {
        Self(owner)
    }

    /// Return the network name of the scratchpad.
    /// This is used to locate the scratchpad on the network.
    pub fn xorname(&self) -> XorName {
        XorName::from_content(&self.0.to_bytes())
    }

    /// Return the owner.
    pub fn owner(&self) -> &PublicKey {
        &self.0
    }

    /// Serialize this [`ScratchpadAddress`] into a hex-encoded string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Parse a hex-encoded string into a [`ScratchpadAddress`].
    pub fn from_hex(hex: &str) -> Result<Self, AddressParseError> {
        let owner = PublicKey::from_hex(hex)?;
        Ok(Self(owner))
    }
}

impl std::fmt::Display for ScratchpadAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bls::SecretKey;

    #[test]
    fn test_scratchpad_hex_conversion() {
        let owner = SecretKey::random().public_key();
        let addr = ScratchpadAddress::new(owner);
        let hex = addr.to_hex();
        let addr2 = ScratchpadAddress::from_hex(&hex).unwrap();

        assert_eq!(addr, addr2);

        let bad_hex = format!("{hex}0");
        let err = ScratchpadAddress::from_hex(&bad_hex);
        assert!(err.is_err());
    }
}
