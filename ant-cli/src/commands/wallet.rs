// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::actions::NetworkContext;
use crate::wallet::DUMMY_NETWORK;
use crate::wallet::fs::{select_wallet_private_key, store_private_key};
use crate::wallet::input::request_password;
use autonomi::{Wallet, get_evm_network};
use color_eyre::Result;
use color_eyre::eyre::eyre;
use prettytable::{Cell, Row, Table};

const WALLET_PASSWORD_REQUIRED: bool = false;

pub fn create(no_password: bool, password: Option<String>) -> Result<()> {
    let maybe_encryption_password = maybe_request_password(no_password, password)?;

    let wallet_private_key = Wallet::random_private_key();

    let wallet_address = Wallet::new_from_private_key(DUMMY_NETWORK, &wallet_private_key)
        .map_err(|e| eyre!("Unexpected error: Failed to create wallet from private key: {e}"))?
        .address()
        .to_string();

    // Save the private key file
    let file_path = store_private_key(&wallet_private_key, maybe_encryption_password)?;

    println!("Wallet address: {wallet_address}");
    println!("Wallet private key: {wallet_private_key}");
    println!("Stored wallet in: {file_path:?}");

    Ok(())
}

pub fn import(
    mut wallet_private_key: String,
    no_password: bool,
    password: Option<String>,
) -> Result<()> {
    // Validate imported key
    Wallet::new_from_private_key(DUMMY_NETWORK, &wallet_private_key)
        .map_err(|_| eyre!("Please provide a valid private key in hex format"))?;

    let maybe_encryption_password = maybe_request_password(no_password, password)?;

    let wallet_address = Wallet::new_from_private_key(DUMMY_NETWORK, &wallet_private_key)
        .map_err(|e| eyre!("Unexpected error: Failed to create wallet from private key: {e}"))?
        .address()
        .to_string();

    // Prepend with 0x if it isn't already
    if !wallet_private_key.starts_with("0x") {
        wallet_private_key = format!("0x{wallet_private_key}");
    }

    // Save the private key file
    let file_path = store_private_key(&wallet_private_key, maybe_encryption_password)?;

    println!("Wallet address: {wallet_address}");
    println!("Stored wallet in: {file_path:?}");

    Ok(())
}

pub fn export() -> Result<()> {
    let wallet_private_key = select_wallet_private_key()?;

    let wallet_address = Wallet::new_from_private_key(DUMMY_NETWORK, &wallet_private_key)
        .map_err(|e| eyre!("Failed to create wallet from private key loaded from disk: {e}"))?
        .address()
        .to_string();

    println!("Wallet address: {wallet_address}");
    println!("Wallet private key: {wallet_private_key}");

    Ok(())
}

pub async fn balance(network_context: NetworkContext) -> Result<()> {
    let network = get_evm_network(
        network_context.peers.local,
        Some(network_context.network_id.as_u8()),
    )?;
    let wallet = crate::wallet::load_wallet(&network)?;

    let token_balance = wallet.balance_of_tokens().await?;
    let gas_balance = wallet.balance_of_gas_tokens().await?;

    println!("Wallet balances: {}", wallet.address());

    let mut table = Table::new();

    table.add_row(Row::new(vec![
        Cell::new("Token Balance"),
        Cell::new(&token_balance.to_string()),
    ]));

    table.add_row(Row::new(vec![
        Cell::new("Gas Balance"),
        Cell::new(&gas_balance.to_string()),
    ]));

    table.printstd();

    Ok(())
}

fn maybe_request_password(no_password: bool, password: Option<String>) -> Result<Option<String>> {
    if no_password && password.is_some() {
        return Err(eyre!(
            "Only one of `--no-password` or `--password` may be specified"
        ));
    }

    // Set a password for encryption or not
    let maybe_password = match (no_password, password) {
        (true, _) => None,
        (false, Some(pass)) => Some(pass.to_owned()),
        (false, None) => request_password(WALLET_PASSWORD_REQUIRED),
    };

    Ok(maybe_password)
}
