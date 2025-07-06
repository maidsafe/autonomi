// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::AttoTokens;
use ant_logging::LogBuilder;
use autonomi::{vault::app_name_to_vault_content_type, Client};
use eyre::Result;
use serial_test::serial;
use test_utils::{evm::get_funded_wallet, gen_random_data};

#[tokio::test]
#[serial]
async fn vault_cost() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("vault", false);
    let client = Client::init_local().await?;
    let main_key = bls::SecretKey::random();

    // Quoting cost for a Vault with 1TB max_size
    let cost = client
        .vault_cost(&main_key, 1024 * 1024 * 1024 * 1024)
        .await?;
    println!("1TB Vault cost: {cost}");

    assert_eq!(cost, AttoTokens::from_u64(787416));

    Ok(())
}

#[tokio::test]
#[serial]
async fn vault_expand() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("vault", false);
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let main_key = bls::SecretKey::random();

    let content_type = app_name_to_vault_content_type("TestData");
    let original_content = gen_random_data(1024);

    let cost = client
        .write_bytes_to_vault(
            original_content.clone(),
            wallet.clone().into(),
            &main_key,
            content_type,
        )
        .await?;
    println!("1KB Vault update cost: {cost}");

    let (fetched_content, fetched_content_type) = client.fetch_and_decrypt_vault(&main_key).await?;
    println!("1KB Vault fetched");
    assert_eq!(fetched_content_type, content_type);
    assert_eq!(fetched_content, original_content);

    // Update content to 2KB. Shall not incur any cost.
    let update_content_2_kb = gen_random_data(2 * 1024);
    let cost = client
        .write_bytes_to_vault(
            update_content_2_kb.clone(),
            wallet.clone().into(),
            &main_key,
            content_type,
        )
        .await?;
    assert_eq!(cost, AttoTokens::zero());
    println!("2KB Vault update cost: {cost}");

    let (fetched_content, fetched_content_type) = client.fetch_and_decrypt_vault(&main_key).await?;
    println!("2KB Vault fetched");
    assert_eq!(fetched_content_type, content_type);
    assert_eq!(fetched_content, update_content_2_kb);

    // Update content to 10MB. Shall only incur cost paying two extra Scratchpad.
    let update_content_10_mb = gen_random_data(10 * 1024 * 1024);
    let cost = client
        .write_bytes_to_vault(
            update_content_10_mb.clone(),
            wallet.into(),
            &main_key,
            content_type,
        )
        .await?;
    assert_eq!(cost, AttoTokens::from_u64(6));
    println!("10MB Vault update cost: {cost}");

    // Short break is required to avoid client choked by the last query round
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    let (fetched_content, fetched_content_type) = client.fetch_and_decrypt_vault(&main_key).await?;
    println!("10MB Vault fetched");
    assert_eq!(fetched_content_type, content_type);
    assert_eq!(fetched_content, update_content_10_mb);

    Ok(())
}
