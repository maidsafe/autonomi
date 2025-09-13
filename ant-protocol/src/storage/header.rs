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
}

/// Allowing 10 data types to be defined, leaving margin for future.
pub const RECORD_KIND_PAYMENT_STARTING_INDEX: u32 = 10;

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

/// Utility to serialize a record with PaymentProofType (supports both EVM and native token payments)
/// Returns Bytes to avoid accidental clone allocations
pub fn try_serialize_record_with_payment<T: serde::Serialize>(
    payment_proof: crate::storage::PaymentProofType,
    data: &T,
    record_kind: RecordKind,
) -> Result<Bytes, Error> {
    let combined_data = (payment_proof, data);
    try_serialize_record(&combined_data, record_kind)
}

/// Utility to deserialize a record with backward compatible payment proof parsing
/// This function first tries to parse as PaymentProofType (new format), then falls back to ProofOfPayment (old format)
#[cfg(feature = "evm-integration")]
pub fn try_deserialize_record_with_payment<T: serde::de::DeserializeOwned>(
    record: &Record,
) -> Result<(crate::storage::PaymentProofType, T), Error> {
    use crate::storage::PaymentProofType;
    
    // First try new format with PaymentProofType
    if let Ok((proof_type, data)) = try_deserialize_record::<(PaymentProofType, T)>(record) {
        return Ok((proof_type, data));
    }

    // Fallback to old format for backward compatibility
    if let Ok((old_proof, data)) = try_deserialize_record::<(ant_evm::ProofOfPayment, T)>(record) {
        return Ok((PaymentProofType::Evm(old_proof), data));
    }

    // If both formats fail, return the original error
    Err(Error::RecordParsingFailed)
}

/// Utility to deserialize a record with native token payment (native-only version)
/// This function works without the evm-integration feature
#[cfg(not(feature = "evm-integration"))]
pub fn try_deserialize_record_with_payment<T: serde::de::DeserializeOwned>(
    record: &Record,
) -> Result<(crate::storage::PaymentProofType, T), Error> {
    use crate::storage::PaymentProofType;
    
    // Only try PaymentProofType format (which only supports Native when evm-integration is disabled)
    if let Ok((proof_type, data)) = try_deserialize_record::<(PaymentProofType, T)>(record) {
        return Ok((proof_type, data));
    }

    // If parsing fails, return error
    Err(Error::RecordParsingFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::RecordKey;

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

    #[test]
    fn test_payment_proof_type_serialization() -> Result<()> {
        use crate::storage::{NativePaymentProof, NativeTokens, PaymentProofType};
        use bls::SecretKey;

        // Create a test native payment proof
        let native_proof = NativePaymentProof::new(
            SecretKey::random().public_key(),
            [0x11; 16],
            NativeTokens::from_u64(1000),
            [0x12, 0x34, 0x56, 0x78],
        );

        let payment_proof = PaymentProofType::Native(native_proof.clone());
        let test_data = "test_data_content";

        // Test serialization with PaymentProofType
        let serialized = try_serialize_record_with_payment(
            payment_proof,
            &test_data,
            RecordKind::DataWithPayment(DataTypes::GraphEntry),
        )?;

        // Create a record for deserialization testing
        let record = Record {
            key: RecordKey::new(b"test_key"),
            value: serialized.to_vec(),
            publisher: None,
            expires: None,
        };

        // Test deserialization with PaymentProofType
        let (deserialized_proof, deserialized_data) = 
            try_deserialize_record_with_payment::<String>(&record)?;

        // Verify the data round-tripped correctly
        assert_eq!(deserialized_data, test_data);
        
        // Verify the payment proof round-tripped correctly
        match deserialized_proof {
            PaymentProofType::Native(proof) => {
                assert_eq!(proof.payment_transaction, native_proof.payment_transaction);
                assert_eq!(proof.expected_amount, native_proof.expected_amount);
                assert_eq!(proof.recipient_derivation_index, native_proof.recipient_derivation_index);
                assert_eq!(proof.record_key_hash, native_proof.record_key_hash);
            },
            #[cfg(feature = "evm-integration")]
            PaymentProofType::Evm(_) => {
                panic!("Expected native payment proof, got EVM proof");
            }
        }

        Ok(())
    }

    #[test]
    #[cfg(feature = "evm-integration")]
    fn test_backward_compatible_deserialization() -> Result<()> {
        use crate::storage::PaymentProofType;
        use ant_evm::ProofOfPayment;

        // Create a mock EVM ProofOfPayment (simplified for testing)
        let evm_proof = ProofOfPayment {
            peer_quotes: Vec::new(),
        }; // Empty proof for testing
        let test_data = "legacy_test_data";

        // Serialize using the old format (ProofOfPayment directly)
        let serialized = try_serialize_record(
            &(evm_proof.clone(), test_data),
            RecordKind::DataWithPayment(DataTypes::Chunk),
        )?;

        // Create a record for deserialization testing
        let record = Record {
            key: RecordKey::new(b"legacy_test_key"),
            value: serialized.to_vec(),
            publisher: None,
            expires: None,
        };

        // Test that the new deserialization function can handle old format
        let (deserialized_proof, deserialized_data) = 
            try_deserialize_record_with_payment::<String>(&record)?;

        // Verify the data round-tripped correctly
        assert_eq!(deserialized_data, test_data);
        
        // Verify the payment proof was converted to PaymentProofType::Evm
        match deserialized_proof {
            PaymentProofType::Evm(_proof) => {
                // Basic verification that we got an EVM proof
                // In a real test, we'd verify the proof contents
                // Test passed - EVM proof was correctly deserialized
            },
            PaymentProofType::Native(_) => {
                panic!("Expected EVM payment proof, got native proof");
            }
        }

        Ok(())
    }

    #[test]
    fn test_payment_proof_type_methods() {
        use crate::storage::{NativePaymentProof, NativeTokens, PaymentProofType};
        use bls::SecretKey;

        // Test native payment proof
        let native_proof = NativePaymentProof::new(
            SecretKey::random().public_key(),
            [0x22; 16],
            NativeTokens::from_u64(2000),
            [0xAB, 0xCD, 0xEF, 0x12],
        );

        let payment_proof = PaymentProofType::Native(native_proof.clone());

        // Test type checking methods
        assert!(payment_proof.is_native());
        
        #[cfg(feature = "evm-integration")]
        assert!(!payment_proof.is_evm());

        // Test getter methods
        assert_eq!(payment_proof.as_native(), Some(&native_proof));
        
        #[cfg(feature = "evm-integration")]
        assert_eq!(payment_proof.as_evm(), None);

        // Test conversion methods
        let converted_native = payment_proof.clone().into_native();
        assert_eq!(converted_native, Some(native_proof));

        #[cfg(feature = "evm-integration")]
        {
            let converted_evm = payment_proof.into_evm();
            assert_eq!(converted_evm, None);
        }
    }
}
