// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::mutable_key_type)]
mod common;

use crate::common::{client::get_all_rpc_addresses, get_all_peer_ids, get_antnode_rpc_client};
use ant_logging::LogBuilder;
use ant_protocol::antnode_proto::KBucketsRequest;
use color_eyre::Result;
use libp2p::{
    kad::{KBucketKey, K_VALUE},
    PeerId,
};
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};
use tonic::Request;
use tracing::{error, info, trace};

/// Sleep for sometime for the nodes to discover each other before verification
/// Also can be set through the env variable of the same name.
const SLEEP_BEFORE_VERIFICATION: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread")]
async fn verify_routing_table() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_multi_threaded_tokio_test("verify_routing_table", false);

    let sleep_duration = std::env::var("SLEEP_BEFORE_VERIFICATION")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("Failed to prase sleep value into u64")
        })
        .map(Duration::from_secs)
        .unwrap_or(SLEEP_BEFORE_VERIFICATION);
    info!("Sleeping for {sleep_duration:?} before verification");
    tokio::time::sleep(sleep_duration).await;

    let node_rpc_address = get_all_rpc_addresses(false).await?;

    let all_peers = get_all_peer_ids(&node_rpc_address).await?;
    trace!("All peers: {all_peers:?}");
    let mut all_failed_list = BTreeMap::new();

    for (node_index, rpc_address) in node_rpc_address.iter().enumerate() {
        let mut rpc_client = get_antnode_rpc_client(*rpc_address).await?;

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
        trace!("KBuckets for node #{node_index}: {current_peer} are: {k_buckets:?}");

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
                        println!("{peer:?} found inside the kbucket with ilog2 {ilog2_distance:?} of {current_peer:?} RT");
                        continue;
                    } else if bucket.len() == K_VALUE.get() {
                        println!("{peer:?} should be inside the ilog2 bucket: {ilog2_distance:?} of {current_peer:?}. But skipped as the bucket is full");
                        info!("{peer:?} should be inside the ilog2 bucket: {ilog2_distance:?} of {current_peer:?}. But skipped as the bucket is full");
                        continue;
                    } else {
                        println!("{peer:?} not found inside the kbucket with ilog2 {ilog2_distance:?} of {current_peer:?} RT");
                        error!("{peer:?} not found inside the kbucket with ilog2 {ilog2_distance:?} of {current_peer:?} RT");
                        failed_list.push(*peer);
                    }
                }
                None => {
                    info!("Current peer {current_peer:?} should be {ilog2_distance} ilog2 distance away from {peer:?}, but that kbucket is not present for current_peer.");
                    failed_list.push(*peer);
                }
            }
        }
        if !failed_list.is_empty() {
            all_failed_list.insert(current_peer, failed_list);
        }
    }
    if !all_failed_list.is_empty() {
        error!("Failed to verify routing table:\n{all_failed_list:?}");
        panic!("Failed to verify routing table.");
    }
    Ok(())
}
