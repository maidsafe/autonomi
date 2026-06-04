// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! # Example: Spawn a local Autonomi network and get a cost quote
//!
//! This example shows how to:
//! 1. Start an EVM testnet (Anvil)
//! 2. Spawn a local Autonomi network (25 nodes)
//! 3. Connect an Autonomi client to the network
//! 4. Get a cost quote for storing 1MB of data
//!
//! ## Prerequisites
//!
//! Install Foundry (provides Anvil for local EVM):
//!
//! **macOS / Linux:**
//! ```bash
//! curl -L https://foundry.paradigm.xyz | bash
//! foundryup
//! ```
//!
//! **Windows (PowerShell):**
//! ```powershell
//! irm https://foundry.paradigm.xyz | iex
//! foundryup
//! ```
//!
//! ## Run
//! ```bash
//! cargo run --example spawn_local_network
//! ```

#![allow(clippy::expect_used)]

use ant_evm::RewardsAddress;
use ant_logging::LogBuilder;
use ant_node::BootstrapConfig;
use ant_node::spawn::network_spawner::NetworkSpawner;
use autonomi::Client;
use bytes::Bytes;
use evmlib::testnet::Testnet;
use std::time::Duration;
use tokio::time::sleep;

const NETWORK_SIZE: usize = 25;
const DISCOVERY_WAIT_SECS: u64 = 15;

#[tokio::main]
async fn main() {
    // Initialize logging using the standard autonomi logging
    let _log_guard = LogBuilder::init_single_threaded_tokio_test();

    // Step 1: Start EVM testnet
    println!("Starting EVM testnet...");
    let testnet = Testnet::new().await;
    let evm_network = testnet.to_network();
    println!("EVM testnet ready at {}", evm_network.rpc_url());

    // Step 2: Spawn local Autonomi network
    println!("Spawning {NETWORK_SIZE} nodes...");

    // WARNING: RewardsAddress::default() is the zero address (0x0...0).
    // Any rewards would be lost! This is fine ONLY for local testing.
    let running_network = NetworkSpawner::new()
        .with_evm_network(evm_network)
        .with_rewards_address(RewardsAddress::default())
        .with_bootstrap_config(BootstrapConfig::new(true))
        .with_size(NETWORK_SIZE)
        .spawn()
        .await
        .expect("Failed to spawn network");

    // Show spawned nodes
    for (i, node) in running_network.running_nodes().iter().enumerate() {
        let addrs = node
            .get_listen_addrs_with_peer_id()
            .await
            .expect("Failed to get listen addrs");
        println!("Node {} listening on: {:?}", i + 1, addrs);
    }

    println!("Waiting {DISCOVERY_WAIT_SECS} seconds for peer discovery...");
    sleep(Duration::from_secs(DISCOVERY_WAIT_SECS)).await;

    // Step 3: Connect client to the local network
    println!("Connecting client to network...");
    let bootstrap_peer = running_network.bootstrap_peer().await;
    let client = Client::init_with_peers(vec![bootstrap_peer])
        .await
        .expect("Failed to connect client");
    println!("Client connected!");

    // Step 4: Get a cost quote
    println!("Getting cost quote for 1MB of data...");
    let data = Bytes::from(vec![0u8; 1024 * 1024]);
    let cost = client.data_cost(data).await.expect("Failed to get cost");
    println!("Cost to store 1MB: {cost}");

    // Cleanup
    println!("Shutting down...");
    running_network.shutdown();
    println!("Done!");
}
