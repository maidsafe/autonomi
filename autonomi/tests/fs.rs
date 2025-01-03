// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::Client;
use eyre::Result;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::time::Duration;
use test_utils::evm::get_funded_wallet;
use tokio::time::sleep;
use walkdir::WalkDir;

// With a local evm network, and local network, run:
// EVM_NETWORK=local cargo test --features="local" --package autonomi --test fs
#[tokio::test]
async fn dir_upload_download() -> Result<()> {
    let _log_appender_guard =
        LogBuilder::init_single_threaded_tokio_test("dir_upload_download", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();

    let addr = client
        .dir_and_archive_upload_public("tests/file/test_dir".into(), &wallet)
        .await?;

    sleep(Duration::from_secs(10)).await;

    client
        .dir_download_public(addr, "tests/file/test_dir_fetched".into())
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
async fn file_into_vault() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("file", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let client_sk = bls::SecretKey::random();

    let addr = client
        .dir_and_archive_upload_public("tests/file/test_dir".into(), &wallet)
        .await?;
    sleep(Duration::from_secs(2)).await;

    let archive = client.archive_get_public(addr).await?;
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
