use crate::common::{Address, QuoteHash, TxHash, U256};
use crate::transaction::verify_data_payment;
use alloy::primitives::address;
use alloy::transports::http::reqwest;
use std::str::FromStr;
use std::sync::LazyLock;

pub mod common;
pub mod contract;
pub mod cryptography;
pub(crate) mod event;
pub mod testnet;
pub mod transaction;
pub mod utils;
pub mod wallet;

static PUBLIC_ARBITRUM_ONE_HTTP_RPC_URL: LazyLock<reqwest::Url> = LazyLock::new(|| {
    "https://arb1.arbitrum.io/rpc"
        .parse()
        .expect("Invalid RPC URL")
});

const ARBITRUM_ONE_PAYMENT_TOKEN_ADDRESS: Address =
    address!("4bc1aCE0E66170375462cB4E6Af42Ad4D5EC689C");

// Should be updated when the smart contract changes!
const ARBITRUM_ONE_DATA_PAYMENTS_ADDRESS: Address =
    address!("708353783756C62818aCdbce914d90E0245F7319");

#[derive(Clone, Debug, PartialEq)]
pub struct CustomNetwork {
    pub rpc_url_http: reqwest::Url,
    pub payment_token_address: Address,
    pub data_payments_address: Address,
}

impl CustomNetwork {
    pub fn new(rpc_url: &str, payment_token_addr: &str, data_payments_addr: &str) -> Self {
        Self {
            rpc_url_http: reqwest::Url::parse(rpc_url).expect("Invalid RPC URL"),
            payment_token_address: Address::from_str(payment_token_addr)
                .expect("Invalid payment token address"),
            data_payments_address: Address::from_str(data_payments_addr)
                .expect("Invalid data payments address"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Network {
    ArbitrumOne,
    Custom(CustomNetwork),
}

impl Network {
    pub fn identifier(&self) -> &str {
        match self {
            Network::ArbitrumOne => "arbitrum-one",
            Network::Custom(_) => "custom",
        }
    }

    pub fn rpc_url(&self) -> &reqwest::Url {
        match self {
            Network::ArbitrumOne => &PUBLIC_ARBITRUM_ONE_HTTP_RPC_URL,
            Network::Custom(custom) => &custom.rpc_url_http,
        }
    }

    pub fn payment_token_address(&self) -> &Address {
        match self {
            Network::ArbitrumOne => &ARBITRUM_ONE_PAYMENT_TOKEN_ADDRESS,
            Network::Custom(custom) => &custom.payment_token_address,
        }
    }

    pub fn data_payments_address(&self) -> &Address {
        match self {
            Network::ArbitrumOne => &ARBITRUM_ONE_DATA_PAYMENTS_ADDRESS,
            Network::Custom(custom) => &custom.data_payments_address,
        }
    }

    pub async fn verify_data_payment(
        &self,
        tx_hash: TxHash,
        quote_hash: QuoteHash,
        reward_addr: Address,
        amount: U256,
        quote_expiration_timestamp_in_secs: u64,
    ) -> Result<(), transaction::Error> {
        verify_data_payment(
            self,
            tx_hash,
            quote_hash,
            reward_addr,
            amount,
            quote_expiration_timestamp_in_secs,
        )
        .await
    }
}

impl Default for Network {
    fn default() -> Self {
        Self::ArbitrumOne
    }
}
