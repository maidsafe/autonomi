// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(deprecated)]

use ant_logging::LogBuilder;
use autonomi::Client;
use bytes::Bytes;
use eyre::Result;
use rand::Rng;
use std::time::Duration;
use test_utils::evm::get_funded_wallet;
use tokio::time::sleep;

#[tokio::test]
async fn register() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("register", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    // Owner key of the register.
    let key = bls::SecretKey::random();

    // Create a register with the value [1, 2, 3, 4]
    let rand_name: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();
    let register = client
        .register_create(
            Some(vec![1, 2, 3, 4].into()),
            &rand_name,
            key.clone(),
            &wallet,
        )
        .await
        .unwrap();

    sleep(Duration::from_secs(10)).await;

    // Fetch the register again
    let register = client.register_get(*register.address()).await.unwrap();

    // Update the register with the value [5, 6, 7, 8]
    client
        .register_update(register.clone(), vec![5, 6, 7, 8].into(), key)
        .await
        .unwrap();

    sleep(Duration::from_secs(2)).await;

    // Fetch and verify the register contains the updated value
    let register = client.register_get(*register.address()).await.unwrap();
    assert_eq!(register.values(), vec![Bytes::from(vec![5, 6, 7, 8])]);

    Ok(())
}
