// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod common;

use ant_logging::LogBuilder;
use ant_networking::sort_peers_by_key;
use ant_protocol::{
    antnode_proto::{NodeInfoRequest, RecordAddressesRequest},
    NetworkAddress, PrettyPrintRecordKey, CLOSE_GROUP_SIZE,
};
use autonomi::Client;
use bytes::Bytes;
use common::{
    client::{get_all_rpc_addresses, get_client_and_funded_wallet},
    get_all_peer_ids, get_antnode_rpc_client, NodeRestart,
};
use eyre::{eyre, Result};
use itertools::Itertools;
use libp2p::{
    kad::{KBucketKey, RecordKey},
    PeerId,
};
use rand::{rngs::OsRng, Rng};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    net::SocketAddr,
    time::{Duration, Instant},
};
use tonic::Request;
use tracing::{debug, error, info};

const CHUNK_SIZE: usize = 1024;

// VERIFICATION_DELAY is set based on the dead peer detection interval
// Once a node has been restarted, it takes VERIFICATION_DELAY time
// for the old peer to be removed from the routing table.
// Replication is then kicked off to distribute the data to the new closest
// nodes, hence verification has to be performed after this.
const VERIFICATION_DELAY: Duration = Duration::from_secs(60);

/// Number of times to retry verification if it fails
const VERIFICATION_ATTEMPTS: usize = 5;

/// Length of time to wait before re-verifying the data location
const REVERIFICATION_DELAY: Duration =
    Duration::from_secs(ant_node::PERIODIC_REPLICATION_INTERVAL_MAX_S / 2);

// Default number of churns that should be performed. After each churn, we
// wait for VERIFICATION_DELAY time before verifying the data location.
// It can be overridden by setting the 'CHURN_COUNT' env var.
const CHURN_COUNT: u8 = 20;

/// Default number of chunks that should be PUT to the network.
/// It can be overridden by setting the 'CHUNK_COUNT' env var.
const CHUNK_COUNT: usize = 5;

type NodeIndex = usize;
type RecordHolders = HashMap<RecordKey, HashSet<NodeIndex>>;

#[tokio::test(flavor = "multi_thread")]
async fn verify_data_location() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_multi_threaded_tokio_test("verify_data_location", false);

    let churn_count = if let Ok(str) = std::env::var("CHURN_COUNT") {
        str.parse::<u8>()?
    } else {
        CHURN_COUNT
    };
    let chunk_count = if let Ok(str) = std::env::var("CHUNK_COUNT") {
        str.parse::<usize>()?
    } else {
        CHUNK_COUNT
    };
    println!(
        "Performing data location verification with a churn count of {churn_count} and n_chunks {chunk_count}\nIt will take approx {:?}",
        VERIFICATION_DELAY*churn_count as u32
    );
    info!(
        "Performing data location verification with a churn count of {churn_count} and n_chunks {chunk_count}\nIt will take approx {:?}",
        VERIFICATION_DELAY*churn_count as u32
    );
    let node_rpc_address = get_all_rpc_addresses(true).await?;
    let mut all_peers = get_all_peer_ids(&node_rpc_address).await?;

    let (client, wallet) = get_client_and_funded_wallet().await;

    store_chunks(&client, chunk_count, &wallet).await?;

    // Verify data location initially
    verify_location(&all_peers, &node_rpc_address).await?;

    // Churn nodes and verify the location of the data after VERIFICATION_DELAY
    let mut current_churn_count = 0;

    let mut node_restart = NodeRestart::new(true, false).await?;
    let mut node_index = 0;
    'main: loop {
        if current_churn_count >= churn_count {
            break 'main Ok(());
        }
        current_churn_count += 1;

        let antnode_rpc_endpoint = match node_restart.restart_next(false, false).await? {
            None => {
                // we have reached the end.
                break 'main Ok(());
            }
            Some(antnode_rpc_endpoint) => antnode_rpc_endpoint,
        };

        // wait for the dead peer to be removed from the RT and the replication flow to finish
        println!(
            "\nNode has been restarted, waiting for {VERIFICATION_DELAY:?} before verification"
        );
        info!("\nNode has been restarted, waiting for {VERIFICATION_DELAY:?} before verification");
        tokio::time::sleep(VERIFICATION_DELAY).await;

        // get the new PeerId for the current NodeIndex
        let mut rpc_client = get_antnode_rpc_client(antnode_rpc_endpoint).await?;

        let response = rpc_client
            .node_info(Request::new(NodeInfoRequest {}))
            .await?;
        let new_peer_id = PeerId::from_bytes(&response.get_ref().peer_id)?;
        // The below indexing assumes that, the way we do iteration to retrieve all_peers inside get_all_rpc_addresses
        // and get_all_peer_ids is the same as how we do the iteration inside NodeRestart.
        // todo: make this more cleaner.
        if all_peers[node_index] == new_peer_id {
            println!("new and old peer id are the same {new_peer_id:?}");
            return Err(eyre!("new and old peer id are the same {new_peer_id:?}"));
        }
        all_peers[node_index] = new_peer_id;
        node_index += 1;

        print_node_close_groups(&all_peers);

        verify_location(&all_peers, &node_rpc_address).await?;
    }
}

fn print_node_close_groups(all_peers: &[PeerId]) {
    let all_peers = all_peers.to_vec();
    info!("\nNode close groups:");

    for (node_index, peer) in all_peers.iter().enumerate() {
        let key = NetworkAddress::from(*peer).as_kbucket_key();
        let closest_peers = sort_peers_by_key(
            all_peers
                .iter()
                .map(|peer_id| (*peer_id, Default::default()))
                .collect_vec(),
            &key,
            CLOSE_GROUP_SIZE,
        )
        .expect("failed to sort peer");
        let closest_peers_idx = closest_peers
            .iter()
            .map(|(closest_peer, _)| {
                all_peers
                    .iter()
                    .position(|p| p == closest_peer)
                    .expect("peer to be in iterator")
            })
            .collect::<Vec<_>>();
        info!("Close for {node_index}: {peer:?} are {closest_peers_idx:?}");
    }
}

async fn get_records_and_holders(node_rpc_addresses: &[SocketAddr]) -> Result<RecordHolders> {
    let mut record_holders = RecordHolders::default();

    for (node_index, rpc_address) in node_rpc_addresses.iter().enumerate() {
        let mut rpc_client = get_antnode_rpc_client(*rpc_address).await?;

        let records_response = rpc_client
            .record_addresses(Request::new(RecordAddressesRequest {}))
            .await?;

        for bytes in records_response.get_ref().addresses.iter() {
            let key = RecordKey::from(bytes.clone());
            let holders = record_holders.entry(key).or_insert(HashSet::new());
            holders.insert(node_index);
        }
    }
    debug!("Obtained the current set of Record Key holders");
    Ok(record_holders)
}

// Fetches the record_holders and verifies that the record is stored by the actual closest peers to the RecordKey
// It has a retry loop built in.
async fn verify_location(all_peers: &Vec<PeerId>, node_rpc_addresses: &[SocketAddr]) -> Result<()> {
    let mut failed = HashMap::new();

    println!("*********************************************");
    println!("Verifying data across all peers {all_peers:?}");
    info!("*********************************************");
    info!("Verifying data across all peers {all_peers:?}");

    let mut verification_attempts = 0;
    while verification_attempts < VERIFICATION_ATTEMPTS {
        failed.clear();
        let record_holders = get_records_and_holders(node_rpc_addresses).await?;
        for (key, actual_holders_idx) in record_holders.iter() {
            println!("Verifying {:?}", PrettyPrintRecordKey::from(key));
            info!("Verifying {:?}", PrettyPrintRecordKey::from(key));
            let record_key = KBucketKey::from(key.to_vec());
            let expected_holders = sort_peers_by_key(
                all_peers
                    .iter()
                    .map(|peer_id| (*peer_id, Default::default()))
                    .collect_vec(),
                &record_key,
                CLOSE_GROUP_SIZE,
            )?
            .into_iter()
            .map(|(peer_id, _)| peer_id)
            .collect::<BTreeSet<_>>();

            let actual_holders = actual_holders_idx
                .iter()
                .map(|i| all_peers[*i])
                .collect::<BTreeSet<_>>();

            info!(
                "Expected to be held by {:?} nodes: {expected_holders:?}",
                expected_holders.len()
            );
            info!(
                "Actually held by {:?} nodes      : {actual_holders:?}",
                actual_holders.len()
            );

            if actual_holders != expected_holders {
                // print any expect holders that are not in actual holders
                let mut missing_peers = Vec::new();
                expected_holders
                    .iter()
                    .filter(|expected| !actual_holders.contains(expected))
                    .for_each(|expected| missing_peers.push(*expected));

                if !missing_peers.is_empty() {
                    error!(
                        "Record {:?} is not stored by {missing_peers:?}",
                        PrettyPrintRecordKey::from(key),
                    );
                    println!(
                        "Record {:?} is not stored by {missing_peers:?}",
                        PrettyPrintRecordKey::from(key),
                    );
                }
            }

            let mut failed_peers = Vec::new();
            expected_holders
                .iter()
                .filter(|expected| !actual_holders.contains(expected))
                .for_each(|expected| failed_peers.push(*expected));

            if !failed_peers.is_empty() {
                failed.insert(key.clone(), failed_peers);
            }
        }

        // The retry will take long time, result in the overall test failed due to timedout.
        // Hence need an early bail out here.
        let just_missed_one = failed.values().all(|failed_peers| failed_peers.len() <= 1);

        if !(failed.is_empty() || just_missed_one) {
            error!("Verification failed for {:?} entries", failed.len());
            println!("Verification failed for {:?} entries", failed.len());

            failed.iter().for_each(|(key, failed_peers)| {
                let key_addr = NetworkAddress::from(key);
                let pretty_key = PrettyPrintRecordKey::from(key);
                failed_peers.iter().for_each(|peer| {
                    let peer_addr = NetworkAddress::from(*peer);
                    let ilog2_distance = peer_addr.distance(&key_addr).ilog2();
                    println!("Record {pretty_key:?} is not stored inside {peer:?}, with ilog2 distance to be {ilog2_distance:?}");
                    error!("Record {pretty_key:?} is not stored inside {peer:?}, with ilog2 distance to be {ilog2_distance:?}");
                });
            });
            info!("State of each node:");
            record_holders.iter().for_each(|(key, node_index)| {
                info!(
                    "Record {:?} is currently held by node indices {node_index:?}",
                    PrettyPrintRecordKey::from(key)
                );
            });
            info!("Node index map:");
            all_peers
                .iter()
                .enumerate()
                .for_each(|(idx, peer)| info!("{idx} : {peer:?}"));
            verification_attempts += 1;
            println!("Sleeping before retrying verification. {verification_attempts}/{VERIFICATION_ATTEMPTS}");
            info!("Sleeping before retrying verification. {verification_attempts}/{VERIFICATION_ATTEMPTS}");
            if verification_attempts < VERIFICATION_ATTEMPTS {
                tokio::time::sleep(REVERIFICATION_DELAY).await;
            }
        } else {
            // if successful, break out of the loop
            break;
        }
    }

    // Replication only pick peer candidates closing to self.
    // With responsible_range switched to `distance`, this makes some `edge` peers could
    // be skipped for some `edge` records that it supposed to kept, but not picked as candidate.
    // This will be a more noticable behaviour with small sized network, which could have sparsed
    // and uneven distribution more likely, with the `network density sampling scheme`.
    // Hence, allowing a small `glitch` for this test setup only.
    if failed.is_empty() {
        println!("All the Records have been verified!");
        info!("All the Records have been verified!");
        Ok(())
    } else {
        let just_missed_one = failed.values().all(|failed_peers| failed_peers.len() <= 1);
        if just_missed_one {
            println!("Still have one failed peer after {VERIFICATION_ATTEMPTS} times");
            info!("Still have one failed peer after {VERIFICATION_ATTEMPTS} times");
            Ok(())
        } else {
            println!("Verification failed after {VERIFICATION_ATTEMPTS} times");
            error!("Verification failed after {VERIFICATION_ATTEMPTS} times");
            Err(eyre!("Verification failed for: {failed:?}"))
        }
    }
}

// Generate random Chunks and store them to the Network
async fn store_chunks(
    client: &Client,
    chunk_count: usize,
    wallet: &evmlib::wallet::Wallet,
) -> Result<()> {
    let start = Instant::now();
    let mut rng = OsRng;

    let mut uploaded_chunks_count = 0;
    loop {
        if uploaded_chunks_count >= chunk_count {
            break;
        }

        let random_bytes: Vec<u8> = ::std::iter::repeat(())
            .map(|()| rng.gen::<u8>())
            .take(CHUNK_SIZE)
            .collect();

        let random_bytes = Bytes::from(random_bytes);

        client.data_put_public(random_bytes, wallet.into()).await?;

        uploaded_chunks_count += 1;

        println!("Stored Chunk with len {CHUNK_SIZE}");
        info!("Stored Chunk with len {CHUNK_SIZE}");
    }

    println!(
        "{chunk_count:?} Chunks were stored in {:?}",
        start.elapsed()
    );
    info!(
        "{chunk_count:?} Chunks were stored in {:?}",
        start.elapsed()
    );

    // to make sure the last chunk was stored
    tokio::time::sleep(Duration::from_secs(10)).await;

    Ok(())
}
