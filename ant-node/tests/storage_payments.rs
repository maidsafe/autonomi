// // Copyright 2024 MaidSafe.net limited.
// //
// // This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// // Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// // under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// // KIND, either express or implied. Please review the Licences for the specific language governing
// // permissions and limitations relating to use of the SAFE Network Software.

// mod common;

// use crate::common::{client::get_client_and_funded_wallet, random_content};
// use assert_fs::TempDir;
// use eyre::{eyre, Result};
// use libp2p::PeerId;
// use rand::Rng;
// use sn_client::{Error as ClientError, FilesDownload, Uploader, WalletClient};
// use ant_evm::{Amount, AttoTokens, PaymentQuote};
// use ant_logging::LogBuilder;
// use crate::networking::{GetRecordError, NetworkError};
// use ant_protocol::{
//     error::Error as ProtocolError,
//     storage::ChunkAddress,
//     NetworkAddress,
// };
// use std::collections::BTreeMap;
// use tokio::time::{sleep, Duration};
// use tracing::info;
// use xor_name::XorName;

// #[tokio::test]
// async fn storage_payment_succeeds() -> Result<()> {
//     let _log_guards = LogBuilder::init_single_threaded_tokio_test("storage_payments", true);

//     let paying_wallet_dir = TempDir::new()?;

//     let (client, paying_wallet) = get_client_and_funded_wallet(paying_wallet_dir.path()).await?;

//     let balance_before = paying_wallet.balance();
//     let mut wallet_client = WalletClient::new(client.clone(), paying_wallet);

//     // generate a random number (between 50 and 100) of random addresses
//     let mut rng = rand::thread_rng();
//     let random_content_addrs = (0..rng.gen_range(50..100))
//         .map(|_| {
//             ant_protocol::NetworkAddress::ChunkAddress(ChunkAddress::new(XorName::random(&mut rng)))
//         })
//         .collect::<Vec<_>>();
//     info!(
//         "Paying for {} random addresses...",
//         random_content_addrs.len()
//     );

//     let _cost = wallet_client
//         .pay_for_storage(random_content_addrs.clone().into_iter())
//         .await?;

//     info!("Verifying balance has been paid from the wallet...");

//     let paying_wallet = wallet_client.into_wallet();
//     assert!(
//         paying_wallet.balance() < balance_before,
//         "balance should have decreased after payment"
//     );

//     Ok(())
// }

// #[tokio::test]
// async fn storage_payment_fails_with_insufficient_money() -> Result<()> {
//     let _log_guards = LogBuilder::init_single_threaded_tokio_test("storage_payments", true);

//     let paying_wallet_dir: TempDir = TempDir::new()?;
//     let chunks_dir = TempDir::new()?;

//     let (client, paying_wallet) = get_client_and_funded_wallet(paying_wallet_dir.path()).await?;

//     let (files_api, content_bytes, _random_content_addrs, chunks) =
//         random_content(&client, paying_wallet_dir.to_path_buf(), chunks_dir.path())?;

//     let mut wallet_client = WalletClient::new(client.clone(), paying_wallet);
//     let subset_len = chunks.len() / 3;
//     let _storage_cost = wallet_client
//         .pay_for_storage(
//             chunks
//                 .clone()
//                 .into_iter()
//                 .take(subset_len)
//                 .map(|(name, _)| NetworkAddress::ChunkAddress(ChunkAddress::new(name))),
//         )
//         .await?;

//     // now let's request to upload all addresses, even that we've already paid for a subset of them
//     let verify_store = false;
//     let res = files_api
//         .upload_test_bytes(content_bytes.clone(), verify_store)
//         .await;
//     assert!(
//         res.is_err(),
//         "Should have failed to store as we didnt pay for everything"
//     );
//     Ok(())
// }

// // TODO: reenable
// #[ignore = "Currently we do not cache the proofs in the wallet"]
// #[tokio::test]
// async fn storage_payment_proofs_cached_in_wallet() -> Result<()> {
//     let _log_guards = LogBuilder::init_single_threaded_tokio_test("storage_payments", true);

//     let paying_wallet_dir: TempDir = TempDir::new()?;

//     let (client, paying_wallet) = get_client_and_funded_wallet(paying_wallet_dir.path()).await?;
//     let wallet_original_balance = paying_wallet.balance().as_atto();
//     let mut wallet_client = WalletClient::new(client.clone(), paying_wallet);

//     // generate a random number (between 50 and 100) of random addresses
//     let mut rng = rand::thread_rng();
//     let random_content_addrs = (0..rng.gen_range(50..100))
//         .map(|_| {
//             ant_protocol::NetworkAddress::ChunkAddress(ChunkAddress::new(XorName::random(&mut rng)))
//         })
//         .collect::<Vec<_>>();

//     // let's first pay only for a subset of the addresses
//     let subset_len = random_content_addrs.len() / 3;
//     info!("Paying for {subset_len} random addresses...",);
//     let storage_payment_result = wallet_client
//         .pay_for_storage(random_content_addrs.clone().into_iter().take(subset_len))
//         .await?;

//     let total_cost = storage_payment_result
//         .storage_cost
//         .checked_add(storage_payment_result.royalty_fees)
//         .ok_or(eyre!("Total storage cost exceed possible token amount"))?;

//     // check we've paid only for the subset of addresses, 1 nano per addr
//     let new_balance = AttoTokens::from_atto(wallet_original_balance - total_cost.as_atto());
//     info!("Verifying new balance on paying wallet is {new_balance} ...");
//     let paying_wallet = wallet_client.into_wallet();
//     // assert_eq!(paying_wallet.balance(), new_balance);// TODO adapt to evm

//     // let's verify payment proofs for the subset have been cached in the wallet
//     assert!(random_content_addrs
//         .iter()
//         .take(subset_len)
//         .all(|name| paying_wallet
//             .api()
//             .get_recent_payment(&name.as_xorname().unwrap())
//             .is_ok()));

//     // now let's request to pay for all addresses, even that we've already paid for a subset of them
//     let mut wallet_client = WalletClient::new(client.clone(), paying_wallet);
//     let storage_payment_result = wallet_client
//         .pay_for_storage(random_content_addrs.clone().into_iter())
//         .await?;
//     let total_cost = storage_payment_result
//         .storage_cost
//         .checked_add(storage_payment_result.royalty_fees)
//         .ok_or(eyre!("Total storage cost exceed possible token amount"))?;

//     // check we've paid only for addresses we haven't previously paid for, 1 nano per addr
//     let new_balance = AttoTokens::from_atto(
//         wallet_original_balance - (Amount::from(random_content_addrs.len()) * total_cost.as_atto()),
//     );
//     println!("Verifying new balance on paying wallet is now {new_balance} ...");
//     let paying_wallet = wallet_client.into_wallet();
//     // TODO adapt to evm
//     // assert_eq!(paying_wallet.balance(), new_balance);

//     // let's verify payment proofs now for all addresses have been cached in the wallet
//     // assert!(random_content_addrs
//     //     .iter()
//     //     .all(|name| paying_wallet.get_payment_unique_pubkeys(name) == transfer_outputs_map.get(name)));

//     Ok(())
// }

// #[tokio::test]
// async fn storage_payment_chunk_upload_succeeds() -> Result<()> {
//     let _log_guards = LogBuilder::init_single_threaded_tokio_test("storage_payments", true);

//     let paying_wallet_dir = TempDir::new()?;
//     let chunks_dir = TempDir::new()?;

//     let (client, paying_wallet) = get_client_and_funded_wallet(paying_wallet_dir.path()).await?;
//     let mut wallet_client = WalletClient::new(client.clone(), paying_wallet);

//     let (files_api, _content_bytes, file_addr, chunks) =
//         random_content(&client, paying_wallet_dir.to_path_buf(), chunks_dir.path())?;

//     info!("Paying for {} random addresses...", chunks.len());

//     let _cost = wallet_client
//         .pay_for_storage(
//             chunks
//                 .iter()
//                 .map(|(name, _)| NetworkAddress::ChunkAddress(ChunkAddress::new(*name))),
//         )
//         .await?;

//     let mut uploader = Uploader::new(client.clone(), paying_wallet_dir.to_path_buf());
//     uploader.set_show_holders(true);
//     uploader.insert_chunk_paths(chunks);
//     let _upload_stats = uploader.start_upload().await?;

//     let mut files_download = FilesDownload::new(files_api);
//     let _ = files_download.file_download_public(file_addr, None).await?;

//     Ok(())
// }

// #[ignore = "This test sends out invalid 0 transactions and needs to be fixed"]
// #[tokio::test]
// async fn storage_payment_chunk_upload_fails_if_no_tokens_sent() -> Result<()> {
//     let _log_guards = LogBuilder::init_single_threaded_tokio_test("storage_payments", true);

//     let paying_wallet_dir = TempDir::new()?;
//     let chunks_dir = TempDir::new()?;

//     let (client, paying_wallet) = get_client_and_funded_wallet(paying_wallet_dir.path()).await?;
//     let mut wallet_client = WalletClient::new(client.clone(), paying_wallet);

//     let (files_api, content_bytes, content_addr, chunks) =
//         random_content(&client, paying_wallet_dir.to_path_buf(), chunks_dir.path())?;

//     let mut no_data_payments = BTreeMap::default();
//     for (chunk_name, _) in chunks.iter() {
//         no_data_payments.insert(
//             *chunk_name,
//             (
//                 ant_evm::utils::dummy_address(),
//                 PaymentQuote::test_dummy(*chunk_name, AttoTokens::from_u64(0)),
//                 PeerId::random().to_bytes(),
//             ),
//         );
//     }

//     // TODO adapt to evm
//     // let _ = wallet_client
//     //     .mut_wallet()
//     //     .send_storage_payment(&no_data_payments)
//     //     .await?;

//     sleep(Duration::from_secs(5)).await;

//     files_api
//         .upload_test_bytes(content_bytes.clone(), false)
//         .await?;

//     info!("Reading {content_addr:?} expected to fail");
//     let mut files_download = FilesDownload::new(files_api);
//     assert!(
//         matches!(
//             files_download.file_download_public(content_addr, None).await,
//             Err(ClientError::Network(NetworkError::GetRecordError(
//                 GetRecordError::RecordNotFound
//             )))
//         ),
//         "read bytes should fail as we didn't store them"
//     );

//     Ok(())
// }
