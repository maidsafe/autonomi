// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Streaming download tests

use ant_logging::LogBuilder;
use autonomi::Client;
use eyre::Result;
use test_utils::{evm::get_funded_wallet, gen_random_data};

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_streaming_download() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // init client
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let data = gen_random_data(1024 * 1024 * 5); // 5MB test data

    // put data
    let (_cost, data_addr) = client.data_put_public(data.clone(), wallet.into()).await?;

    // download data with get
    let data_from_get = client.data_get_public(&data_addr).await?;

    // download data with stream
    let data_stream = client.data_stream_public(&data_addr).await?;
    let mut data_from_stream = Vec::new();

    for chunk_result in data_stream {
        let chunk = chunk_result?;
        data_from_stream.extend_from_slice(&chunk);
    }

    // make sure results are the same
    assert_eq!(
        data, data_from_get,
        "Original data should match data from get"
    );
    assert_eq!(
        data, data_from_stream,
        "Original data should match data from stream"
    );
    assert_eq!(
        data_from_get, data_from_stream,
        "Data from get and stream should match"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_streaming_large_blob() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // init client
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet();
    let data = gen_random_data(1024 * 1024 * 100); // 100MB test data

    // put data
    let (_cost, data_addr) = client.data_put_public(data.clone(), wallet.into()).await?;

    // download data with get - this should fail for large files
    let get_result = client.data_get_public(&data_addr).await;
    assert!(
        get_result.is_err(),
        "data_get_public should fail for 100MB file"
    );

    // download data with stream - this should work for large files
    let data_stream = client.data_stream_public(&data_addr).await?;
    let mut data_from_stream = Vec::new();

    for chunk_result in data_stream {
        let chunk = chunk_result?;
        data_from_stream.extend_from_slice(&chunk);
    }

    // make sure streaming returns the original data
    assert_eq!(
        data.len(),
        data_from_stream.len(),
        "Data lengths should match"
    );
    assert_eq!(
        data, data_from_stream,
        "Original data should match data from stream"
    );

    Ok(())
}
