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
use std::env;

const MAINNET_ID: u8 = 1;
const ALPHANET_ID: u8 = 2;

/// environment variable to connect to a custom EVM network
pub const RPC_URL: &str = "RPC_URL";
const RPC_URL_BUILD_TIME_VAL: Option<&str> = option_env!("RPC_URL");
pub const PAYMENT_TOKEN_ADDRESS: &str = "PAYMENT_TOKEN_ADDRESS";
const PAYMENT_TOKEN_ADDRESS_BUILD_TIME_VAL: Option<&str> = option_env!("PAYMENT_TOKEN_ADDRESS");
pub const DATA_PAYMENTS_ADDRESS: &str = "DATA_PAYMENTS_ADDRESS";
const DATA_PAYMENTS_ADDRESS_BUILD_TIME_VAL: Option<&str> = option_env!("DATA_PAYMENTS_ADDRESS");
pub const MERKLE_PAYMENTS_ADDRESS: &str = "MERKLE_PAYMENTS_ADDRESS";
const MERKLE_PAYMENTS_ADDRESS_BUILD_TIME_VAL: Option<&str> = option_env!("MERKLE_PAYMENTS_ADDRESS");

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to get EVM network: {0}")]
    FailedToGetEvmNetwork(String),
}

/// Generate a random Address.
pub fn dummy_address() -> Address {
    use rand::Rng;
    Address::new(rand::rngs::OsRng.r#gen())
}

/// Generate a random Hash.
pub fn dummy_hash() -> Hash {
    use rand::Rng;
    Hash::new(rand::rngs::OsRng.r#gen())
}

use std::sync::OnceLock;

static EVM_NETWORK: OnceLock<Network> = OnceLock::new();

/// Initialize the EVM Network.
///
/// Try to obtain it first from environment variables. If that fails and `local` is true,
/// try to get it from hardcoded values. Lastly, attempt to obtain it based on the network ID,
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
        Err(_) if local => Ok(local_evm_network_hardcoded()),
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
                        error!(
                            "Network ID {} requires EVM network configuration via environment variables",
                            id
                        );
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
        info!(
            "Using Arbitrum Sepolia Test EVM network as EVM_NETWORK is set to 'arbitrum-sepolia-test'"
        );
        Ok(Network::ArbitrumSepoliaTest)
    } else if let Ok(evm_vars) = evm_vars {
        info!("Using custom EVM network from environment variables");
        let merkle_addr = env::var(MERKLE_PAYMENTS_ADDRESS)
            .ok()
            .or_else(|| MERKLE_PAYMENTS_ADDRESS_BUILD_TIME_VAL.map(|s| s.to_string()));

        let network = CustomNetwork::new(
            &evm_vars[0],
            &evm_vars[1],
            &evm_vars[2],
            merkle_addr.as_deref(),
        );
        Ok(Network::Custom(network))
    } else if use_local_evm {
        Ok(local_evm_network_hardcoded())
    } else {
        error!("Failed to obtain the desired EVM network through environment variables");
        Err(Error::FailedToGetEvmNetwork(
            "Failed to obtain the desired EVM network through environment variables".to_string(),
        ))
    }
}

/// Get the `Network::Custom` from the hardcoded values.
fn local_evm_network_hardcoded() -> Network {
    // Merkle payments address is deterministic when deployed by Anvil's third default account (Charlie)
    // Deployed at nonce 0 by account 0x70997970C51812dc3A010C7d01b50e0d17dc79C8
    let network = CustomNetwork::new(
        "http://localhost:61611",
        "0x5FbDB2315678afecb367f032d93F642f64180aa3",
        "0x8464135c8F25Da09e49BC8782676a84730C318bC",
        Some("0x663F3ad617193148711d28f5334eE4Ed07016602"),
    );
    Network::Custom(network)
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
