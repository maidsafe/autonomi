// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::client::payment::PaymentOption;
use autonomi::scratchpad::ScratchpadError;
use autonomi::AttoTokens;
use autonomi::{
    client::scratchpad::{Bytes, Scratchpad},
    Client,
};
use eyre::Result;
use serial_test::serial;
use test_utils::evm::get_funded_wallet;

#[tokio::test]
#[serial]
async fn scratchpad_put_manual() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("scratchpad", false);

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
async fn scratchpad_put() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("scratchpad", false);

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
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("scratchpad", false);

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
