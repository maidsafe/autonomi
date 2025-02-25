// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use serde::{Deserialize, Serialize};
use std::hash::Hash;
use xor_name::XorName;

use super::AddressParseError;

/// Address of a [`crate::storage::chunks::Chunk`]
/// It is derived from the content of the chunk
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
pub struct ChunkAddress(XorName);

impl ChunkAddress {
    /// Creates a new ChunkAddress.
    pub fn new(xor_name: XorName) -> Self {
        Self(xor_name)
    }

    /// Returns the XorName
    pub fn xorname(&self) -> &XorName {
        &self.0
    }

    /// Returns the hex string representation of the address.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Creates a new ChunkAddress from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, AddressParseError> {
        let bytes = hex::decode(hex)?;
        let xor = XorName(
            bytes
                .try_into()
                .map_err(|_| AddressParseError::InvalidLength)?,
        );
        Ok(Self(xor))
    }
}

impl std::fmt::Display for ChunkAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
    }
}
