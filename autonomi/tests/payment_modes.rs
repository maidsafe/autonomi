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
use bytes::Bytes;
use eyre::Result;
use rand::{RngCore, thread_rng};
use std::time::Duration;
use test_utils::evm::get_funded_wallet;
use tokio::time::sleep;

/// Test both Standard and SingleNode payment modes for data upload using in-memory operations
#[tokio::test(flavor = "multi_thread")]
async fn test_payment_modes_file_upload() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Create client connected to local network
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::Wallet(wallet.clone());

    // Generate random bytes for both modes (1MB each)
    let mut standard_data_vec = vec![0u8; 1024 * 1024]; // 1MB
    thread_rng().fill_bytes(&mut standard_data_vec);
    let standard_data = Bytes::from(standard_data_vec);

    let mut single_data_vec = vec![0u8; 1024 * 1024]; // 1MB  
    thread_rng().fill_bytes(&mut single_data_vec);
    let single_data = Bytes::from(single_data_vec);

    // Test 1: Standard payment mode (default)
    println!("Testing Standard payment mode");
    let client_standard = client.clone();
    let (cost_standard, addr_standard) = client_standard
        .data_put_public(standard_data.clone(), payment_option.clone())
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
        .data_put_public(single_data.clone(), payment_option.clone())
        .await?;

    println!(
        "SingleNode mode upload completed, cost: {}, address: {}",
        cost_single,
        addr_single.to_hex()
    );

    // Wait for data propagation
    sleep(Duration::from_secs(5)).await;

    // Test 3: Verify both data can be downloaded
    let downloaded_standard = client.data_get_public(&addr_standard).await?;
    let downloaded_single = client.data_get_public(&addr_single).await?;

    // Test 4: Verify downloaded content matches original
    assert_eq!(
        standard_data, downloaded_standard,
        "Standard mode download content mismatch"
    );
    assert_eq!(
        single_data, downloaded_single,
        "SingleNode mode download content mismatch"
    );

    // Verify the addresses are different (since content is different)
    assert_ne!(
        addr_standard, addr_single,
        "Addresses should be different for different content"
    );

    println!("✅ Both payment modes work correctly - data uploaded and downloaded successfully");
    println!("   Standard mode cost: {cost_standard}");
    println!("   SingleNode mode cost: {cost_single}");
    println!("   Standard data size: {} bytes", standard_data.len());
    println!("   SingleNode data size: {} bytes", single_data.len());

    Ok(())
}

/// Test cost estimation for both Standard and SingleNode payment modes using in-memory data
#[tokio::test(flavor = "multi_thread")]
async fn test_payment_modes_cost_estimation() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Create client connected to local network
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::Wallet(wallet.clone());

    // Create unique random data for each test to avoid deduplication
    // Small data for Standard mode (1MB)
    let mut small_data_standard_vec = vec![0u8; 1024 * 1024]; // 1MB
    thread_rng().fill_bytes(&mut small_data_standard_vec);
    let small_data_standard = Bytes::from(small_data_standard_vec);

    // Small data for SingleNode mode (1MB) - different content
    let mut small_data_single_vec = vec![0u8; 1024 * 1024]; // 1MB
    thread_rng().fill_bytes(&mut small_data_single_vec);
    let small_data_single = Bytes::from(small_data_single_vec);

    // Medium data for Standard mode (20MB) - significantly larger
    let mut medium_data_standard_vec = vec![0u8; 20 * 1024 * 1024]; // 20MB
    thread_rng().fill_bytes(&mut medium_data_standard_vec);
    let medium_data_standard = Bytes::from(medium_data_standard_vec);

    // Medium data for SingleNode mode (20MB) - different content
    let mut medium_data_single_vec = vec![0u8; 20 * 1024 * 1024]; // 20MB
    thread_rng().fill_bytes(&mut medium_data_single_vec);
    let medium_data_single = Bytes::from(medium_data_single_vec);

    // Test 1: Upload small data with Standard mode
    println!("Testing small data upload with Standard mode");
    let client_standard = client.clone();
    let (small_cost_standard, addr_small_standard) = client_standard
        .data_put_public(small_data_standard.clone(), payment_option.clone())
        .await?;
    println!("Standard mode - Small data upload cost: {small_cost_standard}");

    // Test 2: Upload small data with SingleNode mode
    println!("Testing small data upload with SingleNode mode");
    let client_single = client.clone().with_payment_mode(PaymentMode::SingleNode);
    let (small_cost_single, addr_small_single) = client_single
        .data_put_public(small_data_single.clone(), payment_option.clone())
        .await?;
    println!("SingleNode mode - Small data upload cost: {small_cost_single}");

    // Test 3: Upload medium data with both modes
    let (medium_cost_standard, addr_medium_standard) = client_standard
        .data_put_public(medium_data_standard.clone(), payment_option.clone())
        .await?;
    let (medium_cost_single, addr_medium_single) = client_single
        .data_put_public(medium_data_single.clone(), payment_option.clone())
        .await?;

    println!("Standard mode - Medium data upload cost: {medium_cost_standard}");
    println!("SingleNode mode - Medium data upload cost: {medium_cost_single}");

    // Wait for data propagation
    sleep(Duration::from_secs(5)).await;

    // Test 4: Verify data can be downloaded back
    let downloaded_small_standard = client.data_get_public(&addr_small_standard).await?;
    let downloaded_small_single = client.data_get_public(&addr_small_single).await?;
    let downloaded_medium_standard = client.data_get_public(&addr_medium_standard).await?;
    let downloaded_medium_single = client.data_get_public(&addr_medium_single).await?;

    assert_eq!(
        small_data_standard, downloaded_small_standard,
        "Small data downloaded from Standard mode should match original"
    );
    assert_eq!(
        small_data_single, downloaded_small_single,
        "Small data downloaded from SingleNode mode should match original"
    );
    assert_eq!(
        medium_data_standard, downloaded_medium_standard,
        "Medium data downloaded from Standard mode should match original"
    );
    assert_eq!(
        medium_data_single, downloaded_medium_single,
        "Medium data downloaded from SingleNode mode should match original"
    );

    // Debug: Print actual sizes and costs for analysis
    println!(
        "Debug - Small Standard data size: {} bytes",
        small_data_standard.len()
    );
    println!(
        "Debug - Small SingleNode data size: {} bytes",
        small_data_single.len()
    );
    println!(
        "Debug - Medium Standard data size: {} bytes",
        medium_data_standard.len()
    );
    println!(
        "Debug - Medium SingleNode data size: {} bytes",
        medium_data_single.len()
    );

    // Verify cost relationships
    // Note: The network may use fixed costs per upload or chunk-based pricing
    // that doesn't scale linearly with size. We'll verify basic cost sanity.
    assert!(
        medium_cost_standard > small_cost_standard,
        "Medium data should have higher or equal cost than small data. Medium: {medium_cost_standard}, Small: {small_cost_standard}"
    );
    assert!(
        medium_cost_single > small_cost_single,
        "Medium data should have higher or equal cost than small data in SingleNode mode. Medium: {medium_cost_single}, Small: {small_cost_single}"
    );

    // Verify payment modes have different costs (SingleNode should be different from Standard)
    assert_ne!(
        small_cost_standard, small_cost_single,
        "Standard and SingleNode modes should have different costs for same size data"
    );
    assert_ne!(
        medium_cost_standard, medium_cost_single,
        "Standard and SingleNode modes should have different costs for same size data"
    );

    // All costs should be greater than zero
    assert!(
        small_cost_standard > ant_evm::AttoTokens::zero(),
        "Cost should be greater than zero"
    );
    assert!(
        small_cost_single > ant_evm::AttoTokens::zero(),
        "Cost should be greater than zero"
    );

    println!("✅ Payment mode cost tests completed successfully");
    println!("   Standard and SingleNode modes have different cost structures");
    println!("   Costs scale appropriately with data size");
    println!("   All data was successfully uploaded and downloaded");

    Ok(())
}
