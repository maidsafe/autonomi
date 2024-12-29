// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::Client;
use anyhow::Result;
use test_utils::{evm::get_funded_wallet, gen_random_data};

#[tokio::test]
async fn test_put() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test("put", false);

    let client = Client::init_with_config(Default::default()).await?;
    let wallet = get_funded_wallet();
    let data = gen_random_data(1024 * 1024 * 10);

    let addr = client.data_put_public(data.clone(), wallet.into()).await?;

    Ok(())
}
