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

/// This is the max manually observed overhead when compressing random 4MB of data using Brotli.
/// It might be possible there could be edge-cases where the overhead is even higher.
const BROTLI_MAX_OVERHEAD_BYTES: usize = 16;

/// When encrypting chunks, a bit of padding is added to make the size a multiple of 16.
/// When the chunk size is already a multiple of 16, a full block padding will be added.
const PKCS7_MAX_PADDING_BYTES: usize = 16;

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
    /// The maximum size of an unencrypted/raw chunk is 4MB.
    pub const MAX_RAW_SIZE: usize = 4 * 1024 * 1024;

    /// The maximum size of an encrypted chunk is 4MB + 32 bytes.
    /// + 16 bytes Brotli compression overhead for random data.
    /// + 16 bytes due to Pkcs7 encryption padding.
    pub const MAX_SIZE: usize =
        Self::MAX_RAW_SIZE + BROTLI_MAX_OVERHEAD_BYTES + PKCS7_MAX_PADDING_BYTES;

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
        self.size() > Self::MAX_SIZE
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
