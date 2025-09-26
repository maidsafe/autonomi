// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use autonomi::Wallet;
use color_eyre::{
    Section,
    eyre::{Context, Result, eyre},
};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum DataDirError {
    #[error(
        "Multiple accounts found: {0:?}. Please specify which account to use or provide the SECRET_KEY for the account you want to use"
    )]
    MultipleAccounts(Vec<String>),
    #[error(
        "No existing user data directories found. Please provide the SECRET_KEY for the account you want to use"
    )]
    NoExistingUserDirFound,
}

/// Get the base client data directory path
pub(crate) fn get_client_data_dir_base() -> Result<PathBuf> {
    let mut home_dirs = dirs_next::data_dir()
        .ok_or_else(|| eyre!("Failed to obtain data dir, your OS might not be supported."))?;
    home_dirs.push("autonomi");
    home_dirs.push("client");
    Ok(home_dirs)
}

/// Get the client data directory path
/// Automatically detects the wallet directory to use:
/// - If only one wallet directory exists, uses it
/// - If multiple wallet directories exist, try to get wallet from environment else returns error
/// - If no wallet directories exist, tries to get wallet from environment else returns error
pub fn get_client_data_dir_path() -> Result<PathBuf> {
    let base_dir = get_client_data_dir_base()?;

    // Check if there are any existing accounts user data directories
    let existing_users = get_existing_user_dirs()?;

    let wallet_addr = match &existing_users[..] {
        // Exactly one account exists, use it
        [one] => one.clone(),
        // No accounts exist yet, try to get address from current environment
        // First try from SECRET_KEY env var
        [] => match get_wallet_pk() {
            Ok(pk) => pk,
            Err(_) => return Err(DataDirError::NoExistingUserDirFound.into()),
        },
        // Multiple wallets exist, try SECRET_KEY env var else return error
        [_, ..] => match get_wallet_pk() {
            Ok(pk) => pk,
            Err(_) => return Err(DataDirError::MultipleAccounts(existing_users).into()),
        },
    };

    // Migrate legacy data if needed (user data stored directly under client/ without wallet address)
    super::data_dir_migration::migrate_legacy_data_if_needed(&wallet_addr)?;

    // Create the wallet directory
    let mut wallet_dir = base_dir;
    wallet_dir.push(&wallet_addr);
    std::fs::create_dir_all(wallet_dir.as_path())
        .wrap_err("Failed to create data dir")
        .with_suggestion(|| {
            format!(
                "make sure you have the correct permissions to access the data dir: {wallet_dir:?}"
            )
        })?;

    Ok(wallet_dir)
}

/// Get existing wallet directories under the client data dir
fn get_existing_user_dirs() -> Result<Vec<String>> {
    let base_dir = get_client_data_dir_base()?;

    if !base_dir.exists() {
        return Ok(Vec::new());
    }

    let mut wallet_dirs = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&base_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                // Check if it looks like a wallet address (starts with 0x and has the right length)
                if dir_name.starts_with("0x") && dir_name.len() == 42 {
                    wallet_dirs.push(dir_name);
                }
            }
        }
    }

    Ok(wallet_dirs)
}

fn get_wallet_pk() -> Result<String> {
    let secret_key = crate::wallet::load_wallet_private_key()?;
    let wallet = Wallet::new_from_private_key(crate::wallet::DUMMY_NETWORK, &secret_key)
        .map_err(|_| eyre!("Invalid SECRET_KEY provided"))?;
    Ok(wallet.address().to_string())
}
