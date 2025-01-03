// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Optionally enable nightly `doc_cfg`. Allows items to be annotated, e.g.: "Available on crate feature X only".
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod address;
pub mod payment;
pub mod quote;

pub mod data;
pub mod files;
pub mod graph;
pub mod pointer;

#[cfg(feature = "external-signer")]
#[cfg_attr(docsrs, doc(cfg(feature = "external-signer")))]
pub mod external_signer;
pub mod registers;
pub mod vault;

// private module with utility functions
mod rate_limiter;
mod utils;

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore, PeersArgs};
pub use ant_evm::Amount;
use ant_evm::EvmNetwork;
use ant_networking::{interval, multiaddr_is_global, Network, NetworkBuilder, NetworkEvent};
use ant_protocol::version::IDENTIFY_PROTOCOL_STR;
use libp2p::{identity::Keypair, Multiaddr};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::sync::mpsc;

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
    pub(crate) client_event_sender: Arc<Option<mpsc::Sender<ClientEvent>>>,
    pub(crate) evm_network: EvmNetwork,
}

/// Configuration for [`Client::init_with_config`].
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Whether we're expected to connect to a local network.
    ///
    /// If `local` feature is enabled, [`ClientConfig::default()`] will set this to `true`.
    pub local: bool,

    /// List of peers to connect to.
    ///
    /// If not provided, the client will use the default bootstrap peers.
    pub peers: Option<Vec<Multiaddr>>,
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
    #[error("Failed to bootstrap the client")]
    Bootstrap(#[from] ant_bootstrap::Error),
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
            local: true,
            ..Default::default()
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
            local,
            peers: Some(peers),
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
        let (network, event_receiver) = build_client_and_run_swarm(config.local);

        let peers_args = PeersArgs {
            disable_mainnet_contacts: config.local,
            addrs: config.peers.unwrap_or_default(),
            ..Default::default()
        };

        let peers = match peers_args.get_addrs(None, None).await {
            Ok(peers) => peers,
            Err(e) => return Err(e.into()),
        };

        let network_clone = network.clone();
        let peers = peers.to_vec();
        let _handle = ant_networking::time::spawn(async move {
            for addr in peers {
                if let Err(err) = network_clone.dial(addr.clone()).await {
                    error!("Failed to dial addr={addr} with err: {err:?}");
                };
            }
        });

        // Wait until we have added a few peers to our routing table.
        let (sender, receiver) = futures::channel::oneshot::channel();
        ant_networking::time::spawn(handle_event_receiver(event_receiver, sender));
        receiver.await.expect("sender should not close")?;
        debug!("Enough peers were added to our routing table, initialization complete");

        Ok(Self {
            network,
            client_event_sender: Arc::new(None),
            evm_network: Default::default(),
        })
    }

    /// Connect to the network.
    ///
    /// This will timeout after [`CONNECT_TIMEOUT_SECS`] secs.
    ///
    /// ```no_run
    /// # use autonomi::client::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let peers = ["/ip4/127.0.0.1/udp/1234/quic-v1".parse()?];
    /// #[allow(deprecated)]
    /// let client = Client::connect(&peers).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[deprecated(
        since = "0.2.4",
        note = "Use [`Client::init`] or [`Client::init_with_config`] instead"
    )]
    pub async fn connect(peers: &[Multiaddr]) -> Result<Self, ConnectError> {
        // Any global address makes the client non-local
        let local = !peers.iter().any(multiaddr_is_global);

        let (network, event_receiver) = build_client_and_run_swarm(local);

        // Spawn task to dial to the given peers
        let network_clone = network.clone();
        let peers = peers.to_vec();
        let _handle = ant_networking::time::spawn(async move {
            for addr in peers {
                if let Err(err) = network_clone.dial(addr.clone()).await {
                    error!("Failed to dial addr={addr} with err: {err:?}");
                    eprintln!("addr={addr} Failed to dial: {err:?}");
                };
            }
        });

        let (sender, receiver) = futures::channel::oneshot::channel();
        ant_networking::time::spawn(handle_event_receiver(event_receiver, sender));

        receiver.await.expect("sender should not close")?;
        debug!("Client is connected to the network");

        // With the switch to the new bootstrap cache scheme,
        // Seems the too many `initial dial`s could result in failure,
        // when startup quoting/upload tasks got started up immediatly.
        // Hence, put in a forced wait to allow `initial network discovery` to be completed.
        ant_networking::time::sleep(Duration::from_secs(5)).await;

        Ok(Self {
            network,
            client_event_sender: Arc::new(None),
            evm_network: Default::default(),
        })
    }

    /// Receive events from the client.
    pub fn enable_client_events(&mut self) -> mpsc::Receiver<ClientEvent> {
        let (client_event_sender, client_event_receiver) =
            tokio::sync::mpsc::channel(CLIENT_EVENT_CHANNEL_SIZE);
        self.client_event_sender = Arc::new(Some(client_event_sender));
        debug!("All events to the clients are enabled");

        client_event_receiver
    }

    pub fn set_evm_network(&mut self, evm_network: EvmNetwork) {
        self.evm_network = evm_network;
    }
}

fn build_client_and_run_swarm(local: bool) -> (Network, mpsc::Receiver<NetworkEvent>) {
    let mut network_builder = NetworkBuilder::new(Keypair::generate_ed25519(), local);

    if let Ok(mut config) = BootstrapCacheConfig::default_config() {
        if local {
            config.disable_cache_writing = true;
        }
        if let Ok(cache) = BootstrapCacheStore::new(config) {
            network_builder.bootstrap_cache(cache);
        }
    }

    // TODO: Re-export `Receiver<T>` from `ant-networking`. Else users need to keep their `tokio` dependency in sync.
    // TODO: Think about handling the mDNS error here.
    let (network, event_receiver, swarm_driver) =
        network_builder.build_client().expect("mdns to succeed");

    let _swarm_driver = ant_networking::time::spawn(swarm_driver.run());
    debug!("Client swarm driver is running");

    (network, event_receiver)
}

async fn handle_event_receiver(
    mut event_receiver: mpsc::Receiver<NetworkEvent>,
    sender: futures::channel::oneshot::Sender<Result<(), ConnectError>>,
) {
    // We switch this to `None` when we've sent the oneshot 'connect' result.
    let mut sender = Some(sender);
    let mut unsupported_protocols = vec![];

    let mut timeout_timer = interval(Duration::from_secs(CONNECT_TIMEOUT_SECS));
    timeout_timer.tick().await;

    loop {
        tokio::select! {
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
    pub record_count: usize,
    pub tokens_spent: Amount,
}
