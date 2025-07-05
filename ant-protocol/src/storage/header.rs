// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::Error;
use crate::PrettyPrintRecordKey;
use bytes::{BufMut, Bytes, BytesMut};
use libp2p::kad::Record;
use prometheus_client::encoding::EncodeLabelValue;
use rmp_serde::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use xor_name::XorName;

/// Data types that natively suppported by autonomi network.
#[derive(EncodeLabelValue, Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Hash)]
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
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
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
}

/// Allowing 10 data types to be defined, leaving margin for future.
pub const RECORD_KIND_PAYMENT_STARTING_INDEX: u32 = 10;

impl Serialize for RecordKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let index = match self {
            Self::DataOnly(ref data_types) => data_types.get_index(),
            Self::DataWithPayment(ref data_types) => {
                RECORD_KIND_PAYMENT_STARTING_INDEX + data_types.get_index()
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
        let data_type_index = if num < RECORD_KIND_PAYMENT_STARTING_INDEX {
            num
        } else {
            num - RECORD_KIND_PAYMENT_STARTING_INDEX
        };

        if let Some(data_type) = DataTypes::from_index(data_type_index) {
            if num < RECORD_KIND_PAYMENT_STARTING_INDEX {
                Ok(Self::DataOnly(data_type))
            } else {
                Ok(Self::DataWithPayment(data_type))
            }
        } else {
            Err(serde::de::Error::custom(format!(
                "Unexpected index {num} for RecordKind variant",
            )))
        }
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
            RecordKind::DataOnly(data_type) | RecordKind::DataWithPayment(data_type) => {
                Ok(data_type)
            }
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

        Ok(())
    }

    #[test]
    fn test_record_kind_serialization() -> Result<()> {
        let kinds = vec![
            RecordKind::DataOnly(DataTypes::Chunk),
            RecordKind::DataWithPayment(DataTypes::Chunk),
            RecordKind::DataOnly(DataTypes::GraphEntry),
            RecordKind::DataWithPayment(DataTypes::GraphEntry),
            RecordKind::DataOnly(DataTypes::Scratchpad),
            RecordKind::DataWithPayment(DataTypes::Scratchpad),
            RecordKind::DataOnly(DataTypes::Pointer),
            RecordKind::DataWithPayment(DataTypes::Pointer),
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
}
