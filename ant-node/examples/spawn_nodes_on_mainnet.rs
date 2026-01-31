// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! # Example: Spawn nodes on the Autonomi Mainnet
//!
//! This example shows how to:
//! 1. Spawn nodes that connect to the real Autonomi mainnet
//! 2. Wait for peer discovery
//! 3. Check peer connections
//!
//! ## Important
//!
//! - Connects to **MAINNET** (real production network)
//! - Uses **Arbitrum One** blockchain (real tokens)
//! - Set YOUR rewards address to receive real rewards
//! - Uses **relay mode** for nodes behind NAT (common for home connections)
//!
//! ## Run
//!
//! Set your rewards address via environment variable:
//! ```bash
//! REWARDS_ADDRESS=<WALLET/REWARDS ADDRESS> cargo run --example spawn_nodes_on_mainnet
//! ```

#![allow(clippy::expect_used)]

use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogBuilder;
use ant_node::spawn::node_spawner::NodeSpawner;
use ant_node::{BootstrapConfig, RunningNode};
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;

const NODE_COUNT: usize = 3;
const DISCOVERY_WAIT_SECS: u64 = 180;
const MAINNET_ID: u8 = 1;

#[tokio::main]
async fn main() {
    // Initialize logging
    let _log_guard = LogBuilder::init_single_threaded_tokio_test();

    // Set network ID to mainnet (determines which bootstrap peers to fetch)
    ant_protocol::version::set_network_id(MAINNET_ID);

    // Set env var REWARDS_ADDRESS or replace "YOUR_WALLET/REWARDS_ADDRESS_HERE" with your address
    let rewards_address_str = env::var("REWARDS_ADDRESS")
        .unwrap_or_else(|_| "YOUR_WALLET/REWARDS_ADDRESS_HERE".to_string());
    let rewards_address = RewardsAddress::from_str(&rewards_address_str).expect(
        "Invalid address! Set REWARDS_ADDRESS env var or replace the placeholder in code\n",
    );

    // Configure for LIVE network (not local, not first node)
    let bootstrap_config = BootstrapConfig::new(false).with_first(false);

    // Spawn nodes
    println!("Spawning {NODE_COUNT} nodes...");
    let mut running_nodes: Vec<RunningNode> = vec![];

    for _ in 0..NODE_COUNT {
        let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

        // For home connections behind NAT:
        // - relay_client: true (connect via relay servers)
        let node = NodeSpawner::new()
            .with_socket_addr(socket_addr)
            .with_evm_network(EvmNetwork::ArbitrumOne)
            .with_rewards_address(rewards_address)
            .with_bootstrap_config(bootstrap_config.clone())
            .with_relay_client(true)
            .spawn()
            .await
            .expect("Failed to spawn node");

        running_nodes.push(node);
    }

    // Show spawned nodes
    for (i, node) in running_nodes.iter().enumerate() {
        let addrs = node
            .get_listen_addrs_with_peer_id()
            .await
            .expect("Failed to get listen addrs");
        println!("Node {} listening on: {:?}", i + 1, addrs);
    }

    // Wait for peer discovery
    println!("Waiting {DISCOVERY_WAIT_SECS} seconds for peer discovery...");
    sleep(Duration::from_secs(DISCOVERY_WAIT_SECS)).await;

    // Check peer connections
    println!("Checking peer connections...");
    let mut success = true;
    for (i, node) in running_nodes.iter().enumerate() {
        match node.get_swarm_local_state().await {
            Ok(state) => {
                let peers = state.peers_in_routing_table;
                if peers > 0 {
                    println!("Node {}: {} peers in routing table", i + 1, peers);
                } else {
                    println!("Node {}: NO peers found", i + 1);
                    success = false;
                }
            }
            Err(e) => {
                println!("Node {}: error - {}", i + 1, e);
                success = false;
            }
        }
    }

    if success {
        println!("All nodes connected to mainnet!");
    } else {
        println!("Some nodes failed to find peers");
    }

    // Cleanup
    println!("Shutting down...");
    for node in running_nodes {
        node.shutdown();
    }
    println!("Done!");
}
