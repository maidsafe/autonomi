// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use autonomi::self_encryption::DataMapLevel;
use bytes::{BufMut, Bytes, BytesMut};
use self_encryption::{ChunkInfo, DataMap, EncryptedChunk, MAX_CHUNK_SIZE, decrypt};
use self_encryption_old::encrypt as old_encrypt;
use serde::Serialize;
use tracing::error;

/// Test backward compatibility between old and new self_encryption versions
///
/// This test verifies that data encrypted with version (0.30.0) can be
/// decrypted with the new version.
#[test]
fn test_self_encryption_backward_compatibility() -> Result<(), Box<dyn std::error::Error>> {
    // Test 1 Encrypt with old version, decrypt with new version
    test_old_encrypt_new_decrypt()?;

    Ok(())
}

/// Test encrypting data with new version and decrypting with old version
fn test_old_encrypt_new_decrypt() -> Result<(), Box<dyn std::error::Error>> {
    let content_size: usize = 20 * MAX_CHUNK_SIZE + 100;
    let mut content = vec![0u8; content_size];
    for (i, c) in content.iter_mut().enumerate().take(content_size) {
        *c = (i % 17) as u8;
    }
    let content_bytes = Bytes::from(content);

    let (old_data_map, old_encrypted_chunks) = old_encrypt(content_bytes.clone())?;

    // Convert old format of DataMap and EncryptedChunk into new
    let chunk_identifiers: Vec<ChunkInfo> = old_data_map
        .infos()
        .iter()
        .map(|ck_info| ChunkInfo {
            index: ck_info.index,
            dst_hash: ck_info.dst_hash,
            src_hash: ck_info.src_hash,
            src_size: ck_info.src_size,
        })
        .collect();
    let data_map = DataMap {
        chunk_identifiers,
        child: None,
    };
    let encrypted_chunks: Vec<EncryptedChunk> = old_encrypted_chunks
        .iter()
        .map(|enc| EncryptedChunk {
            content: enc.content.clone(),
        })
        .collect();

    let decrypted_content = decrypt(&data_map, &encrypted_chunks)?;

    assert_eq!(decrypted_content, content_bytes);
    Ok(())
}

/// Comprehensive E2E test for backward compatibility involving network operations.
/// This test verifies that data encrypted with old self_encryption can be uploaded
/// to the network and then downloaded and decrypted using the new self_encryption.
#[tokio::test]
#[serial_test::serial]
async fn test_backward_compatibility_e2e_network() -> Result<(), Box<dyn std::error::Error>> {
    use ant_logging::LogBuilder;
    use autonomi::client::payment::PaymentOption;
    use autonomi::{Chunk, Client};
    use test_utils::evm::get_funded_wallet;

    // Initialize logging for the test
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Step 1: Setup client and payment (like test_analyze_chunk)
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    // Step 2: Generate test data and encrypt using old self_encryption way
    let content_size: usize = 20 * MAX_CHUNK_SIZE + 100;
    let mut content = vec![0u8; content_size];
    for (i, c) in content.iter_mut().enumerate().take(content_size) {
        *c = (i % 17) as u8;
    }
    let content_bytes = Bytes::from(content);

    println!("Original content length: {} bytes", content_bytes.len());

    // Use old self_encryption to generate data_map and chunks
    let (old_data_map, old_encrypted_chunks) = old_encrypt(content_bytes.clone())?;

    println!("Generated {} encrypted chunks", old_encrypted_chunks.len());
    println!("Data map generated: {old_data_map:?}");

    // Step 3: Pack the generated data_map using old methods to create data_map_bytes
    let data_map_bytes = wrap_data_map_old(&DataMapLevel::First(old_data_map))?;
    println!("Data map bytes length: {} bytes", data_map_bytes.len());

    // Step 4: Upload all generated chunks to network (like test_analyze_chunk)
    let chunks: Vec<Chunk> = old_encrypted_chunks
        .iter()
        .map(|enc| Chunk::new(enc.content.clone()))
        .collect();
    println!("Uploading {} encrypted chunks...", chunks.len());
    let mut chunk_addresses = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let (_cost, addr) = client.chunk_put(chunk, payment_option.clone()).await?;
        chunk_addresses.push(addr);
        println!(
            "Uploaded chunk {}/{} to address: {}",
            i + 1,
            chunks.len(),
            addr.to_hex()
        );
    }

    // Step 5: Wait for data replication
    println!("Waiting for data replication...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Step 6: Download the content back by calling fetch_from_data_map_chunk function
    println!("Downloading and decrypting content using backward-compatible function...");
    let decrypted_content = client.fetch_from_data_map_chunk(&data_map_bytes).await?;

    println!(
        "Downloaded content length: {} bytes",
        decrypted_content.len()
    );

    // Step 7: Compare the fetched content with the original content
    println!("Comparing original and downloaded content...");
    assert_eq!(
        decrypted_content, content_bytes,
        "Downloaded content does not match original content"
    );

    println!("✅ Backward compatibility E2E network test: SUCCESS");
    println!(
        "Successfully uploaded data using old self_encryption format and downloaded/decrypted using new format"
    );

    Ok(())
}

fn wrap_data_map_old(data_map: &DataMapLevel) -> Result<Bytes, rmp_serde::encode::Error> {
    // we use an initial/starting size of 300 bytes as that's roughly the current size of a DataMapLevel instance.
    let mut bytes = BytesMut::with_capacity(300).writer();
    let mut serialiser = rmp_serde::Serializer::new(&mut bytes);
    data_map
        .serialize(&mut serialiser)
        .inspect_err(|err| error!("Failed to serialize data map: {err:?}"))?;
    Ok(bytes.into_inner().freeze())
}
