// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::{LogBuilder, LogOutputDest};
use autonomi::chunk::DataMapChunk;
use autonomi::client::payment::PaymentOption;
use autonomi::data::DataAddress;
use autonomi::pointer::PointerTarget;
use autonomi::register::RegisterAddress;
use autonomi::{client::chunk::Chunk, Bytes, Client};
use autonomi::{
    ChunkAddress, GraphEntry, GraphEntryAddress, Pointer, PointerAddress, Scratchpad,
    ScratchpadAddress,
};
use eyre::Result;
use test_utils::local_network_spawner::{spawn_local_network, DEFAULT_LOCAL_NETWORK_SIZE};

macro_rules! log_and_print {
    ($($arg:tt)*) => {
        let msg = format!($($arg)*);
        println!("{}", msg);
        tracing::info!("{}", msg);
    };
}

fn setup_logging_client_and_nodes(node_count: usize) -> Result<ant_logging::MultiNodeLogHandle> {
    let logging_targets = vec![];

    let log_dest = LogOutputDest::parse_from_str("data-dir")?;

    let mut log_builder = LogBuilder::new(logging_targets);
    log_builder.output_dest(log_dest);

    let multi_node_log_handle = log_builder
        .initialize_with_multi_nodes_logging_for_unique_spans(node_count)
        /* TODO: Original function, to be removed once the newer function is stress tested
        .initialize_with_multi_node_logging_for_tests(node_count) */
        .expect("Failed to initialize logging");

    Ok(multi_node_log_handle)
}

#[tokio::test]
async fn test_data_addresses_use() -> Result<()> {
    let multi_node_log_handle = setup_logging_client_and_nodes(DEFAULT_LOCAL_NETWORK_SIZE)?;

    let spawned_local_network = spawn_local_network(DEFAULT_LOCAL_NETWORK_SIZE).await?;
    let client = spawned_local_network.client;
    let wallet = spawned_local_network.wallet;

    // Create client span following same pattern as nodes (span name + field)
    let _span_guard = tracing::info_span!("client").entered();

    tracing::info!("Starting client test operations");

    // put the chunk
    let chunk = Chunk::new(Bytes::from("Chunk content example"));
    let payment_option = PaymentOption::from(&wallet);
    let (_cost, addr) = client.chunk_put(&chunk, payment_option).await?;
    assert_eq!(addr, *chunk.address());
    let chunk_addr = addr.to_hex();
    log_and_print!("Chunk: {}", chunk_addr);

    let parsed_chunk_addr = ChunkAddress::from_hex(&chunk_addr)?;
    assert_eq!(parsed_chunk_addr, *chunk.address());

    // put data
    let data = Bytes::from("Private data example");
    let payment_option = PaymentOption::from(&wallet);
    let (_cost, addr) = client.data_put(data, payment_option).await?;
    let data_addr = addr.to_hex();
    log_and_print!("Private Data (hex DataMapChunk): {}", data_addr);

    let parsed_data_addr = DataMapChunk::from_hex(&data_addr)?;
    assert_eq!(parsed_data_addr, addr);

    // put public data
    let data = Bytes::from("Public data example");
    let payment_option = PaymentOption::from(&wallet);
    let (_cost, addr) = client.data_put_public(data, payment_option).await?;
    let public_data_addr = addr.to_hex();
    log_and_print!("Public Data (XorName): {}", public_data_addr);

    let parsed_public_data_addr = DataAddress::from_hex(&public_data_addr)?;
    assert_eq!(parsed_public_data_addr, addr);

    // put graph entry
    let key = bls::SecretKey::random();
    let other_key = bls::SecretKey::random();
    let content = [0u8; 32];
    let graph_entry = GraphEntry::new(
        &key,
        vec![other_key.public_key()],
        content,
        vec![(other_key.public_key(), content)],
    );
    let payment_option = PaymentOption::from(&wallet);
    let (_cost, addr) = client.graph_entry_put(graph_entry, payment_option).await?;
    let graph_entry_addr = addr.to_hex();
    log_and_print!("Graph Entry: {}", graph_entry_addr);
    let graph_entry_bls_pubkey = key.public_key().to_hex();
    log_and_print!("Graph Entry (bls pubkey): {}", graph_entry_bls_pubkey);

    let parsed_graph_entry_addr = GraphEntryAddress::from_hex(&graph_entry_addr)?;
    assert_eq!(parsed_graph_entry_addr, addr);
    let parsed_graph_entry_bls_pubkey = GraphEntryAddress::from_hex(&graph_entry_bls_pubkey)?;
    assert_eq!(parsed_graph_entry_bls_pubkey, addr);

    // put pointer
    let key = bls::SecretKey::random();
    let pointer = Pointer::new(&key, 0, PointerTarget::GraphEntryAddress(addr));
    let payment_option = PaymentOption::from(&wallet);
    let (_cost, addr) = client.pointer_put(pointer, payment_option).await?;
    let pointer_addr = addr.to_hex();
    log_and_print!("Pointer: {}", pointer_addr);
    let pointer_bls_pubkey = key.public_key().to_hex();
    log_and_print!("Pointer (bls pubkey): {}", pointer_bls_pubkey);

    let parsed_pointer_addr = PointerAddress::from_hex(&pointer_addr)?;
    assert_eq!(parsed_pointer_addr, addr);
    let parsed_pointer_bls_pubkey = PointerAddress::from_hex(&pointer_bls_pubkey)?;
    assert_eq!(parsed_pointer_bls_pubkey, addr);

    // put scratchpad
    let key = bls::SecretKey::random();
    let scratchpad = Scratchpad::new(&key, 0, &Bytes::from("Scratchpad content example"), 0);
    let payment_option = PaymentOption::from(&wallet);
    let (_cost, addr) = client.scratchpad_put(scratchpad, payment_option).await?;
    let scratchpad_addr = addr.to_hex();
    log_and_print!("Scratchpad: {}", scratchpad_addr);
    let scratchpad_bls_pubkey = key.public_key().to_hex();
    log_and_print!("Scratchpad (bls pubkey): {}", scratchpad_bls_pubkey);

    let parsed_scratchpad_addr = ScratchpadAddress::from_hex(&scratchpad_addr)?;
    assert_eq!(parsed_scratchpad_addr, addr);
    let parsed_scratchpad_bls_pubkey = ScratchpadAddress::from_hex(&scratchpad_bls_pubkey)?;
    assert_eq!(parsed_scratchpad_bls_pubkey, addr);

    // put register
    let key = bls::SecretKey::random();
    let payment_option = PaymentOption::from(&wallet);
    let value = Client::register_value_from_bytes(b"Register content example")?;
    let (_cost, addr) = client.register_create(&key, value, payment_option).await?;
    let register_addr = addr.to_hex();
    log_and_print!("Register: {}", register_addr);
    let register_bls_pubkey = key.public_key().to_hex();
    log_and_print!("Register (bls pubkey): {}", register_bls_pubkey);

    let parsed_register_addr = RegisterAddress::from_hex(&register_addr)?;
    assert_eq!(parsed_register_addr, addr);
    let parsed_register_bls_pubkey = RegisterAddress::from_hex(&register_bls_pubkey)?;
    assert_eq!(parsed_register_bls_pubkey, addr);

    // put private dir
    let payment_option = PaymentOption::from(&wallet);
    let path = "tests/file/test_dir/".into();
    let (_cost, archive_datamap) = client.dir_upload(path, payment_option.clone()).await?;
    let archive_datamap_addr = archive_datamap.to_hex();
    log_and_print!("Private Archive (DataMap): {}", archive_datamap_addr);

    let parsed_archive_datamap_addr = DataMapChunk::from_hex(&archive_datamap_addr)?;
    assert_eq!(parsed_archive_datamap_addr, archive_datamap);

    // put public dir
    let path = "tests/file/test_dir/".into();
    let (_cost, archive_addr) = client
        .dir_upload_public(path, payment_option.clone())
        .await?;
    let archive_addr_str = archive_addr.to_hex();
    log_and_print!("Public Archive (XorName): {}", archive_addr_str);

    let parsed_archive_addr = DataAddress::from_hex(&archive_addr_str)?;
    assert_eq!(parsed_archive_addr, archive_addr);

    // ===== COPY LOGS TO ACTUAL NODE DATA DIRECTORIES =====
    let peer_ids: Vec<String> = spawned_local_network
        .ant_network
        .running_nodes()
        .iter()
        .map(|node| node.peer_id().to_string())
        .collect();

    multi_node_log_handle.copy_logs_to_node_data_dirs(&peer_ids)?;
    multi_node_log_handle.delete_temp_node_dirs()?;

    Ok(())
}
