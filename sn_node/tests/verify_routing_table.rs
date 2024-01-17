// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::mutable_key_type)]
mod common;

use crate::common::{client::get_all_rpc_addresses, get_all_peer_ids};
use color_eyre::Result;
use libp2p::{
    kad::{KBucketKey, K_VALUE},
    PeerId,
};
use sn_logging::LogBuilder;
use sn_protocol::safenode_proto::{safe_node_client::SafeNodeClient, KBucketsRequest};
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};
use tonic::Request;

/// Sleep for sometime for the nodes for discover each other before verification
/// Also can be set through the env variable of the same name.
const SLEEP_BEFORE_VERIFICATION: Duration = Duration::from_secs(5);

#[tokio::test(flavor = "multi_thread")]
async fn verify_routing_table() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_multi_threaded_tokio_test("verify_routing_table");

    let sleep_duration = std::env::var("SLEEP_BEFORE_VERIFICATION")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("Failed to prase sleep value into u64")
        })
        .map(Duration::from_secs)
        .unwrap_or(SLEEP_BEFORE_VERIFICATION);
    println!("Sleeping for {sleep_duration:?} before verification");
    tokio::time::sleep(sleep_duration).await;

    let node_rpc_address = get_all_rpc_addresses();

    let all_peers = get_all_peer_ids(&node_rpc_address).await?;
    let mut all_failed_list = BTreeMap::new();

    for (node_index, rpc_address) in node_rpc_address.iter().enumerate() {
        let endpoint = format!("https://{rpc_address}");
        let mut rpc_client = SafeNodeClient::connect(endpoint).await?;

        let response = rpc_client
            .k_buckets(Request::new(KBucketsRequest {}))
            .await?;

        let k_buckets = response.get_ref().kbuckets.clone();
        let k_buckets = k_buckets
            .into_iter()
            .map(|(ilog2, peers)| {
                let peers = peers
                    .peers
                    .into_iter()
                    .map(|peer_bytes| PeerId::from_bytes(&peer_bytes).unwrap())
                    .collect::<HashSet<_>>();
                (ilog2, peers)
            })
            .collect::<BTreeMap<_, _>>();

        let current_peer = all_peers[node_index];
        let current_peer_key = KBucketKey::from(current_peer);

        let mut failed_list = Vec::new();
        for peer in all_peers.iter() {
            let ilog2_distance = match KBucketKey::from(*peer).distance(&current_peer_key).ilog2() {
                Some(distance) => distance,
                // None if same key
                None => continue,
            };
            match k_buckets.get(&ilog2_distance) {
                Some(bucket) => {
                    if bucket.contains(peer) {
                        continue;
                    } else if bucket.len() == K_VALUE.get() {
                        println!("{peer:?} should be inside the ilog2 bucket: {ilog2_distance:?} of {current_peer:?}. But skipped as the bucket is full");
                        continue;
                    } else {
                        println!("{peer:?} not found inside the kbucket with ilog2 {ilog2_distance:?} of {current_peer:?} RT");
                        failed_list.push(*peer);
                    }
                }
                None => {
                    println!("Current peer {current_peer:?} should be {ilog2_distance} ilog2 distance away from {peer:?}, but that kbucket is not present for current_peer.");
                    failed_list.push(*peer);
                }
            }
        }
        if !failed_list.is_empty() {
            all_failed_list.insert(current_peer, failed_list);
        }
    }
    if !all_failed_list.is_empty() {
        println!("Failed to verify routing table:\n{all_failed_list:?}");
        panic!("Failed to verify routing table");
    }
    Ok(())
}
