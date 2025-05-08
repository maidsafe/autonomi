// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(dead_code)]

use crate::common::{Address, Hash};
use crate::{CustomNetwork, Network};
use alloy::network::Ethereum;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
    SimpleNonceManager,
};
use alloy::providers::{Identity, ProviderBuilder, RootProvider};
use alloy::transports::http::reqwest;
use dirs_next::data_dir;
use rand::Rng;
use std::env;
use std::path::PathBuf;

const MAINNET_ID: u8 = 1;
const ALPHANET_ID: u8 = 2;

pub const EVM_TESTNET_CSV_FILENAME: &str = "evm_testnet_data.csv";

/// environment variable to connect to a custom EVM network
pub const RPC_URL: &str = "RPC_URL";
const RPC_URL_BUILD_TIME_VAL: Option<&str> = option_env!("RPC_URL");
pub const PAYMENT_TOKEN_ADDRESS: &str = "PAYMENT_TOKEN_ADDRESS";
const PAYMENT_TOKEN_ADDRESS_BUILD_TIME_VAL: Option<&str> = option_env!("PAYMENT_TOKEN_ADDRESS");
pub const DATA_PAYMENTS_ADDRESS: &str = "DATA_PAYMENTS_ADDRESS";
const DATA_PAYMENTS_ADDRESS_BUILD_TIME_VAL: Option<&str> = option_env!("DATA_PAYMENTS_ADDRESS");

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to get EVM network: {0}")]
    FailedToGetEvmNetwork(String),
}

/// Generate a random Address.
pub fn dummy_address() -> Address {
    Address::new(rand::rngs::OsRng.gen())
}

/// Generate a random Hash.
pub fn dummy_hash() -> Hash {
    Hash::new(rand::rngs::OsRng.gen())
}

use std::sync::OnceLock;

static EVM_NETWORK: OnceLock<Network> = OnceLock::new();

/// Initialize the EVM Network.
///
/// Try to obtain it first from environment variables. If that fails and `local` is true,
/// try to get it from the local CSV file. Lastly, attempt to obtain it based on the network ID,
/// where 1 is reserved for the mainnet, 2 is reserved for the alpha network, and any other value
/// between 3 and 255 is reserved for testnets. In the case of a testnet, the network to use must
/// be configured via the environment variables. We can't just default to Sepolia because sometimes
/// we want to use Anvil.
///
/// If all of these fail an error will be returned. It doesn't really make sense to have a default
/// for the EVM network. Doing so actually results in confusion for users where sometimes payments
/// can be rejected because they are on the wrong network.
pub fn get_evm_network(local: bool, network_id: Option<u8>) -> Result<Network, Error> {
    if let Some(network) = EVM_NETWORK.get() {
        return Ok(network.clone());
    }

    let res = match get_evm_network_from_env() {
        Ok(evm_network) => Ok(evm_network),
        Err(_) if local => Ok(local_evm_network_from_csv()
            .map_err(|e| Error::FailedToGetEvmNetwork(e.to_string()))?),
        Err(_) => {
            if let Some(id) = network_id {
                match id {
                    MAINNET_ID => {
                        info!("Using Arbitrum One based on network ID {}", id);
                        Ok(Network::ArbitrumOne)
                    }
                    ALPHANET_ID => {
                        info!("Using Arbitrum Sepolia Test based on network ID {}", id);
                        Ok(Network::ArbitrumSepoliaTest)
                    }
                    _ => {
                        error!("Network ID {} requires EVM network configuration via environment variables", id);
                        Err(Error::FailedToGetEvmNetwork(format!(
                            "Network ID {id} requires EVM network to be configured via environment variables" 
                        )))
                    }
                }
            } else {
                error!("Failed to obtain the desired EVM network via any means");
                Err(Error::FailedToGetEvmNetwork(
                    "Failed to obtain the desired EVM network via any means".to_string(),
                ))
            }
        }
    };

    if let Ok(network) = res.as_ref() {
        let _ = EVM_NETWORK.set(network.clone());
    }

    res
}

pub fn get_evm_testnet_csv_path() -> Result<PathBuf, Error> {
    let file = data_dir()
        .ok_or(Error::FailedToGetEvmNetwork(
            "failed to get data dir when fetching evm testnet CSV file".to_string(),
        ))?
        .join("autonomi")
        .join(EVM_TESTNET_CSV_FILENAME);
    Ok(file)
}

/// Get the `Network` from environment variables.
///
/// Returns an error if we cannot obtain the network from any means.
fn get_evm_network_from_env() -> Result<Network, Error> {
    let evm_vars = [
        env::var(RPC_URL)
            .ok()
            .or_else(|| RPC_URL_BUILD_TIME_VAL.map(|s| s.to_string())),
        env::var(PAYMENT_TOKEN_ADDRESS)
            .ok()
            .or_else(|| PAYMENT_TOKEN_ADDRESS_BUILD_TIME_VAL.map(|s| s.to_string())),
        env::var(DATA_PAYMENTS_ADDRESS)
            .ok()
            .or_else(|| DATA_PAYMENTS_ADDRESS_BUILD_TIME_VAL.map(|s| s.to_string())),
    ]
    .into_iter()
    .map(|var| {
        var.ok_or(Error::FailedToGetEvmNetwork(format!(
            "missing env var, make sure to set all of: {RPC_URL}, {PAYMENT_TOKEN_ADDRESS}, {DATA_PAYMENTS_ADDRESS}"
        )))
    })
    .collect::<Result<Vec<String>, Error>>();

    let use_local_evm = std::env::var("EVM_NETWORK")
        .map(|v| v == "local")
        .unwrap_or(false);
    if use_local_evm {
        info!("Using local EVM network as EVM_NETWORK is set to 'local'");
    }

    let use_arbitrum_one = std::env::var("EVM_NETWORK")
        .map(|v| v == "arbitrum-one")
        .unwrap_or(false);

    let use_arbitrum_sepolia_test = std::env::var("EVM_NETWORK")
        .map(|v| v == "arbitrum-sepolia-test")
        .unwrap_or(false);

    if use_arbitrum_one {
        info!("Using Arbitrum One EVM network as EVM_NETWORK is set to 'arbitrum-one'");
        Ok(Network::ArbitrumOne)
    } else if use_arbitrum_sepolia_test {
        info!("Using Arbitrum Sepolia Test EVM network as EVM_NETWORK is set to 'arbitrum-sepolia-test'");
        Ok(Network::ArbitrumSepoliaTest)
    } else if let Ok(evm_vars) = evm_vars {
        info!("Using custom EVM network from environment variables");
        Ok(Network::Custom(CustomNetwork::new(
            &evm_vars[0],
            &evm_vars[1],
            &evm_vars[2],
        )))
    } else if use_local_evm {
        local_evm_network_from_csv()
    } else {
        error!("Failed to obtain the desired EVM Network through environment variables");
        Err(Error::FailedToGetEvmNetwork(
            "Failed to obtain the desired EVM Network through environment variables".to_string(),
        ))
    }
}

/// Get the `Network::Custom` from the local EVM testnet CSV file
fn local_evm_network_from_csv() -> Result<Network, Error> {
    // load the csv
    let csv_path = get_evm_testnet_csv_path()?;

    if !csv_path.exists() {
        error!("evm data csv path does not exist {:?}", csv_path);
        return Err(Error::FailedToGetEvmNetwork(format!(
            "evm data csv path does not exist {csv_path:?}"
        )));
    }

    let csv = std::fs::read_to_string(&csv_path).map_err(|_| {
        Error::FailedToGetEvmNetwork(format!("failed to read evm testnet CSV file {csv_path:?}"))
    })?;
    let parts: Vec<&str> = csv.split(',').collect();
    match parts.as_slice() {
        [rpc_url, payment_token_address, chunk_payments_address, _] => Ok(Network::Custom(
            CustomNetwork::new(rpc_url, payment_token_address, chunk_payments_address),
        )),
        _ => {
            error!("Invalid data in evm testnet CSV file");
            Err(Error::FailedToGetEvmNetwork(
                "invalid data in evm testnet CSV file".to_string(),
            ))
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn http_provider(
    rpc_url: reqwest::Url,
) -> FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        NonceFiller<SimpleNonceManager>,
    >,
    RootProvider,
    Ethereum,
> {
    ProviderBuilder::new()
        .with_simple_nonce_management()
        .connect_http(rpc_url)
}
