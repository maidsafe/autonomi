// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::client::payment::PaymentOption;
use autonomi::pointer::PointerTarget;
use autonomi::GraphEntryAddress;
use autonomi::{client::analyze::Analysis, GraphEntry, Pointer, Scratchpad};
use autonomi::{client::chunk::Chunk, Bytes, Client};
use eyre::Result;
use serial_test::serial;
use test_utils::evm::get_funded_wallet;

#[tokio::test]
#[serial]
async fn test_analyze_chunk() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("analyze chunk", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let chunk = Chunk::new(Bytes::from("Chunk content example"));
    let (_cost, addr) = client.chunk_put(&chunk, payment_option).await?;
    assert_eq!(addr, *chunk.address());
    let chunk_addr = addr.to_hex();
    println!("Chunk: {chunk_addr}");

    let analysis = client.analyze_address(&chunk_addr, true).await?;
    assert_eq!(analysis, Analysis::Chunk(chunk));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_data() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("analyze data", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let data = Bytes::from("Private data example");
    let (_cost, addr) = client.data_put(data, payment_option).await?;
    let data_addr = addr.to_hex();
    println!("Private Data (hex DataMapChunk): {data_addr}");

    let analysis = client.analyze_address(&data_addr, true).await?;
    println!("Analysis: {analysis}");
    assert!(matches!(analysis, Analysis::RawDataMap { .. }));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_public_data() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("analyze public data", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let data = Bytes::from("Public data example");
    let (_cost, addr) = client.data_put_public(data, payment_option).await?;
    let public_data_addr = addr.to_hex();
    println!("Public Data (XorName): {public_data_addr}");

    let analysis = client.analyze_address(&public_data_addr, true).await?;
    println!("Analysis: {analysis}");
    assert!(matches!(analysis, Analysis::DataMap { .. }));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_graph_entry() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("analyze graph entry", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let key = bls::SecretKey::random();
    let other_key = bls::SecretKey::random();
    let content = [0u8; 32];
    let graph_entry = GraphEntry::new(
        &key,
        vec![other_key.public_key()],
        content,
        vec![(other_key.public_key(), content)],
    );
    let (_cost, addr) = client
        .graph_entry_put(graph_entry.clone(), payment_option)
        .await?;
    let graph_entry_addr = addr.to_hex();
    println!("Graph Entry: {graph_entry_addr}");
    let graph_entry_bls_pubkey = key.public_key().to_hex();
    println!("Graph Entry (bls pubkey): {graph_entry_bls_pubkey}");

    let analysis = client.analyze_address(&graph_entry_addr, true).await?;
    assert_eq!(analysis, Analysis::GraphEntry(graph_entry));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_pointer() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("analyze pointer", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let target_addr = GraphEntryAddress::from_hex("b6f6ca699551882e2306ad9045e35c8837a3b99810af55ed358efe7166b7f6b4213ded09b200465f25d5d013fc7c35f9")?;
    let key = bls::SecretKey::random();
    let pointer = Pointer::new(&key, 0, PointerTarget::GraphEntryAddress(target_addr));
    let (_cost, addr) = client.pointer_put(pointer.clone(), payment_option).await?;
    let pointer_addr = addr.to_hex();
    println!("Pointer: {pointer_addr}");
    let pointer_bls_pubkey = key.public_key().to_hex();
    println!("Pointer (bls pubkey): {pointer_bls_pubkey}");

    let analysis = client.analyze_address(&pointer_addr, true).await?;
    assert_eq!(analysis, Analysis::Pointer(pointer));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_scratchpad() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("analyze scratchpad", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let key = bls::SecretKey::random();
    let scratchpad = Scratchpad::new(&key, 0, &Bytes::from("Scratchpad content example"), 0);
    let (_cost, addr) = client
        .scratchpad_put(scratchpad.clone(), payment_option)
        .await?;
    let scratchpad_addr = addr.to_hex();
    println!("Scratchpad: {scratchpad_addr}");
    let scratchpad_bls_pubkey = key.public_key().to_hex();
    println!("Scratchpad (bls pubkey): {scratchpad_bls_pubkey}");

    let analysis = client.analyze_address(&scratchpad_addr, true).await?;
    assert_eq!(analysis, Analysis::Scratchpad(scratchpad));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_register() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("analyze register", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let key = bls::SecretKey::random();
    let value = Client::register_value_from_bytes(b"Register content example")?;
    let (_cost, addr) = client.register_create(&key, value, payment_option).await?;
    let register_addr = addr.to_hex();
    println!("Register: {register_addr}");
    let register_bls_pubkey = key.public_key().to_hex();
    println!("Register (bls pubkey): {register_bls_pubkey}");

    let analysis = client.analyze_address(&register_addr, true).await?;
    println!("Analysis: {analysis}");
    assert!(matches!(analysis, Analysis::Register { .. }));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_private_dir() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("analyze private dir", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let path = "tests/file/test_dir/".into();
    let (_cost, archive_datamap) = client.dir_upload(path, payment_option.clone()).await?;
    let archive_datamap_addr = archive_datamap.to_hex();
    println!("Private Archive (DataMap): {archive_datamap_addr}");

    let analysis = client.analyze_address(&archive_datamap_addr, true).await?;
    println!("Analysis: {analysis}");
    assert!(matches!(analysis, Analysis::PrivateArchive { .. }));
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_analyze_public_dir() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("analyze public dir", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::from(&wallet);

    let path = "tests/file/test_dir/".into();
    let (_cost, archive_addr) = client.dir_upload_public(path, payment_option).await?;
    let archive_addr_str = archive_addr.to_hex();
    println!("Public Archive (XorName): {archive_addr_str}");

    let analysis = client.analyze_address(&archive_addr_str, true).await?;
    println!("Analysis: {analysis}");
    assert!(matches!(analysis, Analysis::PublicArchive { .. }));

    Ok(())
}
