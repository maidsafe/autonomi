// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[macro_use]
extern crate tracing;

/// Errors.
pub mod error;
/// Messages types
pub mod messages;
/// Storage types for spends, chunks and registers.
pub mod storage;

use self::storage::{ChunkAddress, RegisterAddress, SpendAddress};
use bytes::Bytes;
use libp2p::{
    kad::{KBucketDistance as Distance, KBucketKey as Key, RecordKey},
    PeerId,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{self, Debug, Display, Formatter};
use xor_name::XorName;

/// This is the address in the network by which proximity/distance
/// to other items (whether nodes or data chunks) are calculated.
///
/// This is the mapping from the XOR name used
/// by for example self encryption, or the libp2p `PeerId`,
/// to the key used in the Kademlia DHT.
/// All our xorname calculations shall be replaced with the `KBucketKey` calculations,
/// for getting proximity/distance to other items (whether nodes or data).
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum NetworkAddress {
    /// The NetworkAddress is representing a PeerId.
    PeerId(Vec<u8>),
    /// The NetworkAddress is representing a ChunkAddress.
    ChunkAddress(ChunkAddress),
    /// The NetworkAddress is representing a SpendAddress.
    SpendAddress(SpendAddress),
    /// The NetworkAddress is representing a ChunkAddress.
    RegisterAddress(RegisterAddress),
    /// The NetworkAddress is representing a RecordKey.
    RecordKey(Vec<u8>),
}

impl NetworkAddress {
    /// Return a `NetworkAddress` representation of the `ChunkAddress`.
    pub fn from_chunk_address(chunk_address: ChunkAddress) -> Self {
        NetworkAddress::ChunkAddress(chunk_address)
    }

    /// Return a `NetworkAddress` representation of the `SpendAddress`.
    pub fn from_cash_note_address(cash_note_address: SpendAddress) -> Self {
        NetworkAddress::SpendAddress(cash_note_address)
    }

    /// Return a `NetworkAddress` representation of the `RegisterAddress`.
    pub fn from_register_address(register_address: RegisterAddress) -> Self {
        NetworkAddress::RegisterAddress(register_address)
    }

    /// Return a `NetworkAddress` representation of the `PeerId` by encapsulating its bytes.
    pub fn from_peer(peer_id: PeerId) -> Self {
        NetworkAddress::PeerId(peer_id.to_bytes())
    }

    /// Return a `NetworkAddress` representation of the `RecordKey` by encapsulating its bytes.
    pub fn from_record_key(record_key: RecordKey) -> Self {
        NetworkAddress::RecordKey(record_key.to_vec())
    }

    /// Return the encapsulated bytes of this `NetworkAddress`.
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            NetworkAddress::PeerId(bytes) | NetworkAddress::RecordKey(bytes) => bytes.to_vec(),
            NetworkAddress::ChunkAddress(chunk_address) => chunk_address.xorname().0.to_vec(),
            NetworkAddress::SpendAddress(cash_note_address) => {
                cash_note_address.xorname().0.to_vec()
            }
            NetworkAddress::RegisterAddress(register_address) => {
                register_address.xorname().0.to_vec()
            }
        }
    }

    /// Try to return the represented `PeerId`.
    pub fn as_peer_id(&self) -> Option<PeerId> {
        if let NetworkAddress::PeerId(bytes) = self {
            if let Ok(peer_id) = PeerId::from_bytes(bytes) {
                return Some(peer_id);
            }
        }

        None
    }

    /// Try to return the represented `XorName`.
    pub fn as_xorname(&self) -> Option<XorName> {
        match self {
            NetworkAddress::SpendAddress(cash_note_address) => Some(*cash_note_address.xorname()),
            NetworkAddress::ChunkAddress(chunk_address) => Some(*chunk_address.xorname()),
            NetworkAddress::RegisterAddress(register_address) => Some(register_address.xorname()),
            _ => None,
        }
    }

    /// Try to return the represented `RecordKey`.
    pub fn as_record_key(&self) -> Option<RecordKey> {
        match self {
            NetworkAddress::RecordKey(bytes) => Some(RecordKey::new(bytes)),
            _ => None,
        }
    }

    /// Return the convertable `RecordKey`.
    pub fn to_record_key(&self) -> RecordKey {
        match self {
            NetworkAddress::RecordKey(bytes) => RecordKey::new(bytes),
            NetworkAddress::ChunkAddress(chunk_address) => RecordKey::new(chunk_address.xorname()),
            NetworkAddress::RegisterAddress(register_address) => {
                RecordKey::new(&register_address.xorname())
            }
            NetworkAddress::SpendAddress(cash_note_address) => {
                RecordKey::new(cash_note_address.xorname())
            }
            NetworkAddress::PeerId(bytes) => RecordKey::new(bytes),
        }
    }

    /// Return the `KBucketKey` representation of this `NetworkAddress`.
    ///
    /// The `KBucketKey` is used for calculating proximity/distance to other items (whether nodes or data).
    /// Important to note is that it will always SHA256 hash any bytes it receives.
    /// Therefore, the canonical use of distance/proximity calculations in the network
    /// is via the `KBucketKey`, or the convenience methods of `NetworkAddress`.
    pub fn as_kbucket_key(&self) -> Key<Vec<u8>> {
        Key::new(self.as_bytes())
    }

    /// Compute the distance of the keys according to the XOR metric.
    pub fn distance(&self, other: &NetworkAddress) -> Distance {
        self.as_kbucket_key().distance(&other.as_kbucket_key())
    }

    // NB: Leaving this here as to demonstrate what we can do with this.
    // /// Return the uniquely determined key with the given distance to `self`.
    // ///
    // /// This implements the following equivalence:
    // ///
    // /// `self xor other = distance <==> other = self xor distance`
    // pub fn for_distance(&self, d: Distance) -> libp2p::kad::kbucket::KeyBytes {
    //     self.as_kbucket_key().for_distance(d)
    // }
}

impl Debug for NetworkAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name_str = match self {
            NetworkAddress::PeerId(_) => "NetworkAddress::PeerId(".to_string(),
            NetworkAddress::ChunkAddress(chunk_address) => {
                format!(
                    "NetworkAddress::ChunkAddress({:?} - ",
                    chunk_address.xorname()
                )
            }
            NetworkAddress::SpendAddress(cash_note_address) => {
                format!(
                    "NetworkAddress::SpendAddress({:?} - ",
                    cash_note_address.xorname()
                )
            }
            NetworkAddress::RegisterAddress(register_address) => format!(
                "NetworkAddress::RegisterAddress({:?} - ",
                register_address.xorname()
            ),
            NetworkAddress::RecordKey(_) => "NetworkAddress::RecordKey(".to_string(),
        };
        write!(
            f,
            "{name_str} - {:?})",
            PrettyPrintRecordKey::from(self.to_record_key()),
        )
    }
}

impl Display for NetworkAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkAddress::PeerId(id) => {
                write!(f, "NetworkAddress::PeerId({})", hex::encode(id))
            }
            NetworkAddress::ChunkAddress(addr) => {
                write!(f, "NetworkAddress::ChunkAddress({addr:?})")
            }
            NetworkAddress::SpendAddress(addr) => {
                write!(f, "NetworkAddress::SpendAddress({addr:?})")
            }
            NetworkAddress::RegisterAddress(addr) => {
                write!(f, "NetworkAddress::RegisterAddress({addr:?})")
            }
            NetworkAddress::RecordKey(key) => {
                write!(f, "NetworkAddress::RecordKey({})", hex::encode(key))
            }
        }
    }
}

/// Pretty print a `kad::RecordKey` as a hex string.
/// So clients can use the hex string for xorname and record keys interchangeably.
/// This makes errors actionable for clients.
/// The only cost is converting kad::RecordKey into it before sending it in errors: `record_key.into()`
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct PrettyPrintRecordKey(RecordKey);

// Implementing Serialize for PrettyPrintRecordKey
impl Serialize for PrettyPrintRecordKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Use the `to_vec` function of the inner RecordKey to get the bytes
        // and then serialize those bytes
        self.0.to_vec().serialize(serializer)
    }
}

// Implementing Deserialize for PrettyPrintRecordKey
impl<'de> Deserialize<'de> for PrettyPrintRecordKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize to bytes first
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        // Then use the bytes to create a RecordKey and wrap it in PrettyPrintRecordKey
        Ok(PrettyPrintRecordKey(RecordKey::new(&bytes)))
    }
}
// seamless conversion from `kad::RecordKey` to `PrettyPrintRecordKey`
impl From<RecordKey> for PrettyPrintRecordKey {
    fn from(key: RecordKey) -> Self {
        PrettyPrintRecordKey(key)
    }
}

impl std::fmt::Display for PrettyPrintRecordKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b: Vec<u8> = self.0.as_ref().to_vec();
        let record_key_b = Bytes::from(b);
        write!(f, "{:64x}", record_key_b)
    }
}

impl std::fmt::Debug for PrettyPrintRecordKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
