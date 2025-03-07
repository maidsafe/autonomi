use std::{str::FromStr, sync::Arc};

use autonomi::{
    client::payment::PaymentOption, Bytes, Chunk, ChunkAddress, Client, Network, Wallet,
};

use libp2p::Multiaddr;
use napi::bindgen_prelude::*;
use napi_derive::napi;

type Result<T> = std::result::Result<T, napi::Error>;

// Re-export error type for convenience
#[napi]
pub enum ErrorCode {
    NetworkError,
    InvalidArgument,
    AuthenticationFailed,
    NotFound,
    AlreadyExists,
    InternalError,
}

// Convert Rust errors to JavaScript errors
fn map_error<E: std::fmt::Display>(err: E) -> napi::Error {
    napi::Error::new(Status::GenericFailure, format!("{}", err))
}

/// Client for the Autonomi network
#[napi]
pub struct JsClient {
    inner: Arc<tokio::sync::Mutex<Client>>,
}

#[napi]
impl JsClient {
    /// Initialize the client with default configuration
    #[napi]
    pub async fn init() -> Result<JsClient> {
        let client = Client::init().await.map_err(map_error)?;

        Ok(JsClient {
            inner: Arc::new(tokio::sync::Mutex::new(client)),
        })
    }

    /// Initialize a client that is configured to be local
    #[napi]
    pub async fn init_local() -> Result<JsClient> {
        let client = Client::init_local().await.map_err(map_error)?;

        Ok(JsClient {
            inner: Arc::new(tokio::sync::Mutex::new(client)),
        })
    }

    /// Initialize a client that bootstraps from a list of peers
    #[napi]
    pub async fn init_with_peers(peers: Vec<String>) -> Result<JsClient> {
        let peers: std::result::Result<Vec<Multiaddr>, _> =
            peers.iter().map(|p| Multiaddr::from_str(p)).collect();

        let peers = peers.map_err(map_error)?;

        let client = Client::init_with_peers(peers).await.map_err(map_error)?;

        Ok(JsClient {
            inner: Arc::new(tokio::sync::Mutex::new(client)),
        })
    }

    /// Get a chunk from the network
    #[napi]
    pub async fn chunk_get(&self, addr: String) -> Result<Buffer> {
        let addr = ChunkAddress::try_from_hex(&addr).map_err(map_error)?;

        let client = self.inner.clone();
        let client = client.lock().await;

        let chunk = client.chunk_get(&addr).await.map_err(map_error)?;

        Ok(Buffer::from(chunk.value.to_vec()))
    }

    /// Manually upload a chunk to the network
    #[napi]
    pub async fn chunk_put(
        &self,
        data: Buffer,
        payment_option: &JsPaymentOption,
    ) -> Result<String> {
        let client_inner = self.inner.clone();
        let client_guard = client_inner.lock().await;

        let chunk = Chunk::new(Bytes::from(data.as_ref().to_vec()));

        let (_cost, addr) = client_guard
            .chunk_put(&chunk, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        // Ok((cost.to_string(), addr.to_hex()))
        Ok(addr.to_hex())
    }
}

/// A wallet for interacting with the network's payment system
#[napi]
pub struct JsWallet(Wallet);

#[napi]
impl JsWallet {
    /// Creates a new wallet from a private key string
    #[napi(constructor)]
    pub fn new(private_key: String) -> Result<Self> {
        let wallet = Wallet::new_from_private_key(
            Network::ArbitrumOne, // Default network
            &private_key,
        )
        .map_err(map_error)?;

        Ok(Self(wallet))
    }

    /// Returns a string representation of the wallet's address
    #[napi]
    pub fn address(&self) -> String {
        self.0.address().to_string()
    }

    /// Returns the raw balance of payment tokens in the wallet
    #[napi]
    pub async fn balance(&self) -> Result<String> {
        let balance = self.0.balance_of_tokens().await.map_err(map_error)?;

        Ok(balance.to_string())
    }

    /// Returns the current balance of gas tokens in the wallet
    #[napi]
    pub async fn balance_of_gas(&self) -> Result<String> {
        let balance = self.0.balance_of_gas_tokens().await.map_err(map_error)?;

        Ok(balance.to_string())
    }
}

/// Options for making payments on the network
#[napi]
pub struct JsPaymentOption(PaymentOption);

#[napi]
impl JsPaymentOption {
    #[napi(constructor)]
    pub fn new(wallet: &JsWallet) -> Self {
        Self(wallet.0.clone().into())
    }
}
