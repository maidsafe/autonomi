// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::client::payment::PaymentOption;
use autonomi::Client;
use eyre::Result;
use serial_test::serial;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::time::Duration;
use test_utils::evm::get_funded_wallet;
use tokio::time::sleep;
use walkdir::WalkDir;

// With a local evm network, and local network, run:
// EVM_NETWORK=local cargo test --package autonomi --test fs
#[tokio::test]
#[serial]
async fn dir_upload_download() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("dir_upload_download", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let (_cost, addr) = client
        .dir_upload_public("tests/file/test_dir".into(), wallet.into())
        .await?;

    sleep(Duration::from_secs(10)).await;

    client
        .dir_download_public(&addr, "tests/file/test_dir_fetched".into())
        .await?;

    // compare the two directories
    assert_eq!(
        compute_dir_sha256("tests/file/test_dir")?,
        compute_dir_sha256("tests/file/test_dir_fetched")?,
    );
    Ok(())
}

fn compute_sha256(path: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = BufReader::new(File::open(path)?);
    let mut buffer = [0; 1024];
    while let Ok(read_bytes) = file.read(&mut buffer) {
        if read_bytes == 0 {
            break;
        }
        hasher.update(&buffer[..read_bytes]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn compute_dir_sha256(dir: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let sha = compute_sha256(
                entry
                    .path()
                    .to_str()
                    .expect("Failed to convert path to string"),
            )?;
            hasher.update(sha.as_bytes());
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[tokio::test]
#[serial]
async fn file_into_vault() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("file", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let client_sk = bls::SecretKey::random();

    let (_cost, addr) = client
        .dir_upload_public("tests/file/test_dir".into(), wallet.clone().into())
        .await?;
    sleep(Duration::from_secs(2)).await;

    let archive = client.archive_get_public(&addr).await?;
    let set_version = 0;
    client
        .write_bytes_to_vault(archive.to_bytes()?, wallet.into(), &client_sk, set_version)
        .await?;

    // now assert over the stored account packet
    let new_client = Client::init_local().await?;

    let (ap, got_version) = new_client.fetch_and_decrypt_vault(&client_sk).await?;
    assert_eq!(set_version, got_version);
    let ap_archive_fetched =
        autonomi::client::files::archive_public::PublicArchive::from_bytes(ap)?;

    assert_eq!(
        archive, ap_archive_fetched,
        "archive fetched should match archive put"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn file_advanced_use() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("file_advanced_use", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let payment_option = PaymentOption::Wallet(wallet);

    // upload a directory
    let (cost, mut archive) = client
        .dir_content_upload("tests/file/test_dir/dir_a".into(), payment_option.clone())
        .await?;
    println!("cost to upload private directory: {cost:?}");
    println!("archive: {archive:#?}");

    // upload an additional file separately
    let (cost, file_datamap) = client
        .file_content_upload(
            "tests/file/test_dir/example_file_b".into(),
            payment_option.clone(),
        )
        .await?;
    println!("cost to upload additional file: {cost:?}");

    // add that file to the archive with custom metadata
    let custom_metadata = autonomi::client::files::Metadata {
        created: 42,
        modified: 84,
        size: 126,
        extra: Some("custom metadata".to_string()),
    };
    archive.add_file("example_file_b".into(), file_datamap, custom_metadata);

    // upload an additional file separately
    let (cost, file_datamap) = client
        .file_content_upload(
            "tests/file/test_dir/example_file_a".into(),
            payment_option.clone(),
        )
        .await?;
    println!("cost to upload additional file: {cost:?}");

    // add that file to the archive with custom metadata
    let custom_metadata = autonomi::client::files::Metadata::new_with_size(126);
    archive.add_file("example_file_a".into(), file_datamap, custom_metadata);

    // upload the archive
    let (cost, archive_datamap) = client.archive_put(&archive, payment_option.clone()).await?;
    println!("cost to upload archive: {cost:?}");

    // download the entire directory
    let dest = "tests/file/test_dir_fetched2";
    client.dir_download(&archive_datamap, dest.into()).await?;

    // compare the two directories
    assert_eq!(
        compute_dir_sha256("tests/file/test_dir")?,
        compute_dir_sha256(dest)?,
    );

    Ok(())
}
