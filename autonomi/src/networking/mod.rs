// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// all modules are private to this networking module
pub(crate) mod common;
mod config;
mod driver;
mod interface;
mod retries;
mod utils;
pub mod version;

use crate::client::CONNECT_TIMEOUT_SECS;
use ant_bootstrap::bootstrap::Bootstrap;
// export the utils
pub(crate) use utils::multiaddr_is_global;

// re-export the types our API exposes to avoid dependency version conflicts
pub use ant_evm::PaymentQuote;
pub use ant_protocol::NetworkAddress;
pub use config::{RetryStrategy, Strategy};
pub use libp2p::kad::PeerInfo;
pub use libp2p::{
    Multiaddr, PeerId,
    kad::{Quorum, Record},
};

// internal needs
use crate::networking::version::PackageVersion;
use ant_protocol::{CLOSE_GROUP_SIZE, PrettyPrintRecordKey};
use driver::NetworkDriver;
use futures::stream::{FuturesUnordered, StreamExt};
use interface::NetworkTask;
use libp2p::kad::NoKnownPeers;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout};

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
    #[error("Provided NonZeroUsize is invalid: {0}")]
    InvalidNonZeroUsize(String),

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
    #[error("Invalid Merkle candidate node: {0}")]
    InvalidNodeMerkleCandidate(String),
    #[error(
        "Failed to get enough quotes: {got_quotes}/{CLOSE_GROUP_SIZE} quotes, got {record_exists_responses} record exists responses, and {errors_len} errors: {errors:?}"
    )]
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
    #[error(
        "Failed to get enough holders for the get record request. Expected: {expected_holders}, got: {got_holders}, holders: {holders:?}"
    )]
    GetRecordQuorumFailed {
        got_holders: usize,
        expected_holders: usize,
        holders: HashMap<PeerId, Record>,
    },
    #[error("Failed to get record: {0}")]
    GetRecordError(String),

    /// Invalid retry strategy
    #[error("Invalid retry strategy, check your config or use the default")]
    InvalidRetryStrategy,
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
    pub fn new(bootstrap: Bootstrap) -> Result<Self, NoKnownPeers> {
        let (task_sender, task_receiver) = mpsc::channel(100);
        let driver = NetworkDriver::new(bootstrap, task_receiver);

        // run the network driver in a background task
        tokio::spawn(async move {
            let _ = driver.run().await;
        });

        let network = Self {
            task_sender: Arc::new(task_sender),
        };

        Ok(network)
    }

    /// Wait until we made [`CLOSE_GROUP_SIZE`] connections to the network.
    pub async fn wait_for_connectivity(&self) -> Result<(), crate::client::ConnectError> {
        let timeout_duration = Duration::from_secs(CONNECT_TIMEOUT_SECS); // Total timeout
        let check_interval = Duration::from_millis(100); // How often to check

        debug!(
            "Waiting for connectivity with timeout of {}s, need {} peers",
            CONNECT_TIMEOUT_SECS, CLOSE_GROUP_SIZE
        );

        match timeout(timeout_duration, async {
            loop {
                match self.get_connections_made().await {
                    Ok(count) => {
                        if count >= CLOSE_GROUP_SIZE {
                            return Ok(());
                        }
                    }
                    Err(err) => {
                        tracing::warn!("Failed to get connections made: {err}, retrying...");
                    }
                }
                sleep(check_interval).await;
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Err(crate::client::ConnectError::TimedOut),
        }
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
        // For data_type like ScratchPad, it is observed the holders will be 7
        // which result in the expected_holders to be 4, and could result in false alert.
        let candidates = std::cmp::min(CLOSE_GROUP_SIZE, to.len());
        let total = NonZeroUsize::new(candidates)
            .ok_or(NetworkError::InvalidNonZeroUsize(candidates.to_string()))?;
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
        while let Some((res, peer)) = tasks.next().await {
            match res {
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

        // we don't have enough oks, return an error
        let ok_peers = ok_res.iter().map(|p| p.peer_id).collect::<Vec<_>>();
        warn!(
            "Put record {key} failed, only the following peers stored the record: {ok_peers:?}, needed {expected_holders} peers. Errors: {err_res:?}"
        );

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

        rx.await?
    }

    /// Get the closest peers to an address on the Network
    /// Defaults to N_CLOSEST_PEERS peers.
    pub async fn get_closest_peers(
        &self,
        addr: NetworkAddress,
        count: Option<usize>,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        let count = if let Some(c) = count {
            NonZeroUsize::new(c).ok_or(NetworkError::InvalidNonZeroUsize(c.to_string()))?
        } else {
            N_CLOSEST_PEERS
        };
        self.get_closest_n_peers(addr, count).await
    }

    /// Get the N closest peers to an address on the Network
    /// This function verifies the candidates by:
    /// 1. Getting N candidates via Kademlia
    /// 2. Querying each candidate for their view of closest peers
    /// 3. N Candidates are collected from the aggregated results, prefering high witness among the close group
    /// 4. Peers are returned in the ascending order of distance to the target
    pub async fn get_closest_n_peers(
        &self,
        addr: NetworkAddress,
        n: NonZeroUsize,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetClosestPeers {
            addr: addr.clone(),
            resp: tx,
            n,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;

        let candidates = match rx.await? {
            Ok(peers) => {
                if peers.len() < n.get() {
                    info!(
                        "Kad get_closest network query giving less candidates ({}/{})",
                        peers.len(),
                        n.get()
                    );
                    return Err(NetworkError::InsufficientPeers {
                        got_peers: peers.len(),
                        expected_peers: n.get(),
                        peers,
                    });
                }

                peers
            }
            Err(e) => return Err(e),
        };

        // Log initial candidates with distances
        trace!("Initial candidates from Kad query targeting {addr:?}:");
        for peer in &candidates {
            let peer_addr = NetworkAddress::from(peer.peer_id);
            let distance = addr.distance(&peer_addr);
            trace!(
                "  Candidate peer: {:?}, distance: {:?}",
                peer.peer_id, distance
            );
        }

        // Verify candidates by querying them individually for their closest peers
        let mut query_tasks = vec![];
        for peer in &candidates {
            let network = self.clone();
            let addr = addr.clone();
            let peer = peer.clone();
            // Get some extra to ensure enough candidates can be built up
            let n_value = n.get() + 2;
            query_tasks.push(async move {
                let result = network
                    .get_closest_peers_from_peer(addr, peer.clone(), Some(n_value))
                    .await;
                (peer.peer_id, result)
            });
        }

        // Process queries in parallel
        // returned with: `Vec<(PeerId, Result<Vec<(NetworkAddress, Vec<Multiaddr>)>, NetworkError>)>`
        let results = crate::utils::process_tasks_with_max_concurrency(query_tasks, n.get()).await;

        // Count peer appearances across all successful responses
        // Also collect addresses from peer responses (more accurate than DHT cached ones)
        let mut peer_counts: HashMap<PeerId, usize> = HashMap::new();
        let mut peer_addrs: HashMap<PeerId, HashSet<Multiaddr>> = HashMap::new();

        for (responder_peer_id, result) in results.iter() {
            if let Ok(peers_list) = result {
                // Log the responder and their returned peer list
                trace!("Closegroup to {addr:?} responded from peer {responder_peer_id:?}:");

                // Add the responder itself with higher weight to peer_counts since it successfully responded
                *peer_counts.entry(*responder_peer_id).or_insert(0) += 2;

                // Add the responder's addresses from candidates
                if let Some(responder_info) =
                    candidates.iter().find(|p| p.peer_id == *responder_peer_id)
                {
                    let addr_set = peer_addrs.entry(*responder_peer_id).or_default();
                    for addr in &responder_info.addrs {
                        addr_set.insert(addr.clone());
                    }
                }

                // Count appearances in the response and collect addresses
                for (peer_addr, addrs) in peers_list {
                    if let Some(peer_id) = peer_addr.as_peer_id() {
                        let distance = addr.distance(peer_addr);
                        trace!("  Reported peer: {peer_id:?}, distance: {distance:?}");

                        *peer_counts.entry(peer_id).or_insert(0) += 1;

                        // Aggregate unique addresses for this peer
                        let addr_set = peer_addrs.entry(peer_id).or_default();
                        for addr in addrs {
                            addr_set.insert(addr.clone());
                        }
                    }
                }
            } else {
                info!("Failed to get closest peers from node {responder_peer_id:?}");
            }
        }

        // Build all candidates from peer_counts with their counts and distances
        let mut candidate_with_metrics: Vec<_> = peer_counts
            .iter()
            .map(|(peer_id, &count)| {
                let peer_addr = NetworkAddress::from(*peer_id);
                let distance = addr.distance(&peer_addr);
                (*peer_id, count, distance)
            })
            .collect();

        // Sort by count (high to low), then by distance (low to high)
        candidate_with_metrics.sort_by(|a, b| {
            // First compare by count (descending)
            match b.1.cmp(&a.1) {
                std::cmp::Ordering::Equal => {
                    // If counts are equal, compare by distance (ascending)
                    a.2.cmp(&b.2)
                }
                other => other,
            }
        });

        debug!(
            "Sorted {} candidates by count and distance to target",
            candidate_with_metrics.len()
        );

        // Take the first N candidates and build PeerInfo
        let mut verified_candidates: Vec<PeerInfo> = candidate_with_metrics
            .into_iter()
            .take(n.get())
            .map(|(peer_id, count, distance)| {
                trace!("Selected candidate: {peer_id:?}, count: {count}, distance: {distance:?}");

                // Use addresses from peer responses if available, otherwise use original
                let addrs = if let Some(addrs_set) = peer_addrs.get(&peer_id) {
                    addrs_set.iter().cloned().collect()
                } else {
                    // Fallback to original candidate addresses if available
                    candidates
                        .iter()
                        .find(|c| c.peer_id == peer_id)
                        .map(|c| c.addrs.clone())
                        .unwrap_or_default()
                };

                PeerInfo { peer_id, addrs }
            })
            .collect();

        if verified_candidates.len() < n.get() {
            return Err(NetworkError::InsufficientPeers {
                got_peers: verified_candidates.len(),
                expected_peers: n.get(),
                peers: verified_candidates,
            });
        }

        // Sort final candidates by distance to target (low to high)
        verified_candidates.sort_by_key(|peer| {
            let peer_addr = NetworkAddress::from(peer.peer_id);
            addr.distance(&peer_addr)
        });

        debug!(
            "Final {} candidates sorted by distance to target",
            verified_candidates.len()
        );

        Ok(verified_candidates)
    }

    /// Get a record directly from a specific peer on the Network
    /// Returns:
    /// - Some(Record) if the peer holds the record
    /// - None if the peer doesn't hold the record or the request fails
    pub async fn get_record_from_peer(
        &self,
        addr: NetworkAddress,
        peer: PeerInfo,
    ) -> Result<Option<Record>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetRecordFromPeer {
            addr,
            peer,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;
        rx.await?
    }

    /// Get closest peers from a specific peer on the Network
    /// Returns a list of `(NetworkAddress, Vec<Multiaddr>)` tuples
    pub async fn get_closest_peers_from_peer(
        &self,
        addr: NetworkAddress,
        peer: PeerInfo,
        num_of_peers: Option<usize>,
    ) -> Result<Vec<(NetworkAddress, Vec<Multiaddr>)>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetClosestPeersFromPeer {
            addr,
            peer,
            num_of_peers,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;
        rx.await?
    }

    /// Get storage proofs directly from a specific peer on the Network
    /// Returns a vector of (NetworkAddress, ChunkProof) tuples
    pub async fn get_storage_proofs_from_peer(
        &self,
        addr: NetworkAddress,
        peer: PeerInfo,
        nonce: u64,
        difficulty: usize,
    ) -> Result<
        Vec<(
            NetworkAddress,
            Result<ant_protocol::messages::ChunkProof, ant_protocol::error::Error>,
        )>,
        NetworkError,
    > {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetStorageProofsFromPeer {
            addr,
            peer,
            nonce,
            difficulty,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;
        rx.await?
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

    /// Get a Merkle candidate quote from a specific peer
    /// Used for Merkle batch payment system
    ///
    /// # Arguments
    /// * `addr` - The target address (from MidpointProof::address())
    /// * `peer` - The peer to request the quote from
    /// * `data_type` - The data type being uploaded
    /// * `data_size` - The data size
    /// * `merkle_payment_timestamp` - Unix timestamp for the payment
    ///
    /// # Returns
    /// * `MerklePaymentCandidateNode` - Signed quote from the peer
    ///
    /// # Validation
    /// * Verifies the signature is valid
    /// * Verifies the timestamp matches the requested timestamp
    /// * Verifies data_type and data_size match the requested values
    pub async fn get_merkle_candidate_quote(
        &self,
        addr: NetworkAddress,
        peer: PeerInfo,
        data_type: u32,
        data_size: usize,
        merkle_payment_timestamp: u64,
    ) -> Result<ant_evm::merkle_payments::MerklePaymentCandidateNode, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetMerkleCandidateQuote {
            addr,
            peer,
            data_type,
            data_size,
            merkle_payment_timestamp,
            resp: tx,
        };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;
        let candidate = rx.await??;

        // Validate the candidate node
        // 1. Verify signature
        if !candidate.verify_signature() {
            return Err(NetworkError::InvalidNodeMerkleCandidate(
                "Invalid signature".to_string(),
            ));
        }

        // 2. Verify timestamp matches
        if candidate.merkle_payment_timestamp != merkle_payment_timestamp {
            return Err(NetworkError::InvalidNodeMerkleCandidate(format!(
                "Timestamp mismatch: expected {}, got {}",
                merkle_payment_timestamp, candidate.merkle_payment_timestamp
            )));
        }

        // 3. Verify data_type matches
        if candidate.quoting_metrics.data_type != data_type {
            return Err(NetworkError::InvalidNodeMerkleCandidate(format!(
                "Data type mismatch: expected {}, got {}",
                data_type, candidate.quoting_metrics.data_type
            )));
        }

        // 4. Verify data_size matches
        if candidate.quoting_metrics.data_size != data_size {
            return Err(NetworkError::InvalidNodeMerkleCandidate(format!(
                "Data size mismatch: expected {}, got {}",
                data_size, candidate.quoting_metrics.data_size
            )));
        }

        Ok(candidate)
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
        // request 10 quotes, hope that at least 5 respond
        let minimum_quotes = CLOSE_GROUP_SIZE;
        let closest_peers = self
            .get_closest_peers_with_retries(addr.clone(), Some(10))
            .await?;
        let closest_peers_id = closest_peers.iter().map(|p| p.peer_id).collect::<Vec<_>>();
        debug!("Get quotes for {addr}: got closest peers: {closest_peers_id:?}");

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
                debug!("Get quotes for {addr}: got enough quotes from peers: {peer_ids:?}");
                return Ok(Some(quotes));
            } else if no_need_to_pay.len() >= CLOSE_GROUP_SIZE_MAJORITY {
                let peer_ids = no_need_to_pay.iter().map(|p| p.peer_id).collect::<Vec<_>>();
                debug!(
                    "Get quotes for {addr}: got enough peers that claimed no payment is needed: {peer_ids:?}"
                );
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

    /// Request the node version of a peer on the network.
    /// Requires the node address(es) to be passed if the node is not in the local routing table.
    pub async fn get_node_version(&self, peer: PeerInfo) -> Result<PackageVersion, String> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::GetVersion { peer, resp: tx };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| "Network driver offline".to_string())?;

        let version_string = rx
            .await
            .map_err(|e| format!("Failed to receive version: {e}"))?;

        match version_string {
            Ok(version_str) => PackageVersion::try_from(version_str),
            Err(e) => Err(format!("Network error: {e}")),
        }
    }

    /// Get information about the routing table
    pub async fn get_connections_made(&self) -> Result<usize, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let task = NetworkTask::ConnectionsMade { resp: tx };
        self.task_sender
            .send(task)
            .await
            .map_err(|_| NetworkError::NetworkDriverOffline)?;
        tracing::trace!("Waiting for connections made response");
        rx.await?
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
