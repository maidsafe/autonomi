// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::Address;
use alloy::primitives::address;
use alloy::transports::http::reqwest;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::str::FromStr;
use std::sync::LazyLock;

#[macro_use]
extern crate tracing;

pub mod common;
pub mod contract;
pub mod cryptography;
#[cfg(feature = "external-signer")]
pub mod external_signer;
pub mod quoting_metrics;
pub mod testnet;
pub mod utils;
pub mod wallet;

static PUBLIC_ARBITRUM_ONE_HTTP_RPC_URL: LazyLock<reqwest::Url> = LazyLock::new(|| {
    "https://arb1.arbitrum.io/rpc"
        .parse()
        .expect("Invalid RPC URL")
});

static PUBLIC_ARBITRUM_SEPOLIA_HTTP_RPC_URL: LazyLock<reqwest::Url> = LazyLock::new(|| {
    "https://sepolia-rollup.arbitrum.io/rpc"
        .parse()
        .expect("Invalid RPC URL")
});

const ARBITRUM_ONE_PAYMENT_TOKEN_ADDRESS: Address =
    address!("4bc1aCE0E66170375462cB4E6Af42Ad4D5EC689C");

const ARBITRUM_SEPOLIA_PAYMENT_TOKEN_ADDRESS: Address =
    address!("BE1802c27C324a28aeBcd7eeC7D734246C807194");

// Should be updated when the smart contract changes!
const ARBITRUM_ONE_DATA_PAYMENTS_ADDRESS: Address =
    address!("607483B50C5F06c25cDC316b6d1E071084EeC9f5");

const ARBITRUM_SEPOLIA_DATA_PAYMENTS_ADDRESS: Address =
    address!("993C7739f50899A997fEF20860554b8a28113634");

/// Timeout for transactions
const TX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[serde_as]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomNetwork {
    #[serde_as(as = "DisplayFromStr")]
    pub rpc_url_http: reqwest::Url,
    pub payment_token_address: Address,
    pub data_payments_address: Address,
}

impl CustomNetwork {
    fn new(rpc_url: &str, payment_token_addr: &str, data_payments_addr: &str) -> Self {
        Self {
            rpc_url_http: reqwest::Url::parse(rpc_url).expect("Invalid RPC URL"),
            payment_token_address: Address::from_str(payment_token_addr)
                .expect("Invalid payment token address"),
            data_payments_address: Address::from_str(data_payments_addr)
                .expect("Invalid chunk payments address"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Network {
    #[default]
    ArbitrumOne,
    ArbitrumSepolia,
    Custom(CustomNetwork),
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::ArbitrumOne => write!(f, "evm-arbitrum-one"),
            Network::ArbitrumSepolia => write!(f, "evm-arbitrum-sepolia"),
            Network::Custom(_) => write!(f, "evm-custom"),
        }
    }
}

impl Network {
    pub fn new_custom(rpc_url: &str, payment_token_addr: &str, chunk_payments_addr: &str) -> Self {
        Self::Custom(CustomNetwork::new(
            rpc_url,
            payment_token_addr,
            chunk_payments_addr,
        ))
    }

    pub fn identifier(&self) -> &str {
        match self {
            Network::ArbitrumOne => "arbitrum-one",
            Network::ArbitrumSepolia => "arbitrum-sepolia",
            Network::Custom(_) => "custom",
        }
    }

    pub fn rpc_url(&self) -> &reqwest::Url {
        match self {
            Network::ArbitrumOne => &PUBLIC_ARBITRUM_ONE_HTTP_RPC_URL,
            Network::ArbitrumSepolia => &PUBLIC_ARBITRUM_SEPOLIA_HTTP_RPC_URL,
            Network::Custom(custom) => &custom.rpc_url_http,
        }
    }

    pub fn payment_token_address(&self) -> &Address {
        match self {
            Network::ArbitrumOne => &ARBITRUM_ONE_PAYMENT_TOKEN_ADDRESS,
            Network::ArbitrumSepolia => &ARBITRUM_SEPOLIA_PAYMENT_TOKEN_ADDRESS,
            Network::Custom(custom) => &custom.payment_token_address,
        }
    }

    pub fn data_payments_address(&self) -> &Address {
        match self {
            Network::ArbitrumOne => &ARBITRUM_ONE_DATA_PAYMENTS_ADDRESS,
            Network::ArbitrumSepolia => &ARBITRUM_SEPOLIA_DATA_PAYMENTS_ADDRESS,
            Network::Custom(custom) => &custom.data_payments_address,
        }
    }
}
