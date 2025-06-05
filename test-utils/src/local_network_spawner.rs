use ant_bootstrap::InitialPeersConfig;
use ant_node::spawn::network_spawner::{NetworkSpawner, RunningNetwork};
use autonomi::{Client, ClientConfig};
use evmlib::{testnet::Testnet, wallet::Wallet, Network};
use eyre::Result;

/// Default number of nodes to spawn in a local network
pub const DEFAULT_LOCAL_NETWORK_SIZE: usize = 25;

/// Represents a running local network with all its components
/// client and wallet are always used
/// ant_network and evm_testnet are in the structure to keep the networks alive
/// The networks are automatically shutdown when they go out of scope
/// config is used when an additional client is needed
pub struct SpawnedLocalNetwork {
    /// Autonomi client
    pub client: Client,
    /// Funded wallet used for the network
    pub wallet: Wallet,
    /// Running network instance
    pub ant_network: RunningNetwork,
    /// EVM testnet instance
    pub evm_testnet: Testnet,
    /// EVM network
    pub evm_network: Network,
    /// Client configuration used to spawn the client
    pub config: ClientConfig,
}

/// Spawns a local Autonomi network and returns a client connected to it
///
/// # Arguments
/// * `network_size` - Number of nodes to spawn in the network
///
/// # Returns
/// A `LocalSpawnedNetwork` struct containing all network components
pub async fn spawn_local_network(network_size: usize) -> Result<SpawnedLocalNetwork> {
    // EVM testnet
    let evm_testnet = evmlib::testnet::Testnet::new().await;

    // Network config for the EVM testnet
    let evm_network = evm_testnet.to_network();

    // Private key for the EVM testnet default wallet
    let evm_sk = evm_testnet.default_wallet_private_key();

    // Wallet with almost infinite gas and ANT test tokens
    let funded_wallet =
        Wallet::new_from_private_key(evm_network.clone(), &evm_sk).expect("Invalid private key");

    // Local Autonomi network
    let ant_network = NetworkSpawner::new()
        .with_evm_network(evm_network.clone())
        .with_rewards_address(funded_wallet.address()) // This MUST be set to something else than 0x0!
        .with_local(true)
        .with_size(network_size)
        .spawn()
        .await?;

    // One of the peers in the local network
    let bootstrap_peer = ant_network.bootstrap_peer().await;

    let config = ClientConfig {
        init_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![bootstrap_peer],
            network_contacts_url: vec![],
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: None,
        },
        evm_network: evm_network.clone(),
        strategy: autonomi::ClientOperatingStrategy::default(),
        network_id: None,
    };

    // Autonomi client
    let client = Client::init_with_config(config.clone()).await?;
    println!("Networks and Client initialized successfully");

    Ok(SpawnedLocalNetwork {
        client,
        wallet: funded_wallet,
        ant_network,
        evm_testnet,
        evm_network,
        config,
    })
}
