// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::client::payment::PaymentOption;
use autonomi::AttoTokens;
use autonomi::{
    chunk::ChunkAddress,
    client::pointer::{Pointer, PointerTarget},
    Client,
};
use eyre::Result;
use serial_test::serial;
use test_utils::evm::get_funded_wallet;
use xor_name::XorName;

#[tokio::test]
#[serial]
async fn pointer_put_manual() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("pointer", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let key = bls::SecretKey::random();
    let public_key = key.public_key();
    let target =
        PointerTarget::ChunkAddress(ChunkAddress::new(XorName::random(&mut rand::thread_rng())));
    let pointer = Pointer::new(&key, 0, target);

    // estimate the cost of the pointer
    let cost = client.pointer_cost(&public_key).await?;
    println!("pointer cost: {cost}");

    // put the pointer
    let payment_option = PaymentOption::from(&wallet);
    let (cost, addr) = client.pointer_put(pointer.clone(), payment_option).await?;
    assert_eq!(addr, pointer.address());
    println!("pointer put 1 cost: {cost}");

    // wait for the pointer to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the pointer is stored
    let got = client.pointer_get(&addr).await?;
    assert_eq!(got, pointer.clone());
    println!("pointer got 1");

    // try update pointer and make it point to itself
    let target2 = PointerTarget::PointerAddress(addr);
    let pointer2 = Pointer::new(&key, 1, target2);
    let payment_option = PaymentOption::from(&wallet);
    let (cost, _) = client.pointer_put(pointer2.clone(), payment_option).await?;
    assert_eq!(cost, AttoTokens::zero());
    println!("pointer put 2 cost: {cost}");

    // wait for the pointer to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the pointer is updated
    let got = client.pointer_get(&addr).await?;
    assert_eq!(got, pointer2.clone());
    println!("pointer got 2");

    Ok(())
}

#[tokio::test]
#[serial]
async fn pointer_put() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("pointer", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let key = bls::SecretKey::random();
    let public_key = key.public_key();
    let target =
        PointerTarget::ChunkAddress(ChunkAddress::new(XorName::random(&mut rand::thread_rng())));

    // estimate the cost of the pointer
    let cost = client.pointer_cost(&public_key).await?;
    println!("pointer cost: {cost}");

    // put the pointer
    let payment_option = PaymentOption::from(&wallet);
    let (cost, addr) = client
        .pointer_create(&key, target.clone(), payment_option)
        .await?;
    println!("pointer create cost: {cost}");

    // wait for the pointer to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the pointer is stored
    let got = client.pointer_get(&addr).await?;
    assert_eq!(got, Pointer::new(&key, 0, target));
    println!("pointer got 1");

    // try update pointer and make it point to itself
    let target2 = PointerTarget::PointerAddress(addr);
    client.pointer_update(&key, target2.clone()).await?;

    // wait for the pointer to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the pointer is updated
    let got = client.pointer_get(&addr).await?;
    assert_eq!(got, Pointer::new(&key, 1, target2));
    println!("pointer got 2");

    Ok(())
}
