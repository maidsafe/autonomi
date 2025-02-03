// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::wallet::load_wallet_private_key;
use autonomi::client::register::SecretKey as RegisterSecretKey;
use autonomi::client::vault::VaultSecretKey;
use autonomi::{Network, Wallet};
use color_eyre::eyre::{eyre, Context, Result};
use color_eyre::Section;
use std::env;
use std::fs;
use std::path::PathBuf;

const SECRET_KEY_ENV: &str = "SECRET_KEY";
const REGISTER_SIGNING_KEY_ENV: &str = "REGISTER_SIGNING_KEY";

const REGISTER_SIGNING_KEY_FILE: &str = "register_signing_key";

/// EVM wallet
pub fn load_evm_wallet_from_env(evm_network: &Network) -> Result<Wallet> {
    let secret_key =
        get_secret_key_from_env().wrap_err("The secret key is required to perform this action")?;
    let wallet = Wallet::new_from_private_key(evm_network.clone(), &secret_key)
        .wrap_err("Failed to load EVM wallet from key")?;
    Ok(wallet)
}

/// EVM wallet private key
pub fn get_secret_key_from_env() -> Result<String> {
    env::var(SECRET_KEY_ENV).wrap_err(eyre!(
        "make sure you've provided the {SECRET_KEY_ENV} env var"
    ))
}

pub fn get_vault_secret_key() -> Result<VaultSecretKey> {
    let secret_key = load_wallet_private_key()?;
    autonomi::client::vault::derive_vault_key(&secret_key)
        .wrap_err("Failed to derive vault secret key from EVM secret key")
}

pub fn create_register_signing_key_file(key: RegisterSecretKey) -> Result<PathBuf> {
    let dir = super::data_dir::get_client_data_dir_path()
        .wrap_err("Could not access directory to write key to")?;
    let file_path = dir.join(REGISTER_SIGNING_KEY_FILE);
    fs::write(&file_path, key.to_hex()).wrap_err("Could not write key to file")?;
    Ok(file_path)
}

fn parse_register_signing_key(key_hex: &str) -> Result<RegisterSecretKey> {
    RegisterSecretKey::from_hex(key_hex)
        .wrap_err("Failed to parse register signing key")
        .with_suggestion(|| {
            "the register signing key should be a hex encoded string of a bls secret key"
        })
        .with_suggestion(|| {
            "you can generate a new secret key with the `register generate-key` subcommand"
        })
}

pub fn get_register_signing_key() -> Result<RegisterSecretKey> {
    // try env var first
    let why_env_failed = match env::var(REGISTER_SIGNING_KEY_ENV) {
        Ok(key) => return parse_register_signing_key(&key),
        Err(e) => e,
    };

    // try from data dir
    let dir = super::data_dir::get_client_data_dir_path()
        .wrap_err(format!("Failed to obtain register signing key from env var: {why_env_failed}, reading from disk also failed as couldn't access data dir"))
        .with_suggestion(|| format!("make sure you've provided the {REGISTER_SIGNING_KEY_ENV} env var"))
        .with_suggestion(|| "you can generate a new secret key with the `register generate-key` subcommand")?;

    // load the key from file
    let key_path = dir.join(REGISTER_SIGNING_KEY_FILE);
    let key_hex = fs::read_to_string(&key_path)
        .wrap_err("Failed to read secret key from file")
        .with_suggestion(|| format!("make sure you've provided the {REGISTER_SIGNING_KEY_ENV} env var or have the key in a file at {key_path:?}"))
        .with_suggestion(|| "you can generate a new secret key with the `register generate-key` subcommand")?;

    // parse the key
    parse_register_signing_key(&key_hex)
}

pub fn get_register_signing_key_path() -> Result<PathBuf> {
    let dir = super::data_dir::get_client_data_dir_path()
        .wrap_err("Could not access directory for register signing key")?;
    let file_path = dir.join(REGISTER_SIGNING_KEY_FILE);
    Ok(file_path)
}
