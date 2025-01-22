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

/// Address of a GraphEntry, is derived from the owner's unique public key
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct GraphEntryAddress(pub XorName);

impl GraphEntryAddress {
    pub fn from_owner(owner: PublicKey) -> Self {
        Self(XorName::from_content(&owner.to_bytes()))
    }

    pub fn new(xor_name: XorName) -> Self {
        Self(xor_name)
    }

    pub fn xorname(&self) -> &XorName {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Debug for GraphEntryAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GraphEntryAddress({})", &self.to_hex()[0..6])
    }
}
