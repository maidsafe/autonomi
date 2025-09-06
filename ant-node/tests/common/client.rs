// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::expect_used)]

use crate::common::get_antnode_rpc_client;
use ant_evm::Amount;
use ant_protocol::antnode_proto::{NodeInfoRequest, RestartRequest};
use ant_service_management::{NodeRegistryManager, get_local_node_registry_path};
use autonomi::Client;
use evmlib::wallet::Wallet;
use eyre::Result;
use std::str::FromStr;
use std::{net::SocketAddr, path::Path};
use test_utils::evm::get_funded_wallet;
use test_utils::evm::get_new_wallet;
use tokio::sync::Mutex;
use tonic::Request;
use tracing::{debug, info};

/// This is a limited hard coded value as Droplet version has to contact the faucet to get the funds.
/// This is limited to 10 requests to the faucet, where each request yields 100 SNT
pub const INITIAL_WALLET_BALANCE: u64 = 3 * 100 * 1_000_000_000;

/// 100 SNT is added when `add_funds_to_wallet` is called.
/// This is limited to 1 request to the faucet, where each request yields 100 SNT
pub const ADD_FUNDS_TO_WALLET: u64 = 100 * 1_000_000_000;

/// The node count for a locally running network that the tests expect
pub const LOCAL_NODE_COUNT: usize = 25;
// The number of times to try to load the faucet wallet
const LOAD_FAUCET_WALLET_RETRIES: usize = 6;

// mutex to restrict access to faucet wallet from concurrent tests
static FAUCET_WALLET_MUTEX: Mutex<()> = Mutex::const_new(());

pub async fn get_client_and_funded_wallet() -> (Client, Wallet) {
    (
        LocalNetwork::get_client().await,
        LocalNetwork::get_funded_wallet(),
    )
}

/// Get the node count
pub fn get_node_count() -> usize {
    LOCAL_NODE_COUNT
}

/// Get the list of all RPC addresses
pub async fn get_all_rpc_addresses(_skip_genesis_for_droplet: bool) -> Result<Vec<SocketAddr>> {
    let local_node_reg_path = &get_local_node_registry_path()?;
    let local_node_registry = NodeRegistryManager::load(local_node_reg_path).await?;
    let mut rpc_endpoints = Vec::new();
    for node in local_node_registry.nodes.read().await.iter() {
        let node_data = node.read().await;
        if let Some(rpc_addr) = node_data.rpc_socket_addr {
            rpc_endpoints.push(rpc_addr);
        }
    }

    Ok(rpc_endpoints)
}

/// Transfer tokens from the provided wallet to a newly created wallet
/// Returns the newly created wallet
pub async fn transfer_to_new_wallet(from: &Wallet, amount: usize) -> Result<Wallet> {
    LocalNetwork::transfer_to_new_wallet(from, amount).await
}

pub struct LocalNetwork;
impl LocalNetwork {
    ///  Get a new Client for testing
    pub async fn get_client() -> Client {
        Client::init_local()
            .await
            .expect("Client shall be successfully created.")
    }

    fn get_funded_wallet() -> Wallet {
        get_funded_wallet()
    }

    /// Transfer tokens from the provided wallet to a newly created wallet
    /// Returns the newly created wallet
    async fn transfer_to_new_wallet(from: &Wallet, amount: usize) -> Result<Wallet> {
        let wallet_balance = from.balance_of_tokens().await?;
        let gas_balance = from.balance_of_gas_tokens().await?;

        debug!("Wallet balance: {wallet_balance}, Gas balance: {gas_balance}");

        let new_wallet = get_new_wallet()?;

        from.transfer_tokens(new_wallet.address(), Amount::from(amount))
            .await?;

        from.transfer_gas_tokens(
            new_wallet.address(),
            Amount::from_str("10000000000000000000")?,
        )
        .await?;

        Ok(new_wallet)
    }

    // Restart a local node by sending in the SafenodeRpcCmd::Restart to the node's RPC endpoint.
    pub async fn restart_node(rpc_endpoint: SocketAddr, retain_peer_id: bool) -> Result<()> {
        let mut rpc_client = get_antnode_rpc_client(rpc_endpoint).await?;

        let response = rpc_client
            .node_info(Request::new(NodeInfoRequest {}))
            .await?;
        let root_dir = Path::new(&response.get_ref().data_dir);
        debug!("Obtained root dir from node {root_dir:?}.");

        let record_store = root_dir.join("record_store");
        if record_store.exists() {
            println!("Removing content from the record store {record_store:?}");
            info!("Removing content from the record store {record_store:?}");
            std::fs::remove_dir_all(record_store)?;
        }
        let secret_key_file = root_dir.join("secret-key");
        if secret_key_file.exists() {
            println!("Removing secret-key file {secret_key_file:?}");
            info!("Removing secret-key file {secret_key_file:?}");
            std::fs::remove_file(secret_key_file)?;
        }
        let wallet_dir = root_dir.join("wallet");
        if wallet_dir.exists() {
            println!("Removing wallet dir {wallet_dir:?}");
            info!("Removing wallet dir {wallet_dir:?}");
            std::fs::remove_dir_all(wallet_dir)?;
        }

        let _response = rpc_client
            .restart(Request::new(RestartRequest {
                delay_millis: 0,
                retain_peer_id,
            }))
            .await?;

        println!("Node restart requested to RPC service at {rpc_endpoint}");
        info!("Node restart requested to RPC service at {rpc_endpoint}");
        Ok(())
    }
}
