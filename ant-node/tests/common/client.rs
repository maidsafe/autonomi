// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::get_antnode_rpc_client;
use ant_evm::Amount;
use ant_protocol::antnode_proto::{NodeInfoRequest, RestartRequest};
use ant_service_management::{get_local_node_registry_path, NodeRegistry};
use autonomi::Client;
use evmlib::wallet::Wallet;
use eyre::Result;
use std::str::FromStr;
use std::{net::SocketAddr, path::Path};
use test_utils::evm::get_funded_wallet;
use test_utils::evm::get_new_wallet;
use test_utils::testnet::DeploymentInventory;
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
    match DeploymentInventory::load() {
        Ok(_inventory) => {
            todo!("Not implemented yet for WanNetwork");
        }
        Err(_) => (
            LocalNetwork::get_client().await,
            LocalNetwork::get_funded_wallet(),
        ),
    }
}

/// Get the node count
/// If SN_INVENTORY flag is passed, the node count is obtained from the droplet
/// else return the local node count
pub fn get_node_count() -> usize {
    match DeploymentInventory::load() {
        Ok(_inventory) => {
            todo!("Not implemented yet for WanNetwork");
            // inventory.rpc_endpoints.len()
        }
        Err(_) => LOCAL_NODE_COUNT,
    }
}

/// Get the list of all RPC addresses
/// If SN_INVENTORY flag is passed, the RPC addresses of all the droplet nodes are returned
/// else generate local addresses for NODE_COUNT nodes
///
/// The genesis address is skipped for droplets as we don't want to restart the Genesis node there.
/// The restarted node relies on the genesis multiaddr to bootstrap after restart.
pub fn get_all_rpc_addresses(_skip_genesis_for_droplet: bool) -> Result<Vec<SocketAddr>> {
    match DeploymentInventory::load() {
        Ok(_inventory) => {
            todo!("Not implemented yet for WanNetwork");
            // if !skip_genesis_for_droplet {
            //     return Ok(inventory.rpc_endpoints.values().cloned().collect());
            // }
            // // else filter out genesis
            // let genesis_ip = inventory
            //     .vm_list
            //     .iter()
            //     .find_map(|(name, addr)| {
            //         if name.contains("genesis") {
            //             Some(*addr)
            //         } else {
            //             None
            //         }
            //     })
            //     .ok_or_eyre("Could not get the genesis VM's addr")?;

            // let rpc_endpoints = inventory
            //     .rpc_endpoints
            //     .into_iter()
            //     .filter(|(_, addr)| addr.ip() != genesis_ip)
            //     .map(|(_, addr)| addr)
            //     .collect();
            // Ok(rpc_endpoints)
        }
        Err(_) => {
            let local_node_reg_path = &get_local_node_registry_path()?;
            let local_node_registry = NodeRegistry::load(local_node_reg_path)?;
            let rpc_endpoints = local_node_registry
                .nodes
                .iter()
                .filter_map(|n| n.rpc_socket_addr)
                .collect::<Vec<SocketAddr>>();
            Ok(rpc_endpoints)
        }
    }
}

/// Transfer tokens from the provided wallet to a newly created wallet
/// Returns the newly created wallet
pub async fn transfer_to_new_wallet(from: &Wallet, amount: usize) -> Result<Wallet> {
    match DeploymentInventory::load() {
        Ok(_inventory) => {
            todo!("Not implemented yet for WanNetwork");
            // Droplet::get_funded_wallet(client, to_wallet_dir, inventory.faucet_address, false).await
        }
        Err(_) => LocalNetwork::transfer_to_new_wallet(from, amount).await,
    }
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

pub struct WanNetwork;
impl WanNetwork {
    // /// Create a new client and bootstrap from the provided safe_peers
    // pub async fn get_client(inventory: &DeploymentInventory) -> Client {
    //     let secret_key = bls::SecretKey::random();

    //     let mut bootstrap_peers = Vec::new();
    //     for peer in inventory
    //         .peers
    //         .iter()
    //         .chain(vec![&inventory.genesis_multiaddr])
    //     {
    //         match parse_peer_addr(peer) {
    //             Ok(peer) => bootstrap_peers.push(peer),
    //             Err(err) => error!("Can't parse ANT_PEERS {peer:?} with error {err:?}"),
    //         }
    //     }
    //     if bootstrap_peers.is_empty() {
    //         panic!("Could parse/find any bootstrap peers");
    //     }

    //     println!("Client bootstrap with peer {bootstrap_peers:?}");
    //     info!("Client bootstrap with peer {bootstrap_peers:?}");
    //     Client::new(secret_key, Some(bootstrap_peers), None, None)
    //         .await
    //         .expect("Client shall be successfully created.")
    // }

    // // Restart a remote antnode service by sending a RPC to the antctl daemon.
    // pub async fn restart_node(
    //     peer_id: &PeerId,
    //     daemon_endpoint: SocketAddr,
    //     retain_peer_id: bool,
    // ) -> Result<()> {
    //     let mut rpc_client = get_antctl_rpc_client(daemon_endpoint).await?;

    //     let _response = rpc_client
    //         .restart_node_service(Request::new(NodeServiceRestartRequest {
    //             peer_id: peer_id.to_bytes(),
    //             delay_millis: 0,
    //             retain_peer_id,
    //         }))
    //         .await?;

    //     println!("Node restart requested to antctld {daemon_endpoint}");
    //     info!("Node restart requested to antctld {daemon_endpoint}");

    //     Ok(())
    // }
}
