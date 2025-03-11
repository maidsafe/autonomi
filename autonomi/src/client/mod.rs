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

pub mod config;
pub mod key_derivation;
pub mod payment;
pub mod quote;

#[cfg(feature = "external-signer")]
#[cfg_attr(docsrs, doc(cfg(feature = "external-signer")))]
pub mod external_signer;

// private module with utility functions
mod utils;

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore, InitialPeersConfig};
pub use ant_evm::Amount;
use ant_evm::EvmNetwork;
use ant_networking::{
    interval, multiaddr_is_global, Network, NetworkBuilder, NetworkError, NetworkEvent,
};
use ant_protocol::{version::IDENTIFY_PROTOCOL_STR, NetworkAddress};
use config::{ClientConfig, ClientOperatingStrategy};
use libp2p::{identity::Keypair, Multiaddr};
use payment::PayError;
use quote::CostError;
use std::{collections::HashSet, time::Duration};
use tokio::sync::{mpsc, watch};

/// Time before considering the connection timed out.
pub const CONNECT_TIMEOUT_SECS: u64 = 10;

const CLIENT_EVENT_CHANNEL_SIZE: usize = 100;

// Amount of peers to confirm into our routing table before we consider the client ready.
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
    pub(crate) network: Network,
    pub(crate) client_event_sender: Option<mpsc::Sender<ClientEvent>>,
    /// The EVM network to use for the client.
    evm_network: EvmNetwork,
    /// The configuration for operations on the client.
    config: ClientOperatingStrategy,
    // Shutdown signal for child tasks. Sends signal when dropped.
    _shutdown_tx: watch::Sender<bool>,
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

        let (shutdown_tx, network, event_receiver) =
            build_client_and_run_swarm(config.init_peers_config.local, initial_peers);

        // Wait until we have added a few peers to our routing table.
        let (sender, receiver) = futures::channel::oneshot::channel();
        ant_networking::time::spawn(handle_event_receiver(
            event_receiver,
            sender,
            shutdown_tx.subscribe(),
        ));
        receiver.await.expect("sender should not close")?;
        debug!("Enough peers were added to our routing table, initialization complete");

        Ok(Self {
            network,
            client_event_sender: None,
            evm_network: config.evm_network,
            config: config.strategy,
            _shutdown_tx: shutdown_tx,
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

fn build_client_and_run_swarm(
    local: bool,
    initial_peers: Vec<Multiaddr>,
) -> (watch::Sender<bool>, Network, mpsc::Receiver<NetworkEvent>) {
    let mut network_builder =
        NetworkBuilder::new(Keypair::generate_ed25519(), local, initial_peers);

    if let Ok(mut config) = BootstrapCacheConfig::default_config(local) {
        if local {
            config.disable_cache_writing = true;
        }
        if let Ok(cache) = BootstrapCacheStore::new(config) {
            network_builder.bootstrap_cache(cache);
        }
    }

    // TODO: Re-export `Receiver<T>` from `ant-networking`. Else users need to keep their `tokio` dependency in sync.
    // TODO: Think about handling the mDNS error here.
    let (network, event_receiver, swarm_driver) = network_builder.build_client();

    // TODO: Implement graceful SwarmDriver shutdown for client.
    // Create a shutdown signal channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let _swarm_driver = ant_networking::time::spawn(swarm_driver.run(shutdown_rx));

    debug!("Client swarm driver is running");

    (shutdown_tx, network, event_receiver)
}

async fn handle_event_receiver(
    mut event_receiver: mpsc::Receiver<NetworkEvent>,
    sender: futures::channel::oneshot::Sender<Result<(), ConnectError>>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    // We switch this to `None` when we've sent the oneshot 'connect' result.
    let mut sender = Some(sender);
    let mut unsupported_protocols = vec![];

    let mut timeout_timer = interval(Duration::from_secs(CONNECT_TIMEOUT_SECS));
    timeout_timer.tick().await;

    loop {
        tokio::select! {
            // polls futures in order they appear here (as opposed to random)
            biased;

            // Check for a shutdown command.
            result = shutdown_rx.changed() => {
                if result.is_ok() && *shutdown_rx.borrow() || result.is_err() {
                    info!("Shutdown signal received or sender dropped. Exiting event receiver loop.");
                    break;
                }
            }
            _ = timeout_timer.tick() =>  {
                if let Some(sender) = sender.take() {
                    if unsupported_protocols.len() > 1 {
                        let protocols: HashSet<String> =
                            unsupported_protocols.iter().cloned().collect();
                        sender
                            .send(Err(ConnectError::TimedOutWithIncompatibleProtocol(
                                protocols,
                                IDENTIFY_PROTOCOL_STR.read().expect("Failed to obtain read lock for IDENTIFY_PROTOCOL_STR. A call to set_network_id performed. This should not happen").clone(),
                            )))
                            .expect("receiver should not close");
                    } else {
                        sender
                            .send(Err(ConnectError::TimedOut))
                            .expect("receiver should not close");
                    }
                }
            }
            event = event_receiver.recv() => {
                let event = event.expect("receiver should not close");
                match event {
                    NetworkEvent::PeerAdded(_peer_id, peers_len) => {
                        tracing::trace!("Peer added: {peers_len} in routing table");

                        if peers_len >= CLOSE_GROUP_SIZE {
                            if let Some(sender) = sender.take() {
                                sender.send(Ok(())).expect("receiver should not close");
                            }
                        }
                    }
                    NetworkEvent::PeerWithUnsupportedProtocol { their_protocol, .. } => {
                        tracing::warn!(their_protocol, "Peer with unsupported protocol");

                        if sender.is_some() {
                            unsupported_protocols.push(their_protocol);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // TODO: Handle closing of network events sender
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
