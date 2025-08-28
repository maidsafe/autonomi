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
pub mod encryption;
pub mod key_derivation;
pub mod payment;
pub mod quote;

#[cfg(feature = "external-signer")]
#[cfg_attr(docsrs, doc(cfg(feature = "external-signer")))]
pub mod external_signer;

// private module with utility functions
mod data_map_restoration;
mod utils;

use crate::client::config::{ClientConfig, ClientOperatingStrategy};

pub use ant_evm::Amount;
pub use ant_protocol::CLOSE_GROUP_SIZE;
pub use autonomi_core::client::ChunkBatchUploadState;
pub use autonomi_core::{
    ClientEvent, ClientInitSetup, ConnectError, GetError, PutError, UploadSummary,
};

use ant_evm::EvmNetwork;
use libp2p::Multiaddr;
use tokio::sync::mpsc;

/// Time before considering the connection timed out.
pub const CONNECT_TIMEOUT_SECS: u64 = 10;

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
    /// core client to undertake fundamental actions.
    pub(crate) core_client: autonomi_core::Client,
}

impl Client {
    /// Initialize the client with default configuration.
    ///
    /// See [`Client::init_with_config`].
    pub async fn init() -> Result<Self, ConnectError> {
        let core_client = autonomi_core::Client::init(ClientInitSetup::Default)
            .await
            .map_err(|e| ConnectError::from_error(&e))?;

        Ok(Self { core_client })
    }

    /// Initialize a client that is configured to be local.
    ///
    /// See [`Client::init_with_config`].
    pub async fn init_local() -> Result<Self, ConnectError> {
        let bootstrap_cache_config = crate::BootstrapCacheConfig::new(true)
            .inspect_err(|err| {
                warn!("Failed to create bootstrap cache config: {err}");
            })
            .ok();

        let config = ClientConfig {
            init_peers_config: ant_bootstrap::InitialPeersConfig {
                local: true,
                ..Default::default()
            },
            evm_network: ant_evm::EvmNetwork::new(true)
                .map_err(|e| ConnectError::EvmNetworkError(format!("{e:?}")))?,
            strategy: Default::default(),
            network_id: None,
            bootstrap_cache_config,
        };

        Self::init_with_config(config).await
    }

    /// Initialize a client that is configured to be connected to the alpha network (Impossible Futures).
    pub async fn init_alpha() -> Result<Self, ConnectError> {
        let core_client = autonomi_core::Client::init(ClientInitSetup::Alpha)
            .await
            .map_err(|e| ConnectError::from_error(&e))?;

        Ok(Self { core_client })
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
        let core_client = autonomi_core::Client::init(ClientInitSetup::Peers(peers))
            .await
            .map_err(|e| ConnectError::from_error(&e))?;

        Ok(Self { core_client })
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
        let core_client = autonomi_core::Client::init(ClientInitSetup::Config(config))
            .await
            .map_err(|e| ConnectError::from_error(&e))?;

        Ok(Self { core_client })
    }

    /// Set the `ClientOperatingStrategy` for the client.
    pub fn with_strategy(mut self, strategy: ClientOperatingStrategy) -> Self {
        self.core_client = self.core_client.with_strategy(strategy);
        self
    }

    /// Set whether to retry failed uploads automatically.
    pub fn with_retry_failed(mut self, retry_failed: u64) -> Self {
        self.core_client = self.core_client.with_retry_failed(retry_failed);
        self
    }

    /// Receive events from the client.
    pub fn enable_client_events(&mut self) -> mpsc::Receiver<ClientEvent> {
        self.core_client.enable_client_events()
    }

    /// Get the evm network.
    pub fn evm_network(&self) -> &EvmNetwork {
        self.core_client.evm_network()
    }
}
