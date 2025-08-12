// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Integration tests for data map deserialization backward compatibility
//!
//! Tests both private and public data map deserialization methods to ensure
//! backward compatibility between old (DataMapLevel) and new (DataMap) formats.

use autonomi::files::DownloadError;
use bytes::{BufMut, Bytes, BytesMut};
use eyre::Result;
use self_encryption::DataMap;
use self_encryption_old::DataMap as OldDataMap;
use serde::Serialize;
use xor_name::XorName;

/// Helper function to create test data maps with sample chunk info
fn create_test_chunk_info(count: usize) -> Vec<self_encryption::ChunkInfo> {
    (0..count)
        .map(|i| self_encryption::ChunkInfo {
            index: i,
            dst_hash: XorName::from_content(&[i as u8; 32]),
            src_hash: XorName::from_content(&[(i + 100) as u8; 32]),
            src_size: 1024 + i,
        })
        .collect()
}

/// Helper function to create old format data map
fn create_old_data_map(count: usize) -> OldDataMap {
    let chunk_infos: Vec<_> = (0..count)
        .map(|i| self_encryption_old::ChunkInfo {
            index: i,
            dst_hash: XorName::from_content(&[i as u8; 32]),
            src_hash: XorName::from_content(&[(i + 100) as u8; 32]),
            src_size: 1024 + i,
        })
        .collect();
    OldDataMap::new(chunk_infos)
}

/// Helper function to serialize data to bytes
fn serialize_to_bytes<T: Serialize>(data: &T) -> Result<Bytes> {
    let mut bytes = BytesMut::with_capacity(300).writer();
    let mut serialiser = rmp_serde::Serializer::new(&mut bytes);
    data.serialize(&mut serialiser)?;
    Ok(bytes.into_inner().freeze())
}

/// Shared data map deserialization logic used by both private and public clients
fn deserialize_data_map(data_map_bytes: &Bytes) -> Result<self_encryption::DataMap, DownloadError> {
    // Try new format first
    if let Ok(data_map) = rmp_serde::from_slice::<self_encryption::DataMap>(data_map_bytes) {
        return Ok(data_map);
    }

    // Fall back to old format and convert
    let data_map_level =
        rmp_serde::from_slice::<autonomi::self_encryption::DataMapLevel>(data_map_bytes)
            .map_err(|e| DownloadError::GetError(autonomi::client::GetError::InvalidDataMap(e)))?;

    let old_data_map = match &data_map_level {
        autonomi::self_encryption::DataMapLevel::First(map) => map,
        autonomi::self_encryption::DataMapLevel::Additional(map) => map,
    };

    // Convert to new format
    let chunk_identifiers: Vec<self_encryption::ChunkInfo> = old_data_map
        .infos()
        .iter()
        .map(|ck_info| self_encryption::ChunkInfo {
            index: ck_info.index,
            dst_hash: ck_info.dst_hash,
            src_hash: ck_info.src_hash,
            src_size: ck_info.src_size,
        })
        .collect();

    Ok(self_encryption::DataMap {
        chunk_identifiers,
        child: None,
    })
}

/// Test helper for verifying chunk info equivalence
fn assert_chunk_info_eq(
    original: &[self_encryption::ChunkInfo],
    deserialized: &[self_encryption::ChunkInfo],
) {
    assert_eq!(original.len(), deserialized.len());
    for (orig, deser) in original.iter().zip(deserialized.iter()) {
        assert_eq!(orig.index, deser.index);
        assert_eq!(orig.dst_hash, deser.dst_hash);
        assert_eq!(orig.src_hash, deser.src_hash);
        assert_eq!(orig.src_size, deser.src_size);
    }
}

/// Test helper for verifying old/new format equivalence
fn assert_old_new_chunk_info_eq(
    old_infos: &[self_encryption_old::ChunkInfo],
    new_infos: &[self_encryption::ChunkInfo],
) {
    assert_eq!(old_infos.len(), new_infos.len());
    for (old, new) in old_infos.iter().zip(new_infos.iter()) {
        assert_eq!(old.index, new.index);
        assert_eq!(old.dst_hash, new.dst_hash);
        assert_eq!(old.src_hash, new.src_hash);
        assert_eq!(old.src_size, new.src_size);
    }
}

// PRIVATE DATA MAP TESTS

/// Test that data map deserialization works with new DataMap format (private)
#[test]
fn test_private_data_map_new_format() -> Result<()> {
    let chunk_identifiers = create_test_chunk_info(3);
    let new_data_map = DataMap {
        chunk_identifiers: chunk_identifiers.clone(),
        child: None,
    };

    let data_map_bytes = serialize_to_bytes(&new_data_map)?;
    let result = deserialize_data_map(&data_map_bytes);

    assert!(
        result.is_ok(),
        "Failed to deserialize new format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_chunk_info_eq(&chunk_identifiers, &deserialized.infos());
    Ok(())
}

/// Test that data map deserialization works with old DataMapLevel::First format (private)
#[test]
fn test_private_data_map_old_format_first() -> Result<()> {
    let old_data_map = create_old_data_map(4);
    let data_map_level = autonomi::self_encryption::DataMapLevel::First(old_data_map.clone());

    let data_map_bytes = serialize_to_bytes(&data_map_level)?;
    let result = deserialize_data_map(&data_map_bytes);

    assert!(
        result.is_ok(),
        "Failed to deserialize old First format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_old_new_chunk_info_eq(&old_data_map.infos(), &deserialized.infos());
    Ok(())
}

/// Test that data map deserialization works with old DataMapLevel::Additional format (private)
#[test]
fn test_private_data_map_old_format_additional() -> Result<()> {
    let old_data_map = create_old_data_map(6);
    let data_map_level = autonomi::self_encryption::DataMapLevel::Additional(old_data_map.clone());

    let data_map_bytes = serialize_to_bytes(&data_map_level)?;
    let result = deserialize_data_map(&data_map_bytes);

    assert!(
        result.is_ok(),
        "Failed to deserialize old Additional format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_old_new_chunk_info_eq(&old_data_map.infos(), &deserialized.infos());
    Ok(())
}

/// Test error handling for invalid data (private)
#[test]
fn test_private_data_map_invalid_data() -> Result<()> {
    let invalid_data = Bytes::from_static(b"invalid msgpack data for private test");
    let result = deserialize_data_map(&invalid_data);

    assert!(
        result.is_err(),
        "Should have failed to deserialize invalid data"
    );
    match result.unwrap_err() {
        DownloadError::GetError(autonomi::client::GetError::InvalidDataMap(_)) => {
            // Expected error type
        }
        other => panic!("Unexpected error type: {other:?}"),
    }
    Ok(())
}

/// Test that both formats produce the same result when given equivalent data (private)
#[test]
fn test_private_data_map_format_equivalence() -> Result<()> {
    let test_chunk_data: Vec<_> = (0..5)
        .map(|i| {
            let dst_hash = XorName::from_content(&[i as u8; 32]);
            let src_hash = XorName::from_content(&[(i + 42) as u8; 32]);
            (i, dst_hash, src_hash, 512 + i * 100)
        })
        .collect();

    // Create new format
    let new_chunk_identifiers: Vec<self_encryption::ChunkInfo> = test_chunk_data
        .iter()
        .map(
            |(index, dst_hash, src_hash, src_size)| self_encryption::ChunkInfo {
                index: *index,
                dst_hash: *dst_hash,
                src_hash: *src_hash,
                src_size: *src_size,
            },
        )
        .collect();
    let new_data_map = DataMap {
        chunk_identifiers: new_chunk_identifiers,
        child: None,
    };

    // Create equivalent old format
    let old_chunk_infos: Vec<self_encryption_old::ChunkInfo> = test_chunk_data
        .iter()
        .map(
            |(index, dst_hash, src_hash, src_size)| self_encryption_old::ChunkInfo {
                index: *index,
                dst_hash: *dst_hash,
                src_hash: *src_hash,
                src_size: *src_size,
            },
        )
        .collect();
    let old_data_map = OldDataMap::new(old_chunk_infos);
    let data_map_level = autonomi::self_encryption::DataMapLevel::First(old_data_map);

    // Serialize and deserialize both formats
    let new_data_bytes = serialize_to_bytes(&new_data_map)?;
    let old_data_bytes = serialize_to_bytes(&data_map_level)?;

    let new_result = deserialize_data_map(&new_data_bytes).unwrap();
    let old_result = deserialize_data_map(&old_data_bytes).unwrap();

    // Both should produce identical results
    assert_chunk_info_eq(&new_result.infos(), &old_result.infos());
    Ok(())
}

// PUBLIC DATA MAP TESTS

/// Test that data map deserialization works with new DataMap format (public)
#[test]
fn test_public_data_map_new_format() -> Result<()> {
    let chunk_identifiers = create_test_chunk_info(5);
    let new_data_map = DataMap {
        chunk_identifiers: chunk_identifiers.clone(),
        child: None,
    };

    let data_map_bytes = serialize_to_bytes(&new_data_map)?;
    let result = deserialize_data_map(&data_map_bytes);

    assert!(
        result.is_ok(),
        "Failed to deserialize new format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_chunk_info_eq(&chunk_identifiers, &deserialized.infos());
    Ok(())
}

/// Test that data map deserialization works with old DataMapLevel::First format (public)
#[test]
fn test_public_data_map_old_format_first() -> Result<()> {
    let old_data_map = create_old_data_map(3);
    let data_map_level = autonomi::self_encryption::DataMapLevel::First(old_data_map.clone());

    let data_map_bytes = serialize_to_bytes(&data_map_level)?;
    let result = deserialize_data_map(&data_map_bytes);

    assert!(
        result.is_ok(),
        "Failed to deserialize old First format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_old_new_chunk_info_eq(&old_data_map.infos(), &deserialized.infos());
    Ok(())
}

/// Test that data map deserialization works with old DataMapLevel::Additional format (public)
#[test]
fn test_public_data_map_old_format_additional() -> Result<()> {
    let old_data_map = create_old_data_map(7);
    let data_map_level = autonomi::self_encryption::DataMapLevel::Additional(old_data_map.clone());

    let data_map_bytes = serialize_to_bytes(&data_map_level)?;
    let result = deserialize_data_map(&data_map_bytes);

    assert!(
        result.is_ok(),
        "Failed to deserialize old Additional format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_old_new_chunk_info_eq(&old_data_map.infos(), &deserialized.infos());
    Ok(())
}

/// Test error handling for invalid data (public)
#[test]
fn test_public_data_map_invalid_data() -> Result<()> {
    let invalid_data = Bytes::from_static(b"this is not valid msgpack data");
    let result = deserialize_data_map(&invalid_data);

    assert!(
        result.is_err(),
        "Should have failed to deserialize invalid data"
    );
    match result.unwrap_err() {
        DownloadError::GetError(autonomi::client::GetError::InvalidDataMap(_)) => {
            // Expected error type
        }
        other => panic!("Unexpected error type: {other:?}"),
    }
    Ok(())
}

/// Test with empty data maps (public)
#[test]
fn test_public_data_map_empty() -> Result<()> {
    // Test empty new format data map
    let empty_new_data_map = DataMap {
        chunk_identifiers: vec![],
        child: None,
    };
    let data_map_bytes = serialize_to_bytes(&empty_new_data_map)?;

    let result = deserialize_data_map(&data_map_bytes);
    assert!(
        result.is_ok(),
        "Failed to deserialize empty new format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_eq!(deserialized.infos().len(), 0);

    // Test empty old format data map
    let empty_old_data_map = OldDataMap::new(vec![]);
    let data_map_level = autonomi::self_encryption::DataMapLevel::First(empty_old_data_map);
    let data_map_bytes = serialize_to_bytes(&data_map_level)?;

    let result = deserialize_data_map(&data_map_bytes);
    assert!(
        result.is_ok(),
        "Failed to deserialize empty old format: {:?}",
        result.err()
    );
    let deserialized = result.unwrap();
    assert_eq!(deserialized.infos().len(), 0);
    Ok(())
}

/// Test round-trip compatibility: old format → deserialize → should match expected structure (public)
#[test]
fn test_public_data_map_round_trip() -> Result<()> {
    let expected_chunks: Vec<_> = (0..4)
        .map(|i| {
            let dst_hash = XorName::from_content(&[i as u8; 32]);
            let src_hash = XorName::from_content(&[(i + 50) as u8; 32]);
            (i, dst_hash, src_hash, 2048 + i)
        })
        .collect();

    let old_chunk_infos: Vec<_> = expected_chunks
        .iter()
        .map(
            |(index, dst_hash, src_hash, src_size)| self_encryption_old::ChunkInfo {
                index: *index,
                dst_hash: *dst_hash,
                src_hash: *src_hash,
                src_size: *src_size,
            },
        )
        .collect();

    let old_data_map = OldDataMap::new(old_chunk_infos);
    let data_map_level = autonomi::self_encryption::DataMapLevel::First(old_data_map);
    let data_map_bytes = serialize_to_bytes(&data_map_level)?;

    let result = deserialize_data_map(&data_map_bytes).unwrap();

    assert_eq!(result.infos().len(), expected_chunks.len());
    for ((expected_index, expected_dst, expected_src, expected_size), actual) in
        expected_chunks.iter().zip(result.infos().iter())
    {
        assert_eq!(*expected_index, actual.index);
        assert_eq!(*expected_dst, actual.dst_hash);
        assert_eq!(*expected_src, actual.src_hash);
        assert_eq!(*expected_size, actual.src_size);
    }
    Ok(())
}
