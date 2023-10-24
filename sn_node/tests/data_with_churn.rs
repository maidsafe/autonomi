// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod common;

use assert_fs::TempDir;
use common::{
    get_client_and_wallet, get_funded_wallet, get_wallet, node_restart,
    PAYING_WALLET_INITIAL_BALANCE,
};
use eyre::{bail, eyre, Result};
use rand::{rngs::OsRng, Rng};
use sn_client::{Client, Error, Files, WalletClient};
use sn_logging::LogBuilder;
use sn_protocol::{
    storage::{ChunkAddress, RegisterAddress, SpendAddress},
    NetworkAddress,
};
use sn_transfers::LocalWallet;
use sn_transfers::{CashNote, MainSecretKey, NanoTokens};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    fs::{create_dir_all, File},
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use tokio::{sync::RwLock, task::JoinHandle, time::sleep};
use tracing::{debug, trace};
use xor_name::XorName;

const NODE_COUNT: u32 = 25;

const EXTRA_CHURN_COUNT: u32 = 5;
const CHURN_CYCLES: u32 = 1;
const CHUNK_CREATION_RATIO_TO_CHURN: u32 = 15;
const REGISTER_CREATION_RATIO_TO_CHURN: u32 = 5;
const CASHNOTE_CREATION_RATIO_TO_CHURN: u32 = 1;

const CHUNKS_SIZE: usize = 1024 * 1024;

const CONTENT_QUERY_RATIO_TO_CHURN: u32 = 12;
const MAX_NUM_OF_QUERY_ATTEMPTS: u8 = 5;

// Default total amount of time we run the checks for before reporting the outcome.
// It can be overriden by setting the 'TEST_DURATION_MINS' env var.
const TEST_DURATION: Duration = Duration::from_secs(60 * 60); // 1hr

const TRANSFERS_WALLET_INITIAL_BALANCE: u64 = 200_000_000_000;

type ContentList = Arc<RwLock<VecDeque<NetworkAddress>>>;
type CashNoteMap = Arc<RwLock<BTreeMap<SpendAddress, CashNote>>>;

struct ContentError {
    net_addr: NetworkAddress,
    attempts: u8,
    last_err: Error,
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
    let _log_appender_guard = LogBuilder::init_multi_threaded_tokio_test("data_with_churn");

    let test_duration = if let Ok(str) = std::env::var("TEST_DURATION_MINS") {
        Duration::from_secs(60 * str.parse::<u64>()?)
    } else {
        TEST_DURATION
    };

    let churn_period = if let Ok(str) = std::env::var("TEST_TOTAL_CHURN_CYCLES") {
        println!("Using value set in 'TEST_TOTAL_CHURN_CYCLES' env var: {str}");
        let cycles = str.parse::<u32>()?;
        test_duration / cycles
    } else {
        // Ensure at least some nodes got churned twice.
        test_duration / std::cmp::max(CHURN_CYCLES * NODE_COUNT, NODE_COUNT + EXTRA_CHURN_COUNT)
    };
    println!("Nodes will churn every {churn_period:?}");

    // Create a cross thread usize for tracking churned nodes
    let churn_count = Arc::new(RwLock::new(0_usize));

    // Allow to disable Registers data creation/checks, storing and querying only Chunks during churn.
    // Default to be not carry out chunks only during churn.
    let chunks_only = std::env::var("CHUNKS_ONLY").is_ok();

    println!(
        "Running this test for {test_duration:?}{}...",
        if chunks_only { " (Chunks only)" } else { "" }
    );

    // The testnet will create a `faucet` at last. To avoid mess up with that,
    // wait for a while to ensure the spends of that got settled.
    sleep(std::time::Duration::from_secs(10)).await;

    println!("Creating a client and paying wallet...");
    let paying_wallet_dir = TempDir::new()?;
    let (client, paying_wallet) =
        get_client_and_wallet(paying_wallet_dir.path(), PAYING_WALLET_INITIAL_BALANCE).await?;

    // Waiting for the paying_wallet funded.
    sleep(std::time::Duration::from_secs(10)).await;

    println!(
        "Client and paying_wallet created with signing key: {:?}",
        client.signer_pk()
    );

    // Shared bucket where we keep track of content created/stored on the network
    let content = ContentList::default();

    // Shared bucket where we keep track of CashNotes created/stored on the network
    let cash_notes = CashNoteMap::default();

    // Spawn a task to create Registers and CashNotes at random locations,
    // at a higher frequency than the churning events
    if !chunks_only {
        println!("Creating transfer wallet taking balance from the payment wallet");
        let transfers_wallet_dir = TempDir::new()?;
        let transfers_wallet = get_funded_wallet(
            &client,
            paying_wallet,
            transfers_wallet_dir.path(),
            TRANSFERS_WALLET_INITIAL_BALANCE,
        )
        .await?;
        println!("Transfer wallet created");

        // Waiting for the transfers_wallet funded.
        sleep(std::time::Duration::from_secs(10)).await;

        create_registers_task(
            client.clone(),
            content.clone(),
            churn_period,
            paying_wallet_dir.path().to_path_buf(),
        );

        create_cash_note_task(
            client.clone(),
            transfers_wallet,
            content.clone(),
            cash_notes.clone(),
            churn_period,
        );
    }

    println!("Uploading some chunks before carry out node churning");

    // Spawn a task to store Chunks at random locations, at a higher frequency than the churning events
    store_chunks_task(
        client.clone(),
        content.clone(),
        churn_period,
        paying_wallet_dir.path().to_path_buf(),
    );

    // Spawn a task to churn nodes
    churn_nodes_task(churn_count.clone(), test_duration, churn_period);

    // Shared bucket where we keep track of the content which erred when creating/storing/fetching.
    // We remove them from this bucket if we are then able to query/fetch them successfully.
    // We only try to query them 'MAX_NUM_OF_QUERY_ATTEMPTS' times, then report them effectivelly as failures.
    let content_erred = ContentErredList::default();

    // Shared bucket where we keep track of the content we failed to fetch for 'MAX_NUM_OF_QUERY_ATTEMPTS' times.
    let failures = ContentErredList::default();

    // Spawn a task to randomly query/fetch the content we create/store
    query_content_task(
        client.clone(),
        content.clone(),
        content_erred.clone(),
        cash_notes.clone(),
        churn_period,
        paying_wallet_dir.path().to_path_buf(),
    );

    // Spawn a task to retry querying the content that failed, up to 'MAX_NUM_OF_QUERY_ATTEMPTS' times,
    // and mark them as failures if they effectivelly cannot be retrieved.
    retry_query_content_task(
        client.clone(),
        content_erred.clone(),
        failures.clone(),
        cash_notes.clone(),
        churn_period,
        paying_wallet_dir.path().to_path_buf(),
    );

    println!("All tasks have been spawned. The test is now running...");

    let start_time = Instant::now();
    while start_time.elapsed() < test_duration {
        let failed = failures.read().await;
        println!(
            "Current failures after {:?} ({}): {:?}",
            start_time.elapsed(),
            failed.len(),
            failed.values()
        );
        sleep(churn_period).await;
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
    debug!("Final querying confirmation of content");

    // take one read lock to avoid holding the lock for the whole loop
    // prevent any late content uploads being added to the list
    let content = content.read().await;
    let uploaded_content_count = content.len();
    let mut handles = Vec::new();
    for net_addr in content.iter() {
        let client = client.clone();
        let net_addr = net_addr.clone();
        let cash_notes = cash_notes.clone();

        let failures = failures.clone();
        let wallet_dir = paying_wallet_dir.to_path_buf().clone();
        let handle = tokio::spawn(async move {
            final_retry_query_content(
                &client,
                &net_addr,
                cash_notes,
                churn_period,
                failures,
                &wallet_dir,
            )
            .await
        });
        handles.push(handle);
    }
    let results: Vec<_> = futures::future::join_all(handles).await;

    let content_queried_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(
        content_queried_count, uploaded_content_count,
        "Not all content was queried successfully"
    );

    println!("{:?} pieces of content queried", content_queried_count);

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

// Spawns a task which periodically creates CashNotes at random locations.
fn create_cash_note_task(
    client: Client,
    transfers_wallet: LocalWallet,
    content: ContentList,
    cash_notes: CashNoteMap,
    churn_period: Duration,
) {
    let _handle = tokio::spawn(async move {
        // Create CashNote at a higher frequency than the churning events
        let delay = churn_period / CASHNOTE_CREATION_RATIO_TO_CHURN;

        let mut wallet_client = WalletClient::new(client.clone(), transfers_wallet);

        loop {
            sleep(delay).await;

            let dest_pk = MainSecretKey::random().main_pubkey();
            let cash_note = wallet_client
                .send_cash_note(NanoTokens::from(10), dest_pk, false)
                .await
                .expect("Failed to send CashNote to {dest_pk}");

            let cash_note_addr = SpendAddress::from_unique_pubkey(&cash_note.unique_pubkey());
            let net_addr = NetworkAddress::SpendAddress(cash_note_addr);
            println!("Created CashNote at {cash_note_addr:?} after {delay:?}");
            content.write().await.push_back(net_addr);
            let _ = cash_notes.write().await.insert(cash_note_addr, cash_note);
        }
    });
}

// Spawns a task which periodically creates Registers at random locations.
fn create_registers_task(
    client: Client,
    content: ContentList,
    churn_period: Duration,
    paying_wallet_dir: PathBuf,
) {
    let _handle = tokio::spawn(async move {
        // Create Registers at a higher frequency than the churning events
        let delay = churn_period / REGISTER_CREATION_RATIO_TO_CHURN;

        let paying_wallet = get_wallet(&paying_wallet_dir).await;

        let mut wallet_client = WalletClient::new(client.clone(), paying_wallet);

        loop {
            let meta = XorName(rand::random());
            let owner = client.signer_pk();

            let addr = RegisterAddress::new(meta, owner);
            println!("Creating Register at {addr:?} in {delay:?}");
            sleep(delay).await;

            match client
                .create_and_pay_for_register(meta, &mut wallet_client, true)
                .await
            {
                Ok(_) => content
                    .write()
                    .await
                    .push_back(NetworkAddress::RegisterAddress(addr)),
                Err(err) => println!("Discarding new Register ({addr:?}) due to error: {err:?}"),
            }
        }
    });
}

// Spawns a task which periodically stores Chunks at random locations.
fn store_chunks_task(
    client: Client,
    content: ContentList,
    churn_period: Duration,
    paying_wallet_dir: PathBuf,
) {
    let _handle: JoinHandle<Result<()>> = tokio::spawn(async move {
        let temp_dir = tempdir().expect("Can not create a temp directory for store_chunks_task!");
        let output_dir = temp_dir.path().join("chunk_path");
        create_dir_all(output_dir.clone())
            .expect("failed to create output dir for encrypted chunks");

        // Store Chunks at a higher frequency than the churning events
        let delay = churn_period / CHUNK_CREATION_RATIO_TO_CHURN;

        let file_api = Files::new(client, paying_wallet_dir);
        let mut rng = OsRng;

        loop {
            let random_bytes: Vec<u8> = ::std::iter::repeat(())
                .map(|()| rng.gen::<u8>())
                .take(CHUNKS_SIZE)
                .collect();
            let chunk_size = random_bytes.len();

            let chunk_name = XorName::from_content(&random_bytes);

            let file_path = temp_dir.path().join(&hex::encode(chunk_name));
            let mut chunk_file =
                File::create(&file_path).expect("failed to create temp chunk file");
            chunk_file
                .write_all(&random_bytes)
                .expect("failed to write to temp chunk file");

            let (addr, _file_size, chunks) = file_api
                .chunk_file(&file_path, &output_dir)
                .expect("Failed to chunk bytes");

            println!(
                "Paying storage for ({}) new Chunk/s of file ({} bytes) at {addr:?} in {delay:?}",
                chunks.len(),
                chunk_size
            );
            sleep(delay).await;

            let chunks_len = chunks.len();

            let (addr, cost) = match file_api
                .pay_and_upload_bytes_test(chunk_name, chunks.clone())
                .await
            {
                Ok((addr, storage_cost, royalties_fees)) => {
                    let cost = storage_cost
                        .checked_add(royalties_fees)
                        .ok_or(eyre!("Total storage cost exceed possible token amount"))?;

                    (addr, cost)
                }
                Err(err) => {
                    bail!("Bailing w/ new Chunk ({addr:?}) due to error: {err:?}");
                }
            };

            println!(
                "Storing ({}) Chunk/s at cost: {cost:?} of file ({} bytes) at {addr:?} in {delay:?}",
                chunks_len,
                chunk_size
            );
            sleep(delay).await;

            for (chunk_name, _chunk_path) in chunks {
                content
                    .write()
                    .await
                    .push_back(NetworkAddress::ChunkAddress(ChunkAddress::new(chunk_name)));
            }
        }
    });
}

// Spawns a task which periodically queries a content by randomly choosing it from the list
// of content created by another task.
fn query_content_task(
    client: Client,
    content: ContentList,
    content_erred: ContentErredList,
    cash_notes: CashNoteMap,
    churn_period: Duration,
    root_dir: PathBuf,
) {
    let _handle = tokio::spawn(async move {
        let delay = churn_period / CONTENT_QUERY_RATIO_TO_CHURN;
        loop {
            let len = content.read().await.len();
            if len == 0 {
                println!("No content created/stored just yet, let's try in {delay:?} ...");
                sleep(delay).await;
                continue;
            }

            // let's choose a random content to query, picking it from the list of created
            let index = rand::thread_rng().gen_range(0..len);
            let net_addr = content.read().await[index].clone();
            trace!("Querying content (bucket index: {index}) at {net_addr:?} in {delay:?}");
            sleep(delay).await;

            match query_content(&client, &root_dir, &net_addr, cash_notes.clone()).await {
                Ok(_) => {
                    let _ = content_erred.write().await.remove(&net_addr);
                }
                Err(last_err) => {
                    println!(
                        "Failed to query content (index: {index}) at {net_addr}: {last_err:?}"
                    );
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

// Spawns a task which periodically picks up a node, and restarts it to cause churn in the network.
fn churn_nodes_task(
    churn_count: Arc<RwLock<usize>>,
    test_duration: Duration,
    churn_period: Duration,
) {
    let start = Instant::now();
    let _handle = tokio::spawn(async move {
        let mut node_index = 1;
        let mut addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12000);
        loop {
            sleep(churn_period).await;

            // break out if we've run the duration of churn
            if start.elapsed() > test_duration {
                println!("Test duration reached, stopping churn nodes task");
                break;
            }

            addr.set_port(12000 + node_index);

            println!("Restarting node through its RPC service at {addr}");

            if let Err(err) = node_restart(addr).await {
                println!("Failed to restart node with RPC endpoint {addr}: {err}");
                continue;
            }

            *churn_count.write().await += 1;

            node_index += 1;
            if node_index > NODE_COUNT as u16 {
                node_index = 1;
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
    cash_notes: CashNoteMap,
    churn_period: Duration,
    wallet_dir: PathBuf,
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
                if let Err(last_err) =
                    query_content(&client, &wallet_dir, &net_addr, cash_notes.clone()).await
                {
                    println!("Erred content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
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
    cash_notes: CashNoteMap,
    churn_period: Duration,
    failures: ContentErredList,
    wallet_dir: &Path,
) -> Result<()> {
    let mut attempts = 1;
    let net_addr = net_addr.clone();
    loop {
        println!("Final querying content at {net_addr}, attempt: #{attempts} ...");
        debug!("Final querying content at {net_addr}, attempt: #{attempts} ...");
        if let Err(last_err) =
            query_content(client, wallet_dir, &net_addr, cash_notes.clone()).await
        {
            if attempts == MAX_NUM_OF_QUERY_ATTEMPTS {
                println!("Final check: Content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
                bail!("Final check: Content is still not retrievable at {net_addr} after {attempts} attempts: {last_err:?}");
            } else {
                attempts += 1;
                let delay = 2 * churn_period;
                println!("Delaying last check for {delay:?} ...");
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

async fn query_content(
    client: &Client,
    wallet_dir: &Path,
    net_addr: &NetworkAddress,
    cash_notes: CashNoteMap,
) -> Result<(), Error> {
    match net_addr {
        NetworkAddress::SpendAddress(addr) => {
            if let Some(cash_note) = cash_notes.read().await.get(addr) {
                match client.verify(cash_note).await {
                    Ok(_) => Ok(()),
                    Err(err) => Err(Error::CouldNotVerifyTransfer(format!(
                        "Verification of cash_note {addr:?} failed with error: {err:?}"
                    ))),
                }
            } else {
                Err(Error::CouldNotVerifyTransfer(format!(
                    "Do not have the CashNote: {addr:?}"
                )))
            }
        }
        NetworkAddress::RegisterAddress(addr) => {
            let _ = client.get_register(*addr).await?;
            Ok(())
        }
        NetworkAddress::ChunkAddress(addr) => {
            let file_api = Files::new(client.clone(), wallet_dir.to_path_buf());
            let _ = file_api.read_bytes(*addr, None, false).await?;
            Ok(())
        }
        _other => Ok(()), // we don't create/store any other type of content in this test yet
    }
}
