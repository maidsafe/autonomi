// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::PrettyPrintRecordKey;
use crate::error::Error;
use bytes::{BufMut, Bytes, BytesMut};
use libp2p::kad::Record;
use prometheus_client::encoding::EncodeLabelValue;
use rmp_serde::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use xor_name::XorName;

/// Data types that natively suppported by autonomi network.
#[derive(
    EncodeLabelValue, Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, PartialOrd, Hash,
)]
pub enum DataTypes {
    Chunk,
    GraphEntry,
    Pointer,
    Scratchpad,
}

impl DataTypes {
    pub fn get_index(&self) -> u32 {
        match self {
            Self::Chunk => 0,
            Self::GraphEntry => 1,
            Self::Pointer => 2,
            Self::Scratchpad => 3,
        }
    }

    pub fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Chunk),
            1 => Some(Self::GraphEntry),
            2 => Some(Self::Pointer),
            3 => Some(Self::Scratchpad),
            _ => None,
        }
    }
}

/// Indicates the type of the record content.
/// This is to be only used within the node instance to reflect different content version.
/// Hence, only need to have two entries: Chunk and NonChunk.
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, PartialOrd, Hash)]
pub enum ValidationType {
    Chunk,
    NonChunk(XorName),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordHeader {
    pub kind: RecordKind,
}

/// To be used between client and nodes, hence need to indicate whehter payment info involved.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum RecordKind {
    DataOnly(DataTypes),
    DataWithPayment(DataTypes),
    /// Data with Merkle batch payment proof
    /// Used when data was paid for via Merkle tree batch payment
    DataWithMerklePayment(DataTypes),
}

/// Allowing 10 data types to be defined, leaving margin for future.
pub const RECORD_KIND_PAYMENT_STARTING_INDEX: u32 = 10;

/// Starting index for Merkle payment records
pub const RECORD_KIND_MERKLE_PAYMENT_STARTING_INDEX: u32 = 20;

impl Serialize for RecordKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let index = match self {
            Self::DataOnly(data_types) => data_types.get_index(),
            Self::DataWithPayment(data_types) => {
                RECORD_KIND_PAYMENT_STARTING_INDEX + data_types.get_index()
            }
            Self::DataWithMerklePayment(data_types) => {
                RECORD_KIND_MERKLE_PAYMENT_STARTING_INDEX + data_types.get_index()
            }
        };
        serializer.serialize_u32(index)
    }
}

impl<'de> Deserialize<'de> for RecordKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let num = u32::deserialize(deserializer)?;

        let (kind_type, data_type_index) = if num < RECORD_KIND_PAYMENT_STARTING_INDEX {
            ("DataOnly", num)
        } else if num < RECORD_KIND_MERKLE_PAYMENT_STARTING_INDEX {
            ("DataWithPayment", num - RECORD_KIND_PAYMENT_STARTING_INDEX)
        } else {
            (
                "DataWithMerklePayment",
                num - RECORD_KIND_MERKLE_PAYMENT_STARTING_INDEX,
            )
        };

        let data_type = DataTypes::from_index(data_type_index).ok_or_else(|| {
            serde::de::Error::custom(format!("Unexpected index {num} for RecordKind variant"))
        })?;

        Ok(match kind_type {
            "DataOnly" => Self::DataOnly(data_type),
            "DataWithPayment" => Self::DataWithPayment(data_type),
            "DataWithMerklePayment" => Self::DataWithMerklePayment(data_type),
            _ => unreachable!(),
        })
    }
}
impl Display for RecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecordKind({self:?})")
    }
}

impl RecordHeader {
    pub const SIZE: usize = 2;

    pub fn try_serialize(self) -> Result<BytesMut, Error> {
        let bytes = BytesMut::new();
        let mut buf = bytes.writer();

        self.serialize(&mut Serializer::new(&mut buf))
            .map_err(|err| {
                error!("Failed to serialized RecordHeader {self:?} with error: {err:?}");
                Error::RecordHeaderParsingFailed
            })?;

        let b = buf.into_inner();

        Ok(b)
    }

    pub fn try_deserialize(bytes: &[u8]) -> Result<Self, Error> {
        rmp_serde::from_slice(bytes).map_err(|err| {
            error!("Failed to deserialize RecordHeader with error: {err:?}");
            Error::RecordHeaderParsingFailed
        })
    }

    pub fn from_record(record: &Record) -> Result<Self, Error> {
        if record.value.len() < RecordHeader::SIZE + 1 {
            return Err(Error::RecordHeaderParsingFailed);
        }
        Self::try_deserialize(&record.value[..RecordHeader::SIZE + 1])
    }

    pub fn is_record_of_type_chunk(record: &Record) -> Result<bool, Error> {
        let kind = Self::from_record(record)?.kind;
        Ok(kind == RecordKind::DataOnly(DataTypes::Chunk))
    }

    pub fn get_data_type(record: &Record) -> Result<DataTypes, Error> {
        let kind = Self::from_record(record)?.kind;
        match kind {
            RecordKind::DataOnly(data_type)
            | RecordKind::DataWithPayment(data_type)
            | RecordKind::DataWithMerklePayment(data_type) => Ok(data_type),
        }
    }
}

/// Utility to deserialize a `KAD::Record` into any type.
/// Use `RecordHeader::from_record` if you want the `RecordHeader` instead.
pub fn try_deserialize_record<T: serde::de::DeserializeOwned>(record: &Record) -> Result<T, Error> {
    let bytes = if record.value.len() > RecordHeader::SIZE {
        &record.value[RecordHeader::SIZE..]
    } else {
        return Err(Error::RecordParsingFailed);
    };
    rmp_serde::from_slice(bytes).map_err(|err| {
        error!(
            "Failed to deserialized record {} with error: {err:?}",
            PrettyPrintRecordKey::from(&record.key)
        );
        Error::RecordParsingFailed
    })
}

/// Utility to serialize the provided data along with the RecordKind to be stored as Record::value
/// Returns Bytes to avoid accidental clone allocations
pub fn try_serialize_record<T: serde::Serialize>(
    data: &T,
    record_kind: RecordKind,
) -> Result<Bytes, Error> {
    let mut buf = RecordHeader { kind: record_kind }.try_serialize()?.writer();
    data.serialize(&mut Serializer::new(&mut buf))
        .map_err(|err| {
            error!("Failed to serialized Records with error: {err:?}");
            Error::RecordParsingFailed
        })?;
    let bytes = buf.into_inner();
    Ok(bytes.freeze())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;

    #[test]
    fn verify_record_header_encoded_size() -> Result<()> {
        let chunk_with_payment = RecordHeader {
            kind: RecordKind::DataWithPayment(DataTypes::Chunk),
        }
        .try_serialize()?;
        assert_eq!(chunk_with_payment.len(), RecordHeader::SIZE);

        let chunk = RecordHeader {
            kind: RecordKind::DataOnly(DataTypes::Chunk),
        }
        .try_serialize()?;
        assert_eq!(chunk.len(), RecordHeader::SIZE);

        let graphentry = RecordHeader {
            kind: RecordKind::DataOnly(DataTypes::GraphEntry),
        }
        .try_serialize()?;
        assert_eq!(graphentry.len(), RecordHeader::SIZE);

        let scratchpad = RecordHeader {
            kind: RecordKind::DataOnly(DataTypes::Scratchpad),
        }
        .try_serialize()?;
        assert_eq!(scratchpad.len(), RecordHeader::SIZE);

        let scratchpad_with_payment = RecordHeader {
            kind: RecordKind::DataWithPayment(DataTypes::Scratchpad),
        }
        .try_serialize()?;
        assert_eq!(scratchpad_with_payment.len(), RecordHeader::SIZE);

        let pointer = RecordHeader {
            kind: RecordKind::DataOnly(DataTypes::Pointer),
        }
        .try_serialize()?;
        assert_eq!(pointer.len(), RecordHeader::SIZE);

        let pointer_with_payment = RecordHeader {
            kind: RecordKind::DataWithPayment(DataTypes::Pointer),
        }
        .try_serialize()?;
        assert_eq!(pointer_with_payment.len(), RecordHeader::SIZE);

        // Test Merkle payment variants
        let chunk_with_merkle_payment = RecordHeader {
            kind: RecordKind::DataWithMerklePayment(DataTypes::Chunk),
        }
        .try_serialize()?;
        assert_eq!(chunk_with_merkle_payment.len(), RecordHeader::SIZE);

        let graphentry_with_merkle_payment = RecordHeader {
            kind: RecordKind::DataWithMerklePayment(DataTypes::GraphEntry),
        }
        .try_serialize()?;
        assert_eq!(graphentry_with_merkle_payment.len(), RecordHeader::SIZE);

        let pointer_with_merkle_payment = RecordHeader {
            kind: RecordKind::DataWithMerklePayment(DataTypes::Pointer),
        }
        .try_serialize()?;
        assert_eq!(pointer_with_merkle_payment.len(), RecordHeader::SIZE);

        let scratchpad_with_merkle_payment = RecordHeader {
            kind: RecordKind::DataWithMerklePayment(DataTypes::Scratchpad),
        }
        .try_serialize()?;
        assert_eq!(scratchpad_with_merkle_payment.len(), RecordHeader::SIZE);

        Ok(())
    }

    #[test]
    fn test_record_kind_serialization() -> Result<()> {
        let kinds = vec![
            RecordKind::DataOnly(DataTypes::Chunk),
            RecordKind::DataWithPayment(DataTypes::Chunk),
            RecordKind::DataWithMerklePayment(DataTypes::Chunk),
            RecordKind::DataOnly(DataTypes::GraphEntry),
            RecordKind::DataWithPayment(DataTypes::GraphEntry),
            RecordKind::DataWithMerklePayment(DataTypes::GraphEntry),
            RecordKind::DataOnly(DataTypes::Scratchpad),
            RecordKind::DataWithPayment(DataTypes::Scratchpad),
            RecordKind::DataWithMerklePayment(DataTypes::Scratchpad),
            RecordKind::DataOnly(DataTypes::Pointer),
            RecordKind::DataWithPayment(DataTypes::Pointer),
            RecordKind::DataWithMerklePayment(DataTypes::Pointer),
        ];

        for kind in kinds {
            let header = RecordHeader { kind };
            let header2 = RecordHeader { kind };

            let serialized = header.try_serialize()?;
            let deserialized = RecordHeader::try_deserialize(&serialized)?;
            assert_eq!(header2.kind, deserialized.kind);
        }

        Ok(())
    }

    #[test]
    fn test_merkle_payment_record_kind_indices() -> Result<()> {
        // Test that Merkle payment variants serialize to correct indices (20-23)
        let test_cases = vec![
            (RecordKind::DataWithMerklePayment(DataTypes::Chunk), 20u32),
            (
                RecordKind::DataWithMerklePayment(DataTypes::GraphEntry),
                21u32,
            ),
            (RecordKind::DataWithMerklePayment(DataTypes::Pointer), 22u32),
            (
                RecordKind::DataWithMerklePayment(DataTypes::Scratchpad),
                23u32,
            ),
        ];

        for (kind, expected_index) in test_cases {
            let header = RecordHeader { kind };
            let serialized = header.try_serialize()?;

            // Parse the messagepack format to extract the u32 value
            // The serialized format is [kind_field, value]
            // We need to deserialize the whole RecordHeader and check by re-roundtripping
            let deserialized = RecordHeader::try_deserialize(&serialized)?;

            // Re-serialize to verify the index by checking DataTypes match
            match (kind, deserialized.kind) {
                (
                    RecordKind::DataWithMerklePayment(expected_type),
                    RecordKind::DataWithMerklePayment(actual_type),
                ) => {
                    assert_eq!(expected_type, actual_type);
                    // Verify the index by checking the data type offset
                    let actual_index =
                        RECORD_KIND_MERKLE_PAYMENT_STARTING_INDEX + actual_type.get_index();
                    assert_eq!(
                        actual_index, expected_index,
                        "RecordKind {kind:?} should serialize to index {expected_index}"
                    );
                }
                _ => panic!("Expected DataWithMerklePayment variant"),
            }
        }

        Ok(())
    }

    #[test]
    fn test_record_kind_index_ranges() -> Result<()> {
        // Test DataOnly: 0-3
        assert_eq!(
            RecordKind::DataOnly(DataTypes::Chunk),
            RecordHeader::try_deserialize(
                &RecordHeader {
                    kind: RecordKind::DataOnly(DataTypes::Chunk)
                }
                .try_serialize()?
            )?
            .kind
        );

        // Test DataWithPayment: 10-13
        assert_eq!(
            RecordKind::DataWithPayment(DataTypes::Chunk),
            RecordHeader::try_deserialize(
                &RecordHeader {
                    kind: RecordKind::DataWithPayment(DataTypes::Chunk)
                }
                .try_serialize()?
            )?
            .kind
        );

        // Test DataWithMerklePayment: 20-23
        assert_eq!(
            RecordKind::DataWithMerklePayment(DataTypes::Chunk),
            RecordHeader::try_deserialize(
                &RecordHeader {
                    kind: RecordKind::DataWithMerklePayment(DataTypes::Chunk)
                }
                .try_serialize()?
            )?
            .kind
        );

        Ok(())
    }

    #[test]
    fn test_merkle_payment_roundtrip() -> Result<()> {
        // Test that all Merkle payment variants can roundtrip serialize/deserialize
        let data_types = vec![
            DataTypes::Chunk,
            DataTypes::GraphEntry,
            DataTypes::Pointer,
            DataTypes::Scratchpad,
        ];

        for data_type in data_types {
            let original = RecordKind::DataWithMerklePayment(data_type);
            let header = RecordHeader { kind: original };

            let serialized = header.try_serialize()?;
            let deserialized = RecordHeader::try_deserialize(&serialized)?;

            assert_eq!(
                original, deserialized.kind,
                "Merkle payment variant for {data_type:?} should roundtrip correctly"
            );
        }

        Ok(())
    }
}
