// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Core Autonomi client with simplified API

pub mod config;
pub mod data_content;
pub mod data_types;
pub mod download;
pub mod payment;
pub mod put_error_state;
pub mod quote;
mod record_get;
pub mod upload;
pub mod utils;

use crate::networking::multiaddr_is_global;
use config::{ClientConfig, ClientOperatingStrategy};
use payment::PayError;
use quote::CostError;
use utils::determine_data_type_from_address;

pub use crate::networking::{Network, NetworkError};
pub use data_content::DataContent;
pub use put_error_state::ChunkBatchUploadState;

use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmNetwork;
use ant_protocol::NetworkAddress;
use ant_protocol::storage::RecordKind;
use libp2p::Multiaddr;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

/// Time before considering the connection timed out.
pub const CONNECT_TIMEOUT_SECS: u64 = 10;

const CLIENT_EVENT_CHANNEL_SIZE: usize = 100;

/// Configuration for client initialization
#[derive(Debug, Clone)]
pub enum ClientInitSetup {
    /// Use default configuration
    Default,
    /// Connect to alpha network
    Alpha,
    /// Bootstrap from specific peers
    Peers(Vec<Multiaddr>),
    /// Use custom configuration
    Config(ClientConfig),
}

/// Core client errors
#[allow(clippy::enum_variant_names)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Connection error: {0}")]
    ConnectError(#[from] ConnectError),
    #[error("Put error: {0}")]
    PutError(#[from] PutError),
    #[error("Get error: {0}")]
    GetError(#[from] GetError),
    #[error("Network error: {0}")]
    NetworkError(#[from] NetworkError),
    #[error("Cost error: {0}")]
    CostError(#[from] CostError),
    #[error("Payment error: {0}")]
    PayError(#[from] PayError),
}

impl Error {
    /// Try to downcast this error to a ConnectError
    pub fn as_connect_error(&self) -> Option<&ConnectError> {
        match self {
            Error::ConnectError(e) => Some(e),
            _ => None,
        }
    }

    /// Try to downcast this error to a PutError
    pub fn as_put_error(&self) -> Option<&PutError> {
        match self {
            Error::PutError(e) => Some(e),
            _ => None,
        }
    }

    /// Try to downcast this error to a GetError
    pub fn as_get_error(&self) -> Option<&GetError> {
        match self {
            Error::GetError(e) => Some(e),
            _ => None,
        }
    }

    /// Try to downcast this error to a NetworkError
    pub fn as_network_error(&self) -> Option<&NetworkError> {
        match self {
            Error::NetworkError(e) => Some(e),
            _ => None,
        }
    }

    /// Try to downcast this error to a CostError
    pub fn as_cost_error(&self) -> Option<&CostError> {
        match self {
            Error::CostError(e) => Some(e),
            _ => None,
        }
    }

    /// Try to downcast this error to a PayError
    pub fn as_pay_error(&self) -> Option<&PayError> {
        match self {
            Error::PayError(e) => Some(e),
            _ => None,
        }
    }
}

/// Error returned by [`Client::init`].
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// Did not manage to populate the routing table with enough peers.
    #[error("Failed to populate our routing table with enough peers in time")]
    TimedOut,

    /// Same as [`ConnectError::TimedOut`] but with a list of incompatible protocols.
    #[error("Failed to populate our routing table due to incompatible protocol: {0:?}")]
    TimedOutWithIncompatibleProtocol(std::collections::HashSet<String>, String),

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

impl ConnectError {
    /// Try to create a ConnectError from a general Error
    pub fn from_error(e: &Error) -> Self {
        match e {
            Error::ConnectError(connect_error) => Self::from_connect_error(connect_error),
            err => ConnectError::EvmNetworkError(format!("{err:?}")),
        }
    }

    fn from_connect_error(connect_error: &ConnectError) -> Self {
        match connect_error {
            ConnectError::TimedOut => ConnectError::TimedOut,
            ConnectError::TimedOutWithIncompatibleProtocol(protocols, message) => {
                ConnectError::TimedOutWithIncompatibleProtocol(protocols.clone(), message.clone())
            }
            ConnectError::Bootstrap(_) => {
                ConnectError::EvmNetworkError("Bootstrap error".to_string())
            } // Use safe default since ant_bootstrap::Error doesn't implement Clone
            ConnectError::NoKnownPeers(_) => {
                ConnectError::EvmNetworkError("No known peers error".to_string())
            } // Use safe default since libp2p::kad::NoKnownPeers doesn't implement Clone
            ConnectError::EvmNetworkError(msg) => ConnectError::EvmNetworkError(msg.clone()),
        }
    }
}

/// Errors that can occur during the put operation.
#[derive(Debug, thiserror::Error)]
pub enum PutError {
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] self_encryption::Error),
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
    },
    #[error("Batch upload: {0}")]
    Batch(ChunkBatchUploadState),
}

impl PutError {
    /// Try to create a PutError from a general Error
    pub fn from_error(e: &Error) -> Self {
        match e {
            Error::PutError(put_error) => Self::from_put_error(put_error),
            Error::CostError(_) => PutError::Serialization("Cost error".to_string()), // Use safe default since CostError doesn't implement Clone
            Error::PayError(_) => PutError::Serialization("Payment error".to_string()), // Use safe default since PayError doesn't implement Clone
            Error::NetworkError(network_error) => PutError::Network {
                address: Box::new(NetworkAddress::from(
                    ant_protocol::storage::ChunkAddress::new(xor_name::XorName::default()),
                )), // Use default address since we don't have the original
                network_error: network_error.clone(),
            },
            err => PutError::Serialization(format!("{err:?}")),
        }
    }

    fn from_put_error(put_error: &PutError) -> Self {
        match put_error {
            PutError::SelfEncryption(_e) => {
                PutError::Serialization("Self-encryption error {_e:?}".to_string())
            } // Use safe default since self_encryption::Error doesn't implement Clone
            PutError::CostError(_e) => PutError::Serialization("Cost error {_e:?}".to_string()), // Use safe default since CostError doesn't implement Clone
            PutError::PayError(_e) => PutError::Serialization("Payment error {_e:?}".to_string()), // Use safe default since PayError doesn't implement Clone
            PutError::Serialization(msg) => PutError::Serialization(msg.clone()),
            PutError::Wallet(_e) => PutError::Serialization("Wallet error {_e:?}".to_string()), // Use safe default since ant_evm::EvmError doesn't implement Clone
            PutError::PayeesMissing => PutError::PayeesMissing,
            PutError::Network {
                address,
                network_error,
            } => PutError::Network {
                address: address.clone(),
                network_error: network_error.clone(),
            },
            PutError::Batch(state) => PutError::Batch(state.clone()),
        }
    }
}

/// Errors that can occur during the get operation.
#[derive(Debug, thiserror::Error)]
pub enum GetError {
    #[error("Could not deserialize data map.")]
    InvalidDataMap(rmp_serde::decode::Error),
    #[error("Failed to decrypt data.")]
    Decryption(self_encryption::Error),
    #[error("Failed to deserialize")]
    Deserialization(#[from] rmp_serde::decode::Error),
    #[error("General networking error: {0}")]
    Network(#[from] NetworkError),
    #[error("General protocol error: {0}")]
    Protocol(#[from] ant_protocol::Error),
    #[error("Record could not be found.")]
    RecordNotFound,
    #[error("The RecordKind obtained from the Record did not match with the expected kind: {0}")]
    RecordKindMismatch(RecordKind),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Unable to recognize the so claimed DataMap: {0}")]
    UnrecognizedDataMap(String),
}

impl GetError {
    /// Try to create a GetError from a general Error
    pub fn from_error(e: &Error) -> Self {
        match e {
            Error::GetError(get_error) => Self::from_get_error(get_error),
            Error::NetworkError(network_error) => GetError::Network(network_error.clone()),
            err => GetError::Configuration(format!("{err:?}")),
        }
    }

    fn from_get_error(get_error: &GetError) -> Self {
        match get_error {
            GetError::InvalidDataMap(_e) => {
                GetError::Configuration("Invalid data map error {_e:?}".to_string())
            } // Use safe default since rmp_serde::decode::Error doesn't implement Clone
            GetError::Decryption(_e) => {
                GetError::Configuration("Decryption error {_e:?}".to_string())
            } // Use safe default since self_encryption::Error doesn't implement Clone
            GetError::Deserialization(_e) => {
                GetError::Configuration("Deserialization error {_e:?}".to_string())
            } // Use safe default since rmp_serde::decode::Error doesn't implement Clone
            GetError::Network(err) => GetError::Network(err.clone()),
            GetError::Protocol(_e) => GetError::Configuration("Protocol error {_e:?}".to_string()), // Use safe default since ant_protocol::Error doesn't implement Clone
            GetError::RecordNotFound => GetError::RecordNotFound,
            GetError::RecordKindMismatch(kind) => GetError::RecordKindMismatch(*kind),
            GetError::Configuration(msg) => GetError::Configuration(msg.clone()),
            GetError::UnrecognizedDataMap(msg) => GetError::UnrecognizedDataMap(msg.clone()),
        }
    }
}

/// Represents the core Autonomi client.
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
    retry_failed: u64,
}

impl Client {
    /// Initialize the client with specified setup.
    pub async fn init(init_setup: ClientInitSetup) -> Result<Self, Error> {
        match init_setup {
            ClientInitSetup::Default => Self::init_default().await,
            ClientInitSetup::Alpha => Self::init_alpha().await,
            ClientInitSetup::Peers(peers) => Self::init_with_peers(peers).await,
            ClientInitSetup::Config(config) => Self::init_with_config(config).await,
        }
    }

    async fn init_default() -> Result<Self, Error> {
        let bootstrap_cache_config = config::BootstrapCacheConfig::new(false)
            .inspect_err(|err| {
                warn!("Failed to create bootstrap cache config: {err}");
            })
            .ok();
        Self::init_with_config(ClientConfig {
            bootstrap_cache_config,
            ..Default::default()
        })
        .await
    }

    async fn init_alpha() -> Result<Self, Error> {
        let bootstrap_cache_config = config::BootstrapCacheConfig::new(false)
            .inspect_err(|err| {
                warn!("Failed to create bootstrap cache config: {err}");
            })
            .ok();

        let client_config = ClientConfig {
            init_peers_config: InitialPeersConfig {
                first: false,
                addrs: vec![],
                network_contacts_url: ant_bootstrap::contacts::ALPHANET_CONTACTS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
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

    async fn init_with_peers(peers: Vec<Multiaddr>) -> Result<Self, Error> {
        let local = !peers.iter().any(multiaddr_is_global);

        let bootstrap_cache_config = config::BootstrapCacheConfig::new(local)
            .inspect_err(|err| {
                warn!("Failed to create bootstrap cache config: {err}");
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

    async fn init_with_config(config: ClientConfig) -> Result<Self, Error> {
        if let Some(network_id) = config.network_id {
            ant_protocol::version::set_network_id(network_id);
        }

        let initial_peers = match config.init_peers_config.get_bootstrap_addr(Some(50)).await {
            Ok(peers) => peers,
            Err(e) => return Err(Error::ConnectError(e.into())),
        };

        let network = Network::new(initial_peers, config.bootstrap_cache_config)
            .map_err(|e| Error::ConnectError(ConnectError::NoKnownPeers(e)))?;

        Ok(Self {
            network,
            client_event_sender: None,
            evm_network: config.evm_network,
            config: config.strategy,
            retry_failed: 0,
        })
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

    /// Receive events from the client.
    pub fn enable_client_events(&mut self) -> mpsc::Receiver<ClientEvent> {
        let (client_event_sender, client_event_receiver) =
            tokio::sync::mpsc::channel(CLIENT_EVENT_CHANNEL_SIZE);
        self.client_event_sender = Some(client_event_sender);
        debug!("All events to the clients are enabled");
        client_event_receiver
    }

    /// Get the EVM network configuration used by the client.
    pub fn evm_network(&self) -> &EvmNetwork {
        &self.evm_network
    }

    /// Get the retry_failed setting for the client.
    pub fn retry_failed(&self) -> u64 {
        self.retry_failed
    }

    /// Get the closest peers to a given address.
    pub async fn get_closest_to_address(
        &self,
        address: &NetworkAddress,
    ) -> Result<Vec<libp2p::kad::PeerInfo>, Error> {
        self.network
            .get_closest_peers_with_retries(address.clone())
            .await
            .map_err(Error::NetworkError)
    }

    /// Check if a record exists at the given address.
    /// This is a generic implementation that consolidates chunk_check_existence, pointer_check_existence,
    /// scratchpad_check_existence, and graph_entry_check_existence methods from the original autonomi client.
    pub async fn record_check_existence(&self, address: &NetworkAddress) -> Result<bool, Error> {
        let data_type = determine_data_type_from_address(address)?;
        let strategy = self.get_strategy(data_type);

        debug!("Checking record existence at: {address:?}");

        match self
            .network
            .get_record(address.clone(), strategy.verification_quorum)
            .await
        {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(NetworkError::SplitRecord(..)) => Ok(true), // Split records still exist
            Err(err) => {
                debug!("Error checking record existence: {err:?}");
                Err(Error::NetworkError(err))
            }
        }
    }

    pub fn client_event_sender(&self) -> Option<mpsc::Sender<ClientEvent>> {
        self.client_event_sender.clone()
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
    pub tokens_spent: crate::Amount,
}
