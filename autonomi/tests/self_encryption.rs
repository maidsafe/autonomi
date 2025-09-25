use autonomi::self_encryption::encrypt;
use bytes::Bytes;
use self_encryption::MAX_CHUNK_SIZE;
use tracing::info;

/// Test basic encryption of small data
#[test]
fn test_basic_encryption() {
    let data = Bytes::from(vec![0u8; 1000]);
    let result = encrypt(data);
    assert!(result.is_ok(), "Basic encryption should succeed");

    let (data_map_chunk, chunks) = result.unwrap();
    assert!(!chunks.is_empty(), "Should produce at least one data chunk");
    assert!(
        data_map_chunk.size() <= MAX_CHUNK_SIZE,
        "Data map chunk should be within size limits"
    );
}

/// Test encryption of empty data
#[test]
fn test_empty_data_encryption() {
    let data = Bytes::from(vec![]);
    let result = encrypt(data);
    // The self_encryption crate doesn't support empty data, so this is expected to fail
    assert!(result.is_err(), "Empty data encryption should fail");
}

/// Test encryption of data smaller than minimum chunk size but larger than minimum required
#[test]
fn test_small_data_encryption() {
    // self_encryption requires at least 3K bytes to work properly
    let data = Bytes::from(vec![1u8; 3072]);
    let result = encrypt(data);
    assert!(result.is_ok(), "Small data encryption should succeed");

    let (data_map_chunk, _chunks) = result.unwrap();
    assert!(
        data_map_chunk.size() <= MAX_CHUNK_SIZE,
        "Data map chunk should be within size limits"
    );
}

/// Test encryption of data exactly at the chunk size boundary
#[test]
fn test_chunk_size_boundary() {
    let data = Bytes::from(vec![2u8; MAX_CHUNK_SIZE]);
    let result = encrypt(data);
    assert!(
        result.is_ok(),
        "Chunk size boundary encryption should succeed"
    );

    let (data_map_chunk, _chunks) = result.unwrap();
    assert!(
        data_map_chunk.size() <= MAX_CHUNK_SIZE,
        "Data map chunk should be within size limits"
    );
}

/// Test encryption of data larger than the chunk size
#[test]
fn test_large_data_encryption() {
    // Create data that's large enough to generate multiple chunks
    let data = Bytes::from(vec![3u8; MAX_CHUNK_SIZE * 5]);
    let result = encrypt(data);
    assert!(result.is_ok(), "Large data encryption should succeed");

    let (data_map_chunk, chunks) = result.unwrap();
    assert!(chunks.len() > 1, "Should produce multiple data chunks");
    assert!(
        data_map_chunk.size() <= MAX_CHUNK_SIZE,
        "Data map chunk should be within size limits"
    );

    // Verify each chunk is within size limits
    for chunk in chunks {
        assert!(
            chunk.size() <= MAX_CHUNK_SIZE,
            "Each chunk should be within size limits"
        );
    }
}

/// Test encryption of varied data patterns
#[test]
fn test_varied_data_patterns() {
    // Create data with varied patterns to ensure chunking works properly
    let mut data = Vec::with_capacity(MAX_CHUNK_SIZE * 3);

    // Fill with a pattern
    for i in 0..(MAX_CHUNK_SIZE * 3) {
        data.push(i as u8);
    }

    let result = encrypt(Bytes::from(data));
    assert!(result.is_ok(), "Varied data encryption should succeed");

    let (data_map_chunk, chunks) = result.unwrap();
    assert!(chunks.len() > 1, "Should produce multiple data chunks");
    assert!(
        data_map_chunk.size() <= MAX_CHUNK_SIZE,
        "Data map chunk should be within size limits"
    );
}

/// Test very large data that would require multiple levels of data maps
#[test]
fn test_multi_level_data_maps() {
    // Create very large data that would require nested data maps
    // The actual size required would depend on how the DataMap serializes,
    // but we can use a large enough size to reasonably expect nesting
    let large_size = MAX_CHUNK_SIZE * 500; // Large enough to potentially require multiple data map levels

    info!("Creating test data of size {}", large_size);
    let data = Bytes::from(vec![4u8; large_size]);

    let result = encrypt(data);
    assert!(
        result.is_ok(),
        "Multi-level data map encryption should succeed"
    );

    let (data_map_chunk, chunks) = result.unwrap();
    assert!(
        data_map_chunk.size() <= MAX_CHUNK_SIZE,
        "Data map chunk should be within size limits"
    );
    assert!(chunks.len() > 10, "Should produce many data chunks");
}
