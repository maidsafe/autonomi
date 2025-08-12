// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::{Result, eyre::bail};
use evmlib::{Network, utils::get_evm_network, wallet::Wallet};
use std::env;

pub fn get_funded_wallet() -> evmlib::wallet::Wallet {
    let network = get_evm_network(true, None).expect("Failed to get local EVM network from CSV");
    if matches!(network, Network::ArbitrumOne) {
        panic!("You're trying to use ArbitrumOne network. Use a custom network for testing.");
    }
    // Default deployer wallet of the testnet.
    const DEFAULT_WALLET_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    let private_key = env::var("SECRET_KEY").unwrap_or(DEFAULT_WALLET_PRIVATE_KEY.to_string());

    Wallet::new_from_private_key(network, &private_key).expect("Invalid private key")
}

pub fn get_new_wallet() -> Result<Wallet> {
    let network = get_evm_network(true, None).expect("Failed to get local EVM network from CSV");
    if matches!(network, Network::ArbitrumOne) {
        bail!("You're trying to use ArbitrumOne network. Use a custom network for testing.");
    }

    Ok(Wallet::new_with_random_wallet(network))
}
