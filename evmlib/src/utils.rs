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
};
use alloy::providers::{Identity, ProviderBuilder, ReqwestProvider};
use alloy::transports::http::{reqwest, Client, Http};
use dirs_next::data_dir;
use rand::Rng;
use std::env;
use std::path::PathBuf;

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

pub fn get_evm_testnet_csv_path() -> Result<PathBuf, Error> {
    let file = data_dir()
        .ok_or(Error::FailedToGetEvmNetwork(
            "failed to get data dir when fetching evm testnet CSV file".to_string(),
        ))?
        .join("autonomi")
        .join(EVM_TESTNET_CSV_FILENAME);
    Ok(file)
}

/// Create a custom `Network` from the given values
pub fn get_evm_network(
    rpc_url: &str,
    payment_token_address: &str,
    data_payments_address: &str,
) -> Network {
    Network::Custom(CustomNetwork::new(
        rpc_url,
        payment_token_address,
        data_payments_address,
    ))
}

/// Get the `Network` from environment variables.
///
/// Returns an error if we cannot obtain the network from any means.
pub fn get_evm_network_from_env() -> Result<Network, Error> {
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

    let use_arbitrum_sepolia = std::env::var("EVM_NETWORK")
        .map(|v| v == "arbitrum-sepolia")
        .unwrap_or(false);

    if use_arbitrum_one {
        info!("Using Arbitrum One EVM network as EVM_NETWORK is set to 'arbitrum-one'");
        Ok(Network::ArbitrumOne)
    } else if use_arbitrum_sepolia {
        info!("Using Arbitrum Sepolia EVM network as EVM_NETWORK is set to 'arbitrum-sepolia'");
        Ok(Network::ArbitrumSepolia)
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
        error!("Failed to obtain EVM Network through any means");
        Err(Error::FailedToGetEvmNetwork(
            "Failed to obtain EVM Network through any means".to_string(),
        ))
    }
}

/// Get the `Network::Custom` from the local EVM testnet CSV file
pub fn local_evm_network_from_csv() -> Result<Network, Error> {
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
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    ReqwestProvider,
    Http<Client>,
    Ethereum,
> {
    ProviderBuilder::new()
        .with_recommended_fillers()
        .on_http(rpc_url)
}
