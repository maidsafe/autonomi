// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use bls::PublicKey;
use serde::{Deserialize, Serialize};
use xor_name::XorName;

use super::AddressParseError;

/// Address of a [`crate::storage::graph::GraphEntry`]
/// It is derived from the owner's unique public key
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
pub struct GraphEntryAddress(PublicKey);

impl GraphEntryAddress {
    /// Create a new [`GraphEntryAddress`]
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

    /// Serialize this [`GraphEntryAddress`] into a hex-encoded string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Parse a hex-encoded string into a [`GraphEntryAddress`].
    pub fn from_hex(hex: &str) -> Result<Self, AddressParseError> {
        let owner = PublicKey::from_hex(hex)?;
        Ok(Self(owner))
    }
}

impl std::fmt::Display for GraphEntryAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
    }
}
