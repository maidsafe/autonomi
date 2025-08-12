// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::actions::NetworkContext;
use crate::args::max_fee_per_gas::MaxFeePerGasParam;
use crate::args::max_fee_per_gas::get_max_fee_per_gas_from_opt_param;
use crate::wallet::load_wallet;
use autonomi::Bytes;
use autonomi::ScratchpadAddress;
use autonomi::TransactionConfig;
use autonomi::client::scratchpad::SecretKey as ScratchpadSecretKey;
use color_eyre::Section;
use color_eyre::eyre::Context;
use color_eyre::eyre::Result;
use color_eyre::eyre::eyre;

/// Generates a new general scratchpad key
///
/// # Arguments
/// * `overwrite` - If true, overwrites existing key if it exists
pub fn generate_key(overwrite: bool) -> Result<()> {
    // check if the key already exists
    let key_path = crate::keys::get_scratchpad_signing_key_path()?;
    if key_path.exists() && !overwrite {
        error!("Scratchpad key already exists at: {key_path:?}");
        return Err(eyre!("Scratchpad key already exists at: {}", key_path.display()))
            .with_suggestion(|| "if you want to overwrite the existing key, run the command with the --overwrite flag")
            .with_warning(|| "overwriting the existing key might result in loss of access to any existing scratchpads created using that key");
    }

    // generate and write a new key to file
    let key = ScratchpadSecretKey::random();
    let path = crate::keys::create_scratchpad_signing_key_file(key)
        .wrap_err("Failed to create new scratchpad key")?;
    info!("Created new scratchpad key at: {path:?}");
    println!("✅ Created new scratchpad key at: {}", path.display());
    Ok(())
}

/// Estimates the cost to create a scratchpad
///
/// # Arguments
/// * `name` - Name of the scratchpad
/// * `network_context` - Network context for the operation
pub async fn cost(name: String, network_context: NetworkContext) -> Result<()> {
    let key = crate::keys::get_scratchpad_signing_key(&name)
        .wrap_err("The scratchpad key is required to perform this action")?;
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let cost = client
        .scratchpad_cost(&key.public_key())
        .await
        .wrap_err("Failed to get cost for scratchpad")?;
    info!("Estimated cost to create a scratchpad with name {name}: {cost}");
    println!("✅ The estimated cost to create a scratchpad with name {name} is: {cost}");
    Ok(())
}

/// Creates a new scratchpad
///
/// # Arguments
/// * `context` - Network context for the operation
/// * `name` - Name of the scratchpad
/// * `data` - Data to store in the scratchpad
/// * `max_fee_per_gas` - Optional maximum fee per gas
pub async fn create(
    context: NetworkContext,
    name: String,
    data: String,
    max_fee_per_gas_param: Option<MaxFeePerGasParam>,
) -> Result<()> {
    let scratchpad_key = crate::keys::get_scratchpad_signing_key(&name)
        .wrap_err("The scratchpad key is required to perform this action")?;
    let client = crate::actions::connect_to_network(context)
        .await
        .map_err(|(err, _)| err)?;

    let mut wallet = load_wallet(client.evm_network())?;

    let max_fee_per_gas =
        get_max_fee_per_gas_from_opt_param(max_fee_per_gas_param, client.evm_network())?;
    wallet.set_transaction_config(TransactionConfig { max_fee_per_gas });

    println!("Creating scratchpad with name: {name}");
    info!("Creating scratchpad with name: {name}");

    let bytes = Bytes::from(data);
    let (cost, address) = client
        .scratchpad_create(&scratchpad_key, Default::default(), &bytes, wallet.into())
        .await
        .wrap_err("Failed to create scratchpad")?;

    println!("✅ Scratchpad created at address: {address}");
    println!("With name: {name}");
    info!("Scratchpad created at address: {address} with name: {name}");
    println!("Total cost: {cost} AttoTokens");

    crate::user_data::write_local_scratchpad(address, &name)
        .wrap_err("Failed to save scratchpad to local user data")
        .with_suggestion(|| "Local user data saves the scratchpad address above to disk (for the scratchpad list command), without it you need to keep track of the address yourself")?;
    info!("Saved scratchpad to local user data");

    Ok(())
}

/// Display information to share a scratchpad
///
/// # Arguments
/// * `name` - Name of the scratchpad to share
pub fn share(name: String) -> Result<()> {
    let scratchpad_key = crate::keys::get_scratchpad_signing_key(&name)
        .wrap_err("The scratchpad key is required to perform this action")?;

    let hex = scratchpad_key.to_hex();
    println!("Share this secret key with the recipient: {hex}");
    println!("The recipient can use this key to read and write to the scratchpad");
    println!(
        "The recipient can use the following command to get the scratchpad: `ant scratchpad get --secret-key {hex}`"
    );
    Ok(())
}

/// Gets the data from an existing scratchpad on the network
///
/// # Arguments
/// * `context` - Network context for the operation
/// * `name` - Name of the scratchpad to load
/// * `secret_key` - Whether this is an imported secret_key
pub async fn get(context: NetworkContext, name: String, secret_key: bool, hex: bool) -> Result<()> {
    let client = crate::actions::connect_to_network(context)
        .await
        .map_err(|(err, _)| err)?;

    let scratchpad_key = if secret_key {
        // If secret_key is true, we expect the name to be a hex-encoded secret key
        ScratchpadSecretKey::from_hex(&name).wrap_err("Failed to parse secret key from hex")?
    } else {
        crate::keys::get_scratchpad_signing_key(&name)
            .wrap_err("The scratchpad key is required to perform this action")?
    };

    println!("Retrieving scratchpad from network...");
    let address = ScratchpadAddress::new(scratchpad_key.public_key());
    let scratchpad = client
        .scratchpad_get(&address)
        .await
        .wrap_err("Failed to retrieve scratchpad from network")?;

    let data = scratchpad
        .decrypt_data(&scratchpad_key)
        .wrap_err("Failed to decrypt scratchpad data")?;

    println!("✅ Successfully loaded scratchpad:");
    println!("Address: {}", address.to_hex());
    println!("Counter: {}", scratchpad.counter());
    if hex {
        println!("Data in hex: {}", hex::encode(data));
    } else {
        println!("Data: {}", String::from_utf8_lossy(&data));
    }

    Ok(())
}

/// Edits the contents of an existing scratchpad
///
/// # Arguments
/// * `context` - Network context for the operation
/// * `name` - Name of the scratchpad to edit
/// * `secret_key` - Whether this is an imported secret_key
/// * `data` - The new data to store in the scratchpad
pub async fn edit(
    context: NetworkContext,
    name: String,
    secret_key: bool,
    data: String,
) -> Result<()> {
    let client = crate::actions::connect_to_network(context)
        .await
        .map_err(|(err, _)| err)?;

    let scratchpad_key = if secret_key {
        // If secret_key is true, we expect the name to be a hex-encoded secret key
        ScratchpadSecretKey::from_hex(&name).wrap_err("Failed to parse secret key from hex")?
    } else {
        crate::keys::get_scratchpad_signing_key(&name)
            .wrap_err("The scratchpad key is required to perform this action")?
    };

    let bytes = Bytes::from(data);

    // get network current scratchpad
    println!("Retrieving scratchpad from network...");
    let address = ScratchpadAddress::new(scratchpad_key.public_key());
    let net_scratchpad = client
        .scratchpad_get(&address)
        .await
        .wrap_err("Failed to retrieve scratchpad from network")?;
    println!(
        "Got current scratchpad at address {address:?} with counter: {}",
        net_scratchpad.counter()
    );

    // get latest between local cached scratchpad and network scratchpad
    let maybe_local_scratchpad = crate::user_data::get_local_scratchpad_value(&name);
    let current_scratchpad = match maybe_local_scratchpad {
        Ok(local_scratchpad) if local_scratchpad.counter() > net_scratchpad.counter() => {
            println!(
                "Using cached scratchpad value as it is more recent: {} > {}",
                local_scratchpad.counter(),
                net_scratchpad.counter()
            );
            local_scratchpad
        }
        _ => net_scratchpad,
    };

    println!("Updating scratchpad data...");
    info!("Updating scratchpad data");

    let new_scratchpad = client
        .scratchpad_update_from(
            &current_scratchpad,
            &scratchpad_key,
            Default::default(),
            &bytes,
        )
        .await
        .wrap_err("Failed to update scratchpad")?;

    println!("✅ Scratchpad updated");
    println!("New counter: {}", new_scratchpad.counter());
    if secret_key {
        println!("With secret key: {}", scratchpad_key.to_hex());
    } else {
        println!("With name: {name}");
    }
    info!("Scratchpad updated");

    if !secret_key {
        let addr = ScratchpadAddress::new(scratchpad_key.public_key());
        crate::user_data::write_local_scratchpad(addr, &name)
            .wrap_err("Failed to save scratchpad to local user data")
            .with_suggestion(|| "Local user data saves the scratchpad address above to disk (for the scratchpad list command), without it you need remember the name yourself")?;
        crate::user_data::write_local_scratchpad_value(&name, &new_scratchpad)
            .wrap_err("Failed to save scratchpad value to local user data")
            .with_suggestion(
                || "Local user data caches the scratchpad data to disk for use in future updates",
            )?;
        info!("Saved scratchpad to local user data");
    }

    Ok(())
}

/// Lists all previous scratchpads
pub fn list(verbose: bool) -> Result<()> {
    println!("Retrieving local scratchpad data...");
    let scratchpads = crate::user_data::get_local_scratchpads()?;
    println!("✅ You have {} scratchpad(s):", scratchpads.len());
    for (name, address) in scratchpads {
        println!("{name} - {address}");
        if verbose {
            let maybe_scratchpad = crate::user_data::get_local_scratchpad_value(&name);
            if let Ok(scratchpad) = maybe_scratchpad {
                println!("  Counter: {}", scratchpad.counter());
                println!(
                    "  Data: {} bytes of encrypted data",
                    scratchpad.encrypted_data().len()
                );
                println!();
            } else {
                println!("  Counter: <missing from cache>");
                println!("  Data: <missing from cache>");
                println!();
            }
        }
    }
    Ok(())
}
