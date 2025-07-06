// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(feature = "evm-payments")]
mod test {
    use autonomi::Client;
    use bytes::Bytes;
    use eyre::bail;
    use std::time::Duration;
    use test_utils::evm::get_funded_wallet;
    use tokio::time::sleep;

    #[tokio::test]
    async fn file() -> Result<(), Box<dyn std::error::Error>> {
        let _log_appender_guard =
            ant_logging::LogBuilder::init_single_threaded_tokio_test("file", false);

        let mut client = Client::init_local().await?;
        let mut wallet = get_funded_wallet();

        // let data = common::gen_random_data(1024 * 1024 * 1000);
        // let user_key = common::gen_random_data(32);

        let (root, addr) = client
            .upload_from_dir("tests/file/test_dir".into(), &mut wallet)
            .await?;

        sleep(Duration::from_secs(10)).await;

        let root_fetched = client.fetch_root(addr).await?;

        assert_eq!(
            root.map, root_fetched.map,
            "root fetched should match root put"
        );

        Ok(())
    }

    #[cfg(feature = "vault")]
    #[tokio::test]
    async fn file_into_vault() -> eyre::Result<()> {
        common::enable_logging();

        let mut client = Client::init()
            .await?
            .with_vault_entropy(Bytes::from("at least 32 bytes of entropy here"))?;

        let mut wallet = get_funded_wallet();

        let (root, addr) = client
            .upload_from_dir("tests/file/test_dir".into(), &mut wallet)
            .await?;
        sleep(Duration::from_secs(2)).await;

        let root_fetched = client.fetch_root(addr).await?;

        assert_eq!(
            root.map, root_fetched.map,
            "root fetched should match root put"
        );

        // now assert over the stored account packet
        let new_client = Client::init()
            .await?
            .with_vault_entropy(Bytes::from("at least 32 bytes of entropy here"))?;

        if let Some(ap) = new_client.fetch_and_decrypt_vault().await? {
            let ap_root_fetched = Client::deserialise_root(ap)?;

            assert_eq!(
                root.map, ap_root_fetched.map,
                "root fetched should match root put"
            );
        } else {
            bail!("No account packet found");
        }

        Ok(())
    }
}
