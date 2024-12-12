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

pub mod data;
#[cfg(feature = "external-signer")]
#[cfg_attr(docsrs, doc(cfg(feature = "external-signer")))]
pub mod external_signer;
pub mod files;
#[cfg(feature = "registers")]
#[cfg_attr(docsrs, doc(cfg(feature = "registers")))]
pub mod registers;
#[cfg(feature = "vault")]
#[cfg_attr(docsrs, doc(cfg(feature = "vault")))]
pub mod vault;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

// private module with utility functions
mod utils;

use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore, PeersArgs};
pub use ant_evm::Amount;

use ant_networking::{interval, multiaddr_is_global, Network, NetworkBuilder, NetworkEvent};
use ant_protocol::{version::IDENTIFY_PROTOCOL_STR, CLOSE_GROUP_SIZE};
use libp2p::{identity::Keypair, multiaddr::Protocol, Multiaddr};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::sync::mpsc;

/// Time before considering the connection timed out.
pub const CONNECT_TIMEOUT_SECS: u64 = 10;

const CLIENT_EVENT_CHANNEL_SIZE: usize = 100;

/// Represents a connection to the Autonomi network.
///
/// # Example
///
/// To connect to the network, use [`Client::connect`].
///
/// ```no_run
/// # use autonomi::client::Client;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let peers = ["/ip4/127.0.0.1/udp/1234/quic-v1".parse()?];
/// let client = Client::connect(&peers).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    pub(crate) network: Network,
    pub(crate) client_event_sender: Arc<Option<mpsc::Sender<ClientEvent>>>,
}

/// Configuration for [`Client::init_with_config`].
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Whether we're expected to connect to a local network.
    pub local: bool,
    /// List of peers to connect to.
    ///
    /// If not provided, the client will use the default bootstrap peers.
    pub peers: Option<Vec<Multiaddr>>,
}

/// Error returned by [`Client::connect`].
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// Did not manage to connect to enough peers in time.
    #[error("Could not connect to enough peers in time.")]
    TimedOut,
    /// Same as [`ConnectError::TimedOut`] but with a list of incompatible protocols.
    #[error("Could not connect to peers due to incompatible protocol: {0:?}")]
    TimedOutWithIncompatibleProtocol(HashSet<String>, String),
}

impl Client {
    pub async fn init() -> Result<Self, ant_bootstrap::Error> {
        Self::init_with_config(ClientConfig::default()).await
    }

    pub async fn init_with_config(config: ClientConfig) -> Result<Self, ant_bootstrap::Error> {
        let (network, _event_receiver) = build_client_and_run_swarm(config.local);

        let peers_args = PeersArgs {
            disable_mainnet_contacts: config.local,
            ..Default::default()
        };

        let peers = if let Some(peers) = config.peers {
            peers
        } else {
            match peers_args.get_addrs(None, None).await {
                Ok(peers) => peers,
                Err(e) => return Err(e),
            }
        };

        let peers_len = peers.len();
        // Add peers to the routing table.
        let peers_with_p2p: Vec<_> = peers
            .into_iter()
            .filter_map(|addr| {
                addr.iter().find_map(|p| match p {
                    Protocol::P2p(id) => Some((id, addr.clone())),
                    _ => None,
                })
            })
            .collect();
        if peers_with_p2p.len() < peers_len {
            tracing::warn!("Some bootstrap addresses have no peer ID, skipping them");
        }
        let _ = network.add_peer_addresses(peers_with_p2p).await;

        Ok(Self {
            network,
            client_event_sender: Arc::new(None),
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
    /// let client = Client::connect(&peers).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(peers: &[Multiaddr]) -> Result<Self, ConnectError> {
        // Any global address makes the client non-local
        let local = !peers.iter().any(multiaddr_is_global);

        let (network, event_receiver) = build_client_and_run_swarm(local);

        // Spawn task to dial to the given peers
        let network_clone = network.clone();
        let peers = peers.to_vec();
        let _handle = ant_networking::target_arch::spawn(async move {
            for addr in peers {
                if let Err(err) = network_clone.dial(addr.clone()).await {
                    error!("Failed to dial addr={addr} with err: {err:?}");
                    eprintln!("addr={addr} Failed to dial: {err:?}");
                };
            }
        });

        let (sender, receiver) = futures::channel::oneshot::channel();
        ant_networking::target_arch::spawn(handle_event_receiver(event_receiver, sender));

        receiver.await.expect("sender should not close")?;
        debug!("Client is connected to the network");

        Ok(Self {
            network,
            client_event_sender: Arc::new(None),
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

    let _swarm_driver = ant_networking::target_arch::spawn(swarm_driver.run());
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

    #[cfg(not(target_arch = "wasm32"))]
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
