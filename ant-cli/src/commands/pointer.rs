// Copyright 2025 MaidSafe.net limited.
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
use autonomi::ChunkAddress;
use autonomi::Client;
use autonomi::GraphEntryAddress;
use autonomi::PointerAddress;
use autonomi::ScratchpadAddress;
use autonomi::TransactionConfig;
use autonomi::client::pointer::PointerTarget;
use autonomi::client::pointer::SecretKey as PointerSecretKey;
use color_eyre::Section;
use color_eyre::eyre::Context;
use color_eyre::eyre::Result;
use color_eyre::eyre::eyre;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetDataType {
    /// Graph entry address
    GraphEntry,
    /// Scratchpad address
    Scratchpad,
    /// Pointer address
    Pointer,
    /// Chunk address
    Chunk,
    /// Auto-detect the type of the target
    #[default]
    Auto,
}

pub fn parse_target_data_type(s: &str) -> Result<TargetDataType> {
    match s {
        "graph" => Ok(TargetDataType::GraphEntry),
        "scratchpad" => Ok(TargetDataType::Scratchpad),
        "pointer" => Ok(TargetDataType::Pointer),
        "chunk" => Ok(TargetDataType::Chunk),
        "auto" => Ok(TargetDataType::Auto),
        _ => Err(eyre!("Invalid target data type: {s}")),
    }
}

/// Generates a new general pointer key
///
/// # Arguments
/// * `overwrite` - If true, overwrites existing key if it exists
pub fn generate_key(overwrite: bool) -> Result<()> {
    let key_path = crate::keys::get_pointer_signing_key_path()?;
    if key_path.exists() && !overwrite {
        error!("Pointer key already exists at: {key_path:?}");
        return Err(eyre!("Pointer key already exists at: {}", key_path.display()))
            .with_suggestion(|| "if you want to overwrite the existing key, run the command with the --overwrite flag")
            .with_warning(|| "overwriting the existing key might result in loss of access to any existing pointers created using that key");
    }

    let key = PointerSecretKey::random();
    let path = crate::keys::create_pointer_signing_key_file(key)
        .wrap_err("Failed to create new pointer key")?;
    info!("Created new pointer key at: {path:?}");
    println!("✅ Created new pointer key at: {}", path.display());
    Ok(())
}

/// Estimates the cost to create a pointer
///
/// # Arguments
/// * `name` - Name of the pointer
/// * `network_context` - Network context for the operation
pub async fn cost(name: String, network_context: NetworkContext) -> Result<()> {
    let key = crate::keys::get_pointer_signing_key(&name)
        .wrap_err("The pointer key is required to perform this action")?;
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let cost = client
        .pointer_cost(&key.public_key())
        .await
        .wrap_err("Failed to get cost for pointer")?;
    info!("Estimated cost to create a pointer with name {name}: {cost}");
    println!("✅ The estimated cost to create a pointer with name {name} is: {cost}");
    Ok(())
}

/// Creates a new pointer
///
/// # Arguments
/// * `context` - Network context for the operation
/// * `name` - Name of the pointer
/// * `target` - Target address as a string to store in the pointer
/// * `target_data_type` - The type of the target data
/// * `max_fee_per_gas` - Optional maximum fee per gas
pub async fn create(
    context: NetworkContext,
    name: String,
    target: String,
    target_data_type: TargetDataType,
    max_fee_per_gas_param: Option<MaxFeePerGasParam>,
) -> Result<()> {
    let pointer_key = crate::keys::get_pointer_signing_key(&name)
        .wrap_err("The pointer key is required to perform this action")?;
    let client = crate::actions::connect_to_network(context)
        .await
        .map_err(|(err, _)| err)?;

    let target = pointer_target_from_hex(&target, target_data_type, &client).await?;
    let mut wallet = load_wallet(client.evm_network())?;

    let max_fee_per_gas =
        get_max_fee_per_gas_from_opt_param(max_fee_per_gas_param, client.evm_network())?;
    wallet.set_transaction_config(TransactionConfig { max_fee_per_gas });

    println!("Creating pointer with name: {name}");
    info!("Creating pointer with name: {name}");

    let (cost, address) = client
        .pointer_create(&pointer_key, target, wallet.into())
        .await
        .wrap_err("Failed to create pointer")?;

    println!("✅ Pointer created at address: {address}");
    println!("With name: {name}");
    info!("Pointer created at address: {address} with name: {name}");
    println!("Total cost: {cost} AttoTokens");

    crate::user_data::write_local_pointer(address, &name)
        .wrap_err("Failed to save pointer to local user data")
        .with_suggestion(|| "Local user data saves the pointer address above to disk (for the pointer list command), without it you need to keep track of the address yourself")?;
    info!("Saved pointer to local user data");

    Ok(())
}

async fn pointer_target_from_hex(
    target: &str,
    target_data_type: TargetDataType,
    client: &Client,
) -> Result<PointerTarget> {
    match target_data_type {
        TargetDataType::GraphEntry => {
            let graph = GraphEntryAddress::from_hex(target)
                .map_err(|_| eyre!("Failed to parse graph entry address from hex"))?;
            Ok(PointerTarget::GraphEntryAddress(graph))
        }
        TargetDataType::Scratchpad => {
            let scratchpad = ScratchpadAddress::from_hex(target)
                .map_err(|_| eyre!("Failed to parse scratchpad address from hex"))?;
            Ok(PointerTarget::ScratchpadAddress(scratchpad))
        }
        TargetDataType::Pointer => {
            let pointer = PointerAddress::from_hex(target)
                .map_err(|_| eyre!("Failed to parse pointer address from hex"))?;
            Ok(PointerTarget::PointerAddress(pointer))
        }
        TargetDataType::Chunk => {
            let chunk = ChunkAddress::from_hex(target)
                .map_err(|_| eyre!("Failed to parse chunk address from hex"))?;
            Ok(PointerTarget::ChunkAddress(chunk))
        }
        TargetDataType::Auto => {
            println!("Auto-detecting target data type by fetching from network...");
            let target_data_type = client
                .analyze_address_type(target, false)
                .await
                .wrap_err(eyre!("Failed to auto-detect target data type"))
                .with_suggestion(|| {
                    "If you know the type of the target data, you can use the -t flag to specify it"
                })?;
            println!("Auto-detected target data type to be: {target_data_type:?}");
            Ok(target_data_type)
        }
    }
}

/// Display information to share a pointer
///
/// # Arguments
/// * `name` - Name of the pointer to share
pub fn share(name: String) -> Result<()> {
    let pointer_key = crate::keys::get_pointer_signing_key(&name)
        .wrap_err("The pointer key is required to perform this action")?;

    let hex = pointer_key.to_hex();
    println!("Share this secret key with the recipient: {hex}");
    println!("The recipient can use this key to read and write to the pointer");
    println!(
        "The recipient can use the following command to get the pointer: `ant pointer get --secret-key {hex}`"
    );
    Ok(())
}

/// Gets the target from an existing pointer on the network
///
/// # Arguments
/// * `context` - Network context for the operation
/// * `name` - Name of the pointer to load
/// * `secret_key` - Whether this is an imported secret_key
pub async fn get(context: NetworkContext, name: String, secret_key: bool) -> Result<()> {
    let client = crate::actions::connect_to_network(context)
        .await
        .map_err(|(err, _)| err)?;

    let pointer_key = if secret_key {
        PointerSecretKey::from_hex(&name).wrap_err("Failed to parse secret key from hex")?
    } else {
        crate::keys::get_pointer_signing_key(&name)
            .wrap_err("The pointer key is required to perform this action")?
    };

    println!("Retrieving pointer from network...");
    let address = PointerAddress::new(pointer_key.public_key());
    let pointer = client
        .pointer_get(&address)
        .await
        .wrap_err("Failed to retrieve pointer from network")?;

    println!("✅ Successfully loaded pointer:");
    println!("Address: {}", address.to_hex());
    println!("Counter: {}", pointer.counter());
    println!("Target: {:?}", pointer.target());

    Ok(())
}

/// Edits the target of an existing pointer
///
/// # Arguments
/// * `context` - Network context for the operation
/// * `name` - Name of the pointer to edit
/// * `secret_key` - Whether this is an imported secret_key
/// * `target` - The new target to store in the pointer
/// * `target_data_type` - The type of the target data
pub async fn edit(
    context: NetworkContext,
    name: String,
    secret_key: bool,
    target: String,
    target_data_type: TargetDataType,
) -> Result<()> {
    let client = crate::actions::connect_to_network(context)
        .await
        .map_err(|(err, _)| err)?;

    let pointer_key = if secret_key {
        PointerSecretKey::from_hex(&name).wrap_err("Failed to parse secret key from hex")?
    } else {
        crate::keys::get_pointer_signing_key(&name)
            .wrap_err("The pointer key is required to perform this action")?
    };

    let target = pointer_target_from_hex(&target, target_data_type, &client).await?;

    // get network current pointer
    println!("Retrieving pointer from network...");
    let address = PointerAddress::new(pointer_key.public_key());
    let net_pointer = client
        .pointer_get(&address)
        .await
        .wrap_err("Failed to retrieve pointer from network")?;
    println!(
        "Got current pointer at address {address:?} with counter: {}",
        net_pointer.counter()
    );

    // get latest between local cached pointer and network pointer
    let maybe_local_pointer = crate::user_data::get_local_pointer_value(&name);
    let current_pointer = match maybe_local_pointer {
        Ok(local_pointer) if local_pointer.counter() > net_pointer.counter() => {
            println!(
                "Using cached pointer value as it is more recent: {} > {}",
                local_pointer.counter(),
                net_pointer.counter()
            );
            local_pointer
        }
        _ => net_pointer,
    };

    println!("Updating pointer target...");
    info!("Updating pointer target");

    let new_pointer = client
        .pointer_update_from(&current_pointer, &pointer_key, target)
        .await
        .wrap_err("Failed to update pointer")?;

    println!("✅ Pointer updated");
    println!("New counter: {}", new_pointer.counter());
    if secret_key {
        println!("With secret key: {}", pointer_key.to_hex());
    } else {
        println!("With name: {name}");
    }
    info!("Pointer updated");

    if !secret_key {
        let addr = PointerAddress::new(pointer_key.public_key());
        crate::user_data::write_local_pointer(addr, &name)
            .wrap_err("Failed to save pointer to local user data")
            .with_suggestion(|| "Local user data saves the pointer address above to disk (for the pointer list command), without it you need remember the name yourself")?;
        crate::user_data::write_local_pointer_value(&name, &new_pointer)
            .wrap_err("Failed to save pointer value to local user data")
            .with_suggestion(
                || "Local user data caches the pointer data to disk for use in future updates",
            )?;
        info!("Saved pointer to local user data");
    }

    Ok(())
}

/// Lists all previous pointers
pub fn list(verbose: bool) -> Result<()> {
    println!("Retrieving local pointer data...");
    let pointers = crate::user_data::get_local_pointers()?;
    println!("✅ You have {} pointer(s):", pointers.len());
    for (name, address) in pointers {
        println!("{name} - {address}");
        if verbose {
            let maybe_pointer = crate::user_data::get_local_pointer_value(&name);
            if let Ok(pointer) = maybe_pointer {
                println!("  Counter: {}", pointer.counter());
                println!("  Target: {}", pointer.target().to_hex());
                println!();
            } else {
                println!("  Counter: <missing from cache>");
                println!("  Target: <missing from cache>");
                println!();
            }
        }
    }
    Ok(())
}
