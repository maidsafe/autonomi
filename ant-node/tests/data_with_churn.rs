// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod common;

use crate::common::client::get_node_count;
use ant_evm::Amount;
use ant_logging::LogBuilder;
use ant_node::{
    spawn::{
        network_spawner::{NetworkSpawner, RunningNetwork},
        node_spawner::NodeSpawner,
    },
    RunningNode,
};
use ant_protocol::{
    storage::{ChunkAddress, GraphEntry, GraphEntryAddress, PointerTarget, ScratchpadAddress},
    NetworkAddress,
};
use autonomi::{data::DataAddress, Client, ClientConfig, InitialPeersConfig, Wallet};
use bls::{PublicKey, SecretKey};
use bytes::Bytes;
use evmlib::Network;
use eyre::{bail, ErrReport, Result};
use libp2p::Multiaddr;
use rand::Rng;
use self_encryption::MAX_CHUNK_SIZE;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fmt,
    fs::create_dir_all,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};
use tempfile::tempdir;
use test_utils::gen_random_data;
use tokio::{sync::RwLock, task::JoinHandle, time::sleep};
use tracing::{debug, error, info, trace, warn};
use xor_name::XorName;

const TOKENS_TO_TRANSFER: usize = 10000000;

const EXTRA_CHURN_COUNT: u32 = 5;
const CHURN_CYCLES: u32 = 2;
const CHUNK_CREATION_RATIO_TO_CHURN: u32 = 13;
const POINTER_CREATION_RATIO_TO_CHURN: u32 = 11;
const SCRATCHPAD_CREATION_RATIO_TO_CHURN: u32 = 9;
const GRAPHENTRY_CREATION_RATIO_TO_CHURN: u32 = 7;

static DATA_SIZE: LazyLock<usize> = LazyLock::new(|| *MAX_CHUNK_SIZE / 3);

const CONTENT_QUERY_RATIO_TO_CHURN: u32 = 40;
const MAX_NUM_OF_QUERY_ATTEMPTS: u8 = 5;

// Default total amount of time we run the checks for before reporting the outcome.
// It can be overriden by setting the 'TEST_DURATION_MINS' env var.
const TEST_DURATION: Duration = Duration::from_secs(60 * 60); // 1hr

type ContentList = Arc<RwLock<VecDeque<NetworkAddress>>>;

struct ContentError {
    net_addr: NetworkAddress,
    attempts: u8,
    last_err: ErrReport,
}

impl fmt::Debug for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:?}, attempts: {}, last error: {:?}",
            self.net_addr, self.attempts, self.last_err
        )
    }
}

type ContentErredList = Arc<RwLock<BTreeMap<NetworkAddress, ContentError>>>;

#[tokio::test(flavor = "multi_thread")]
async fn data_availability_during_churn() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_multi_threaded_tokio_test("data_with_churn", false);
    let evm_testnet = evmlib::testnet::Testnet::new().await;
    let evm_network = evm_testnet.to_network();
    let evm_sk = evm_testnet.default_wallet_private_key();
    let funded_wallet =
        Wallet::new_from_private_key(evm_network.clone(), &evm_sk).expect("Invalid private key");
    let network = NetworkSpawner::new()
        .with_evm_network(evm_network.clone())
        .with_rewards_address(funded_wallet.address())
        .with_local(true)
        .with_size(20)
        .spawn()
        .await
        .unwrap();
    let peer = network.bootstrap_peer().await;
    let config = ClientConfig {
        init_peers_config: InitialPeersConfig {
            first: false,
            local: true,
            addrs: vec![peer],
            bootstrap_cache_dir: None,
            disable_mainnet_contacts: true,
            ignore_cache: false,
            network_contacts_url: vec![],
        },
        evm_network: evm_network.clone(),
        strategy: autonomi::ClientOperatingStrategy::default(),
    };
    let client = Client::init_with_config(config).await.unwrap();

    let test_duration = if let Ok(str) = std::env::var("TEST_DURATION_MINS") {
        Duration::from_secs(60 * str.parse::<u64>()?)
    } else {
        TEST_DURATION
    };
    let node_count = get_node_count();

    let churn_period = if let Ok(str) = std::env::var("TEST_TOTAL_CHURN_CYCLES") {
        println!("Using value set in 'TEST_TOTAL_CHURN_CYCLES' env var: {str}");
        info!("Using value set in 'TEST_TOTAL_CHURN_CYCLES' env var: {str}");
        let cycles = str.parse::<u32>()?;
        test_duration / cycles
    } else {
        // Ensure at least some nodes got churned twice.
        test_duration
            / std::cmp::max(
                CHURN_CYCLES * node_count as u32,
                node_count as u32 + EXTRA_CHURN_COUNT,
            )
    };
    println!("Nodes will churn every {churn_period:?}");
    info!("Nodes will churn every {churn_period:?}");

    // Create a cross thread usize for tracking churned nodes
    let churn_count = Arc::new(RwLock::new(0_usize));

    // Allow to disable non-chunk data_types creation/checks.
    // Default to be not carry out chunks only during churn.
    let chunks_only = std::env::var("CHUNKS_ONLY").is_ok();

    println!(
        "Running this test for {test_duration:?}{}...",
        if chunks_only { " (Chunks only)" } else { "" }
    );
    info!(
        "Running this test for {test_duration:?}{}...",
        if chunks_only { " (Chunks only)" } else { "" }
    );

    info!(
        "Client and wallet created. Main wallet address: {:?}",
        funded_wallet.address()
    );

    // Shared bucket where we keep track of content created/stored on the network
    let content = ContentList::default();

    println!("Uploading some chunks before carry out node churning");
    info!("Uploading some chunks before carry out node churning");

    let chunk_wallet = Wallet::new_with_random_wallet(evm_network.clone());
    funded_wallet
        .transfer_tokens(chunk_wallet.address(), Amount::from(TOKENS_TO_TRANSFER))
        .await?;
    funded_wallet
        .transfer_gas_tokens(
            chunk_wallet.address(),
            Amount::from_str("10000000000000000000")?,
        )
        .await?;

    // Spawn a task to store Chunks at random locations, at a higher frequency than the churning events
    let store_chunks_handle = store_chunks_task(
        client.clone(),
        chunk_wallet,
        Arc::clone(&content),
        churn_period,
    );

    // Spawn a task to create Pointers at random locations,
    // at a higher frequency than the churning events
    let create_pointer_handle = if !chunks_only {
        let pointer_wallet = Wallet::new_with_random_wallet(evm_network.clone());
        funded_wallet
            .transfer_tokens(pointer_wallet.address(), Amount::from(TOKENS_TO_TRANSFER))
            .await?;
        funded_wallet
            .transfer_gas_tokens(
                pointer_wallet.address(),
                Amount::from_str("10000000000000000000")?,
            )
            .await?;
        let create_pointer_handle = create_pointers_task(
            client.clone(),
            pointer_wallet,
            Arc::clone(&content),
            churn_period,
        );
        Some(create_pointer_handle)
    } else {
        None
    };

    // Spawn a task to create GraphEntry at random locations,
    // at a higher frequency than the churning events
    let create_graph_entry_handle = if !chunks_only {
        let graph_entry_wallet = Wallet::new_with_random_wallet(evm_network.clone());
        funded_wallet
            .transfer_tokens(
                graph_entry_wallet.address(),
                Amount::from(TOKENS_TO_TRANSFER),
            )
            .await?;
        funded_wallet
            .transfer_gas_tokens(
                graph_entry_wallet.address(),
                Amount::from_str("10000000000000000000")?,
            )
            .await?;
        let create_graph_entry_handle = create_graph_entry_task(
            client.clone(),
            graph_entry_wallet,
            Arc::clone(&content),
            churn_period,
        );
        Some(create_graph_entry_handle)
    } else {
        None
    };

    // Spawn a task to create ScratchPad at random locations,
    // at a higher frequency than the churning events
    let create_scratchpad_handle = if !chunks_only {
        let scratchpad_wallet = Wallet::new_with_random_wallet(evm_network.clone());
        funded_wallet
            .transfer_tokens(
                scratchpad_wallet.address(),
                Amount::from(TOKENS_TO_TRANSFER),
            )
            .await?;
        funded_wallet
            .transfer_gas_tokens(
                scratchpad_wallet.address(),
                Amount::from_str("10000000000000000000")?,
            )
            .await?;
        let create_scratchpad_handle = create_scratchpad_task(
            client.clone(),
            scratchpad_wallet,
            Arc::clone(&content),
            churn_period,
        );
        Some(create_scratchpad_handle)
    } else {
        None
    };

    // Spawn a task to churn nodes
    tokio::spawn(async move {
        let _ = data_churn_with_network_restart(
            network,
            &evm_network,
            &funded_wallet,
            true,
            false,
            churn_period,
            test_duration,
        )
        .await;
    });

    // Shared bucket where we keep track of the content which erred when creating/storing/fetching.
    // We remove them from this bucket if we are then able to query/fetch them successfully.
    // We only try to query them 'MAX_NUM_OF_QUERY_ATTEMPTS' times, then report them effectivelly as failures.
    let content_erred = ContentErredList::default();

    // Shared bucket where we keep track of the content we failed to fetch for 'MAX_NUM_OF_QUERY_ATTEMPTS' times.
    let failures = ContentErredList::default();

    // Spawn a task to randomly query/fetch the content we create/store
    query_content_task(
        client.clone(),
        Arc::clone(&content),
        Arc::clone(&content_erred),
        churn_period,
    );

    // Spawn a task to retry querying the content that failed, up to 'MAX_NUM_OF_QUERY_ATTEMPTS' times,
    // and mark them as failures if they effectivelly cannot be retrieved.
    retry_query_content_task(
        client.clone(),
        Arc::clone(&content_erred),
        Arc::clone(&failures),
        churn_period,
    );

    info!("All tasks have been spawned. The test is now running...");
    println!("All tasks have been spawned. The test is now running...");

    let start_time = Instant::now();
    while start_time.elapsed() < test_duration {
        if store_chunks_handle.is_finished() {
            bail!("Store chunks task has finished before the test duration. Probably due to an error.");
        }
        if let Some(handle) = &create_pointer_handle {
            if handle.is_finished() {
                bail!("Create Pointers task has finished before the test duration. Probably due to an error.");
            }
        }
        if let Some(handle) = &create_graph_entry_handle {
            if handle.is_finished() {
                bail!("Create GraphEntry task has finished before the test duration. Probably due to an error.");
            }
        }
        if let Some(handle) = &create_scratchpad_handle {
            if handle.is_finished() {
                bail!("Create ScratchPad task has finished before the test duration. Probably due to an error.");
            }
        }

        let failed = failures.read().await;
        if start_time.elapsed().as_secs() % 10 == 0 {
            println!(
                "Current failures after {:?} ({}): {:?}",
                start_time.elapsed(),
                failed.len(),
                failed.values()
            );
            info!(
                "Current failures after {:?} ({}): {:?}",
                start_time.elapsed(),
                failed.len(),
                failed.values()
            );
        }

        sleep(Duration::from_secs(3)).await;
    }

    println!();
    println!(
        ">>>>>> Test stopping after running for {:?}. <<<<<<",
        start_time.elapsed()
    );
    println!("{:?} churn events happened.", *churn_count.read().await);
    println!();

    // The churning of storing_chunk/querying_chunk are all random,
    // which will have a high chance that newly stored chunk got queried BEFORE
    // the original holders churned out.
    // i.e. the test may pass even without any replication
    // Hence, we carry out a final round of query all data to confirm storage.
    println!("Final querying confirmation of content");
    info!("Final querying confirmation of content");

    // take one read lock to avoid holding the lock for the whole loop
    // prevent any late content uploads being added to the list
    let content = content.read().await;
    let uploaded_content_count = content.len();
    let mut handles = Vec::new();
    for net_addr in content.iter() {
        let client = client.clone();
        let net_addr = net_addr.clone();

        let failures = Arc::clone(&failures);
        let handle = tokio::spawn(async move {
            final_retry_query_content(&client, &net_addr, churn_period, failures).await
        });
        handles.push(handle);
    }
    let results: Vec<_> = futures::future::join_all(handles).await;

    let content_queried_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(
        content_queried_count, uploaded_content_count,
        "Not all content was queried successfully"
    );

    println!("{content_queried_count:?} pieces of content queried");

    assert_eq!(
        content_queried_count, uploaded_content_count,
        "Not all content was queried"
    );

    let failed = failures.read().await;
    if failed.len() > 0 {
        bail!("{} failure/s in test: {:?}", failed.len(), failed.values());
    }

    println!("Test passed after running for {:?}.", start_time.elapsed());
    Ok(())
}

async fn data_churn_with_network_restart(
    running_network: RunningNetwork,
    evm_network: &Network,
    funded_wallet: &Wallet,
    local: bool,
    upnp: bool,
    churn_period: Duration,
    total_period: Duration,
) -> Result<()> {
    println!("data churning for the network spawner initiated");
    let start = Instant::now();
    let mut churn_count = 1;

    let mut running_nodes = running_network.running_nodes().clone();
    'outer: loop {
        let mut restarted_nodes: Vec<RunningNode> = Vec::new();
        let mut initial_peers: Vec<Multiaddr> = vec![];
        for peer in running_nodes.iter() {
            if let Ok(listen_addrs_with_peer_id) = peer.get_listen_addrs_with_peer_id().await {
                initial_peers.extend(listen_addrs_with_peer_id);
            }
        }
        for nodes in running_nodes.clone() {
            sleep(churn_period).await;
            if start.elapsed() > total_period {
                println!("Total period elapsed. Stopping the churn.");
                info!("Total period elapsed. Stopping the churn.");
                break 'outer;
            }
            println!(
                "Churn #{churn_count} Churning a node with peer_id {:?}",
                nodes.peer_id()
            );
            nodes.clone().shutdown();
            println!("Restarting the node with peer_id {:?}", nodes.peer_id());

            churn_count += 1;
            let mut temp_peer = initial_peers.clone();
            if let Ok(listen_addrs_with_peer_id) = nodes.get_listen_addrs_with_peer_id().await {
                for exclude_addr in listen_addrs_with_peer_id {
                    initial_peers = temp_peer
                        .iter() // Use iter() to borrow, not move
                        .filter(|addr| *addr != &exclude_addr) // Deref to compare values
                        .cloned() // Clone to collect into a new Vec
                        .collect();
                    temp_peer = initial_peers.clone();
                }
            }

            let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
            let node = NodeSpawner::new()
                .with_socket_addr(socket_addr)
                .with_evm_network(evm_network.clone())
                .with_rewards_address(funded_wallet.address())
                .with_initial_peers(initial_peers.clone())
                .with_local(local)
                .with_upnp(upnp)
                .with_root_dir(None)
                .spawn()
                .await?;
            sleep(Duration::from_secs(2)).await;
            if let Ok(listen_addrs_with_peer_id) = node.get_listen_addrs_with_peer_id().await {
                initial_peers.extend(listen_addrs_with_peer_id);
            }
            println!(
                "A new Node joined the network with peer_id {:?}",
                node.peer_id()
            );
            restarted_nodes.push(node);
        }
        running_nodes = restarted_nodes;
    }
    Ok(())
}

// Spawns a task which periodically creates ScratchPads at random locations.
fn create_scratchpad_task(
    client: Client,
    wallet: Wallet,
    content: ContentList,
    churn_period: Duration,
) -> JoinHandle<Result<()>> {
    let handle: JoinHandle<Result<()>> = tokio::spawn(async move {
        // Map of the ownership, allowing the later on update can be undertaken.
        let mut owners: HashMap<ScratchpadAddress, SecretKey> = HashMap::new();

        // Create ScratchPad at a higher frequency than the churning events
        let delay = churn_period / SCRATCHPAD_CREATION_RATIO_TO_CHURN;

        loop {
            sleep(delay).await;

            // 50% chance to carry out update instead of creation.
            let is_update: bool = if owners.is_empty() {
                false
            } else {
                rand::random()
            };

            let content_type: u64 = rand::random();
            let data_byte: u8 = rand::random();
            let mut data = vec![data_byte; 100];
            rand::thread_rng().fill(&mut data[..]);
            let bytes = Bytes::from(data);

            let mut retries = 1;
            if is_update {
                let index = rand::thread_rng().gen_range(0..owners.len());
                let iterator: Vec<_> = owners.iter().collect();
                let (addr, owner) = iterator[index];

                loop {
                    match client.scratchpad_update(owner, content_type, &bytes).await {
                        Ok(_) => {
                            println!("Updated ScratchPad at {addr:?} after a delay of: {delay:?}");
                            break;
                        }
                        Err(err) => {
                            println!("Failed to update ScratchPad at {addr:?}. Retrying ...");
                            error!("Failed to update ScratchPad at {addr:?}. Retrying ...");
                            if retries >= 3 {
                                println!(
                                    "Failed to update pointer at {addr:?} after 3 retries: {err}"
                                );
                                error!(
                                    "Failed to update pointer at {addr:?} after 3 retries: {err}"
                                );
                                bail!(
                                    "Failed to update pointer at {addr:?} after 3 retries: {err}"
                                );
                            }
                            retries += 1;
                        }
                    }
                }
            } else {
                let owner = SecretKey::random();
                loop {
                    match client
                        .scratchpad_create(&owner, content_type, &bytes, (&wallet).into())
                        .await
                    {
                        Ok((cost, addr)) => {
                            println!("Created new ScratchPad at {addr:?} with cost of {cost:?} after a delay of: {delay:?}");
                            let net_addr = NetworkAddress::ScratchpadAddress(addr);
                            content.write().await.push_back(net_addr);
                            let _ = owners.insert(addr, owner);
                            break;
                        }
                        Err(err) => {
                            println!("Failed to create ScratchPad: {err:?}. Retrying ...");
                            error!("Failed to create ScratchPad: {err:?}. Retrying ...");
                            if retries >= 3 {
                                println!("Failed to create ScratchPad after 3 retries: {err}");
                                error!("Failed to create ScratchPad after 3 retries: {err}");
                                bail!("Failed to create ScratchPad after 3 retries: {err}");
                            }
                            retries += 1;
                        }
                    }
                }
            }
        }
    });
    handle
}

// Spawns a task which periodically creates GraphEntry at random locations.
fn create_graph_entry_task(
    client: Client,
    wallet: Wallet,
    content_list: ContentList,
    churn_period: Duration,
) -> JoinHandle<Result<()>> {
    let handle: JoinHandle<Result<()>> = tokio::spawn(async move {
        // Map of the ownership, allowing the later on update can be undertaken.
        // In this test scenario, we only test simple GraphEntry tree structure: 1-parent-1-output
        // The tree structure is stored as a vector (last one being the latest)
        let mut growing_history: Vec<Vec<GraphEntryAddress>> = vec![];
        let mut owners: HashMap<PublicKey, SecretKey> = HashMap::new();

        // Create GraphEntry at a higher frequency than the churning events
        let delay = churn_period / GRAPHENTRY_CREATION_RATIO_TO_CHURN;

        loop {
            sleep(delay).await;

            // 50% chance of `growing` (i.e. has existing one as partent) instead of creation new.
            let is_growing: bool = if growing_history.is_empty() {
                false
            } else {
                rand::random()
            };

            let output = SecretKey::random();
            let output_content: [u8; 32] = rand::random();
            let outputs = vec![(output.public_key(), output_content)];

            #[allow(unused_assignments)]
            let mut index = growing_history.len();
            let mut graph_entry_to_put = None;
            if is_growing {
                index = rand::thread_rng().gen_range(0..growing_history.len());
                let Some(addr) = growing_history[index].last() else {
                    println!("Doesn't have history GraphEntry of {index:?}");
                    error!("Doesn't have history GraphEntry of {index:?}");
                    continue;
                };

                let mut retries = 1;
                loop {
                    match client.graph_entry_get(addr).await {
                        Ok(graph_entry) => {
                            println!("Fetched graph_entry at {addr:?}");

                            let Some((old_output, old_content)) = graph_entry.descendants.last()
                            else {
                                println!("Can't get output from the graph_entry of {addr:?}");
                                error!("Can't get output from the graph_entry of {addr:?}");
                                break;
                            };

                            // The previous output now becomes the owner.
                            let Some(owner) = owners.get(old_output) else {
                                println!("Can't get secret_key of {output:?}");
                                error!("Can't get secret_key of {output:?}");
                                break;
                            };

                            let parents = vec![graph_entry.owner];
                            let graph_entry =
                                GraphEntry::new(owner, parents, *old_content, outputs);

                            growing_history[index].push(graph_entry.address());

                            graph_entry_to_put = Some(graph_entry);
                            break;
                        }
                        Err(err) => {
                            println!(
                                "Failed to get graph_entry at {addr:?}: {err:?}. Retrying ..."
                            );
                            error!("Failed to get graph_entry at {addr:?} : {err:?}. Retrying ...");
                            if retries >= 3 {
                                println!(
                                    "Failed to get graph_entry at {addr:?} after 3 retries: {err}"
                                );
                                error!(
                                    "Failed to get graph_entry at {addr:?} after 3 retries: {err}"
                                );
                                bail!(
                                    "Failed to get graph_entry at {addr:?} after 3 retries: {err}"
                                );
                            }
                            retries += 1;
                            sleep(delay).await;
                        }
                    }
                }
            } else {
                let owner = SecretKey::random();
                let content: [u8; 32] = rand::random();
                let parents = vec![];
                let graph_entry = GraphEntry::new(&owner, parents, content, outputs);

                growing_history.push(vec![graph_entry.address()]);
                let _ = owners.insert(owner.public_key(), owner);

                graph_entry_to_put = Some(graph_entry);
            };

            let Some(graph_entry) = graph_entry_to_put else {
                println!("Doesn't have graph_entry to put to network.");
                error!("Doesn't have graph_entry to put to network.");
                continue;
            };

            let _ = owners.insert(output.public_key(), output);

            let mut retries = 1;
            loop {
                match client
                    .graph_entry_put(graph_entry.clone(), (&wallet).into())
                    .await
                {
                    Ok((cost, addr)) => {
                        println!("Uploaded graph_entry to {addr:?} with cost of {cost:?} after a delay of: {delay:?}");
                        let net_addr = NetworkAddress::GraphEntryAddress(addr);
                        content_list.write().await.push_back(net_addr);
                        break;
                    }
                    Err(err) => {
                        println!("Failed to upload graph_entry: {err:?}. Retrying ...");
                        error!("Failed to upload graph_entry: {err:?}. Retrying ...");
                        if retries >= 3 {
                            println!("Failed to upload graph_entry after 3 retries: {err}");
                            error!("Failed to upload graph_entry after 3 retries: {err}");
                            bail!("Failed to upload graph_entry after 3 retries: {err}");
                        }
                        retries += 1;
                        sleep(delay).await;
                    }
                }
            }
        }
    });
    handle
}

// Spawns a task which periodically creates Pointers at random locations.
fn create_pointers_task(
    client: Client,
    wallet: Wallet,
    content: ContentList,
    churn_period: Duration,
) -> JoinHandle<Result<()>> {
    let handle: JoinHandle<Result<()>> = tokio::spawn(async move {
        // Map of the ownership, allowing the later on update can be undertaken.
        let mut owners: HashMap<NetworkAddress, (SecretKey, PointerTarget)> = HashMap::new();

        // Create Pointers at a higher frequency than the churning events
        let delay = churn_period / POINTER_CREATION_RATIO_TO_CHURN;

        loop {
            sleep(delay).await;

            #[allow(unused_assignments)]
            let mut pointer_addr = None;

            // 50% chance to carry out update instead of creation.
            let is_update: bool = if owners.is_empty() {
                false
            } else {
                rand::random()
            };

            let mut retries = 1;

            if is_update {
                let index = rand::thread_rng().gen_range(0..owners.len());
                let iterator: Vec<_> = owners.iter().collect();
                let (addr, (owner, old_target)) = iterator[index];

                let new_target =
                    PointerTarget::ChunkAddress(ChunkAddress::new(XorName(rand::random())));
                loop {
                    match client.pointer_update(owner, new_target.clone()).await {
                        Ok(_) => {
                            println!("Updated Pointer at {addr:?} with {old_target:?} to new target {new_target:?} after a delay of: {delay:?}");
                            pointer_addr = Some((addr.clone(), None, new_target));
                            break;
                        }
                        Err(err) => {
                            println!(
                                "Failed to update pointer at {addr:?} with {old_target:?}: {err:?}. Retrying ..."
                            );
                            error!(
                                "Failed to update pointer at {addr:?} with {old_target:?}: {err:?}. Retrying ..."
                            );
                            if retries >= 3 {
                                println!("Failed to update pointer at {addr:?} with {old_target:?} after 3 retries: {err}");
                                error!("Failed to update pointer at {addr:?} with {old_target:?} after 3 retries: {err}");
                                bail!("Failed to update pointer at {addr:?} with {old_target:?} after 3 retries: {err}");
                            }
                            retries += 1;
                        }
                    }
                }
            } else {
                let owner = SecretKey::random();
                let pointer_target =
                    PointerTarget::ChunkAddress(ChunkAddress::new(XorName(rand::random())));
                loop {
                    match client
                        .pointer_create(&owner, pointer_target.clone(), (&wallet).into())
                        .await
                    {
                        Ok((cost, addr)) => {
                            println!("Created new Pointer ({pointer_target:?}) at {addr:?} with cost of {cost:?} after a delay of: {delay:?}");
                            let net_addr = NetworkAddress::PointerAddress(addr);
                            pointer_addr = Some((net_addr.clone(), Some(owner), pointer_target));
                            content.write().await.push_back(net_addr);
                            break;
                        }
                        Err(err) => {
                            println!(
                                "Failed to create pointer {pointer_target:?}: {err:?}. Retrying ..."
                            );
                            error!(
                                "Failed to create pointer {pointer_target:?}: {err:?}. Retrying ..."
                            );
                            if retries >= 3 {
                                println!("Failed to create pointer {pointer_target:?} after 3 retries: {err}");
                                error!("Failed to create pointer {pointer_target:?} after 3 retries: {err}");
                                bail!("Failed to create pointer {pointer_target:?} after 3 retries: {err}");
                            }
                            retries += 1;
                        }
                    }
                }
            }
            match pointer_addr {
                Some((addr, Some(owner), target)) => {
                    let _ = owners.insert(addr, (owner, target));
                }
                Some((addr, None, new_target)) => {
                    if let Some((_owner, target)) = owners.get_mut(&addr) {
                        *target = new_target;
                    }
                }
                _ => {}
            }
        }
    });
    handle
}

// Spawns a task which periodically stores Chunks at random locations.
fn store_chunks_task(
    client: Client,
    wallet: Wallet,
    content: ContentList,
    churn_period: Duration,
) -> JoinHandle<Result<()>> {
    let handle: JoinHandle<Result<()>> = tokio::spawn(async move {
        let temp_dir = tempdir().expect("Can not create a temp directory for store_chunks_task!");
        let output_dir = temp_dir.path().join("chunk_path");
        create_dir_all(output_dir.clone())
            .expect("failed to create output dir for encrypted chunks");

        // Store Chunks at a higher frequency than the churning events
        let delay = churn_period / CHUNK_CREATION_RATIO_TO_CHURN;

        loop {
            let random_data = gen_random_data(*DATA_SIZE);

            // FIXME: The client does not have the retry repay to different payee feature yet.
            // Retry here for now
            let mut retries = 1;
            loop {
                match client
                    .data_put_public(random_data.clone(), (&wallet).into())
                    .await
                    .inspect_err(|err| {
                        println!("Error to put chunk: {err:?}");
                        error!("Error to put chunk: {err:?}")
                    }) {
                    Ok((_cost, data_map)) => {
                        println!("Stored Chunk/s at {data_map:?} after a delay of: {delay:?}");
                        info!("Stored Chunk/s at {data_map:?} after a delay of: {delay:?}");

                        content
                            .write()
                            .await
                            .push_back(NetworkAddress::ChunkAddress(ChunkAddress::new(
                                *data_map.xorname(),
                            )));
                        break;
                    }
                    Err(err) => {
                        println!("Failed to store chunk: {err:?}. Retrying ...");
                        error!("Failed to store chunk: {err:?}. Retrying ...");
                        if retries >= 3 {
                            println!("Failed to store chunk after 3 retries: {err}");
                            error!("Failed to store chunk after 3 retries: {err}");
                            bail!("Failed to store chunk after 3 retries: {err}");
                        }
                        retries += 1;
                    }
                }
            }

            sleep(delay).await;
        }
    });
    handle
}

// Spawns a task which periodically queries a content by randomly choosing it from the list
// of content created by another task.
fn query_content_task(
    client: Client,
    content: ContentList,
    content_erred: ContentErredList,
    churn_period: Duration,
) {
    let _handle = tokio::spawn(async move {
        let delay = churn_period / CONTENT_QUERY_RATIO_TO_CHURN;
        loop {
            let len = content.read().await.len();
            if len == 0 {
                println!("No content created/stored just yet, let's try in {delay:?} ...");
                info!("No content created/stored just yet, let's try in {delay:?} ...");
                sleep(delay).await;
                continue;
            }

            // let's choose a random content to query, picking it from the list of created
            let index = rand::thread_rng().gen_range(0..len);
            let net_addr = content.read().await[index].clone();
            trace!("Querying content (bucket index: {index}) at {net_addr:?} in {delay:?}");
            sleep(delay).await;

            match query_content(&client, &net_addr).await {
                Ok(_) => {
                    let _ = content_erred.write().await.remove(&net_addr);
                }
                Err(last_err) => {
                    println!(
                        "Failed to query content (index: {index}) at {net_addr}: {last_err:?}"
                    );
                    error!("Failed to query content (index: {index}) at {net_addr}: {last_err:?}");
                    // mark it to try 'MAX_NUM_OF_QUERY_ATTEMPTS' times.
                    let _ = content_erred
                        .write()
                        .await
                        .entry(net_addr.clone())
                        .and_modify(|curr| curr.attempts += 1)
                        .or_insert(ContentError {
                            net_addr,
                            attempts: 1,
                            last_err,
                        });
                }
            }
        }
    });
}

// Checks (periodically) for any content that an error was reported either at the moment of its creation or
// in a later query attempt.
fn retry_query_content_task(
    client: Client,
    content_erred: ContentErredList,
    failures: ContentErredList,
    churn_period: Duration,
) {
    let _handle = tokio::spawn(async move {
        let delay = 2 * churn_period;
        loop {
            sleep(delay).await;

            // let's try to query from the bucket of those that erred upon creation/query
            let erred = content_erred.write().await.pop_first();

            if let Some((net_addr, mut content_error)) = erred {
                let attempts = content_error.attempts + 1;

                println!("Querying erred content at {net_addr}, attempt: #{attempts} ...");
                info!("Querying erred content at {net_addr}, attempt: #{attempts} ...");
                if let Err(last_err) = query_content(&client, &net_addr).await {
                    println!("Erred content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
                    warn!("Erred content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
                    // We only keep it to retry 'MAX_NUM_OF_QUERY_ATTEMPTS' times,
                    // otherwise report it effectivelly as failure.
                    content_error.attempts = attempts;
                    content_error.last_err = last_err;

                    if attempts == MAX_NUM_OF_QUERY_ATTEMPTS {
                        let _ = failures.write().await.insert(net_addr, content_error);
                    } else {
                        let _ = content_erred.write().await.insert(net_addr, content_error);
                    }
                } else {
                    // remove from fails and errs if we had a success and it was added meanwhile perchance
                    let _ = failures.write().await.remove(&net_addr);
                    let _ = content_erred.write().await.remove(&net_addr);
                }
            }
        }
    });
}

async fn final_retry_query_content(
    client: &Client,
    net_addr: &NetworkAddress,
    churn_period: Duration,
    failures: ContentErredList,
) -> Result<()> {
    let mut attempts = 1;
    let net_addr = net_addr.clone();
    loop {
        println!("Final querying content at {net_addr}, attempt: #{attempts} ...");
        debug!("Final querying content at {net_addr}, attempt: #{attempts} ...");
        if let Err(last_err) = query_content(client, &net_addr).await {
            if attempts == MAX_NUM_OF_QUERY_ATTEMPTS {
                println!("Final check: Content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
                error!("Final check: Content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
                bail!("Final check: Content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
            } else {
                attempts += 1;
                let delay = 2 * churn_period;
                debug!("Delaying last check of {net_addr:?} for {delay:?} ...");
                sleep(delay).await;
                continue;
            }
        } else {
            failures.write().await.remove(&net_addr);
            // content retrieved fine
            return Ok(());
        }
    }
}

async fn query_content(client: &Client, net_addr: &NetworkAddress) -> Result<()> {
    match net_addr {
        NetworkAddress::ChunkAddress(addr) => {
            client
                .data_get_public(&DataAddress::new(*addr.xorname()))
                .await?;
            Ok(())
        }
        NetworkAddress::PointerAddress(addr) => {
            let _ = client.pointer_get(addr).await?;
            Ok(())
        }
        NetworkAddress::GraphEntryAddress(addr) => {
            let _ = client.graph_entry_get(addr).await?;
            Ok(())
        }
        NetworkAddress::ScratchpadAddress(addr) => {
            let _ = client.scratchpad_get(addr).await?;
            Ok(())
        }
        // Drain the enum to ensure all native supported data_types are covered
        NetworkAddress::PeerId(_) | NetworkAddress::RecordKey(_) => Ok(()),
    }
}
