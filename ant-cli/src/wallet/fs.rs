// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::wallet::DUMMY_NETWORK;
use crate::wallet::encryption::{decrypt_private_key, encrypt_private_key};
use crate::wallet::input::{get_password_input, get_wallet_selection_input};
use autonomi::{Network, RewardsAddress, Wallet};
use color_eyre::eyre::{Context, bail, eyre};
use color_eyre::{Result, Section};
use const_hex::traits::FromHex;
use prettytable::{Cell, Row, Table};
use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;

const ENCRYPTED_PRIVATE_KEY_EXT: &str = ".encrypted";

pub static SELECTED_WALLET_ADDRESS: OnceLock<String> = OnceLock::new();

/// Creates the wallets folder if it is missing and returns the folder path.
pub(crate) fn get_client_wallet_dir_path() -> Result<PathBuf> {
    let mut home_dirs = crate::access::data_dir::get_client_data_dir_path()
        .wrap_err("Failed to get wallet directory")?;
    home_dirs.push("wallets");

    std::fs::create_dir_all(home_dirs.as_path())
        .wrap_err("Failed to create wallets folder, make sure you have the correct permissions")?;

    Ok(home_dirs)
}

/// Writes the private key (hex-encoded) to disk.
///
/// When a password is set, the private key file will be encrypted.
pub(crate) fn store_private_key(
    private_key: &str,
    encryption_password: Option<String>,
) -> Result<OsString> {
    let wallet = Wallet::new_from_private_key(DUMMY_NETWORK, private_key)
        .map_err(|_| eyre!("Private key is invalid"))?;

    // Wallet address
    let wallet_address = wallet.address().to_string();
    let wallets_folder = get_client_wallet_dir_path()?;

    // If `encryption_password` is provided, the private key will be encrypted with the password.
    // Else it will be saved as plain text.
    if let Some(password) = encryption_password.as_ref() {
        let encrypted_key = encrypt_private_key(private_key, password)?;
        let file_name = format!("{wallet_address}{ENCRYPTED_PRIVATE_KEY_EXT}");
        let file_path = wallets_folder.join(file_name);

        std::fs::write(file_path.clone(), encrypted_key).wrap_err("Failed to store private key")?;

        Ok(file_path.into_os_string())
    } else {
        let file_path = wallets_folder.join(wallet_address);

        std::fs::write(file_path.clone(), private_key).wrap_err("Failed to store private key")?;

        Ok(file_path.into_os_string())
    }
}

/// Loads the private key (hex-encoded) from disk.
///
/// If the private key file is encrypted, the function will prompt for the decryption password in the CLI.
pub(crate) fn load_private_key(wallet_address: &str) -> Result<String> {
    let wallets_folder = get_client_wallet_dir_path()?;

    let mut file_name = wallet_address.to_string();

    // Check if a file with the encrypted extension exists
    let encrypted_file_path =
        wallets_folder.join(format!("{wallet_address}{ENCRYPTED_PRIVATE_KEY_EXT}"));

    let is_plain = wallets_folder.join(&file_name).exists();

    // Trick to favour the plain file in case they both exist
    let is_encrypted = encrypted_file_path.exists() && !is_plain;

    if is_encrypted {
        file_name.push_str(ENCRYPTED_PRIVATE_KEY_EXT);
    }

    let file_path = wallets_folder.join(file_name);

    let mut file =
        std::fs::File::open(&file_path).map_err(|e| eyre!("Private key file not found: {e}"))?;

    let mut buffer = String::new();
    file.read_to_string(&mut buffer)
        .map_err(|_| eyre!("Invalid private key file"))?;

    // If the file is encrypted, prompt for the password and decrypt the key.
    if is_encrypted {
        let password = get_password_input("Enter password to decrypt wallet:");

        decrypt_private_key(&buffer, &password)
            .map_err(|e| eyre!("Failed to decrypt private key: {e}"))
    } else {
        Ok(buffer)
    }
}

pub(crate) fn load_wallet_from_address(wallet_address: &str, network: &Network) -> Result<Wallet> {
    let private_key = load_private_key(wallet_address)?;
    let wallet = Wallet::new_from_private_key(network.clone(), &private_key)
        .map_err(|e| eyre!("Could not initialize wallet: {e}"))?;
    Ok(wallet)
}

pub(crate) fn select_wallet_from_disk(network: &Network) -> Result<Wallet> {
    let wallet_address = select_local_wallet_address()?;
    load_wallet_from_address(&wallet_address, network)
}

pub(crate) fn select_wallet_private_key() -> Result<String> {
    let wallet_address = select_local_wallet_address()?;
    load_private_key(&wallet_address)
}

pub(crate) fn select_local_wallet_address() -> Result<String> {
    // Try if a wallet address was already selected this session
    if let Some(wallet_address) = SELECTED_WALLET_ADDRESS.get() {
        return Ok(wallet_address.clone());
    }

    let wallets_folder = get_client_wallet_dir_path()?;
    let wallet_files = get_wallet_files(&wallets_folder)?;

    let wallet_address = match wallet_files.len() {
        0 => {
            return Err(eyre!("No local wallets found."))
                .with_suggestion(|| "Providing SECRET_KEY as an environment variable also works!");
        }
        1 => Ok(filter_wallet_file_extension(&wallet_files[0])),
        _ => get_wallet_selection(wallet_files),
    }?;

    Ok(SELECTED_WALLET_ADDRESS
        .get_or_init(|| wallet_address)
        .to_string())
}

fn get_wallet_selection(wallet_files: Vec<String>) -> Result<String> {
    list_wallets(&wallet_files);

    let selected_index = get_wallet_selection_input("Select by index:")
        .parse::<usize>()
        .map_err(|_| eyre!("Invalid wallet selection input"))?;

    if selected_index < 1 || selected_index > wallet_files.len() {
        bail!("Invalid wallet selection input");
    }

    Ok(filter_wallet_file_extension(
        &wallet_files[selected_index - 1],
    ))
}

fn list_wallets(wallet_files: &[String]) {
    println!("Wallets:");

    let mut table = Table::new();

    table.add_row(Row::new(vec![
        Cell::new("Index"),
        Cell::new("Address"),
        Cell::new("Encrypted"),
    ]));

    for (index, wallet_file) in wallet_files.iter().enumerate() {
        let encrypted = wallet_file.contains(ENCRYPTED_PRIVATE_KEY_EXT);

        table.add_row(Row::new(vec![
            Cell::new(&(index + 1).to_string()),
            Cell::new(&filter_wallet_file_extension(wallet_file)),
            Cell::new(&encrypted.to_string()),
        ]));
    }

    table.printstd();
}

fn get_wallet_files(wallets_folder: &PathBuf) -> Result<Vec<String>> {
    let wallet_files = std::fs::read_dir(wallets_folder)
        .map_err(|e| eyre!("Failed to read wallets folder: {e}"))?
        .filter_map(Result::ok)
        .filter_map(|dir_entry| dir_entry.file_name().into_string().ok())
        .filter(|file_name| {
            let cleaned_file_name = filter_wallet_file_extension(file_name);
            RewardsAddress::from_hex(cleaned_file_name).is_ok()
        })
        .collect::<Vec<_>>();

    Ok(wallet_files)
}

fn filter_wallet_file_extension(wallet_file: &str) -> String {
    wallet_file.replace(ENCRYPTED_PRIVATE_KEY_EXT, "")
}
