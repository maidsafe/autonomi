// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::ChunkAddress;
use crate::NetworkAddress;
use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use xor_name::XorName;

/// Chunk, an immutable chunk of data
#[derive(Hash, Eq, PartialEq, PartialOrd, Ord, Clone, custom_debug::Debug)]
pub struct Chunk {
    /// Network address. Omitted when serialising and
    /// calculated from the `value` when deserialising.
    pub address: ChunkAddress,
    /// Contained data.
    #[debug(skip)]
    pub value: Bytes,
}

impl Chunk {
    /// The maximum size of a chunk is 4MB.
    /// Note that due to encryption it can turn out bigger. See `Chunk::MAX_ENCRYPTED_SIZE`.
    const MAX_SIZE: usize = 4 * 1024 * 1024;

    /// The maximum size of an encrypted chunk is 4MB + 16 bytes due to Pkcs7 encryption padding.
    pub const MAX_ENCRYPTED_SIZE: usize = Self::MAX_SIZE + 16;

    /// Creates a new instance of `Chunk`.
    pub fn new(value: Bytes) -> Self {
        Self {
            address: ChunkAddress::new(XorName::from_content(value.as_ref())),
            value,
        }
    }

    /// Returns the value.
    pub fn value(&self) -> &Bytes {
        &self.value
    }

    /// Returns the address.
    pub fn address(&self) -> &ChunkAddress {
        &self.address
    }

    /// Returns the NetworkAddress
    pub fn network_address(&self) -> NetworkAddress {
        NetworkAddress::ChunkAddress(self.address)
    }

    /// Returns the name.
    pub fn name(&self) -> &XorName {
        self.address.xorname()
    }

    /// Returns size of this chunk after serialisation.
    pub fn size(&self) -> usize {
        self.value.len()
    }

    /// Returns true if the chunk is too big
    pub fn is_too_big(&self) -> bool {
        self.size() > Self::MAX_ENCRYPTED_SIZE
    }
}

impl Serialize for Chunk {
    fn serialize<S: Serializer>(&self, serialiser: S) -> Result<S::Ok, S::Error> {
        // Address is omitted since it's derived from value
        self.value.serialize(serialiser)
    }
}

impl<'de> Deserialize<'de> for Chunk {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Deserialize::deserialize(deserializer)?;
        Ok(Self::new(value))
    }
}
