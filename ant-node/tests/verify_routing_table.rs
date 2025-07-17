// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::mutable_key_type)]
mod common;

use crate::common::get_all_peer_ids;
use ant_logging::LogBuilder;
use ant_protocol::antnode_proto::k_buckets_response;
use color_eyre::Result;
use libp2p::{
    kad::{KBucketKey, K_VALUE},
    PeerId,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Duration,
};
use test_utils::local_network_spawner::spawn_local_network;
use tracing::{error, info, trace, warn};

/// Sleep for some time for the nodes to discover each other before verification.
/// Also can be set through the env variable of the same name.
const SLEEP_BEFORE_VERIFICATION: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread")]
async fn verify_routing_table() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_multi_threaded_tokio_test();

    // Spawn local network
    let spawned_local_network = spawn_local_network(20).await?;
    let ant_network = spawned_local_network.ant_network;

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

    let all_peers = get_all_peer_ids(&ant_network)?;

    trace!("All peers: {all_peers:?}");
    let mut all_failed_list = BTreeMap::new();

    for (node_index, running_node) in ant_network.running_nodes().iter().enumerate() {
        let k_buckets: HashMap<u32, k_buckets_response::Peers> = running_node
            .get_kbuckets()
            .await
            .expect("failed to get k-buckets")
            .into_iter()
            .map(|(ilog2_distance, peers)| {
                let peers = peers.into_iter().map(|peer| peer.to_bytes()).collect();
                let peers = k_buckets_response::Peers { peers };
                (ilog2_distance, peers)
            })
            .collect();

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

    if all_failed_list.len() >= 5 {
        error!(
            "Failed to verify routing table (failure = {}):\n{all_failed_list:?}",
            all_failed_list.len()
        );
        panic!(
            "Failed to verify routing table (failure = {})",
            all_failed_list.len()
        );
    }
    if !all_failed_list.is_empty() {
        warn!("Failed to verify routing table for some nodes (all good!):\n{all_failed_list:?}");
    }
    Ok(())
}
