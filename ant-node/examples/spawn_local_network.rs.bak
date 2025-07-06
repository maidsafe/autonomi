// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_node::spawn::network_spawner::NetworkSpawner;
use std::time::Duration;
use tokio::time::sleep;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_env("RUST_LOG"))
        .init();

    let network_size = 20;

    let running_network = NetworkSpawner::new()
        .with_evm_network(Default::default())
        .with_local(true)
        .with_size(network_size)
        .spawn()
        .await
        .expect("Failed to spawn network");

    // Wait for nodes to dial each other
    sleep(Duration::from_secs(10)).await;

    for node in running_network.running_nodes() {
        println!("Node listening on: {:?}", node.get_listen_addrs().await);
    }

    running_network.shutdown();
}
