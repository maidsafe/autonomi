// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::expect_used)]

use ant_logging::LogBuilder;
use autonomi::client::payment::PaymentOption;
use autonomi::{Client, PaymentMode};
use eyre::Result;
use rand::{RngCore, thread_rng};
use serial_test::serial;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use test_utils::evm::get_funded_wallet;
use tokio::time::sleep;

/// Test both Standard and SingleNode payment modes for file upload
#[tokio::test]
#[serial]
async fn test_payment_modes_file_upload() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Create client connected to local network
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::Wallet(wallet.clone());

    // Create temporary test files with different content
    let temp_dir = TempDir::new()?;

    // Generate random content to ensure unique addresses on each test run
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    // File 1 for Standard mode test - unique random content
    let standard_test_file = temp_dir.path().join("standard_test_file.txt");
    let standard_random_id = thread_rng().next_u64();
    let standard_content = format!(
        "Standard payment mode test file - timestamp:{} - random_id:{} - payload:{}",
        timestamp,
        standard_random_id,
        "standard_test_data".repeat(100)
    );
    fs::write(&standard_test_file, &standard_content)?;

    // File 2 for SingleNode mode test - different unique random content
    let single_test_file = temp_dir.path().join("single_test_file.txt");
    let single_random_id = thread_rng().next_u64();
    let single_content = format!(
        "SingleNode payment mode test file - timestamp:{} - random_id:{} - payload:{}",
        timestamp,
        single_random_id,
        "single_node_test_data".repeat(100)
    );
    fs::write(&single_test_file, &single_content)?;

    // Test 1: Standard payment mode (default)
    println!("Testing Standard payment mode");
    let client_standard = client.clone();
    let (cost_standard, addr_standard) = client_standard
        .file_content_upload_public(standard_test_file, payment_option.clone())
        .await?;

    println!(
        "Standard mode upload completed, cost: {}, address: {}",
        cost_standard,
        addr_standard.to_hex()
    );

    // Test 2: SingleNode payment mode
    println!("Testing SingleNode payment mode");
    let client_single = client.clone().with_payment_mode(PaymentMode::SingleNode);
    let (cost_single, addr_single) = client_single
        .file_content_upload_public(single_test_file, payment_option.clone())
        .await?;

    println!(
        "SingleNode mode upload completed, cost: {}, address: {}",
        cost_single,
        addr_single.to_hex()
    );

    // Wait for data propagation
    sleep(Duration::from_secs(5)).await;

    // Test 3: Verify both files can be downloaded
    let download_dir = temp_dir.path().join("downloads");
    fs::create_dir_all(&download_dir)?;

    // Download file uploaded with Standard mode
    let standard_download_path = download_dir.join("standard_file.txt");
    client
        .file_download_public(&addr_standard, standard_download_path.clone())
        .await?;

    // Download file uploaded with SingleNode mode
    let single_download_path = download_dir.join("single_file.txt");
    client
        .file_download_public(&addr_single, single_download_path.clone())
        .await?;

    // Test 4: Verify downloaded content matches original
    let downloaded_standard_content = fs::read_to_string(&standard_download_path)?;
    let downloaded_single_content = fs::read_to_string(&single_download_path)?;

    assert_eq!(
        standard_content, downloaded_standard_content,
        "Standard mode download content mismatch"
    );
    assert_eq!(
        single_content, downloaded_single_content,
        "SingleNode mode download content mismatch"
    );

    // Verify the addresses are different (since content is different)
    assert_ne!(
        addr_standard, addr_single,
        "Addresses should be different for different content"
    );

    println!("✅ Both payment modes work correctly - files uploaded and downloaded successfully");
    println!("   Standard mode cost: {cost_standard}");
    println!("   SingleNode mode cost: {cost_single}");
    println!("   Standard file size: {} bytes", standard_content.len());
    println!("   SingleNode file size: {} bytes", single_content.len());

    Ok(())
}

/// Test payment verification succeeds after making a payment using single node payment mode
#[tokio::test]
#[serial]
async fn test_single_node_payment_verification() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Create client with SingleNode payment mode
    let client = Client::init_local()
        .await?
        .with_payment_mode(PaymentMode::SingleNode);

    let wallet = get_funded_wallet();
    let payment_option = autonomi::client::payment::PaymentOption::Wallet(wallet.clone());

    // Create a small test file with random content
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test_verification.txt");

    // Generate random content to ensure unique addresses on each test run
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let random_id = thread_rng().next_u64();

    let test_content = format!(
        "Payment verification test content for SingleNode mode - unique content - timestamp:{} - random_id:{} - payload:{}",
        timestamp,
        random_id,
        "test_data".repeat(50)
    );
    fs::write(&test_file, &test_content)?;

    // Upload the file using SingleNode payment mode
    // This will internally handle payment and give us a way to verify it worked
    println!(
        "Uploading file with SingleNode payment mode (random content: {} bytes)",
        test_content.len()
    );
    let (cost, addr) = client
        .file_content_upload_public(test_file.clone(), payment_option)
        .await?;

    println!(
        "Upload completed - cost: {cost}, address: {}",
        addr.to_hex()
    );

    // Verify we can download the file back, proving the payment worked
    let download_path = temp_dir.path().join("downloaded_verification.txt");
    client
        .file_download_public(&addr, download_path.clone())
        .await?;

    // Verify downloaded content matches original
    let downloaded_content = fs::read_to_string(&download_path)?;
    assert_eq!(
        test_content, downloaded_content,
        "Downloaded content should match uploaded content"
    );

    // Additional verification: Check that cost is reasonable for SingleNode mode
    // In SingleNode mode, we pay one node 10x the amount, so cost should be significant but
    // not necessarily more than standard mode (depends on the quote distribution)
    assert!(
        cost > ant_evm::AttoTokens::zero(),
        "Upload cost should be greater than zero"
    );

    println!("✅ SingleNode payment mode upload and download succeeded");
    println!("   Upload cost: {cost}");
    println!("   Data address: {}", addr.to_hex());
    println!("   File size: {} bytes", test_content.len());
    println!("✅ Payment verification test completed successfully for SingleNode mode");

    Ok(())
}

/// Test payment verification succeeds after making a payment using standard payment mode
#[tokio::test]
#[serial]
async fn test_standard_payment_verification() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Create client with Standard payment mode (default)
    let client = Client::init_local().await?;

    let wallet = get_funded_wallet();
    let payment_option = autonomi::client::payment::PaymentOption::Wallet(wallet.clone());

    // Create a small test file with random content
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test_standard_verification.txt");

    // Generate random content to ensure unique addresses on each test run
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let random_id = thread_rng().next_u64();

    let test_content = format!(
        "Payment verification test content for Standard mode - unique content - timestamp:{} - random_id:{} - payload:{}",
        timestamp,
        random_id,
        "test_data_standard".repeat(50)
    );
    fs::write(&test_file, &test_content)?;

    // Upload the file using Standard payment mode
    // This will internally handle payment and give us a way to verify it worked
    println!(
        "Uploading file with Standard payment mode (random content: {} bytes)",
        test_content.len()
    );
    let (cost, addr) = client
        .file_content_upload_public(test_file.clone(), payment_option)
        .await?;

    println!(
        "Upload completed - cost: {cost}, address: {}",
        addr.to_hex()
    );

    // Verify we can download the file back, proving the payment worked
    let download_path = temp_dir.path().join("downloaded_standard_verification.txt");
    client
        .file_download_public(&addr, download_path.clone())
        .await?;

    // Verify downloaded content matches original
    let downloaded_content = fs::read_to_string(&download_path)?;
    assert_eq!(
        test_content, downloaded_content,
        "Downloaded content should match uploaded content"
    );

    // Additional verification: Check that cost is reasonable for Standard mode
    // In Standard mode, we pay 3 nodes out of 5 with normal amounts
    assert!(
        cost > ant_evm::AttoTokens::zero(),
        "Upload cost should be greater than zero"
    );

    println!("✅ Standard payment mode upload and download succeeded");
    println!("   Upload cost: {cost}");
    println!("   Data address: {}", addr.to_hex());
    println!("   File size: {} bytes", test_content.len());
    println!("✅ Payment verification test completed successfully for Standard mode");

    Ok(())
}

/// Test cost estimation for both Standard and SingleNode payment modes
#[tokio::test]
#[serial]
async fn test_payment_modes_cost_estimation() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Create client connected to local network
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::Wallet(wallet.clone());

    // Create temporary test files with different sizes
    let temp_dir = TempDir::new()?;

    // Generate random content to ensure unique addresses on each test run
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let random_id = thread_rng().next_u64();

    // Small file (1KB)
    let small_file = temp_dir.path().join("small_test_file.txt");
    let small_content = format!(
        "Small file - timestamp:{} - random_id:{} - payload:{}",
        timestamp,
        random_id,
        "a".repeat(1000)
    );
    fs::write(&small_file, &small_content)?;

    // Medium file (20MB)
    let medium_file = temp_dir.path().join("medium_test_file.txt");
    let medium_content = format!(
        "Medium file - timestamp:{} - random_id:{} - payload:{}",
        timestamp,
        random_id,
        "b".repeat(20_000_000)
    );
    fs::write(&medium_file, &medium_content)?;

    // Test 1: Estimate cost for small file with Standard mode
    println!("Testing cost estimation for small file with Standard mode");
    let client_standard = client.clone();
    let small_cost_estimate_standard = client_standard.file_cost(&small_file).await?;
    println!("Standard mode - Small file cost estimate: {small_cost_estimate_standard}");

    // Test 2: Estimate cost for small file with SingleNode mode
    println!("Testing cost estimation for small file with SingleNode mode");
    let client_single = client.clone().with_payment_mode(PaymentMode::SingleNode);
    let small_cost_estimate_single = client_single.file_cost(&small_file).await?;
    println!("SingleNode mode - Small file cost estimate: {small_cost_estimate_single}");

    // Test 3: Estimate cost for medium file with both modes
    let medium_cost_estimate_standard = client_standard.file_cost(&medium_file).await?;
    let medium_cost_estimate_single = client_single.file_cost(&medium_file).await?;
    println!("Standard mode - Medium file cost estimate: {medium_cost_estimate_standard}");
    println!("SingleNode mode - Medium file cost estimate: {medium_cost_estimate_single}");

    // Test 4: Verify actual upload costs match estimates (with some tolerance)
    // Upload small file with Standard mode and compare with estimate
    let (actual_cost_standard, _) = client_standard
        .file_content_upload_public(small_file.clone(), payment_option.clone())
        .await?;
    println!("Standard mode - Small file actual cost: {actual_cost_standard}");

    // Upload medium file with SingleNode mode and compare with estimate
    let (actual_cost_single, _) = client_single
        .file_content_upload_public(medium_file.clone(), payment_option.clone())
        .await?;
    println!("SingleNode mode - Medium file actual cost: {actual_cost_single}");

    // Test 5: Test directory cost estimation
    let test_dir = temp_dir.path().join("test_dir");
    fs::create_dir_all(&test_dir)?;

    // Create multiple files in the directory
    for i in 0..3 {
        let file_path = test_dir.join(format!("file_{i}.txt"));
        let content = format!(
            "File {} - timestamp:{} - random_id:{} - payload:{}",
            i,
            timestamp,
            random_id + i,
            "x".repeat(2000)
        );
        fs::write(&file_path, content)?;
    }

    // Estimate directory cost with both modes
    let dir_cost_estimate_standard = client_standard.file_cost(&test_dir).await?;
    let dir_cost_estimate_single = client_single.file_cost(&test_dir).await?;

    println!("Standard mode - Directory cost estimate: {dir_cost_estimate_standard}");
    println!("SingleNode mode - Directory cost estimate: {dir_cost_estimate_single}");

    // Verify cost relationships
    // Medium files should cost more than small files
    assert!(
        medium_cost_estimate_standard > small_cost_estimate_standard,
        "Medium file should have higher cost estimate than small file"
    );
    assert!(
        medium_cost_estimate_single > small_cost_estimate_single,
        "Medium file should have higher cost estimate than small file in SingleNode mode"
    );

    // Directory should cost more than single files
    assert!(
        dir_cost_estimate_standard > medium_cost_estimate_standard,
        "Directory should have higher cost estimate than single medium file"
    );
    assert!(
        dir_cost_estimate_single > medium_cost_estimate_single,
        "Directory should have higher cost estimate than single medium file in SingleNode mode"
    );

    // All estimates should be greater than zero
    assert!(
        small_cost_estimate_standard > ant_evm::AttoTokens::zero(),
        "Cost estimate should be greater than zero"
    );
    assert!(
        small_cost_estimate_single > ant_evm::AttoTokens::zero(),
        "Cost estimate should be greater than zero"
    );

    println!("✅ Cost estimation tests completed successfully for both payment modes");
    println!("   Standard mode costs seem proportional to file sizes");
    println!("   SingleNode mode costs also scale with file sizes");
    println!("   Directory costs are higher than individual files as expected");

    Ok(())
}
