// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Query management for iterative Kademlia operations.
//! 
//! This module implements the iterative query algorithm used in Kademlia for
//! operations like FindNode, FindValue, and PutValue.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::networking::kad::transport::{
    KadPeerId, KadDistance, KadMessage, KadResponse, QueryId, RecordKey, Record, 
    PeerInfo, KadError, QueryResult,
};
use crate::networking::kad::kbucket::KBucketKey;

/// Configuration for query behavior
#[derive(Clone, Debug)]
pub struct QueryConfig {
    /// Number of concurrent requests (alpha parameter)
    pub alpha: usize,
    /// Maximum timeout for a single query
    pub query_timeout: Duration,
    /// Timeout for individual requests within a query
    pub request_timeout: Duration,
    /// Maximum number of peers to contact per query
    pub max_peers: usize,
    /// Number of times to retry failed requests
    pub max_retries: u32,
    /// Minimum number of peers required for a successful query
    pub min_peers: usize,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            alpha: 3,
            query_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(5),
            max_peers: 100,
            max_retries: 2,
            min_peers: 10,
        }
    }
}

/// Type of Kademlia query being performed
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryType {
    /// Find the closest peers to a target
    FindNode { target: KadPeerId },
    /// Find the value for a specific key
    FindValue { key: RecordKey },
    /// Store a value at a specific key
    PutValue { record: Record },
    /// Find providers for a specific key
    GetProviders { key: RecordKey },
    /// Bootstrap the routing table
    Bootstrap,
}

impl QueryType {
    /// Get the target key for this query type
    pub fn target_key(&self) -> KBucketKey {
        match self {
            QueryType::FindNode { target } => KBucketKey::from_peer_id(target),
            QueryType::FindValue { key } => KBucketKey::from_peer_id(&key.to_kad_peer_id()),
            QueryType::PutValue { record } => KBucketKey::from_peer_id(&record.key.to_kad_peer_id()),
            QueryType::GetProviders { key } => KBucketKey::from_peer_id(&key.to_kad_peer_id()),
            QueryType::Bootstrap => {
                // For bootstrap, use a random key
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                Instant::now().hash(&mut hasher);
                let random_bytes = hasher.finish().to_be_bytes().to_vec();
                KBucketKey::new(random_bytes)
            }
        }
    }

    /// Create the appropriate Kademlia message for this query
    pub fn create_message(&self, requester: KadPeerId) -> KadMessage {
        match self {
            QueryType::FindNode { target } => KadMessage::FindNode {
                target: target.clone(),
                requester,
            },
            QueryType::FindValue { key } => KadMessage::FindValue {
                key: key.clone(),
                requester,
            },
            QueryType::PutValue { record } => KadMessage::PutValue {
                record: record.clone(),
                requester,
            },
            QueryType::GetProviders { key } => KadMessage::GetProviders {
                key: key.clone(),
                requester,
            },
            QueryType::Bootstrap => KadMessage::FindNode {
                target: KadPeerId::new(self.target_key().as_bytes().to_vec()),
                requester,
            },
        }
    }
}

/// State of an individual peer within a query
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerState {
    /// Peer has not been contacted yet
    NotContacted,
    /// Request sent, waiting for response
    Waiting { sent_at: Instant },
    /// Peer responded successfully
    Succeeded { response_time: Duration },
    /// Peer failed to respond or returned error
    Failed { attempts: u32, last_error: Option<String> },
    /// Peer is not reachable
    Unreachable,
}

impl PeerState {
    /// Check if this peer state represents completion (success or failure)
    pub fn is_complete(&self) -> bool {
        matches!(self, PeerState::Succeeded { .. } | PeerState::Failed { .. } | PeerState::Unreachable)
    }

    /// Check if this peer has failed
    pub fn is_failed(&self) -> bool {
        matches!(self, PeerState::Failed { .. } | PeerState::Unreachable)
    }

    /// Check if this peer is currently waiting for a response
    pub fn is_waiting(&self) -> bool {
        matches!(self, PeerState::Waiting { .. })
    }
}

/// Information about a peer tracked during a query
#[derive(Clone, Debug)]
pub struct QueryPeer {
    /// Peer information
    pub peer: PeerInfo,
    /// Current state of this peer in the query
    pub state: PeerState,
    /// Distance to the query target
    pub distance: KadDistance,
}

impl QueryPeer {
    pub fn new(peer: PeerInfo, target: &KBucketKey) -> Self {
        let peer_key = KBucketKey::from_peer_id(&peer.peer_id);
        let distance = peer_key.distance(target);
        
        Self {
            peer,
            state: PeerState::NotContacted,
            distance,
        }
    }

    /// Mark this peer as contacted
    pub fn mark_waiting(&mut self) {
        self.state = PeerState::Waiting { sent_at: Instant::now() };
    }

    /// Mark this peer as succeeded
    pub fn mark_succeeded(&mut self, response_time: Duration) {
        self.state = PeerState::Succeeded { response_time };
    }

    /// Mark this peer as failed
    pub fn mark_failed(&mut self, error: Option<String>) {
        let attempts = match &self.state {
            PeerState::Failed { attempts, .. } => attempts + 1,
            _ => 1,
        };
        
        self.state = PeerState::Failed {
            attempts,
            last_error: error,
        };
    }

    /// Check if this peer can be retried
    pub fn can_retry(&self, max_retries: u32) -> bool {
        match &self.state {
            PeerState::Failed { attempts, .. } => *attempts < max_retries,
            _ => false,
        }
    }
}

/// State of a Kademlia query
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryState {
    /// Query is waiting to start
    Waiting,
    /// Query is actively running
    Running,
    /// Query completed successfully
    Succeeded,
    /// Query failed
    Failed,
    /// Query timed out
    TimedOut,
}

/// A single Kademlia query tracking its progress
#[derive(Clone, Debug)]
pub struct Query {
    /// Unique identifier for this query
    pub id: QueryId,
    /// Type of query being performed
    pub query_type: QueryType,
    /// Target key for the query
    pub target: KBucketKey,
    /// Current state of the query
    pub state: QueryState,
    /// Configuration for this query
    pub config: QueryConfig,
    /// All peers involved in this query
    pub peers: HashMap<KadPeerId, QueryPeer>,
    /// Peers that have been contacted and responded
    pub contacted_peers: HashSet<KadPeerId>,
    /// Queue of peers to contact next
    pub peer_queue: VecDeque<KadPeerId>,
    /// Number of concurrent requests currently active
    pub active_requests: usize,
    /// When this query was started
    pub started_at: Instant,
    /// When this query finished (if completed)
    pub finished_at: Option<Instant>,
    /// Result of the query (if completed)
    pub result: Option<QueryResult>,
    /// Any errors encountered during the query
    pub errors: Vec<KadError>,
}

impl Query {
    /// Create a new query
    pub fn new(query_type: QueryType, config: QueryConfig) -> Self {
        let target = query_type.target_key();
        
        Self {
            id: QueryId::new(),
            query_type,
            target,
            state: QueryState::Waiting,
            config,
            peers: HashMap::new(),
            contacted_peers: HashSet::new(),
            peer_queue: VecDeque::new(),
            active_requests: 0,
            started_at: Instant::now(),
            finished_at: None,
            result: None,
            errors: Vec::new(),
        }
    }

    /// Start the query with initial peers
    pub fn start(&mut self, initial_peers: Vec<PeerInfo>) {
        self.state = QueryState::Running;
        self.started_at = Instant::now();
        
        // Add initial peers to the query
        for peer in initial_peers {
            self.add_peer(peer);
        }
        
        // Sort peers by distance and populate the queue
        self.update_peer_queue();
    }

    /// Add a peer to this query
    pub fn add_peer(&mut self, peer: PeerInfo) {
        let peer_id = peer.peer_id.clone();
        
        // Don't add the same peer twice
        if self.peers.contains_key(&peer_id) {
            return;
        }
        
        let query_peer = QueryPeer::new(peer, &self.target);
        self.peers.insert(peer_id.clone(), query_peer);
        self.peer_queue.push_back(peer_id);
    }

    /// Add multiple peers to this query
    pub fn add_peers(&mut self, peers: Vec<PeerInfo>) {
        for peer in peers {
            self.add_peer(peer);
        }
        self.update_peer_queue();
    }

    /// Get the next peers to contact (up to alpha concurrent requests)
    pub fn next_peers_to_contact(&mut self) -> Vec<KadPeerId> {
        let mut peers_to_contact = Vec::new();
        let slots_available = self.config.alpha.saturating_sub(self.active_requests);
        
        for _ in 0..slots_available {
            if let Some(peer_id) = self.find_next_peer_to_contact() {
                peers_to_contact.push(peer_id);
                self.active_requests += 1;
                
                // Mark peer as waiting
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.mark_waiting();
                }
            } else {
                break;
            }
        }
        
        peers_to_contact
    }

    /// Find the next best peer to contact
    fn find_next_peer_to_contact(&mut self) -> Option<KadPeerId> {
        // First try peers in the queue (closest first)
        while let Some(peer_id) = self.peer_queue.pop_front() {
            if let Some(peer) = self.peers.get(&peer_id) {
                match &peer.state {
                    PeerState::NotContacted => return Some(peer_id),
                    PeerState::Failed { .. } if peer.can_retry(self.config.max_retries) => {
                        return Some(peer_id);
                    }
                    _ => continue,
                }
            }
        }
        
        None
    }

    /// Handle a successful response from a peer
    pub fn handle_response(&mut self, peer_id: &KadPeerId, response: KadResponse) {
        self.active_requests = self.active_requests.saturating_sub(1);
        self.contacted_peers.insert(peer_id.clone());
        
        if let Some(peer) = self.peers.get_mut(peer_id) {
            let response_time = match &peer.state {
                PeerState::Waiting { sent_at } => sent_at.elapsed(),
                _ => Duration::from_secs(0),
            };
            peer.mark_succeeded(response_time);
        }
        
        // Process the response based on query type
        match (&self.query_type, response) {
            (QueryType::FindNode { .. }, KadResponse::Nodes { closer_peers, .. }) |
            (QueryType::Bootstrap, KadResponse::Nodes { closer_peers, .. }) => {
                self.add_peers(closer_peers);
            }
            
            (QueryType::FindValue { key }, KadResponse::Value { record, closer_peers, .. }) => {
                if let Some(found_record) = record {
                    // Found the value! Complete the query
                    self.complete_with_result(QueryResult::GetRecord {
                        key: key.clone(),
                        record: Some(found_record),
                        closest_peers: self.get_closest_successful_peers(),
                    });
                    return;
                } else {
                    // Value not found, continue with closer peers
                    self.add_peers(closer_peers);
                }
            }
            
            (QueryType::GetProviders { key }, KadResponse::Providers { providers, closer_peers, .. }) => {
                if !providers.is_empty() {
                    // Found providers! Complete the query
                    self.complete_with_result(QueryResult::GetProviders {
                        key: key.clone(),
                        providers,
                        closest_peers: self.get_closest_successful_peers(),
                    });
                    return;
                } else {
                    // No providers found yet, continue with closer peers
                    self.add_peers(closer_peers);
                }
            }
            
            (QueryType::PutValue { record }, KadResponse::Ack { .. }) => {
                // Successful store, continue to replicate to more peers
                // TODO: Track replication count
            }
            
            _ => {
                // Unexpected response type
                self.errors.push(KadError::InvalidMessage(
                    "Unexpected response type for query".to_string()
                ));
            }
        }
        
        // Update the peer queue with new peers
        self.update_peer_queue();
        
        // Check if query should complete
        self.check_completion();
    }

    /// Handle a failed request to a peer
    pub fn handle_error(&mut self, peer_id: &KadPeerId, error: KadError) {
        self.active_requests = self.active_requests.saturating_sub(1);
        
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.mark_failed(Some(error.to_string()));
        }
        
        self.errors.push(error);
        
        // Check if query should complete or continue
        self.check_completion();
    }

    /// Update the peer queue, sorting by distance to target
    fn update_peer_queue(&mut self) {
        // Collect all uncontacted or retryable peers
        let mut candidates: Vec<_> = self.peers
            .iter()
            .filter_map(|(peer_id, peer)| {
                match &peer.state {
                    PeerState::NotContacted => Some((peer_id.clone(), peer.distance.clone())),
                    PeerState::Failed { .. } if peer.can_retry(self.config.max_retries) => {
                        Some((peer_id.clone(), peer.distance.clone()))
                    }
                    _ => None,
                }
            })
            .collect();
        
        // Sort by distance (closest first)
        candidates.sort_by(|(_, dist_a), (_, dist_b)| dist_a.cmp(dist_b));
        
        // Rebuild the queue
        self.peer_queue.clear();
        for (peer_id, _) in candidates {
            self.peer_queue.push_back(peer_id);
        }
    }

    /// Check if the query should complete
    fn check_completion(&mut self) {
        // Check timeout
        if self.started_at.elapsed() > self.config.query_timeout {
            self.complete_with_error(KadError::Timeout {
                duration: self.config.query_timeout,
            });
            return;
        }
        
        // Check if we have enough successful responses
        let successful_peers = self.get_closest_successful_peers();
        
        match &self.query_type {
            QueryType::FindNode { target } => {
                if successful_peers.len() >= self.config.min_peers && self.active_requests == 0 {
                    self.complete_with_result(QueryResult::GetClosestPeers {
                        target: target.clone(),
                        peers: successful_peers,
                    });
                }
            }
            
            QueryType::Bootstrap => {
                if successful_peers.len() >= self.config.min_peers && self.active_requests == 0 {
                    self.complete_with_result(QueryResult::Bootstrap {
                        peers_contacted: self.contacted_peers.len() as u32,
                        buckets_refreshed: 1, // TODO: Calculate actual bucket refresh count
                    });
                }
            }
            
            QueryType::PutValue { record } => {
                let successful_stores = self.peers.values()
                    .filter(|p| matches!(p.state, PeerState::Succeeded { .. }))
                    .count();
                
                if successful_stores >= self.config.replication_factor.min(self.config.min_peers) {
                    self.complete_with_result(QueryResult::PutRecord {
                        key: record.key.clone(),
                        success: true,
                        replicas: successful_stores as u32,
                    });
                }
            }
            
            _ => {
                // For FindValue and GetProviders, completion is handled in handle_response
                // when the value/providers are found
            }
        }
        
        // Check if we've exhausted all peers without finding what we need
        if self.active_requests == 0 && self.peer_queue.is_empty() {
            match &self.query_type {
                QueryType::FindValue { key } => {
                    self.complete_with_result(QueryResult::GetRecord {
                        key: key.clone(),
                        record: None,
                        closest_peers: successful_peers,
                    });
                }
                QueryType::GetProviders { key } => {
                    self.complete_with_result(QueryResult::GetProviders {
                        key: key.clone(),
                        providers: Vec::new(),
                        closest_peers: successful_peers,
                    });
                }
                QueryType::PutValue { record } => {
                    let successful_stores = self.peers.values()
                        .filter(|p| matches!(p.state, PeerState::Succeeded { .. }))
                        .count();
                    
                    self.complete_with_result(QueryResult::PutRecord {
                        key: record.key.clone(),
                        success: successful_stores > 0,
                        replicas: successful_stores as u32,
                    });
                }
                _ => {
                    // FindNode and Bootstrap were already handled above
                }
            }
        }
    }

    /// Complete the query with a successful result
    fn complete_with_result(&mut self, result: QueryResult) {
        self.state = QueryState::Succeeded;
        self.finished_at = Some(Instant::now());
        self.result = Some(result);
    }

    /// Complete the query with an error
    fn complete_with_error(&mut self, error: KadError) {
        self.state = match error {
            KadError::Timeout { .. } => QueryState::TimedOut,
            _ => QueryState::Failed,
        };
        self.finished_at = Some(Instant::now());
        self.errors.push(error);
    }

    /// Get the closest peers that responded successfully
    fn get_closest_successful_peers(&self) -> Vec<PeerInfo> {
        let mut successful: Vec<_> = self.peers
            .values()
            .filter(|peer| matches!(peer.state, PeerState::Succeeded { .. }))
            .collect();
        
        successful.sort_by(|a, b| a.distance.cmp(&b.distance));
        
        successful.into_iter()
            .take(self.config.min_peers)
            .map(|peer| peer.peer.clone())
            .collect()
    }

    /// Check if the query is finished
    pub fn is_finished(&self) -> bool {
        !matches!(self.state, QueryState::Waiting | QueryState::Running)
    }

    /// Get the duration of this query
    pub fn duration(&self) -> Duration {
        match self.finished_at {
            Some(finished) => finished.duration_since(self.started_at),
            None => self.started_at.elapsed(),
        }
    }
}

/// Pool for managing multiple concurrent queries
#[derive(Debug)]
pub struct QueryPool {
    /// Active queries indexed by ID
    queries: HashMap<QueryId, Query>,
    /// Default configuration for new queries
    config: QueryConfig,
    /// Maximum number of concurrent queries
    max_concurrent: usize,
}

impl QueryPool {
    /// Create a new query pool
    pub fn new(config: QueryConfig, max_concurrent: usize) -> Self {
        Self {
            queries: HashMap::new(),
            config,
            max_concurrent,
        }
    }

    /// Add a new query to the pool
    pub fn add_query(&mut self, query_type: QueryType) -> Result<QueryId, KadError> {
        if self.queries.len() >= self.max_concurrent {
            return Err(KadError::QueryFailed {
                reason: "Too many concurrent queries".to_string(),
            });
        }
        
        let query = Query::new(query_type, self.config.clone());
        let query_id = query.id;
        self.queries.insert(query_id, query);
        
        Ok(query_id)
    }

    /// Get a query by ID
    pub fn get_query(&self, query_id: &QueryId) -> Option<&Query> {
        self.queries.get(query_id)
    }

    /// Get a mutable reference to a query by ID
    pub fn get_query_mut(&mut self, query_id: &QueryId) -> Option<&mut Query> {
        self.queries.get_mut(query_id)
    }

    /// Remove a completed query from the pool
    pub fn remove_query(&mut self, query_id: &QueryId) -> Option<Query> {
        self.queries.remove(query_id)
    }

    /// Get all active queries
    pub fn active_queries(&self) -> impl Iterator<Item = &Query> {
        self.queries.values()
    }

    /// Get all active query IDs
    pub fn active_query_ids(&self) -> impl Iterator<Item = &QueryId> {
        self.queries.keys()
    }

    /// Remove finished queries and return their results
    pub fn collect_finished(&mut self) -> Vec<(QueryId, Query)> {
        let mut finished = Vec::new();
        let mut to_remove = Vec::new();
        
        for (query_id, query) in &self.queries {
            if query.is_finished() {
                to_remove.push(*query_id);
            }
        }
        
        for query_id in to_remove {
            if let Some(query) = self.queries.remove(&query_id) {
                finished.push((query_id, query));
            }
        }
        
        finished
    }

    /// Get the number of active queries
    pub fn len(&self) -> usize {
        self.queries.len()
    }

    /// Check if the pool is empty
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::kad::transport::{KadAddress, ConnectionStatus};

    fn create_test_peer(id: u8) -> PeerInfo {
        PeerInfo {
            peer_id: KadPeerId::new(vec![id]),
            addresses: vec![KadAddress::new("tcp".to_string(), format!("127.0.0.1:{}", 4000 + id))],
            connection_status: ConnectionStatus::Unknown,
            last_seen: None,
        }
    }

    #[test]
    fn test_query_creation() {
        let target = KadPeerId::new(vec![42]);
        let query_type = QueryType::FindNode { target: target.clone() };
        let config = QueryConfig::default();
        
        let query = Query::new(query_type, config);
        
        assert_eq!(query.state, QueryState::Waiting);
        assert_eq!(query.active_requests, 0);
        assert!(query.peers.is_empty());
    }

    #[test]
    fn test_query_start_with_peers() {
        let target = KadPeerId::new(vec![42]);
        let query_type = QueryType::FindNode { target };
        let config = QueryConfig::default();
        
        let mut query = Query::new(query_type, config);
        let initial_peers = vec![create_test_peer(1), create_test_peer(2), create_test_peer(3)];
        
        query.start(initial_peers);
        
        assert_eq!(query.state, QueryState::Running);
        assert_eq!(query.peers.len(), 3);
        assert_eq!(query.peer_queue.len(), 3);
    }

    #[test]
    fn test_query_next_peers_to_contact() {
        let target = KadPeerId::new(vec![42]);
        let query_type = QueryType::FindNode { target };
        let mut config = QueryConfig::default();
        config.alpha = 2; // Only contact 2 peers concurrently
        
        let mut query = Query::new(query_type, config);
        let initial_peers = vec![
            create_test_peer(1), 
            create_test_peer(2), 
            create_test_peer(3), 
            create_test_peer(4)
        ];
        
        query.start(initial_peers);
        
        let peers_to_contact = query.next_peers_to_contact();
        assert_eq!(peers_to_contact.len(), 2); // Should respect alpha limit
        assert_eq!(query.active_requests, 2);
    }

    #[test]
    fn test_query_handle_response() {
        let target = KadPeerId::new(vec![42]);
        let query_type = QueryType::FindNode { target: target.clone() };
        let config = QueryConfig::default();
        
        let mut query = Query::new(query_type, config);
        let initial_peers = vec![create_test_peer(1)];
        query.start(initial_peers);
        
        let peers_to_contact = query.next_peers_to_contact();
        let responding_peer = &peers_to_contact[0];
        
        // Simulate response with additional peers
        let response = KadResponse::Nodes {
            closer_peers: vec![create_test_peer(2), create_test_peer(3)],
            requester: target,
        };
        
        query.handle_response(responding_peer, response);
        
        // Should have added new peers and marked the responder as succeeded
        assert_eq!(query.peers.len(), 3);
        assert!(query.contacted_peers.contains(responding_peer));
        assert_eq!(query.active_requests, 0);
        
        let responder_state = &query.peers[responding_peer].state;
        assert!(matches!(responder_state, PeerState::Succeeded { .. }));
    }

    #[test]
    fn test_query_handle_error() {
        let target = KadPeerId::new(vec![42]);
        let query_type = QueryType::FindNode { target };
        let config = QueryConfig::default();
        
        let mut query = Query::new(query_type, config);
        let initial_peers = vec![create_test_peer(1)];
        query.start(initial_peers);
        
        let peers_to_contact = query.next_peers_to_contact();
        let failing_peer = &peers_to_contact[0];
        
        let error = KadError::PeerUnreachable {
            peer: failing_peer.to_string(),
        };
        
        query.handle_error(failing_peer, error);
        
        // Should have marked the peer as failed and decremented active requests
        assert_eq!(query.active_requests, 0);
        assert_eq!(query.errors.len(), 1);
        
        let failed_peer_state = &query.peers[failing_peer].state;
        assert!(matches!(failed_peer_state, PeerState::Failed { .. }));
    }

    #[test]
    fn test_query_pool() {
        let config = QueryConfig::default();
        let mut pool = QueryPool::new(config, 10);
        
        let target = KadPeerId::new(vec![42]);
        let query_type = QueryType::FindNode { target };
        
        let query_id = pool.add_query(query_type).unwrap();
        assert_eq!(pool.len(), 1);
        
        assert!(pool.get_query(&query_id).is_some());
        
        let removed = pool.remove_query(&query_id);
        assert!(removed.is_some());
        assert_eq!(pool.len(), 0);
    }
}