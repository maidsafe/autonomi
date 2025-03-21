// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::InitialPeersConfig;
use ant_node::spawn::network_spawner::NetworkSpawner;
use autonomi::{Client, ClientConfig, ClientOperatingStrategy};
use evmlib::testnet::Testnet;

// Test fails in CI because of external node interference.
#[tokio::test]
#[ignore]
async fn test_get_closest_to_address() {
    let evm_testnet = Testnet::new().await;
    let evm_network = evm_testnet.to_network();

    let network = NetworkSpawner::new()
        .with_evm_network(evm_network.clone())
        .with_local(true)
        .with_size(5)
        .spawn()
        .await
        .unwrap();

    let peer = network.bootstrap_peer().await;

    let config = ClientConfig {
        init_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![peer],
            network_contacts_url: vec![],
            local: true,
            disable_mainnet_contacts: true,
            ignore_cache: true,
            bootstrap_cache_dir: None,
        },
        evm_network,
        strategy: ClientOperatingStrategy::default(),
    };

    let client = Client::init_with_config(config).await.unwrap();

    let node = network.running_nodes().first().unwrap();

    let close_nodes = client.get_closest_to_address(node.peer_id()).await.unwrap();

    assert_eq!(close_nodes.len(), 5);
    assert!(close_nodes.iter().any(|(peer, _)| peer == &node.peer_id()));
}
