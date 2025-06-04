// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use eyre::Result;
use test_utils::{
    gen_random_data,
    local_network_spawner::{spawn_local_network, DEFAULT_LOCAL_NETWORK_SIZE},
};

#[tokio::test]
async fn put() -> Result<()> {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    // Spawn local network
    let spawned_local_network = spawn_local_network(DEFAULT_LOCAL_NETWORK_SIZE).await?;
    let client = spawned_local_network.client;
    let wallet = spawned_local_network.wallet;

    let data = gen_random_data(1024 * 1024 * 10);

    let (_cost, addr) = client.data_put_public(data.clone(), wallet.into()).await?;

    let data_fetched = client.data_get_public(&addr).await?;
    assert_eq!(data, data_fetched, "data fetched should match data put");

    Ok(())
}
