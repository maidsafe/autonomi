// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod common;

use common::client::get_node_count;
use eyre::Result;
use rand::seq::SliceRandom;
use sn_logging::LogBuilder;
use sn_node::NodeEvent;
use sn_protocol::safenode_proto::{
    safe_node_client::SafeNodeClient, GossipsubPublishRequest, GossipsubSubscribeRequest,
    GossipsubUnsubscribeRequest, NodeEventsRequest,
};
use std::{net::SocketAddr, time::Duration};
use tokio::time::timeout;
use tokio_stream::StreamExt;
use tonic::Request;

use crate::common::client::get_all_rpc_addresses;

const TEST_CYCLES: u8 = 20;

#[tokio::test]
async fn msgs_over_gossipsub() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test("msgs_over_gossipsub");

    let node_count = get_node_count();
    let nodes_subscribed = node_count / 2; // 12 out of 25 nodes will be subscribers

    let node_rpc_addresses = get_all_rpc_addresses()
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();

    for c in 0..TEST_CYCLES {
        let topic = format!("TestTopic-{}", rand::random::<u64>());
        println!("Testing cicle {}/{TEST_CYCLES} - topic: {topic}", c + 1);
        println!("============================================================");

        // get a random subset of `nodes_subscribed`` out of `node_count` nodes to subscribe to the topic
        let mut rng = rand::thread_rng();
        let random_subs_nodes: Vec<_> = node_rpc_addresses
            .choose_multiple(&mut rng, nodes_subscribed)
            .cloned()
            .collect();

        let mut subs_handles = vec![];
        for (node_index, rpc_addr) in random_subs_nodes.clone() {
            // request current node to subscribe to the topic
            println!("Node #{node_index} ({rpc_addr}) subscribing to {topic} ...");
            node_subscribe_to_topic(rpc_addr, topic.clone()).await?;

            let handle = tokio::spawn(async move {
                let endpoint = format!("https://{rpc_addr}");
                let mut rpc_client = SafeNodeClient::connect(endpoint).await?;
                let response = rpc_client
                    .node_events(Request::new(NodeEventsRequest {}))
                    .await?;

                let mut count: usize = 0;

                let _ = timeout(Duration::from_secs(40), async {
                    let mut stream = response.into_inner();
                    while let Some(Ok(e)) = stream.next().await {
                        match NodeEvent::from_bytes(&e.event) {
                            Ok(NodeEvent::GossipsubMsg { topic, msg }) => {
                                println!(
                                    "Msg received on node #{node_index} '{topic}': {}",
                                    String::from_utf8(msg.to_vec()).unwrap()
                                );
                                count += 1;
                            }
                            Ok(_) => { /* ignored */ }
                            Err(_) => {
                                println!("Error while parsing received NodeEvent");
                            }
                        }
                    }
                })
                .await;

                Ok::<usize, eyre::Error>(count)
            });

            subs_handles.push((node_index, rpc_addr, handle));
        }

        tokio::time::sleep(Duration::from_secs(3)).await;

        // have all other nodes to publish each a different msg to that same topic
        let mut other_nodes = node_rpc_addresses.clone();
        other_nodes
            .retain(|(node_index, _)| random_subs_nodes.iter().all(|(n, _)| n != node_index));
        other_nodes_to_publish_on_topic(other_nodes, topic.clone()).await?;

        for (node_index, addr, handle) in subs_handles.into_iter() {
            let count = handle.await??;
            println!("Messages received by node {node_index}: {count}");
            assert_eq!(
                count,
                node_count - nodes_subscribed,
                "Not enough messages received by node at index {}",
                node_index
            );
            node_unsubscribe_from_topic(addr, topic.clone()).await?;
        }
    }

    Ok(())
}

async fn node_subscribe_to_topic(addr: SocketAddr, topic: String) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let mut rpc_client = SafeNodeClient::connect(endpoint).await?;

    // subscribe to given topic
    let _response = rpc_client
        .subscribe_to_topic(Request::new(GossipsubSubscribeRequest { topic }))
        .await?;

    Ok(())
}

async fn node_unsubscribe_from_topic(addr: SocketAddr, topic: String) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let mut rpc_client = SafeNodeClient::connect(endpoint).await?;

    // unsubscribe from given topic
    let _response = rpc_client
        .unsubscribe_from_topic(Request::new(GossipsubUnsubscribeRequest { topic }))
        .await?;

    Ok(())
}

async fn other_nodes_to_publish_on_topic(
    nodes: Vec<(usize, SocketAddr)>,
    topic: String,
) -> Result<()> {
    for (node_index, addr) in nodes {
        let msg = format!("TestMsgOnTopic-{topic}-from-{node_index}");

        let endpoint = format!("https://{addr}");
        let mut rpc_client = SafeNodeClient::connect(endpoint).await?;
        println!("Node {node_index} to publish on {topic} message: {msg}");

        let _response = rpc_client
            .publish_on_topic(Request::new(GossipsubPublishRequest {
                topic: topic.clone(),
                msg: msg.into(),
            }))
            .await?;
    }

    Ok(())
}
