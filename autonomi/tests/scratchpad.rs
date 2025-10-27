// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::AttoTokens;
use autonomi::client::payment::PaymentOption;
use autonomi::scratchpad::ScratchpadError;
use autonomi::{
    Client,
    client::data_types::scratchpad::print_fork_analysis,
    client::scratchpad::{Bytes, Scratchpad},
};
use eyre::{Result, eyre};
use serial_test::serial;
use test_utils::evm::get_funded_wallet;

#[tokio::test]
#[serial]
async fn scratchpad_put_manual() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let key = bls::SecretKey::random();
    let public_key = key.public_key();
    let content = Bytes::from("Massive Array of Internet Disks");
    let scratchpad = Scratchpad::new(&key, 42, &content, 0);

    // estimate the cost of the scratchpad
    let cost = client.scratchpad_cost(&public_key).await?;
    println!("scratchpad cost: {cost}");

    // put the scratchpad
    let payment_option = PaymentOption::from(&wallet);
    let (cost, addr) = client
        .scratchpad_put(scratchpad.clone(), payment_option)
        .await?;
    assert_eq!(addr, *scratchpad.address());
    println!("scratchpad put 1 cost: {cost}");

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is stored
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(got, scratchpad.clone());
    println!("scratchpad got 1");

    // check that the content is decrypted correctly
    let got_content = got.decrypt_data(&key)?;
    assert_eq!(got_content, content);

    // try update scratchpad
    let content2 = Bytes::from("Secure Access For Everyone");
    let scratchpad2 = Scratchpad::new(&key, 42, &content2, 1);
    let payment_option = PaymentOption::from(&wallet);
    let (cost, _) = client
        .scratchpad_put(scratchpad2.clone(), payment_option)
        .await?;
    assert_eq!(cost, AttoTokens::zero());
    println!("scratchpad put 2 cost: {cost}");

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is updated
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(got, scratchpad2.clone());
    println!("scratchpad got 2");

    // check that the content is decrypted correctly
    let got_content2 = got.decrypt_data(&key)?;
    assert_eq!(got_content2, content2);

    Ok(())
}

#[tokio::test]
#[serial]
async fn scratchpad_put_update_manual() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let key = bls::SecretKey::random();
    let public_key = key.public_key();
    let content = Bytes::from("Massive Array of Internet Disks");
    let scratchpad = Scratchpad::new(&key, 42, &content, 0);

    // estimate the cost of the scratchpad
    let cost = client.scratchpad_cost(&public_key).await?;
    println!("scratchpad cost: {cost}");

    // put the scratchpad
    let payment_option = PaymentOption::from(&wallet);
    let (cost, addr) = client
        .scratchpad_put(scratchpad.clone(), payment_option)
        .await?;
    assert_eq!(addr, *scratchpad.address());
    println!("scratchpad put 1 cost: {cost}");

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is stored
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(got, scratchpad.clone());
    println!("scratchpad got 1");

    // check that the content is decrypted correctly
    let got_content = got.decrypt_data(&key)?;
    assert_eq!(got_content, content);

    // try update scratchpad
    let content2 = Bytes::from("Secure Access For Everyone");
    let scratchpad2 = Scratchpad::new(&key, 42, &content2, 1);
    client.scratchpad_put_update(scratchpad2.clone()).await?;

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is updated
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(got, scratchpad2.clone());
    println!("scratchpad got 2");

    // check that the content is decrypted correctly
    let got_content2 = got.decrypt_data(&key)?;
    assert_eq!(got_content2, content2);

    Ok(())
}

#[tokio::test]
#[serial]
async fn scratchpad_put() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let key = bls::SecretKey::random();
    let public_key = key.public_key();
    let content = Bytes::from("what's the meaning of life the universe and everything?");
    let content_type = 42;

    // estimate the cost of the scratchpad
    let cost = client.scratchpad_cost(&public_key).await?;
    println!("scratchpad cost: {cost}");

    // put the scratchpad
    let payment_option = PaymentOption::from(&wallet);
    let (cost, addr) = client
        .scratchpad_create(&key, content_type, &content, payment_option)
        .await?;
    println!("scratchpad create cost: {cost}");

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is stored
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(*got.owner(), public_key);
    assert_eq!(got.data_encoding(), content_type);
    assert_eq!(got.decrypt_data(&key), Ok(content.clone()));
    assert_eq!(got.counter(), 0);
    assert!(got.verify_signature());
    println!("scratchpad got 1");

    // check that the content is decrypted correctly
    let got_content = got.decrypt_data(&key)?;
    assert_eq!(got_content, content);

    // try update scratchpad
    let content2 = Bytes::from("42");
    client
        .scratchpad_update(&key, content_type, &content2)
        .await?;

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is updated
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(*got.owner(), public_key);
    assert_eq!(got.data_encoding(), content_type);
    assert_eq!(got.decrypt_data(&key), Ok(content2.clone()));
    assert_eq!(got.counter(), 1);
    assert!(got.verify_signature());
    println!("scratchpad got 2");

    // check that the content is decrypted correctly
    let got_content2 = got.decrypt_data(&key)?;
    assert_eq!(got_content2, content2);
    Ok(())
}

#[tokio::test]
#[serial]
async fn scratchpad_errors() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let key = bls::SecretKey::random();
    let content = Bytes::from("what's the meaning of life the universe and everything?");
    let content_type = 42;

    // try update scratchpad, it should fail as we haven't created it
    let res = client.scratchpad_update(&key, content_type, &content).await;
    println!("scratchpad update should fail here: {res:?}");
    assert!(matches!(
        res,
        Err(ScratchpadError::CannotUpdateNewScratchpad)
    ));

    // put the scratchpad normally
    let payment_option = PaymentOption::from(&wallet);
    let (cost, addr) = client
        .scratchpad_create(&key, content_type, &content, payment_option)
        .await?;
    println!("scratchpad create cost: {cost}");

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is stored
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(*got.owner(), key.public_key());
    assert_eq!(got.data_encoding(), content_type);
    assert_eq!(got.decrypt_data(&key), Ok(content.clone()));
    assert_eq!(got.counter(), 0);
    assert!(got.verify_signature());
    println!("scratchpad got 1");

    // try create scratchpad at the same address
    let fork_content = Bytes::from("Fork");
    let payment_option = PaymentOption::from(&wallet);
    let res = client
        .scratchpad_create(&key, content_type, &fork_content, payment_option)
        .await;
    println!("Scratchpad create should fail here: {res:?}");
    assert!(matches!(
        res,
        Err(ScratchpadError::ScratchpadAlreadyExists(_))
    ));

    // wait for the scratchpad to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the scratchpad is stored with original content
    let got = client.scratchpad_get(&addr).await?;
    assert_eq!(*got.owner(), key.public_key());
    assert_eq!(got.data_encoding(), content_type);
    assert_eq!(got.decrypt_data(&key), Ok(content.clone()));
    assert_eq!(got.counter(), 0);
    assert!(got.verify_signature());
    println!("scratchpad got 1");

    // check that the content is decrypted correctly and matches the original
    let got_content = got.decrypt_data(&key)?;
    assert_eq!(got_content, content);
    Ok(())
}

// This test is a good show case to demonstrate how concurrent ops result in a forked Scratchpad.
// However, there is currently around 8% that failed to generate such forked Scratchpad,
// even after certain amounts of re-attempts.
// To avoid this slow down the merge process, currently put this test as `ignored`.
#[tokio::test]
#[serial]
#[ignore]
async fn scratchpad_fork_display() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    const INITIAL_SETUP_DELAY: u64 = 2;
    const CONCURRENT_UPDATES_COUNT: usize = 5;
    const MAX_ATTEMPTS: usize = 10;

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    for attempt in 1..=MAX_ATTEMPTS {
        println!("\nAttempt {attempt} of {MAX_ATTEMPTS}");
        println!("Creating new scratchpad for this attempt...");

        let owner_key = bls::SecretKey::random();
        let initial_data = Bytes::from("Wanna fork?");
        let payment = PaymentOption::from(&wallet);

        println!(
            "Initial data: \"{}\"",
            String::from_utf8_lossy(&initial_data)
        );
        println!("Secret key: {}", hex::encode(owner_key.to_bytes()));

        let (_cost, addr) = client
            .scratchpad_create(&owner_key, 0, &initial_data, payment)
            .await?;
        println!("Created scratchpad at: {}", addr.to_hex());

        let base_scratchpad = client.scratchpad_get(&addr).await?;
        println!("Base counter: {}", base_scratchpad.counter());

        // Initialize batch of clients for the later on concurrent updates
        let mut clients = vec![];
        for _i in 1..=CONCURRENT_UPDATES_COUNT {
            clients.push(Client::init_local().await?);
            tokio::time::sleep(tokio::time::Duration::from_secs(INITIAL_SETUP_DELAY)).await;
        }

        println!("Running concurrent updates...");
        let mut tasks = Vec::new();
        for (i, c) in clients.iter().enumerate().take(CONCURRENT_UPDATES_COUNT) {
            let base_scratchpad_clone = base_scratchpad.clone();
            let owner_key_clone = owner_key.clone();
            let client_instance = c.clone();

            let task = tokio::spawn(async move {
                let data = Bytes::from("Let's fork!");
                let result = client_instance
                    .scratchpad_update_from(&base_scratchpad_clone, &owner_key_clone, 0, &data)
                    .await;

                match result {
                    Ok(_) => format!(
                        "Update {}: Success with \"{}\"",
                        i,
                        String::from_utf8_lossy(&data)
                    ),
                    Err(e) => format!(
                        "Update {}: {}",
                        i,
                        e.to_string().split_whitespace().next().unwrap_or("Error")
                    ),
                }
            });
            tasks.push(task);
        }

        let results = futures::future::try_join_all(tasks).await?;
        for result in results {
            println!("  {result}");
        }

        println!("\nChecking for fork...");
        let result = client.scratchpad_get(&addr).await;
        match result {
            Ok(scratchpad) => {
                let data = scratchpad.decrypt_data(&owner_key)?;
                println!(
                    "Success: counter={}, data=\"{}\"",
                    scratchpad.counter(),
                    String::from_utf8_lossy(&data)
                );
                println!("No fork detected");
            }
            Err(ScratchpadError::Fork(conflicting_scratchpads)) => {
                if let Err(e) = print_fork_analysis(&conflicting_scratchpads, &owner_key) {
                    eprintln!("Failed to print fork analysis: {e}");
                }
                verify_fork_data(&conflicting_scratchpads, &owner_key)?;
                println!("\nFork detection test passed!");
                return Ok(());
            }
            Err(other_error) => return Err(other_error.into()),
        }

        if attempt >= MAX_ATTEMPTS {
            panic!("Maximum attempts reached without fork detection - test failed");
        }
        println!("Retrying with new scratchpad...");
    }
    Ok(())
}

fn verify_fork_data(
    conflicting_scratchpads: &[Scratchpad],
    owner_key: &bls::SecretKey,
) -> Result<()> {
    assert!(
        !conflicting_scratchpads.is_empty(),
        "Fork error should contain conflicting scratchpads"
    );
    assert!(
        conflicting_scratchpads.len() >= 2,
        "Fork should have at least 2 conflicting scratchpads"
    );

    let first_scratchpad = conflicting_scratchpads
        .first()
        .ok_or_else(|| eyre!("Fork error contains no conflicting scratchpads"))?;
    let first_content = first_scratchpad.decrypt_data(owner_key)?;

    for scratchpad in conflicting_scratchpads.iter().skip(1) {
        let content = scratchpad.decrypt_data(owner_key)?;
        assert_eq!(
            content, first_content,
            "All conflicting scratchpads should have same decrypted content"
        );
        assert_ne!(
            scratchpad.encrypted_data(),
            first_scratchpad.encrypted_data(),
            "Conflicting scratchpads should have different encrypted data due to BLS non-deterministic encryption"
        );
    }
    Ok(())
}
