use ant_bootstrap::InitialPeersConfig;
use ant_node::spawn::network_spawner::NetworkSpawner;
use autonomi::{Client, ClientConfig};
use evmlib::wallet::Wallet;

#[tokio::main]
async fn main() -> eyre::Result<()> {
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
    let network = NetworkSpawner::new()
        .with_evm_network(evm_network.clone())
        .with_rewards_address(funded_wallet.address()) // This MUST be set to something else than 0x0!
        .with_local(true)
        .with_size(20)
        .spawn()
        .await?;

    // One of the peers in the local network
    let bootstrap_peer = network.bootstrap_peer().await;

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
    let _client = Client::init_with_config(config).await?;

    // Might have to give the client a sec to connect to the network

    Ok(())
}
