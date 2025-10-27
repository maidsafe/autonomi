// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow expect/panic usage in examples
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use ant_node::BootstrapConfig;
use ant_node::spawn::node_spawner::NodeSpawner;

#[tokio::main]
async fn main() {
    let running_node = NodeSpawner::new()
        .with_bootstrap_config(BootstrapConfig::default())
        .spawn()
        .await
        .expect("Failed to spawn node");

    let listen_addrs = running_node
        .get_listen_addrs_with_peer_id()
        .await
        .expect("Failed to get listen addrs with peer id");

    println!("Node started with listen addrs: {listen_addrs:?}");
}
