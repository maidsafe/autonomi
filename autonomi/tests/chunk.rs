// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::client::payment::PaymentOption;
use autonomi::self_encryption::encrypt;
use autonomi::{client::chunk::Chunk, Client};
use eyre::Result;
use self_encryption::test_helpers::random_bytes;
use serial_test::serial;
use test_utils::{evm::get_funded_wallet, gen_random_data};

async fn chunk_put_with_size(size: usize) -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("chunk", false);

    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let data = gen_random_data(size);
    let data_len = data.len();

    // create a chunk of X bytes
    let chunk = Chunk::new(data);
    println!("chunk size: {data_len} bytes");

    // estimate the cost of the chunk
    let cost = client.chunk_cost(chunk.address()).await?;
    println!("chunk cost: {cost}");

    // put the chunk
    let payment_option = PaymentOption::from(&wallet);
    let (cost, addr) = client.chunk_put(&chunk, payment_option).await?;
    assert_eq!(addr, *chunk.address());
    println!("chunk put 1 cost: {cost}");

    // wait for the chunk to be replicated
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check that the chunk is stored
    let got = client.chunk_get(&addr).await?;
    assert_eq!(got, chunk.clone());
    println!("chunk got 1");

    Ok(())
}

#[tokio::test]
#[serial]
async fn chunk_put_empty() -> Result<()> {
    chunk_put_with_size(0).await
}

#[tokio::test]
#[serial]
async fn chunk_put_1mb() -> Result<()> {
    chunk_put_with_size(1024 * 1024).await // 1MB
}

#[tokio::test]
#[serial]
async fn chunk_put_max_size() -> Result<()> {
    // 4MB + 16 bytes (Brotli compression overhead) + 16 bytes (encryption padding)
    chunk_put_with_size(Chunk::MAX_SIZE).await
}

#[tokio::test]
#[serial]
async fn chunk_put_oversize() -> Result<()> {
    // 4MB + 16 bytes (Brotli compression overhead) + 16 bytes (encryption padding) + 1 byte
    let result = chunk_put_with_size(Chunk::MAX_SIZE + 1).await;

    // Verify we get the expected error
    match result {
        Err(err) => {
            let err_str = err.to_string();
            if err_str.contains("Chunk is too large") {
                // Log success and return Ok(())
                println!("Success: received expected error: {err_str}");
                Ok(())
            } else {
                panic!("Expected 'Chunk is too large' error, got: {err_str}");
            }
        }
        Ok(_) => {
            panic!("Expected error for oversized chunk, but operation succeeded");
        }
    }
}

// Test needs to be run with `MAX_CHUNK_SIZE=4194304` to set the chunk size for `self_encryption`.
#[test]
fn chunk_max_size_after_encryption() {
    const NUM_FILES: usize = 10;

    for _ in 0..NUM_FILES {
        // Generate random files of 3 * the max raw chunk size, so that we get 3 chunks at max capacity.
        let random_file = random_bytes(Chunk::MAX_RAW_SIZE * 3);

        let (_, chunks) = encrypt(random_file).unwrap();

        for chunk in chunks {
            // Make sure that after compression and encryption, the chunk sizes are acceptable.
            assert!(chunk.size() <= Chunk::MAX_SIZE);
        }
    }
}
