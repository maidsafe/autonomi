// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(deprecated)]

use crate::network::NetworkPeers;
use crate::utils::collect_upload_summary;
use crate::wallet::load_wallet;
use autonomi::client::registers::RegisterAddress;
use autonomi::client::registers::RegisterPermissions;
use autonomi::client::registers::RegisterSecretKey;
use autonomi::Client;
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

pub async fn cost(name: &str, peers: NetworkPeers) -> Result<()> {
    let register_key = crate::keys::get_register_signing_key()
        .wrap_err("The register key is required to perform this action")?;
    let client = crate::actions::connect_to_network(peers).await?;

    let cost = client
        .register_cost(name.to_string(), register_key)
        .await
        .wrap_err("Failed to get cost for register")?;
    info!("Estimated cost to create a register with name {name}: {cost}");
    println!("✅ The estimated cost to create a register with name {name} is: {cost}");
    Ok(())
}

pub async fn create(name: &str, value: &str, public: bool, peers: NetworkPeers) -> Result<()> {
    let wallet = load_wallet()?;
    let register_key = crate::keys::get_register_signing_key()
        .wrap_err("The register key is required to perform this action")?;
    let mut client = crate::actions::connect_to_network(peers).await?;
    let event_receiver = client.enable_client_events();
    let (upload_summary_thread, upload_completed_tx) = collect_upload_summary(event_receiver);

    println!("Creating register with name: {name}");
    info!("Creating register with name: {name}");
    let register = if public {
        println!("With public write access");
        info!("With public write access");
        let permissions = RegisterPermissions::new_anyone_can_write();
        client
            .register_create_with_permissions(
                Some(value.as_bytes().to_vec().into()),
                name,
                register_key,
                permissions,
                &wallet,
            )
            .await
            .wrap_err("Failed to create register")?
    } else {
        println!("With private write access");
        info!("With private write access");
        client
            .register_create(
                Some(value.as_bytes().to_vec().into()),
                name,
                register_key,
                &wallet,
            )
            .await
            .wrap_err("Failed to create register")?
    };

    let address = register.address();

    if let Err(e) = upload_completed_tx.send(()) {
        error!("Failed to send upload completed event: {e:?}");
        eprintln!("Failed to send upload completed event: {e:?}");
    }

    let summary = upload_summary_thread.await?;
    if summary.record_count == 0 {
        println!("✅ The register already exists on the network at address: {address}.");
        println!("No tokens were spent.");
    } else {
        println!("✅ Register created at address: {address}");
        println!("With name: {name}");
        println!("And initial value: [{value}]");
        info!("Register created at address: {address} with name: {name}");
        println!("Total cost: {} AttoTokens", summary.tokens_spent);
    }
    info!("Summary of register creation: {summary:?}");

    crate::user_data::write_local_register(address, name)
        .wrap_err("Failed to save register to local user data")
        .with_suggestion(|| "Local user data saves the register address above to disk, without it you need to keep track of the address yourself")?;
    info!("Saved register to local user data");

    Ok(())
}

pub async fn edit(address: String, name: bool, value: &str, peers: NetworkPeers) -> Result<()> {
    let register_key = crate::keys::get_register_signing_key()
        .wrap_err("The register key is required to perform this action")?;
    let client = crate::actions::connect_to_network(peers).await?;

    let address = if name {
        Client::register_address(&address, &register_key)
    } else {
        RegisterAddress::from_hex(&address)
            .wrap_err(format!("Failed to parse register address: {address}"))
            .with_suggestion(|| {
                "if you want to use the name as the address, run the command with the --name flag"
            })?
    };

    println!("Getting register at address: {address}");
    info!("Getting register at address: {address}");
    let register = client
        .register_get(address)
        .await
        .wrap_err(format!("Failed to get register at address: {address}"))?;

    println!("Found register at address: {address}");
    println!("Updating register with new value: {value}");
    info!("Updating register at address: {address} with new value: {value}");

    client
        .register_update(register, value.as_bytes().to_vec().into(), register_key)
        .await
        .wrap_err(format!("Failed to update register at address: {address}"))?;

    println!("✅ Successfully updated register");
    println!("With value: [{value}]");
    info!("Successfully updated register at address: {address}");

    Ok(())
}

pub async fn get(address: String, name: bool, peers: NetworkPeers) -> Result<()> {
    let register_key = crate::keys::get_register_signing_key()
        .wrap_err("The register key is required to perform this action")?;
    let client = crate::actions::connect_to_network(peers).await?;

    let address = if name {
        Client::register_address(&address, &register_key)
    } else {
        RegisterAddress::from_hex(&address)
            .wrap_err(format!("Failed to parse register address: {address}"))
            .with_suggestion(|| {
                "if you want to use the name as the address, run the command with the --name flag"
            })?
    };

    println!("Getting register at address: {address}");
    info!("Getting register at address: {address}");
    let register = client
        .register_get(address)
        .await
        .wrap_err(format!("Failed to get register at address: {address}"))?;
    let values = register.values();

    println!("✅ Register found at address: {address}");
    info!("Register found at address: {address}");
    match values.as_slice() {
        [one] => println!("With value: [{:?}]", String::from_utf8_lossy(one)),
        _ => {
            println!("With multiple concurrent values:");
            for value in values.iter() {
                println!("[{:?}]", String::from_utf8_lossy(value));
            }
        }
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
