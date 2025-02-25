// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::storage::AddressParseError;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use xor_name::XorName;

/// Private data on the network, readable only if you have the DataMapChunk
pub mod private;
/// Public data on the network, readable by anyone with the DataAddr
pub mod public;

/// A [`DataAddress`] which points to a DataMap
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
pub struct DataAddress(XorName);

impl DataAddress {
    /// Creates a new DataAddress.
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

    /// Creates a new DataAddress from a hex string.
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

impl std::fmt::Display for DataAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
    }
}
