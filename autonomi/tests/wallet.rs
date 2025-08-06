// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::LogBuilder;
use autonomi::Wallet;
use autonomi::get_evm_network;
use autonomi::{Amount, RewardsAddress};
use const_hex::traits::FromHex;
use test_utils::evm::get_funded_wallet;

#[tokio::test]
async fn from_private_key() {
    let private_key = "0xdb1049e76a813c94be0df47ec3e20533ca676b1b9fef2ddbce9daa117e4da4aa";
    let network =
        get_evm_network(true, None).expect("Could not get EVM network from environment variables");
    let wallet = Wallet::new_from_private_key(network, private_key).unwrap();

    assert_eq!(
        wallet.address(),
        RewardsAddress::from_hex("0x69D5BF2Bc42bca8782b8D2b4FdfF2b1Fa7644Fe7").unwrap()
    )
}

#[tokio::test]
async fn send_tokens() {
    let _log_appender_guard = LogBuilder::init_single_threaded_tokio_test();

    let network =
        get_evm_network(true, None).expect("Could not get EVM network from environment variables");
    let wallet = get_funded_wallet();

    let receiving_wallet = Wallet::new_with_random_wallet(network);

    let initial_balance = receiving_wallet.balance_of_tokens().await.unwrap();

    assert_eq!(initial_balance, Amount::from(0));

    let _ = wallet
        .transfer_tokens(receiving_wallet.address(), Amount::from(10))
        .await;

    let final_balance = receiving_wallet.balance_of_tokens().await.unwrap();

    assert_eq!(final_balance, Amount::from(10));
}
