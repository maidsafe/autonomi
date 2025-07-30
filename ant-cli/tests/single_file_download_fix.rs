// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::Client;
use eyre::Result;
use serial_test::serial;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::time::Duration;
use test_utils::evm::get_funded_wallet;
use tokio::time::sleep;

/// E2E test for AUTO-177: Verify single public file doesn't get downloaded twice
/// This test verifies that when a file fails to deserialize as PublicArchive,
/// we use the already-downloaded bytes instead of re-downloading
#[tokio::test]
#[serial]
async fn test_single_file_download_no_double_fetch() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("single_file_download_fix", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    // Create a test file that is NOT an archive
    let test_file_path = "test_single_file.txt";
    let test_content =
        b"This is a single file, not an archive.\nIt should be downloaded without double-fetching.";

    // Write test file
    let mut file = File::create(test_file_path)?;
    file.write_all(test_content)?;
    file.sync_all()?;
    drop(file);

    // Upload the single file as public data
    let file_data = fs::read(test_file_path)?;
    let (_cost, addr) = client
        .data_put_public(file_data.clone().into(), wallet.into())
        .await?;

    // Wait for propagation
    sleep(Duration::from_secs(2)).await;

    // Download the file - this should trigger our fix:
    // 1. Try to deserialize as PublicArchive (will fail)
    // 2. Use the same downloaded bytes for single file (no double download)
    let downloaded_path = "downloaded_single_file.txt";
    client
        .archive_get_public_to_file(&addr, downloaded_path)
        .await?;

    // Verify the downloaded content matches
    assert!(
        Path::new(downloaded_path).exists(),
        "Downloaded file should exist"
    );
    let downloaded_content = fs::read(downloaded_path)?;
    assert_eq!(
        file_data, downloaded_content,
        "Downloaded content should match original"
    );

    // Verify content integrity
    let original_hash = compute_sha256(test_file_path)?;
    let downloaded_hash = compute_sha256(downloaded_path)?;
    assert_eq!(original_hash, downloaded_hash, "File hashes should match");

    // Cleanup
    fs::remove_file(test_file_path).ok();
    fs::remove_file(downloaded_path).ok();

    Ok(())
}

/// Helper function to compute SHA256 hash of a file
fn compute_sha256(path: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = BufReader::new(File::open(path)?);
    let mut buffer = [0; 1024];
    while let Ok(read_bytes) = file.read(&mut buffer) {
        if read_bytes == 0 {
            break;
        }
        hasher.update(&buffer[..read_bytes]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
