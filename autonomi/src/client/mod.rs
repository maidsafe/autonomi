// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Optionally enable nightly `doc_cfg`. Allows items to be annotated, e.g.: "Available on crate feature X only".
#![cfg_attr(docsrs, feature(doc_cfg))]

/// The 4 basic Network data types.
/// - Chunk
/// - GraphEntry
/// - Pointer
/// - Scratchpad
pub mod data_types;
pub use data_types::chunk;
pub use data_types::graph;
pub use data_types::pointer;
pub use data_types::scratchpad;

/// High-level types built on top of the basic Network data types.
/// Includes data, files and personnal data vaults
mod high_level;
pub use high_level::data;
pub use high_level::files;
pub use high_level::register;
pub use high_level::vault;

pub mod analyze;
pub mod config;
pub mod key_derivation;
pub mod payment;
pub mod quote;

#[cfg(feature = "external-signer")]
#[cfg_attr(docsrs, doc(cfg(feature = "external-signer")))]
pub mod external_signer;

// private module with utility functions
mod chunk_cache;
mod data_map_restoration;
mod network;
mod put_error_state;

use payment::Receipt;
pub use put_error_state::ChunkBatchUploadState;
use quote::PaymentMode;

use ant_bootstrap::{InitialPeersConfig, contacts::ALPHANET_CONTACTS};
pub use ant_evm::Amount;
use ant_evm::EvmNetwork;
use config::ClientConfig;
use payment::PayError;
use quote::CostError;
use std::collections::HashSet;
use tokio::sync::mpsc;

/// Time before considering the connection timed out.
pub const CONNECT_TIMEOUT_SECS: u64 = 10;

const CLIENT_EVENT_CHANNEL_SIZE: usize = 100;

// Amount of peers to confirm into our routing table before we consider the client ready.
use crate::client::config::ClientOperatingStrategy;
use crate::networking::{Multiaddr, Network, NetworkAddress, NetworkError, multiaddr_is_global};
pub use ant_protocol::CLOSE_GROUP_SIZE;
use ant_protocol::storage::RecordKind;

/// Represents a client for the Autonomi network.
///
/// # Example
///
/// To start interacting with the network, use [`Client::init`].
///
/// ```no_run
/// # use autonomi::client::Client;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::init().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Client {
    /// The Autonomi Network to use for the client.
    pub(crate) network: Network,
    /// Events sent by the client, can be enabled by calling [`Client::enable_client_events`].
    pub(crate) client_event_sender: Option<mpsc::Sender<ClientEvent>>,
    /// The EVM network to use for the client.
    evm_network: EvmNetwork,
    /// The configuration for operations on the client.
    config: ClientOperatingStrategy,
    /// Max times of total chunks to carry out retry on upload failure.
    /// Default to be `0` to indicate not carry out retry.
    retry_failed: u64,
    /// Payment mode to use for uploads
    payment_mode: PaymentMode,
}

/// Error returned by [`Client::init`].
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// Did not manage to populate the routing table with enough peers.
    #[error("Failed to populate our routing table with enough peers in time")]
    TimedOut,

    /// Same as [`ConnectError::TimedOut`] but with a list of incompatible protocols.
    #[error("Failed to populate our routing table due to incompatible protocol: {0:?}")]
    TimedOutWithIncompatibleProtocol(HashSet<String>, String),

    /// An error occurred while bootstrapping the client.
    #[error("Failed to bootstrap the client: {0}")]
    Bootstrap(#[from] ant_bootstrap::Error),

    /// The routing table does not contain any known peers to bootstrap from.
    #[error("No known peers available in the routing table to bootstrap the client")]
    NoKnownPeers(#[from] libp2p::kad::NoKnownPeers),

    /// An error occurred while initializing the EVM network.
    #[error("Failed to initialize the EVM network: {0}")]
    EvmNetworkError(String),
}

/// Errors that can occur during the put operation.
#[derive(Debug, thiserror::Error)]
pub enum PutError {
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error("Error occurred during cost estimation: {0}")]
    CostError(#[from] CostError),
    #[error("Error occurred during payment: {0}")]
    PayError(#[from] PayError),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("A wallet error occurred: {0}")]
    Wallet(#[from] ant_evm::EvmError),
    #[error("The payment proof contains no payees.")]
    PayeesMissing,
    #[error("A network error occurred for {address}: {network_error}")]
    Network {
        address: Box<NetworkAddress>,
        network_error: NetworkError,
        /// if a payment was made, it will be returned here so it can be reused
        payment: Option<Receipt>,
    },
    #[error("Batch upload: {0}")]
    Batch(ChunkBatchUploadState),
}

/// Errors that can occur during the get operation.
#[derive(Debug, thiserror::Error)]
pub enum GetError {
    #[error("Could not deserialize data map.")]
    InvalidDataMap(rmp_serde::decode::Error),
    #[error("Failed to decrypt data.")]
    Decryption(crate::self_encryption::Error),
    #[error("Failed to deserialize")]
    Deserialization(#[from] rmp_serde::decode::Error),
    #[error("General networking error: {0}")]
    Network(#[from] NetworkError),
    #[error("General protocol error: {0}")]
    Protocol(#[from] ant_protocol::Error),
    #[error("Record could not be found.")]
    RecordNotFound,
    // The RecordKind that was obtained did not match with the expected one
    #[error("The RecordKind obtained from the Record did not match with the expected kind: {0}")]
    RecordKindMismatch(RecordKind),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Unable to recogonize the so claimed DataMap: {0}")]
    UnrecognizedDataMap(String),
    /// When trying to download a file that is too large to be handled in memory
    /// you can increase the [`crate::client::config::MAX_IN_MEMORY_DOWNLOAD_SIZE`] env var or use the streaming API.
    #[error(
        "DataMap points to a file too large to be handled in memory, you can increase the MAX_IN_MEMORY_DOWNLOAD_SIZE env var or use streaming to avoid this error."
    )]
    TooLargeForMemory,
}

impl Client {
    /// Initialize the client with default configuration.
    ///
    /// See [`Client::init_with_config`].
    pub async fn init() -> Result<Self, ConnectError> {
        let bootstrap_cache_config = crate::BootstrapCacheConfig::new(false)
            .inspect_err(|errr| {
                warn!("Failed to create bootstrap cache config: {errr}");
            })
            .ok();
        Self::init_with_config(ClientConfig {
            bootstrap_cache_config,
            ..Default::default()
        })
        .await
    }

    /// Initialize a client that is configured to be local.
    ///
    /// See [`Client::init_with_config`].
    pub async fn init_local() -> Result<Self, ConnectError> {
        let bootstrap_cache_config = crate::BootstrapCacheConfig::new(true)
            .inspect_err(|errr| {
                warn!("Failed to create bootstrap cache config: {errr}");
            })
            .ok();

        Self::init_with_config(ClientConfig {
            init_peers_config: InitialPeersConfig {
                local: true,
                ..Default::default()
            },
            evm_network: EvmNetwork::new(true)
                .map_err(|e| ConnectError::EvmNetworkError(e.to_string()))?,
            strategy: Default::default(),
            network_id: None,
            bootstrap_cache_config,
        })
        .await
    }

    /// Initialize a client that is configured to be connected to the the alpha network (Impossible Futures).
    pub async fn init_alpha() -> Result<Self, ConnectError> {
        let bootstrap_cache_config = crate::BootstrapCacheConfig::new(false)
            .inspect_err(|errr| {
                warn!("Failed to create bootstrap cache config: {errr}");
            })
            .ok();

        let client_config = ClientConfig {
            init_peers_config: InitialPeersConfig {
                first: false,
                addrs: vec![],
                network_contacts_url: ALPHANET_CONTACTS.iter().map(|s| s.to_string()).collect(),
                local: false,
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            evm_network: EvmNetwork::ArbitrumSepoliaTest,
            strategy: Default::default(),
            network_id: Some(2),
            bootstrap_cache_config,
        };
        Self::init_with_config(client_config).await
    }

    /// Initialize a client that bootstraps from a list of peers.
    ///
    /// If any of the provided peers is a global address, the client will not be local.
    ///
    /// ```no_run
    /// # use autonomi::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Will set `local` to true.
    /// let client = Client::init_with_peers(vec!["/ip4/127.0.0.1/udp/1234/quic-v1".parse()?]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn init_with_peers(peers: Vec<Multiaddr>) -> Result<Self, ConnectError> {
        // Any global address makes the client non-local
        let local = !peers.iter().any(multiaddr_is_global);

        let bootstrap_cache_config = crate::BootstrapCacheConfig::new(local)
            .inspect_err(|errr| {
                warn!("Failed to create bootstrap cache config: {errr}");
            })
            .ok();

        Self::init_with_config(ClientConfig {
            init_peers_config: InitialPeersConfig {
                local,
                addrs: peers,
                ..Default::default()
            },
            evm_network: EvmNetwork::new(local).unwrap_or_default(),
            strategy: Default::default(),
            network_id: None,
            bootstrap_cache_config,
        })
        .await
    }

    /// Initialize the client with the given configuration.
    ///
    /// This will block until [`CLOSE_GROUP_SIZE`] have been added to the routing table.
    ///
    /// See [`ClientConfig`].
    ///
    /// ```no_run
    /// use autonomi::client::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::init_with_config(Default::default()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn init_with_config(config: ClientConfig) -> Result<Self, ConnectError> {
        if let Some(network_id) = config.network_id {
            ant_protocol::version::set_network_id(network_id);
        }

        let initial_peers = match config.init_peers_config.get_bootstrap_addr(Some(50)).await {
            Ok(peers) => peers,
            Err(e) => return Err(e.into()),
        };

        let network = Network::new(initial_peers, config.bootstrap_cache_config.clone())?;

        // Wait for the network to be ready with enough peers
        match network.wait_for_connectivity().await {
            Ok(()) => {
                Ok(Self {
                    network,
                    client_event_sender: None,
                    evm_network: config.evm_network,
                    config: config.strategy,
                    retry_failed: 0,
                    payment_mode: PaymentMode::Standard,
                })
            }
            Err(e) => {
                // If connection failed and we have a bootstrap cache config, try to wipe the cache and retry once
                if let Some(ref bootstrap_config) = config.bootstrap_cache_config {
                    warn!("Initial connection failed: {e:?}. Attempting to delete bootstrap cache and retry...");

                    // Try to delete the cache file
                    if let Err(delete_err) = ant_bootstrap::BootstrapCacheStore::delete_cache_file(bootstrap_config) {
                        warn!("Failed to delete bootstrap cache: {delete_err}");
                    }

                    // Retry getting bootstrap addresses without cache
                    let mut retry_config = config.init_peers_config.clone();
                    retry_config.ignore_cache = true;

                    let retry_peers = match retry_config.get_bootstrap_addr(Some(50)).await {
                        Ok(peers) => peers,
                        Err(retry_err) => {
                            error!("Failed to get bootstrap addresses on retry: {retry_err}");
                            return Err(e);
                        }
                    };

                    info!("Retrying connection with {} fresh bootstrap peers", retry_peers.len());
                    let retry_network = Network::new(retry_peers, config.bootstrap_cache_config.clone())?;

                    // Try to connect again
                    retry_network.wait_for_connectivity().await?;

                    info!("Successfully connected after wiping bootstrap cache");
                    Ok(Self {
                        network: retry_network,
                        client_event_sender: None,
                        evm_network: config.evm_network,
                        config: config.strategy,
                        retry_failed: 0,
                        payment_mode: PaymentMode::Standard,
                    })
                } else {
                    // No bootstrap cache config, just return the error
                    Err(e)
                }
            }
        }
    }

    /// Set the `ClientOperatingStrategy` for the client.
    pub fn with_strategy(mut self, strategy: ClientOperatingStrategy) -> Self {
        self.config = strategy;
        self
    }

    /// Set whether to retry failed uploads automatically.
    pub fn with_retry_failed(mut self, retry_failed: u64) -> Self {
        self.retry_failed = retry_failed;
        self
    }

    /// Set the payment mode for uploads.
    pub fn with_payment_mode(mut self, payment_mode: PaymentMode) -> Self {
        self.payment_mode = payment_mode;
        self
    }

    /// Receive events from the client.
    pub fn enable_client_events(&mut self) -> mpsc::Receiver<ClientEvent> {
        let (client_event_sender, client_event_receiver) =
            tokio::sync::mpsc::channel(CLIENT_EVENT_CHANNEL_SIZE);
        self.client_event_sender = Some(client_event_sender);
        debug!("All events to the clients are enabled");

        client_event_receiver
    }

    /// Get the evm network.
    pub fn evm_network(&self) -> &EvmNetwork {
        &self.evm_network
    }
}

/// Events that can be sent by the client.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    UploadComplete(UploadSummary),
}

/// Summary of an upload operation.
#[derive(Debug, Clone)]
pub struct UploadSummary {
    /// Records that were uploaded to the network
    pub records_paid: usize,
    /// Records that were already paid for so were not re-uploaded
    pub records_already_paid: usize,
    /// Total cost of the upload
    pub tokens_spent: Amount,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_logging::LogBuilder;

    #[tokio::test]
    async fn test_init_fails() {
        let _guard = LogBuilder::init_single_threaded_tokio_test();

        let initial_peers = vec![
            "/ip4/127.0.0.1/udp/1/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                .parse()
                .unwrap(),
        ];
        let network = Network::new(initial_peers, None).unwrap();

        match network.wait_for_connectivity().await {
            Err(ConnectError::TimedOut) => {} // This is the expected outcome
            Ok(()) => panic!("Expected `ConnectError::TimedOut`, but got `Ok`"),
            Err(err) => {
                panic!("Expected `ConnectError::TimedOut`, but got `{err:?}`")
            }
        }
    }
}
