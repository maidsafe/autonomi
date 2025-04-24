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
use std::collections::HashSet;

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
mod network;
mod utils;

use crate::networking::Multiaddr;
use ant_bootstrap::InitialPeersConfig;
pub use ant_evm::Amount;
use ant_evm::EvmNetwork;
use ant_protocol::NetworkAddress;
use config::ClientConfig;
use payment::PayError;
use quote::CostError;
use tokio::sync::mpsc;

/// Time before considering the connection timed out.
pub const CONNECT_TIMEOUT_SECS: u64 = 10;

const CLIENT_EVENT_CHANNEL_SIZE: usize = 100;

// Amount of peers to confirm into our routing table before we consider the client ready.
use crate::client::config::ClientOperatingStrategy;
use crate::networking::multiaddr_is_global;
use crate::networking::{Network, NetworkError};
use ant_protocol::storage::RecordKind;
pub use ant_protocol::CLOSE_GROUP_SIZE;

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
#[derive(Clone)]
pub struct Client {
    /// The Autonomi Network to use for the client.
    pub(crate) network: Network,
    /// Events sent by the client, can be enabled by calling [`Client::enable_client_events`].
    pub(crate) client_event_sender: Option<mpsc::Sender<ClientEvent>>,
    /// The EVM network to use for the client.
    evm_network: EvmNetwork,
    /// The configuration for operations on the client.
    config: ClientOperatingStrategy,
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
}

/// Errors that can occur during the put operation.
#[derive(Debug, thiserror::Error)]
pub enum PutError {
    #[error("Failed to self-encrypt data.")]
    SelfEncryption(#[from] crate::self_encryption::Error),
    #[error("A network error occurred.")]
    Network(#[from] NetworkError),
    #[error("Error occurred during cost estimation.")]
    CostError(#[from] CostError),
    #[error("Error occurred during payment.")]
    PayError(#[from] PayError),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("A wallet error occurred.")]
    Wallet(#[from] ant_evm::EvmError),
    #[error("The owner key does not match the client's public key")]
    ScratchpadBadOwner,
    #[error("Payment unexpectedly invalid for {0:?}")]
    PaymentUnexpectedlyInvalid(NetworkAddress),
    #[error("The payment proof contains no payees.")]
    PayeesMissing,
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
    #[error("General networking error: {0:?}")]
    Network(#[from] NetworkError),
    #[error("General protocol error: {0:?}")]
    Protocol(#[from] ant_protocol::Error),
    #[error("Record could not be found.")]
    RecordNotFound,
    // The RecordKind that was obtained did not match with the expected one
    #[error("The RecordKind obtained from the Record did not match with the expected kind: {0}")]
    RecordKindMismatch(RecordKind),
}

impl Client {
    /// Initialize the client with default configuration.
    ///
    /// See [`Client::init_with_config`].
    pub async fn init() -> Result<Self, ConnectError> {
        Self::init_with_config(Default::default()).await
    }

    /// Initialize a client that is configured to be local.
    ///
    /// See [`Client::init_with_config`].
    pub async fn init_local() -> Result<Self, ConnectError> {
        Self::init_with_config(ClientConfig {
            init_peers_config: InitialPeersConfig {
                local: true,
                ..Default::default()
            },
            evm_network: EvmNetwork::new(true).unwrap_or_default(),
            strategy: Default::default(),
            network_id: None,
        })
        .await
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

        Self::init_with_config(ClientConfig {
            init_peers_config: InitialPeersConfig {
                local,
                addrs: peers,
                ..Default::default()
            },
            evm_network: EvmNetwork::new(local).unwrap_or_default(),
            strategy: Default::default(),
            network_id: None,
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
        let initial_peers = match config.init_peers_config.get_addrs(None, None).await {
            Ok(peers) => peers,
            Err(e) => return Err(e.into()),
        };

        if let Some(network_id) = config.network_id {
            ant_protocol::version::set_network_id(network_id);
        }
        let network = Network::new(initial_peers)?;

        Ok(Self {
            network,
            client_event_sender: None,
            evm_network: config.evm_network,
            config: config.strategy,
        })
    }

    /// Receive events from the client.
    pub fn enable_client_events(&mut self) -> mpsc::Receiver<ClientEvent> {
        let (client_event_sender, client_event_receiver) =
            tokio::sync::mpsc::channel(CLIENT_EVENT_CHANNEL_SIZE);
        self.client_event_sender = Some(client_event_sender);
        debug!("All events to the clients are enabled");

        client_event_receiver
    }

    pub fn evm_network(&self) -> &EvmNetwork {
        &self.evm_network
    }
}

/// Events that can be broadcasted by the client.
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
