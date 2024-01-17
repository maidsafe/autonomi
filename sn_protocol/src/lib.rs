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
use libp2p::{
    kad::{KBucketDistance as Distance, KBucketKey as Key, RecordKey},
    PeerId,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    fmt::{self, Debug, Display, Formatter},
};
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
    pub fn from_record_key(record_key: &RecordKey) -> Self {
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
            NetworkAddress::PeerId(_) => {
                if let Some(peer_id) = self.as_peer_id() {
                    format!("NetworkAddress::PeerId({peer_id} - ")
                } else {
                    "NetworkAddress::PeerId(".to_string()
                }
            }
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
            NetworkAddress::RecordKey(bytes) => format!(
                "NetworkAddress::RecordKey({:?} - ",
                PrettyPrintRecordKey::from(&RecordKey::new(bytes))
            ),
        };
        write!(
            f,
            "{name_str}{:?})",
            PrettyPrintKBucketKey(self.as_kbucket_key()),
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

/// Pretty print a `kad::KBucketKey` as a hex string.
#[derive(Clone)]
pub struct PrettyPrintKBucketKey(pub Key<Vec<u8>>);

impl std::fmt::Display for PrettyPrintKBucketKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The `KeyBytes` part of `KBucketKey` is private and no API to expose it.
        // Hence here we have to carry out a hash manually to simulate its behaviour.
        let generic_array = Sha256::digest(self.0.preimage());
        for byte in generic_array {
            f.write_fmt(format_args!("{:02x}", byte))?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for PrettyPrintKBucketKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

/// Provides a hex representation of a `kad::RecordKey`.
///
/// This internally stores the RecordKey as a `Cow` type. Use `PrettyPrintRecordKey::from(&RecordKey)` to create a
/// borrowed version for printing/logging.
/// To use in error messages, to pass to other functions, call `PrettyPrintRecordKey::from(&RecordKey).into_owned()` to
///  obtain a cloned, non-referenced `RecordKey`.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct PrettyPrintRecordKey<'a> {
    key: Cow<'a, RecordKey>,
}

impl<'a> Serialize for PrettyPrintRecordKey<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Use the `to_vec` function of the inner RecordKey to get the bytes
        // and then serialize those bytes
        self.key.to_vec().serialize(serializer)
    }
}

// Implementing Deserialize for PrettyPrintRecordKey
impl<'de> Deserialize<'de> for PrettyPrintRecordKey<'static> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize to bytes first
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        // Then use the bytes to create a RecordKey and wrap it in PrettyPrintRecordKey
        Ok(PrettyPrintRecordKey {
            key: Cow::Owned(RecordKey::new(&bytes)),
        })
    }
}
/// This is the only interface to create a PrettyPrintRecordKey.
/// `.into_owned()` must be called explicitly if you want a Owned version to be used for errors/args.
impl<'a> From<&'a RecordKey> for PrettyPrintRecordKey<'a> {
    fn from(key: &'a RecordKey) -> Self {
        PrettyPrintRecordKey {
            key: Cow::Borrowed(key),
        }
    }
}

impl<'a> PrettyPrintRecordKey<'a> {
    /// Creates a owned version that can be then used to pass as error values.
    /// Do not call this if you just want to print/log `PrettyPrintRecordKey`
    pub fn into_owned(self) -> PrettyPrintRecordKey<'static> {
        let cloned_key = match self.key {
            Cow::Borrowed(key) => Cow::Owned(key.clone()),
            Cow::Owned(key) => Cow::Owned(key),
        };

        PrettyPrintRecordKey { key: cloned_key }
    }
}

impl<'a> std::fmt::Display for PrettyPrintRecordKey<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let record_key_bytes = match &self.key {
            Cow::Borrowed(borrowed_key) => borrowed_key.as_ref(),
            Cow::Owned(owned_key) => owned_key.as_ref(),
        };
        for byte in record_key_bytes {
            f.write_fmt(format_args!("{:02x}", byte))?;
        }

        write!(
            f,
            "({:?})",
            PrettyPrintKBucketKey(NetworkAddress::from_record_key(&self.key).as_kbucket_key())
        )
    }
}

impl<'a> std::fmt::Debug for PrettyPrintRecordKey<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

#[cfg(test)]
mod tests {
    use crate::{NetworkAddress, PrettyPrintRecordKey};
    use bls::rand::thread_rng;
    use bytes::Bytes;
    use libp2p::kad::{KBucketKey, RecordKey};
    use sha2::{Digest, Sha256};

    // A struct that implements hex representation of RecordKey using `bytes::Bytes`
    struct OldRecordKeyPrint(RecordKey);

    // old impl using Bytes
    impl std::fmt::Display for OldRecordKeyPrint {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let b: Vec<u8> = self.0.as_ref().to_vec();
            let record_key_b = Bytes::from(b);
            write!(
                f,
                "{:64x}({:?})",
                record_key_b,
                OldKBucketKeyPrint(NetworkAddress::from_record_key(&self.0).as_kbucket_key())
            )
        }
    }

    impl std::fmt::Debug for OldRecordKeyPrint {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self)
        }
    }

    // A struct that implements hex representation of KBucketKey using `bytes::Bytes`
    pub struct OldKBucketKeyPrint(KBucketKey<Vec<u8>>);

    // old impl using Bytes
    impl std::fmt::Display for OldKBucketKeyPrint {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let generic_array = Sha256::digest(self.0.preimage());
            let kbucket_key_b = Bytes::from(generic_array.to_vec());
            write!(f, "{:64x}", kbucket_key_b)
        }
    }

    impl std::fmt::Debug for OldKBucketKeyPrint {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self)
        }
    }

    #[test]
    fn verify_custom_hex_representation() {
        let random = xor_name::XorName::random(&mut thread_rng());
        let key = RecordKey::new(&random.0);
        let pretty_key = PrettyPrintRecordKey::from(&key).into_owned();
        let old_record_key = OldRecordKeyPrint(key);

        assert_eq!(format!("{pretty_key:?}"), format!("{old_record_key:?}"));
    }
}
