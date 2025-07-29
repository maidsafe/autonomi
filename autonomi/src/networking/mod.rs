// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// all modules are private to this networking module
mod bootstrap;
pub(crate) mod common;
mod config;
mod driver;
mod interface;
mod retries;
mod utils;

use std::cmp::min;
// export the utils
pub(crate) use utils::multiaddr_is_global;

// re-export the types our API exposes to avoid dependency version conflicts
pub use ant_evm::PaymentQuote;
pub use ant_protocol::NetworkAddress;
pub use config::{RetryStrategy, Strategy};
pub use libp2p::kad::PeerInfo;
pub use libp2p::{
    kad::{Quorum, Record},
    Multiaddr, PeerId,
};

// internal needs
use crate::networking::bootstrap::{MAX_BOOTSTRAP_DURATION_SECS, MAX_REQUIRED_PEERS};
use ant_protocol::{PrettyPrintRecordKey, CLOSE_GROUP_SIZE};
use driver::NetworkDriver;
use futures::stream::{FuturesUnordered, StreamExt};
use interface::NetworkTask;
use libp2p::kad::NoKnownPeers;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

/// Result type for tasks responses sent by the [`crate::driver::NetworkDriver`] to the [`crate::Network`]
pub(in crate::networking) type OneShotTaskResult<T> = oneshot::Sender<Result<T, NetworkError>>;

/// The majority size within the close group.
pub const CLOSE_GROUP_SIZE_MAJORITY: usize = CLOSE_GROUP_SIZE / 2 + 1;

/// The number of closest peers to request from the network
const N_CLOSEST_PEERS: NonZeroUsize =
    NonZeroUsize::new(CLOSE_GROUP_SIZE + 2).expect("N_CLOSEST_PEERS must be > 0");

/// Errors that can occur when interacting with the [`crate::Network`]
#[derive(Error, Debug, Clone)]
pub enum NetworkError {
    /// The network driver is offline, better restart the client
    #[error("Failed to send task to network driver")]
    NetworkDriverOffline,
    /// Failed to receive task from network driver, better restart the client
    #[error("Failed to receive task from network driver: {0}")]
    NetworkDriverReceive(#[from] tokio::sync::oneshot::error::RecvError),
    /// Incompatible network protocol, either the client or the nodes are outdated
    #[error("Incompatible network protocol, either the client or the nodes are outdated")]
    IncompatibleNetworkProtocol,

    /// Error getting closest peers
    #[error("Get closest peers request timeout")]
    GetClosestPeersTimeout,
    #[error("Received {got_peers} closest peers, expected at least {expected_peers}")]
    InsufficientPeers {
        got_peers: usize,
        expected_peers: usize,
        peers: Vec<PeerInfo>,
    },

    /// Error putting record
    #[error("Cannot put record to 0 targets, provide at least one target in the `to` field")]
    PutRecordMissingTargets,
    #[error("Put verification failed: {0}")]
    PutRecordVerification(String),
    #[error(
        "Put record quorum failed, only the following peers stored the record: {0:?}, needed {1} peers"
    )]
    PutRecordQuorumFailed(Vec<PeerId>, NonZeroUsize),
    #[error("Put record failed, the following peers stored the record: {0:?}, errors: {1:?}")]
    PutRecordTooManyPeerFailed(Vec<PeerId>, Vec<(PeerId, String)>),
    #[error("Put record timeout, only the following peers stored the record: {0:?}")]
    PutRecordTimeout(Vec<PeerId>),
    #[error("Put record rejected: {0}")]
    PutRecordRejected(String),
    #[error("Outdated record rejected: with counter {counter}, expected any above {expected}")]
    OutdatedRecordRejected { counter: u64, expected: u64 },

    /// Error getting quote
    #[error("Failed to get quote: {0}")]
    GetQuoteError(String),
    #[error("Invalid quote: {0}")]
    InvalidQuote(String),
    #[error("Failed to get enough quotes: {got_quotes}/{CLOSE_GROUP_SIZE} quotes, got {record_exists_responses} record exists responses, and {errors_len} errors: {errors:?}")]
    InsufficientQuotes {
        got_quotes: usize,
        record_exists_responses: usize,
        errors_len: usize,
        errors: Vec<NetworkError>,
    },

    /// Error getting record
    #[error("Peers have conflicting entries for this record: {0:?}")]
    SplitRecord(HashMap<PeerId, Record>),
    #[error("Get record timed out, peers found holding the record at timeout: {0:?}")]
    GetRecordTimeout(Vec<PeerId>),
    #[error("Failed to get enough holders for the get record request. Expected: {expected_holders}, got: {got_holders}")]
    GetRecordQuorumFailed {
        got_holders: usize,
        expected_holders: usize,
    },
    #[error("Failed to get record: {0}")]
    GetRecordError(String),

    /// Invalid retry strategy
    #[error("Invalid retry strategy, check your config or use the default")]
    InvalidRetryStrategy,

    /// Bootstrap failure
    #[error("Bootstrap failure: {0}")]
    BootstrapFailure(String),
}

impl NetworkError {
    /// When encountering these, create a new [`Network`] instance
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            NetworkError::NetworkDriverOffline
                | NetworkError::NetworkDriverReceive(_)
                | NetworkError::IncompatibleNetworkProtocol
        )
    }

    /// When encountering these, the request should not be retried
    pub fn cannot_retry(&self) -> bool {
        matches!(self, NetworkError::OutdatedRecordRejected { .. }) || self.is_fatal()
    }
}

/// The Client interface to the Autonomi Network
#[derive(Debug, Clone)]
pub struct Network {
    task_sender: Arc<tokio::sync::mpsc::Sender<NetworkTask>>,
}

impl Network {
    /// Create a new network client
    /// This will start the network driver in a background thread, which is a long-running task that runs until the [`Network`] is dropped
    /// The [`Network`] is cheaply cloneable, prefer cloning over creating new instances to avoid creating multiple network drivers
    pub async fn new(initial_contacts: Vec<Multiaddr>) -> Result<Self, NoKnownPeers> {
        let (task_sender, task_receiver) = mpsc::channel(100);
        let driver = NetworkDriver::new(task_receiver);

        // run the network driver in a background task
        tokio::spawn(async move {
            let _ = driver.run().await;
        });

        let network = Self {
            task_sender: Arc::new(task_sender),
        };

        let (sender, receiver) = oneshot::channel::<u32>();

        network
            .register_bootstrap_observer(
                min(initial_contacts.len() as u32, MAX_REQUIRED_PEERS),
                sender,
            )
            .await?;

        network.connect_to_peers(initial_contacts).await?;

        match timeout(Duration::from_secs(MAX_BOOTSTRAP_DURATION_SECS), receiver).await {
            Ok(result) => match result {
                Ok(peers_count) => {
                    debug!("Bootstrap complete. Connected to {peers_count} peers.");
                    network.trigger_bootstrap().await?;
                }
                Err(err) => {
                    debug!("Bootstrap sender closed: {err:?}");
                    return Err(NoKnownPeers());
                }
            },
            Err(err) => {
                error!("Bootstrap timeout: {err:?}");
                return Err(NoKnownPeers());
            }
        };

        Ok(network)
    }

    /// Connect to peers via the network driver
    pub async fn connect_to_peers(&self, peers: Vec<Multiaddr>) -> Result<(), NoKnownPeers> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::ConnectToPeers { peers, resp: tx };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NoKnownPeers())?;
        rx.await
            .map_err(|_| NoKnownPeers())?
            .map_err(|_| NoKnownPeers())
    }

    /// Register a bootstrap observer
    pub async fn register_bootstrap_observer(
        &self,
        required_peers: u32,
        sender: oneshot::Sender<u32>,
    ) -> Result<(), NoKnownPeers> {
        let task = NetworkTask::RegisterBootstrapObserver {
            required_peers,
            observer_callback: sender,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NoKnownPeers())?;
        Ok(())
    }

    /// Trigger bootstrap process
    pub async fn trigger_bootstrap(&self) -> Result<(), NoKnownPeers> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::TriggerBootstrap { resp: tx };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NoKnownPeers())?;
        rx.await
            .map_err(|_| NoKnownPeers())?
            .map_err(|_| NoKnownPeers())
    }

    /// Get a record from the network
    /// Returns the record if successful
    /// If the record is not found, the result will be None
    pub async fn get_record(
        &self,
        addr: NetworkAddress,
        quorum: Quorum,
    ) -> Result<Option<Record>, NetworkError> {
        let (record, _holders) = self.get_record_and_holders(addr, quorum).await?;
        Ok(record)
    }

    /// Get a record from the network
    /// Returns the record if successful along with the peers that handed it to us
    /// If the record is not found, the result will be None and an empty list of peers
    /// If the Quorum is not met, the result will be None and the list of peers that did manage to deliver the record
    /// As soon as the quorum is met, the request will complete and the result will be returned.
    /// Note that the holders returned is not an exhaustive list of all holders of the record,
    /// it only contains the peers that responded to the request before the quorum was met.
    pub async fn get_record_and_holders(
        &self,
        addr: NetworkAddress,
        quorum: Quorum,
    ) -> Result<(Option<Record>, Vec<PeerId>), NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetRecord {
            addr,
            quorum,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;
        rx.await?
    }

    /// Put a record to the network
    /// The `to` field should not be empty else [`NetworkError::PutRecordMissingTargets`] is returned
    pub async fn put_record(
        &self,
        record: Record,
        to: Vec<PeerInfo>,
        quorum: Quorum,
    ) -> Result<(), NetworkError> {
        let key = PrettyPrintRecordKey::from(&record.key);
        let total = NonZeroUsize::new(to.len()).ok_or(NetworkError::PutRecordMissingTargets)?;
        let expected_holders = expected_holders(quorum, total);

        trace!(
            "Put record {key} to {} peers with quorum {quorum:?}",
            to.len()
        );
        // put record using the request response protocol
        let mut tasks = FuturesUnordered::new();
        for peer in to {
            let record_clone = record.clone();
            tasks.push(async move {
                let res = self.put_record_req(record_clone, peer.clone()).await;
                (res, peer)
            });
        }

        // collect results
        let mut ok_res = vec![];
        let mut err_res = vec![];
        let mut old_nodes_tasks = vec![];
        while let Some((res, peer)) = tasks.next().await {
            match res {
                // redirect to old protocol on old nodes
                Err(NetworkError::IncompatibleNetworkProtocol) => {
                    let record_clone = record.clone();
                    let self_clone = self.clone();
                    let handle = tokio::spawn(async move {
                        let res = self_clone
                            .put_record_kad(record_clone, vec![peer.clone()], Quorum::One)
                            .await;
                        (res, peer)
                    });
                    old_nodes_tasks.push(handle);
                }
                // accumulate oks until Quorum is met
                Ok(()) => {
                    ok_res.push(peer);
                    if ok_res.len() >= expected_holders.get() {
                        return Ok(());
                    }
                }
                Err(e) => err_res.push((peer.peer_id, e.to_string())),
            }
        }
        let new_nodes_ok = ok_res.len();

        // complete with answers from old nodes
        let mut old_nodes_futures = FuturesUnordered::new();
        for handle in old_nodes_tasks {
            old_nodes_futures.push(handle);
        }
        while let Some(join_result) = old_nodes_futures.next().await {
            if let Ok((res, peer)) = join_result {
                match res {
                    // accumulate oks until Quorum is met
                    Ok(()) => {
                        ok_res.push(peer);
                        if ok_res.len() >= expected_holders.get() {
                            let old_nodes_ok = ok_res.len() - new_nodes_ok;
                            trace!("Put record {key} completed with {new_nodes_ok} new nodes ok and {old_nodes_ok} old nodes ok");
                            return Ok(());
                        }
                    }
                    Err(e) => err_res.push((peer.peer_id, e.to_string())),
                }
            }
        }

        // we don't have enough oks, return an error
        let ok_peers = ok_res.iter().map(|p| p.peer_id).collect::<Vec<_>>();
        warn!("Put record {key} failed, only the following peers stored the record: {ok_peers:?}, needed {expected_holders} peers. Errors: {err_res:?}");

        Err(NetworkError::PutRecordTooManyPeerFailed(ok_peers, err_res))
    }

    async fn put_record_req(&self, record: Record, to: PeerInfo) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::PutRecordReq {
            record,
            to,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;

        let res = rx.await?;
        res
    }

    async fn put_record_kad(
        &self,
        record: Record,
        to: Vec<PeerInfo>,
        quorum: Quorum,
    ) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        let network_address = NetworkAddress::from(&record.key);
        let task = NetworkTask::PutRecordKad {
            record,
            to,
            quorum,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;

        let res = rx.await?;

        // In poor network conditions PutRecordQuorumFailed is unreliable.
        // To eliminate false positives, we do a manual record existence check after the put.
        if let Err(NetworkError::PutRecordQuorumFailed(_, _)) = res {
            match self.get_record_and_holders(network_address, quorum).await {
                Ok((Some(_), _)) => return Ok(()),
                _ => return res,
            }
        }

        res
    }

    /// Get the closest peers to an address on the Network
    /// Defaults to N_CLOSEST_PEERS peers.
    pub async fn get_closest_peers(
        &self,
        addr: NetworkAddress,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        self.get_closest_n_peers(addr, N_CLOSEST_PEERS).await
    }

    /// Get the N closest peers to an address on the Network
    pub async fn get_closest_n_peers(
        &self,
        addr: NetworkAddress,
        n: NonZeroUsize,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetClosestPeers { addr, resp: tx, n };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;

        match rx.await? {
            Ok(mut peers) => {
                if peers.len() < n.get() {
                    return Err(NetworkError::InsufficientPeers {
                        got_peers: peers.len(),
                        expected_peers: n.get(),
                        peers,
                    });
                }
                // We sometimes receive more peers than requested (with empty addrs)
                peers.truncate(n.get());
                Ok(peers)
            }
            Err(e) => Err(e),
        }
    }

    /// Get a quote for a record from a Peer on the Network
    /// Returns an Option:
    /// - Some(PaymentQuote) if the quote is successfully received
    /// - None if the record already exists at the peer and no quote is needed
    pub async fn get_quote(
        &self,
        addr: NetworkAddress,
        peer: PeerInfo,
        data_type: u32,
        data_size: usize,
    ) -> Result<Option<(PeerInfo, PaymentQuote)>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetQuote {
            addr,
            peer,
            data_type,
            data_size,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;
        rx.await?
    }

    /// Get the quotes for a Record from the closest Peers to that address on the Network
    /// Returns an Option:
    /// - `Some(Vec<PaymentQuote>)` if the quotes are successfully received
    /// - `None` if the record already exists and no quotes are needed
    pub async fn get_quotes(
        &self,
        addr: NetworkAddress,
        data_type: u32,
        data_size: usize,
    ) -> Result<Option<Vec<(PeerInfo, PaymentQuote)>>, NetworkError> {
        // request 7 quotes, hope that at least 5 respond
        let minimum_quotes = CLOSE_GROUP_SIZE;
        let closest_peers = self.get_closest_peers_with_retries(addr.clone()).await?;
        let closest_peers_id = closest_peers.iter().map(|p| p.peer_id).collect::<Vec<_>>();
        trace!("Get quotes for {addr}: got closest peers: {closest_peers_id:?}");

        // get all quotes
        let mut tasks = FuturesUnordered::new();
        for peer in closest_peers {
            let addr_clone = addr.clone();
            tasks.push(async move {
                let res = self
                    .get_quote(addr_clone, peer.clone(), data_type, data_size)
                    .await;
                (res, peer)
            });
        }

        // count quotes and peers that claim there is no need to pay
        let mut quotes = vec![];
        let mut no_need_to_pay = vec![];
        let mut errors = vec![];
        while let Some((result, peer)) = tasks.next().await {
            match result {
                Ok(Some(quote)) => quotes.push(quote),
                Ok(None) => no_need_to_pay.push(peer),
                Err(e) => errors.push(e),
            }

            // if we have enough quotes, return them
            if quotes.len() >= minimum_quotes {
                let peer_ids = quotes.iter().map(|(p, _)| p.peer_id).collect::<Vec<_>>();
                trace!("Get quotes for {addr}: got enough quotes from peers: {peer_ids:?}");
                return Ok(Some(quotes));
            } else if no_need_to_pay.len() >= CLOSE_GROUP_SIZE_MAJORITY {
                let peer_ids = no_need_to_pay.iter().map(|p| p.peer_id).collect::<Vec<_>>();
                trace!("Get quotes for {addr}: got enough peers that claimed no payment is needed: {peer_ids:?}");
                return Ok(None);
            }
        }

        // we don't have enough happy responses, return an error
        let got_quotes = quotes.len();
        let record_exists_responses = no_need_to_pay.len();
        let errors_len = errors.len();
        Err(NetworkError::InsufficientQuotes {
            got_quotes,
            record_exists_responses,
            errors_len,
            errors,
        })
    }
}

fn expected_holders(quorum: Quorum, total: NonZeroUsize) -> NonZeroUsize {
    match quorum {
        Quorum::One => NonZeroUsize::new(1).expect("0 != 1"),
        Quorum::Majority => NonZeroUsize::new(total.get() / 2 + 1).expect("n/2+1 != 0"),
        Quorum::All => total,
        Quorum::N(n) => n,
    }
}
