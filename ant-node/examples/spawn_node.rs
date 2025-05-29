// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_node::spawn::node_spawner::NodeSpawner;
use autonomi::Multiaddr;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    // Using the genesis node as a bootstrap node for example
    let bootstrap_peers = vec![
        Multiaddr::from_str("/ip4/209.97.181.193/udp/57402/quic-v1/p2p/12D3KooWHygG9a7inESky2KpvHQmbX5o2UC8D29B5njdshAcv1p6")
            .expect("Invalid multiaddr")
    ];

    let running_node = NodeSpawner::new()
        .with_initial_peers(bootstrap_peers)
        .spawn()
        .await
        .expect("Failed to spawn node");

    let listen_addrs = running_node
        .get_listen_addrs_with_peer_id()
        .await
        .expect("Failed to get listen addrs with peer id");

    println!("Node started with listen addrs: {listen_addrs:?}");
}
