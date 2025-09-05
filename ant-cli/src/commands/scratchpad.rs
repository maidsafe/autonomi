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
use autonomi::client::data_types::scratchpad::ScratchpadError;
use color_eyre::Section;
use color_eyre::eyre::Context;
use color_eyre::eyre::Result;
use color_eyre::eyre::eyre;

/// Print detailed fork analysis for conflicting scratchpads
fn print_fork_analysis(conflicting_scratchpads: &[autonomi::client::data_types::scratchpad::Scratchpad], scratchpad_key: &ScratchpadSecretKey) {
    println!("Fork detected!");
    println!();
    println!("FORK ANALYSIS:");
    println!("{}", "=".repeat(60));
    println!("Retrieved {} conflicting scratchpad(s):", conflicting_scratchpads.len());
    
    // Sort by signature to ensure consistent ordering
    let mut sorted_scratchpads = conflicting_scratchpads.to_vec();
    sorted_scratchpads.sort_by(|a, b| hex::encode(a.signature().to_bytes()).cmp(&hex::encode(b.signature().to_bytes())));
    
    // Show each conflicting scratchpad
    for (i, scratchpad) in sorted_scratchpads.iter().enumerate() {
        println!();
        println!("CONFLICTING SCRATCHPAD #{} OF {}:", i + 1, sorted_scratchpads.len());
        println!("  Counter: {}", scratchpad.counter());
        println!("  Data type encoding: {}", scratchpad.data_encoding());
        println!("  PublicKey/Address: {}", hex::encode(scratchpad.owner().to_bytes()));
        println!("  Signature: {}", hex::encode(scratchpad.signature().to_bytes()));
        println!("  Scratchpad hash: {}", hex::encode(scratchpad.scratchpad_hash().0));
        println!("  Encrypted data hash: {}", hex::encode(scratchpad.encrypted_data_hash()));
        println!("  Encrypted data size: {} bytes", scratchpad.encrypted_data().len());
        
        // Decrypt and show data
        match scratchpad.decrypt_data(scratchpad_key) {
            Ok(decrypted_data) => {
                let data_str = String::from_utf8_lossy(&decrypted_data);
                println!("  Decrypted data: \"{}\"", data_str);
                println!("  Decrypted data size: {} bytes", decrypted_data.len());
            }
            Err(decrypt_err) => {
                println!("  Decryption failed: {}", decrypt_err);
            }
        }
    }
}

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
pub async fn get(context: NetworkContext, name: String, secret_key: bool, hex: bool, print_fork_error: bool) -> Result<()> {
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
    let mut detected_fork: Option<Vec<autonomi::client::data_types::scratchpad::Scratchpad>> = None;
    
    let scratchpad_result = client.scratchpad_get(&address).await;
    
    let scratchpad = if print_fork_error {
        if let Err(ScratchpadError::Fork(ref conflicting_scratchpads)) = scratchpad_result {
            detected_fork = Some(conflicting_scratchpads.clone());
        }
        match scratchpad_result.wrap_err("Failed to retrieve scratchpad from network") {
            Ok(scratchpad) => scratchpad,
            Err(error) => {
                // Print the error with full formatting like the original ? operator would
                let report = color_eyre::Report::from(error);
                eprintln!("{:?}", report);
                
                // If it was a fork error, print detailed analysis
                if let Some(conflicting_scratchpads) = detected_fork {
                    print_fork_analysis(&conflicting_scratchpads, &scratchpad_key);
                }
                
                // Exit with error code - we've already printed everything
                std::process::exit(1);
            }
        }
    } else {
        scratchpad_result.wrap_err("Failed to retrieve scratchpad from network")?
    };

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
    print_fork_error: bool,
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
    let mut detected_fork: Option<Vec<autonomi::client::data_types::scratchpad::Scratchpad>> = None;
    
    let scratchpad_result = client.scratchpad_get(&address).await;
    
    let net_scratchpad = if print_fork_error {
        if let Err(ScratchpadError::Fork(ref conflicting_scratchpads)) = scratchpad_result {
            detected_fork = Some(conflicting_scratchpads.clone());
        }
        match scratchpad_result.wrap_err("Failed to retrieve scratchpad from network") {
            Ok(scratchpad) => scratchpad,
            Err(error) => {
                // Print the error with full formatting like the original ? operator would
                let report = color_eyre::Report::from(error);
                eprintln!("{:?}", report);
                
                // If it was a fork error, print detailed analysis
                if let Some(conflicting_scratchpads) = detected_fork {
                    print_fork_analysis(&conflicting_scratchpads, &scratchpad_key);
                }
                
                // Exit with error code - we've already printed everything
                std::process::exit(1);
            }
        }
    } else {
        scratchpad_result.wrap_err("Failed to retrieve scratchpad from network")?
    };
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

/// Prints the full fork error information with decrypted content from all conflicting scratchpads
///
/// This function accesses the raw ScratchpadError::Fork data to decrypt and display
/// the actual content of each conflicting scratchpad, allowing users to see what data conflicts
/// and make informed decisions about which version to keep.
///
/// # Arguments
/// * `context` - Network context for the operation
/// * `name` - Name of the scratchpad to check for fork errors
/// * `secret_key` - Whether this is an imported secret_key instead of a named scratchpad
pub async fn print_fork_error(
    context: NetworkContext,
    name: String,
    secret_key: bool,
) -> Result<()> {
    use autonomi::client::data_types::scratchpad::ScratchpadError;

    let client = crate::actions::connect_to_network(context)
        .await
        .map_err(|(err, _)| err)?;

    let scratchpad_key = if secret_key {
        ScratchpadSecretKey::from_hex(&name).wrap_err("Failed to parse secret key from hex")?
    } else {
        crate::keys::get_scratchpad_signing_key(&name)
            .wrap_err("The scratchpad key is required to perform this action")?
    };

    println!("🔍 Checking scratchpad for fork conditions...");
    println!("Scratchpad key: {}", scratchpad_key.to_hex());
    let address = ScratchpadAddress::new(scratchpad_key.public_key());
    println!("Address: {}", address.to_hex());

    match client.scratchpad_get(&address).await {
        Ok(scratchpad) => {
            println!("✅ No fork detected - scratchpad retrieved successfully:");
            println!("Counter: {}", scratchpad.counter());
            
            match scratchpad.decrypt_data(&scratchpad_key) {
                Ok(data) => {
                    println!("Data: {}", String::from_utf8_lossy(&data));
                }
                Err(e) => {
                    println!("⚠️  Could not decrypt data: {}", e);
                }
            }
        }
        Err(scratchpad_error) => {
            // Direct access to ScratchpadError - no need to unwrap from eyre!
            match scratchpad_error {
                ScratchpadError::Fork(conflicting_scratchpads) => {
                    println!("Fork detected!");
                    println!();
                    println!("FORK ANALYSIS:");
                    println!("{}", "=".repeat(60));
                    println!("Retrieved {} conflicting scratchpad(s):", conflicting_scratchpads.len());
                    
                    // Sort by signature to ensure consistent ordering with Python test
                    let mut sorted_scratchpads = conflicting_scratchpads.clone();
                    sorted_scratchpads.sort_by(|a, b| hex::encode(a.signature().to_bytes()).cmp(&hex::encode(b.signature().to_bytes())));
                    
                    // Show each conflicting scratchpad in simple format
                    for (i, scratchpad) in sorted_scratchpads.iter().enumerate() {
                        println!();
                        println!("CONFLICTING SCRATCHPAD #{} OF {}:", i + 1, sorted_scratchpads.len());
                        println!("  Counter: {}", scratchpad.counter());
                        println!("  Data type encoding: {}", scratchpad.data_encoding());
                        println!("  PublicKey/Address: {}", hex::encode(scratchpad.owner().to_bytes()));
                        println!("  Signature: {}", hex::encode(scratchpad.signature().to_bytes()));
                        println!("  Scratchpad hash: {}", hex::encode(scratchpad.scratchpad_hash().0));
                        println!("  Encrypted data hash: {}", hex::encode(scratchpad.encrypted_data_hash()));
                        println!("  Encrypted data size: {} bytes", scratchpad.encrypted_data().len());
                        
                        // Decrypt and show data
                        match scratchpad.decrypt_data(&scratchpad_key) {
                            Ok(decrypted_data) => {
                                let data_str = String::from_utf8_lossy(&decrypted_data);
                                println!("  Decrypted data: \"{}\"", data_str);
                                println!("  Decrypted data size: {} bytes", decrypted_data.len());
                            }
                            Err(decrypt_err) => {
                                println!("  Decryption failed: {}", decrypt_err);
                            }
                        }
                    }
                }
                other_err => {
                    println!("❌ Non-fork scratchpad error:");
                    println!("{}", other_err);
                }
            }
        }
    }

    Ok(())
}
