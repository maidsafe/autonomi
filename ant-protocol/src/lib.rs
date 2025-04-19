// Copyright 2024 MaidSafe.net limited.
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
/// Helpers for antnode
pub mod node;
/// RPC commands to node
pub mod node_rpc;
/// Storage types for GraphEntry and Chunk
pub mod storage;
/// Network versioning
pub mod version;

// this includes code generated from .proto files
#[expect(clippy::unwrap_used, clippy::clone_on_ref_ptr)]
#[cfg(feature = "rpc")]
pub mod antnode_proto {
    tonic::include_proto!("antnode_proto");
}
pub use error::Error;
pub use error::Error as NetworkError;

use self::storage::{ChunkAddress, GraphEntryAddress, PointerAddress, ScratchpadAddress};

/// Re-export of Bytes used throughout the protocol
pub use bytes::Bytes;

use libp2p::{
    kad::{KBucketDistance as Distance, KBucketKey as Key, RecordKey},
    multiaddr::Protocol,
    Multiaddr, PeerId,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Cow,
    fmt::{self, Debug, Display, Formatter, Write},
};

/// The maximum number of peers to return in a `GetClosestPeers` response.
/// This is the group size used in safe network protocol to be responsible for
/// an item in the network.
/// The peer should be present among the CLOSE_GROUP_SIZE if we're fetching the close_group(peer)
/// The size has been set to 5 for improved performance.
pub const CLOSE_GROUP_SIZE: usize = 5;

/// Returns the UDP port from the provided MultiAddr.
pub fn get_port_from_multiaddr(multi_addr: &Multiaddr) -> Option<u16> {
    // assuming the listening addr contains /ip4/127.0.0.1/udp/56215/quic-v1/p2p/<peer_id>
    for protocol in multi_addr.iter() {
        if let Protocol::Udp(port) = protocol {
            return Some(port);
        }
    }
    None
}

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
    PeerId(Bytes),
    /// The NetworkAddress is representing a ChunkAddress.
    ChunkAddress(ChunkAddress),
    /// The NetworkAddress is representing a GraphEntryAddress.
    GraphEntryAddress(GraphEntryAddress),
    /// The NetworkAddress is representing a ScratchpadAddress.
    ScratchpadAddress(ScratchpadAddress),
    /// The NetworkAddress is representing a PointerAddress.
    PointerAddress(PointerAddress),
    /// The NetworkAddress is representing a RecordKey.
    RecordKey(Bytes),
}

impl NetworkAddress {
    /// Return a `NetworkAddress` representation of the `ChunkAddress`.
    pub fn from_chunk_address(chunk_address: ChunkAddress) -> Self {
        NetworkAddress::ChunkAddress(chunk_address)
    }

    /// Return a `NetworkAddress` representation of the `GraphEntryAddress`.
    pub fn from_graph_entry_address(graph_entry_address: GraphEntryAddress) -> Self {
        NetworkAddress::GraphEntryAddress(graph_entry_address)
    }

    /// Return a `NetworkAddress` representation of the `GraphEntryAddress`.
    pub fn from_scratchpad_address(address: ScratchpadAddress) -> Self {
        NetworkAddress::ScratchpadAddress(address)
    }

    /// Return a `NetworkAddress` representation of the `PeerId` by encapsulating its bytes.
    pub fn from_peer(peer_id: PeerId) -> Self {
        NetworkAddress::PeerId(Bytes::from(peer_id.to_bytes()))
    }

    /// Return a `NetworkAddress` representation of the `RecordKey` by encapsulating its bytes.
    pub fn from_record_key(record_key: &RecordKey) -> Self {
        NetworkAddress::RecordKey(Bytes::copy_from_slice(record_key.as_ref()))
    }

    /// Return a `NetworkAddress` representation of the `PointerAddress`.
    pub fn from_pointer_address(pointer_address: PointerAddress) -> Self {
        NetworkAddress::PointerAddress(pointer_address)
    }

    /// Return the encapsulated bytes of this `NetworkAddress`.
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            NetworkAddress::PeerId(bytes) | NetworkAddress::RecordKey(bytes) => bytes.to_vec(),
            NetworkAddress::ChunkAddress(chunk_address) => chunk_address.xorname().to_vec(),
            NetworkAddress::GraphEntryAddress(graph_entry_address) => {
                graph_entry_address.xorname().to_vec()
            }
            NetworkAddress::ScratchpadAddress(addr) => addr.xorname().to_vec(),
            NetworkAddress::PointerAddress(pointer_address) => pointer_address.xorname().to_vec(),
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
            NetworkAddress::GraphEntryAddress(graph_entry_address) => {
                RecordKey::new(&graph_entry_address.xorname())
            }
            NetworkAddress::PointerAddress(pointer_address) => {
                RecordKey::new(&pointer_address.xorname())
            }
            NetworkAddress::ScratchpadAddress(addr) => RecordKey::new(&addr.xorname()),
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
}

impl Debug for NetworkAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name_str = match self {
            NetworkAddress::PeerId(_) => {
                if let Some(peer_id) = self.as_peer_id() {
                    format!("NetworkAddress::{peer_id:?} - (")
                } else {
                    "NetworkAddress::PeerId(".to_string()
                }
            }
            NetworkAddress::ChunkAddress(chunk_address) => {
                format!(
                    "NetworkAddress::ChunkAddress({}) - (",
                    &chunk_address.to_hex()
                )
            }
            NetworkAddress::GraphEntryAddress(graph_entry_address) => {
                format!(
                    "NetworkAddress::GraphEntryAddress({}) - (",
                    &graph_entry_address.to_hex()
                )
            }
            NetworkAddress::ScratchpadAddress(scratchpad_address) => {
                format!(
                    "NetworkAddress::ScratchpadAddress({}) - (",
                    &scratchpad_address.to_hex()
                )
            }
            NetworkAddress::PointerAddress(pointer_address) => {
                format!(
                    "NetworkAddress::PointerAddress({}) - (",
                    &pointer_address.to_hex()
                )
            }
            NetworkAddress::RecordKey(bytes) => {
                format!("NetworkAddress::RecordKey({:?}) - (", hex::encode(bytes))
            }
        };

        write!(
            f,
            "{name_str}{:?})",
            PrettyPrintKBucketKey(self.as_kbucket_key())
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
                write!(f, "NetworkAddress::ChunkAddress({addr})")
            }
            NetworkAddress::GraphEntryAddress(addr) => {
                write!(f, "NetworkAddress::GraphEntryAddress({addr})")
            }
            NetworkAddress::ScratchpadAddress(addr) => {
                write!(f, "NetworkAddress::ScratchpadAddress({addr})")
            }
            NetworkAddress::RecordKey(key) => {
                write!(f, "NetworkAddress::RecordKey({})", hex::encode(key))
            }
            NetworkAddress::PointerAddress(addr) => {
                write!(f, "NetworkAddress::PointerAddress({addr})")
            }
        }
    }
}

/// Pretty print a `kad::KBucketKey` as a hex string.
#[derive(Clone)]
pub struct PrettyPrintKBucketKey(pub Key<Vec<u8>>);

impl std::fmt::Display for PrettyPrintKBucketKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0.hashed_bytes() {
            f.write_fmt(format_args!("{byte:02x}"))?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for PrettyPrintKBucketKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
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

impl Serialize for PrettyPrintRecordKey<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let record_key_bytes = match &self.key {
            Cow::Borrowed(borrowed_key) => borrowed_key.as_ref(),
            Cow::Owned(owned_key) => owned_key.as_ref(),
        };
        record_key_bytes.serialize(serializer)
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

impl PrettyPrintRecordKey<'_> {
    /// Creates a owned version that can be then used to pass as error values.
    /// Do not call this if you just want to print/log `PrettyPrintRecordKey`
    pub fn into_owned(self) -> PrettyPrintRecordKey<'static> {
        let cloned_key = match self.key {
            Cow::Borrowed(key) => Cow::Owned(key.clone()),
            Cow::Owned(key) => Cow::Owned(key),
        };

        PrettyPrintRecordKey { key: cloned_key }
    }

    pub fn no_kbucket_log(self) -> String {
        let mut content = String::from("");
        let record_key_bytes = match &self.key {
            Cow::Borrowed(borrowed_key) => borrowed_key.as_ref(),
            Cow::Owned(owned_key) => owned_key.as_ref(),
        };
        for byte in record_key_bytes {
            let _ = content.write_fmt(format_args!("{byte:02x}"));
        }
        content
    }
}

impl std::fmt::Display for PrettyPrintRecordKey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let record_key_bytes = match &self.key {
            Cow::Borrowed(borrowed_key) => borrowed_key.as_ref(),
            Cow::Owned(owned_key) => owned_key.as_ref(),
        };
        // print the first 6 chars
        for byte in record_key_bytes.iter().take(3) {
            f.write_fmt(format_args!("{byte:02x}"))?;
        }

        write!(
            f,
            "({:?})",
            PrettyPrintKBucketKey(NetworkAddress::from_record_key(&self.key).as_kbucket_key())
        )
    }
}

impl std::fmt::Debug for PrettyPrintRecordKey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // same as display
        write!(f, "{self}")
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::GraphEntryAddress;
    use crate::NetworkAddress;

    #[test]
    fn verify_graph_entry_addr_is_actionable() {
        let pk = bls::SecretKey::random().public_key();
        let graph_entry_addr = GraphEntryAddress::new(pk);
        let net_addr = NetworkAddress::from_graph_entry_address(graph_entry_addr);

        let graph_entry_addr_hex = &graph_entry_addr.to_hex();
        let net_addr_fmt = format!("{net_addr}");
        let net_addr_dbg = format!("{net_addr:?}");

        assert!(net_addr_fmt.contains(graph_entry_addr_hex));
        assert!(net_addr_dbg.contains(graph_entry_addr_hex));
    }
}
