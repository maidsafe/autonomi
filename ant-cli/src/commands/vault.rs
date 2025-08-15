// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::actions::NetworkContext;
use crate::args::max_fee_per_gas::{MaxFeePerGasParam, get_max_fee_per_gas_from_opt_param};
use crate::wallet::load_wallet;
use autonomi::TransactionConfig;
use autonomi::vault::UserData;
use color_eyre::Section;
use color_eyre::eyre::Context;
use color_eyre::eyre::Result;
use color_eyre::eyre::eyre;

pub async fn cost(network_context: NetworkContext, expected_max_size: u64) -> Result<()> {
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let vault_sk = crate::keys::get_vault_secret_key()?;

    println!("Getting cost to create a new vault...");
    let total_cost = client.vault_cost(&vault_sk, expected_max_size).await?;

    if total_cost.is_zero() {
        println!(
            "Vault already exists, updating an existing vault is free unless the new content exceeds the current vault's paid capacity."
        );
    } else {
        println!("Cost to create a new vault: {total_cost} AttoTokens");
    }
    Ok(())
}

pub async fn create(
    network_context: NetworkContext,
    max_fee_per_gas_param: Option<MaxFeePerGasParam>,
) -> Result<()> {
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let mut wallet = load_wallet(client.evm_network())?;

    let max_fee_per_gas =
        get_max_fee_per_gas_from_opt_param(max_fee_per_gas_param, client.evm_network())?;
    wallet.set_transaction_config(TransactionConfig { max_fee_per_gas });

    let vault_sk = crate::keys::get_vault_secret_key()?;

    println!("Retrieving local user data...");
    let local_user_data = crate::user_data::get_local_user_data()?;
    println!("Pushing to network vault...");
    let total_cost = client
        .vault_put_user_data(&vault_sk, wallet.into(), local_user_data.clone())
        .await?;

    if total_cost.is_zero() {
        println!("✅ Successfully pushed user data to existing vault");
    } else {
        println!("✅ Successfully created new vault containing local user data");
    }

    println!("Total cost: {total_cost} AttoTokens");
    println!("Vault contains:");
    local_user_data.display_stats();
    Ok(())
}

pub async fn sync(force: bool, network_context: NetworkContext) -> Result<()> {
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let vault_sk = crate::keys::get_vault_secret_key()?;
    let wallet = load_wallet(client.evm_network())?;

    if force {
        println!(
            "The force flag was provided, overwriting user data in the vault with local user data..."
        );
    } else {
        println!("Fetching vault from network...");
        let net_user_data = client
            .vault_get_user_data(&vault_sk)
            .await
            .wrap_err("Failed to fetch vault from network")
            .with_suggestion(|| "Make sure you have already created a vault on the network")?;

        prevent_loss_of_keys(&net_user_data)?;

        println!("Syncing vault with local user data...");
        crate::user_data::write_local_user_data(&net_user_data)?;
    }

    println!("Pushing local user data to network vault...");
    let local_user_data = crate::user_data::get_local_user_data()?;
    client
        .vault_put_user_data(&vault_sk, wallet.into(), local_user_data.clone())
        .await
        .with_suggestion(
            || "Make sure you have already created a vault on the network or try again",
        )?;

    println!("✅ Successfully synced vault");
    println!("Vault contains:");
    local_user_data.display_stats();
    Ok(())
}

fn prevent_loss_of_keys(net_user_data: &UserData) -> Result<()> {
    let UserData {
        register_key,
        scratchpad_key,
        pointer_key,
        file_archives: _,
        private_file_archives: _,
        register_addresses: _,
        public_files: _,
        private_files: _,
    } = net_user_data;

    let mut endangered_key_types = Vec::new();

    // check local register key if it differs from one in the vault
    let net_register_key = register_key.clone();
    let local_register_key = crate::access::keys::get_register_signing_key()
        .map(|k| k.to_hex())
        .ok();
    if local_register_key.is_some()
        && net_register_key.is_some()
        && net_register_key != local_register_key
    {
        endangered_key_types.push("register key".to_string());
    }

    // check local scratchpad key if it differs from one in the vault
    let net_scratchpad_key = scratchpad_key.clone();
    let local_scratchpad_key = crate::access::keys::get_scratchpad_general_signing_key()
        .map(|k| k.to_hex())
        .ok();
    if local_scratchpad_key.is_some()
        && net_scratchpad_key.is_some()
        && net_scratchpad_key != local_scratchpad_key
    {
        endangered_key_types.push("scratchpad key".to_string());
    }

    // check local pointer key if it differs from one in the vault
    let net_pointer_key = pointer_key.clone();
    let local_pointer_key = crate::access::keys::get_pointer_general_signing_key()
        .map(|k| k.to_hex())
        .ok();
    if local_pointer_key.is_some()
        && net_pointer_key.is_some()
        && net_pointer_key != local_pointer_key
    {
        endangered_key_types.push("pointer key".to_string());
    }

    if !endangered_key_types.is_empty() {
        let endangered_key_types = endangered_key_types.join(", ");
        return Err(eyre!("The {endangered_key_types} in the vault do not match the local {endangered_key_types}, aborting sync to prevent loss of current keys")
            .with_suggestion(|| "You can overwrite the data in the vault with the local data by providing the `force` flag")
            .with_suggestion(|| "Or you can overwrite the local data with the data in the vault by using the `load` command")
        );
    }

    Ok(())
}

pub async fn load(network_context: NetworkContext) -> Result<()> {
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let vault_sk = crate::keys::get_vault_secret_key()?;

    println!("Retrieving vault from network...");
    let user_data = client.vault_get_user_data(&vault_sk).await?;
    println!("Writing user data to disk...");
    crate::user_data::write_local_user_data(&user_data)?;

    println!("✅ Successfully loaded vault with:");
    user_data.display_stats();
    Ok(())
}
