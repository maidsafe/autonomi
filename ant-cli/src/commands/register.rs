// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(deprecated)]

use crate::wallet::load_wallet;
use autonomi::client::register::RegisterAddress;
use autonomi::client::register::SecretKey as RegisterSecretKey;
use autonomi::{Client, InitialPeersConfig, TransactionConfig};
use color_eyre::eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::Result;
use color_eyre::Section;

pub fn generate_key(overwrite: bool) -> Result<()> {
    // check if the key already exists
    let key_path = crate::keys::get_register_signing_key_path()?;
    if key_path.exists() && !overwrite {
        error!("Register key already exists at: {key_path:?}");
        return Err(eyre!("Register key already exists at: {}", key_path.display()))
            .with_suggestion(|| "if you want to overwrite the existing key, run the command with the --overwrite flag")
            .with_warning(|| "overwriting the existing key might result in loss of access to any existing registers created using that key");
    }

    // generate and write a new key to file
    let key = RegisterSecretKey::random();
    let path = crate::keys::create_register_signing_key_file(key)
        .wrap_err("Failed to create new register key")?;
    info!("Created new register key at: {path:?}");
    println!("✅ Created new register key at: {}", path.display());
    Ok(())
}

pub async fn cost(name: &str, init_peers_config: InitialPeersConfig) -> Result<()> {
    let main_registers_key = crate::keys::get_register_signing_key()
        .wrap_err("The register key is required to perform this action")?;
    let client = crate::actions::connect_to_network(init_peers_config)
        .await
        .map_err(|(err, _)| err)?;

    let key_for_name = Client::register_key_from_name(&main_registers_key, name);

    let cost = client
        .register_cost(&key_for_name.public_key())
        .await
        .wrap_err("Failed to get cost for register")?;
    info!("Estimated cost to create a register with name {name}: {cost}");
    println!("✅ The estimated cost to create a register with name {name} is: {cost}");
    Ok(())
}

pub async fn create(
    name: &str,
    value: &str,
    hex: bool,
    init_peers_config: InitialPeersConfig,
    max_fee_per_gas: Option<u128>,
) -> Result<()> {
    let main_registers_key = crate::keys::get_register_signing_key()
        .wrap_err("The register key is required to perform this action")?;
    let client = crate::actions::connect_to_network(init_peers_config)
        .await
        .map_err(|(err, _)| err)?;

    let mut wallet = load_wallet(client.evm_network())?;

    if let Some(max_fee_per_gas) = max_fee_per_gas {
        wallet.set_transaction_config(TransactionConfig::new(max_fee_per_gas))
    }

    let register_key = Client::register_key_from_name(&main_registers_key, name);

    println!("Creating register with name: {name}");
    info!("Creating register with name: {name}");

    let value_bytes = if hex {
        hex::decode(value.trim_start_matches("0x"))
            .wrap_err("Failed to decode hex value")
            .with_suggestion(|| "Make sure the value is a valid hex string")?
    } else {
        value.as_bytes().to_vec()
    };
    let content = Client::register_value_from_bytes(&value_bytes)?;

    let (cost, address) = client
        .register_create(&register_key, content, wallet.into())
        .await
        .wrap_err("Failed to create register")?;

    println!("✅ Register created at address: {address}");
    println!("With name: {name}");
    if hex {
        println!("And initial hex value: [{}]", hex::encode(&value_bytes));
    } else {
        println!("And initial value: [{value}]");
    }
    info!("Register created at address: {address} with name: {name}");
    println!("Total cost: {cost} AttoTokens");

    crate::user_data::write_local_register(&address, name)
        .wrap_err("Failed to save register to local user data")
        .with_suggestion(|| "Local user data saves the register address above to disk, without it you need to keep track of the address yourself")?;
    info!("Saved register to local user data");

    Ok(())
}

pub async fn edit(
    address: String,
    name: bool,
    value: &str,
    hex: bool,
    init_peers_config: InitialPeersConfig,
    max_fee_per_gas: Option<u128>,
) -> Result<()> {
    let main_registers_key = crate::keys::get_register_signing_key()
        .wrap_err("The register key is required to perform this action")?;
    let client = crate::actions::connect_to_network(init_peers_config)
        .await
        .map_err(|(err, _)| err)?;

    let mut wallet = load_wallet(client.evm_network())?;

    if let Some(max_fee_per_gas) = max_fee_per_gas {
        wallet.set_transaction_config(TransactionConfig::new(max_fee_per_gas))
    }

    let value_bytes = if hex {
        hex::decode(value.trim_start_matches("0x"))
            .wrap_err("Failed to decode hex value")
            .with_suggestion(|| "Make sure the value is a valid hex string")?
    } else {
        value.as_bytes().to_vec()
    };
    let value_bytes = Client::register_value_from_bytes(&value_bytes)?;

    let register_key = if name {
        let name_str = address.clone();
        Client::register_key_from_name(&main_registers_key, &name_str)
    } else {
        let addr = RegisterAddress::from_hex(&address)
            .wrap_err(format!("Failed to parse register address: {address}"))
            .with_suggestion(|| {
                "if you want to use the name as the address, run the command with the --name flag"
            })?;
        let name_str = crate::user_data::get_name_of_local_register_with_address(&addr)
            .wrap_err(format!("Could not find a register with address in local user data: {address}"))
            .with_suggestion(|| "This register is not known to this client, try to create it first.")
            .with_suggestion(|| "If you indeed have created this register before, retry using its name by using the --name flag")?;
        Client::register_key_from_name(&main_registers_key, &name_str)
    };

    println!("Attempting to update register at {address} with new value: {value}");
    info!("Attempting to update register at {address} with new value: {value}");

    let cost = client
        .register_update(&register_key, value_bytes, wallet.into())
        .await
        .wrap_err(format!("Failed to update register at address: {address}"))?;

    println!("✅ Successfully updated register");
    println!("With value: [{value}]");
    println!("Total cost: {cost} AttoTokens");
    info!("Successfully updated register at address: {address}");

    Ok(())
}

pub async fn get(
    address: String,
    name: bool,
    hex: bool,
    init_peers_config: InitialPeersConfig,
) -> Result<()> {
    let client = crate::actions::connect_to_network(init_peers_config)
        .await
        .map_err(|(err, _)| err)?;

    let addr = if name {
        let name_str = address.clone();
        let main_registers_key = crate::keys::get_register_signing_key()
            .wrap_err("The register key is required to perform this action")?;
        let register_key = Client::register_key_from_name(&main_registers_key, &name_str);
        RegisterAddress::new(register_key.public_key())
    } else {
        RegisterAddress::from_hex(&address)
            .wrap_err(format!("Failed to parse register address: {address}"))
            .with_suggestion(|| {
                "if you want to use the name as the address, run the command with the --name flag"
            })?
    };

    if name {
        println!("Getting register with name: {address}");
        info!("Getting register with name: {address}");
    } else {
        println!("Getting register at address: {address}");
        info!("Getting register at address: {address}");
    }
    let value_bytes = client
        .register_get(&addr)
        .await
        .wrap_err(format!("Error getting register at: {address}"))?;

    println!("✅ Register found at: {address}");
    info!("Register found at: {address}");

    if hex {
        let hex_value = hex::encode(value_bytes);
        println!("With hex value: [{hex_value}]");
        info!("With hex value: [{hex_value}]");
    } else {
        let value = String::from_utf8_lossy(&value_bytes);
        println!("With value: [{value}]");
        info!("With value: [{value}]");
    }

    Ok(())
}

pub fn list() -> Result<()> {
    println!("Retrieving local user data...");
    let registers = crate::user_data::get_local_registers()?;
    println!("✅ You have {} register(s):", registers.len());
    for (addr, name) in registers {
        println!("{}: {}", name, addr.to_hex());
    }
    Ok(())
}

pub async fn history(
    address: String,
    name: bool,
    hex: bool,
    init_peers_config: InitialPeersConfig,
) -> Result<()> {
    let client = crate::actions::connect_to_network(init_peers_config)
        .await
        .map_err(|(err, _)| err)?;

    let addr = if name {
        let name_str = address.clone();
        let main_registers_key = crate::keys::get_register_signing_key()
            .wrap_err("The register key is required to perform this action")?;
        let register_key = Client::register_key_from_name(&main_registers_key, &name_str);
        RegisterAddress::new(register_key.public_key())
    } else {
        RegisterAddress::from_hex(&address)
            .wrap_err(format!("Failed to parse register address: {address}"))
            .with_suggestion(|| {
                "if you want to use the name as the address, run the command with the --name flag"
            })?
    };

    if name {
        println!("Getting register history with name: {address}");
        info!("Getting register history with name: {address}");
    } else {
        println!("Getting register history at address: {address}");
        info!("Getting register history at address: {address}");
    }

    let mut history = client.register_history(&addr);

    println!("✅ Register history found at: {address}");
    info!("Register history found at: {address}");
    println!("History of values:");

    let values = history
        .collect()
        .await
        .wrap_err(format!("Error getting register history at: {address}"))?;

    for value in values {
        if hex {
            let hex_value = hex::encode(value);
            println!("[{hex_value}]");
        } else {
            let value_str = String::from_utf8_lossy(&value[..]);
            println!("[{value_str}]");
        }
    }

    Ok(())
}
