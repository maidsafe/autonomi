// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::client::payment::PaymentOption;
use autonomi::smart_equation::{compute, COMPLEX_EQUATION, PLUS_EQUATION};
use autonomi::Client;
use serial_test::serial;
use std::{collections::HashMap, str};
use test_utils::evm::get_funded_wallet;

#[tokio::test]
#[serial]
async fn smart_equation() -> Result<(), Box<dyn std::error::Error>> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("smart_equation", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let owner_key = bls::SecretKey::random();
    let payment_option = PaymentOption::from(&wallet);

    // publish smart_equation
    let (cost, pointer_address) = client
        .publish_smart_equation(
            PLUS_EQUATION.to_string(),
            &owner_key,
            payment_option.clone(),
        )
        .await?;
    println!("smart_equation published at {pointer_address:?} with cost of {cost}");

    // fetch the smart_equation
    let fetched_equation = client.get_smart_equation(pointer_address).await?;
    let fetched_equation: &str =
        str::from_utf8(&fetched_equation).expect("Bytes are not valid UTF-8");
    println!("smart_equation fetched from {pointer_address:?}");

    // verify the fetched smart_equation
    let mut params = HashMap::new();
    params.insert("a".to_string(), 3.0);
    params.insert("b".to_string(), 4.0);
    assert_eq!(compute(params.clone(), fetched_equation), Ok(7.0));

    // update the smart_equation
    client
        .update_smart_equation(
            pointer_address,
            COMPLEX_EQUATION.to_string(),
            &owner_key,
            payment_option,
        )
        .await?;
    println!("smart_equation at {pointer_address:?} got updated");

    // Short break to allow data synced among nodes
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // fetch the smart_equation
    let fetched_equation = client.get_smart_equation(pointer_address).await?;
    let fetched_equation: &str =
        str::from_utf8(&fetched_equation).expect("Bytes are not valid UTF-8");
    println!("smart_equation fetched from {pointer_address:?}");

    // verify the fetched smart_equation
    assert_eq!(compute(params.clone(), fetched_equation), Ok(32.0));

    Ok(())
}
