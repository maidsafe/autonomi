// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Kademlia DHT behavior wrapper for ant-net.
//!
//! This module provides an ant-net wrapper around the Kademlia DHT behavior,
//! enabling distributed hash table operations within the ant-net abstraction layer.

use crate::{
    behavior_manager::{BehaviorAction, BehaviorController, BehaviorHealth, StateRequest, StateResponse},
    event::NetworkEvent,
    types::PeerId,
    AntNetError, Result,
};
use ant_kad::{
    store::MemoryStore,
    Behaviour, Event as KademliaEvent,
    GetRecordOk, PutRecordOk, QueryResult, Record, RecordKey,
};
use async_trait::async_trait;
use bytes::Bytes;
use libp2p::Multiaddr;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

/// Configuration for the Kademlia DHT behavior.
#[derive(Debug, Clone)]
pub struct KademliaBehaviorConfig {
    /// Replication factor (K value).
    pub replication_factor: usize,
    /// Query timeout duration.
    pub query_timeout: Duration,
    /// Record TTL (time to live).
    pub record_ttl: Duration,
    /// Maximum number of concurrent queries.
    pub max_concurrent_queries: usize,
    /// Record storage limit.
    pub max_records: usize,
    /// Routing table refresh interval.
    pub routing_table_refresh_interval: Duration,
    /// Provider record TTL.
    pub provider_record_ttl: Duration,
}

impl Default for KademliaBehaviorConfig {
    fn default() -> Self {
        Self {
            replication_factor: 20,
            query_timeout: Duration::from_secs(60),
            record_ttl: Duration::from_secs(3600), // 1 hour
            max_concurrent_queries: 50,
            max_records: 10000,
            routing_table_refresh_interval: Duration::from_secs(300), // 5 minutes
            provider_record_ttl: Duration::from_secs(1800), // 30 minutes
        }
    }
}

/// Query information for tracking ongoing queries.
#[derive(Debug, Clone)]
pub struct QueryInfo {
    /// The query ID.
    pub query_id: String,
    /// The query type.
    pub query_type: QueryType,
    /// When the query was started.
    pub started_at: Instant,
    /// Query timeout.
    pub timeout: Duration,
    /// Target key for the query.
    pub target_key: Option<RecordKey>,
    /// Peers involved in the query.
    pub peers: HashSet<PeerId>,
}

impl QueryInfo {
    /// Check if this query has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed() >= self.timeout
    }

    /// Get remaining time until timeout.
    pub fn remaining_time(&self) -> Duration {
        self.timeout.saturating_sub(self.started_at.elapsed())
    }
}

/// Types of Kademlia queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryType {
    /// Get record query.
    GetRecord,
    /// Put record query.
    PutRecord,
    /// Get closest peers query.
    GetClosestPeers,
    /// Bootstrap query.
    Bootstrap,
    /// Get providers query.
    GetProviders,
    /// Add provider query.
    AddProvider,
}

impl fmt::Display for QueryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryType::GetRecord => write!(f, "get_record"),
            QueryType::PutRecord => write!(f, "put_record"),
            QueryType::GetClosestPeers => write!(f, "get_closest_peers"),
            QueryType::Bootstrap => write!(f, "bootstrap"),
            QueryType::GetProviders => write!(f, "get_providers"),
            QueryType::AddProvider => write!(f, "add_provider"),
        }
    }
}

/// Statistics for the Kademlia behavior.
#[derive(Debug, Clone, Default)]
pub struct KademliaStats {
    /// Number of records stored.
    pub records_stored: u64,
    /// Number of records retrieved.
    pub records_retrieved: u64,
    /// Number of successful queries.
    pub successful_queries: u64,
    /// Number of failed queries.
    pub failed_queries: u64,
    /// Number of timeouts.
    pub query_timeouts: u64,
    /// Current number of active queries.
    pub active_queries: usize,
    /// Current routing table size.
    pub routing_table_size: usize,
    /// Number of bootstrap operations.
    pub bootstrap_count: u64,
    /// Average query latency.
    pub avg_query_latency: Duration,
}

/// ant-net wrapper for Kademlia DHT behavior.
pub struct KademliaBehaviorWrapper {
    /// The inner Kademlia behavior.
    #[allow(dead_code)]
    inner: Behaviour<MemoryStore>,
    /// Behavior configuration.
    config: KademliaBehaviorConfig,
    /// Active queries tracking.
    active_queries: HashMap<String, QueryInfo>,
    /// Query queue for rate limiting.
    #[allow(dead_code)]
    query_queue: VecDeque<(QueryType, RecordKey, Option<Record>)>,
    /// Local records cache.
    local_records: HashMap<RecordKey, Record>,
    /// Statistics.
    stats: KademliaStats,
    /// Whether the behavior is active.
    active: bool,
    /// Last maintenance time.
    last_maintenance: Instant,
    /// Bootstrap peers.
    bootstrap_peers: Vec<PeerId>,
}

impl KademliaBehaviorWrapper {
    /// Create a new Kademlia behavior wrapper.
    pub fn new(kademlia: Behaviour<MemoryStore>, config: KademliaBehaviorConfig) -> Self {
        // Configure the inner Kademlia behavior
        // Note: In a real implementation, we'd need proper configuration here
        
        Self {
            inner: kademlia,
            config,
            active_queries: HashMap::new(),
            query_queue: VecDeque::new(),
            local_records: HashMap::new(),
            stats: KademliaStats::default(),
            active: true,
            last_maintenance: Instant::now(),
            bootstrap_peers: Vec::new(),
        }
    }

    /// Create a new Kademlia behavior with default configuration.
    pub fn with_default_config(kademlia: Behaviour<MemoryStore>) -> Self {
        Self::new(kademlia, KademliaBehaviorConfig::default())
    }

    /// Add a bootstrap peer.
    pub fn add_bootstrap_peer(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.bootstrap_peers.push(peer_id);
        debug!("Added bootstrap peer: {} at {}", peer_id, address);
    }

    /// Start a bootstrap process.
    pub async fn bootstrap(&mut self) -> Result<String> {
        if !self.active {
            return Err(AntNetError::Behavior("Behavior is not active".to_string()));
        }

        let query_id = format!("bootstrap_{}", uuid::Uuid::new_v4());
        let query_info = QueryInfo {
            query_id: query_id.clone(),
            query_type: QueryType::Bootstrap,
            started_at: Instant::now(),
            timeout: self.config.query_timeout,
            target_key: None,
            peers: HashSet::new(),
        };

        self.active_queries.insert(query_id.clone(), query_info);
        self.stats.active_queries = self.active_queries.len();
        self.stats.bootstrap_count += 1;

        debug!("Started bootstrap query: {}", query_id);
        // In a real implementation, this would trigger the Kademlia bootstrap
        
        Ok(query_id)
    }

    /// Store a record in the DHT.
    pub async fn put_record(&mut self, key: RecordKey, value: Bytes) -> Result<String> {
        if !self.active {
            return Err(AntNetError::Behavior("Behavior is not active".to_string()));
        }

        if self.active_queries.len() >= self.config.max_concurrent_queries {
            return Err(AntNetError::Protocol(
                "Maximum concurrent queries exceeded".to_string()
            ));
        }

        let query_id = format!("put_record_{}", uuid::Uuid::new_v4());
        let record = Record {
            key: key.clone(),
            value: value.to_vec(),
            publisher: None,
            expires: Some(Instant::now() + self.config.record_ttl),
        };

        let query_info = QueryInfo {
            query_id: query_id.clone(),
            query_type: QueryType::PutRecord,
            started_at: Instant::now(),
            timeout: self.config.query_timeout,
            target_key: Some(key.clone()),
            peers: HashSet::new(),
        };

        self.active_queries.insert(query_id.clone(), query_info);
        self.local_records.insert(key, record);
        self.stats.active_queries = self.active_queries.len();

        debug!("Started put_record query: {}", query_id);
        // In a real implementation, this would trigger the Kademlia put_record
        
        Ok(query_id)
    }

    /// Retrieve a record from the DHT.
    pub async fn get_record(&mut self, key: RecordKey) -> Result<String> {
        if !self.active {
            return Err(AntNetError::Behavior("Behavior is not active".to_string()));
        }

        if self.active_queries.len() >= self.config.max_concurrent_queries {
            return Err(AntNetError::Protocol(
                "Maximum concurrent queries exceeded".to_string()
            ));
        }

        // Check local cache first
        if let Some(record) = self.local_records.get(&key) {
            if let Some(expires) = record.expires {
                if Instant::now() < expires {
                    self.stats.records_retrieved += 1;
                    debug!("Found record in local cache: {:?}", key);
                    // Return immediately for cached records
                    return Ok("local_cache".to_string());
                }
            }
        }

        let query_id = format!("get_record_{}", uuid::Uuid::new_v4());
        let query_info = QueryInfo {
            query_id: query_id.clone(),
            query_type: QueryType::GetRecord,
            started_at: Instant::now(),
            timeout: self.config.query_timeout,
            target_key: Some(key),
            peers: HashSet::new(),
        };

        self.active_queries.insert(query_id.clone(), query_info);
        self.stats.active_queries = self.active_queries.len();

        debug!("Started get_record query: {}", query_id);
        // In a real implementation, this would trigger the Kademlia get_record
        
        Ok(query_id)
    }

    /// Get closest peers to a key.
    pub async fn get_closest_peers(&mut self, key: RecordKey) -> Result<String> {
        if !self.active {
            return Err(AntNetError::Behavior("Behavior is not active".to_string()));
        }

        if self.active_queries.len() >= self.config.max_concurrent_queries {
            return Err(AntNetError::Protocol(
                "Maximum concurrent queries exceeded".to_string()
            ));
        }

        let query_id = format!("get_closest_peers_{}", uuid::Uuid::new_v4());
        let query_info = QueryInfo {
            query_id: query_id.clone(),
            query_type: QueryType::GetClosestPeers,
            started_at: Instant::now(),
            timeout: self.config.query_timeout,
            target_key: Some(key),
            peers: HashSet::new(),
        };

        self.active_queries.insert(query_id.clone(), query_info);
        self.stats.active_queries = self.active_queries.len();

        debug!("Started get_closest_peers query: {}", query_id);
        // In a real implementation, this would trigger the Kademlia get_closest_peers
        
        Ok(query_id)
    }

    /// Handle a Kademlia event from the inner behavior.
    #[allow(dead_code)]
    fn handle_kademlia_event(&mut self, event: KademliaEvent) -> Option<NetworkEvent> {
        match event {
            KademliaEvent::OutboundQueryProgressed { id, result, stats: _, step: _ } => {
                let query_id = id.to_string();
                self.handle_query_result(query_id, result)
            }
            KademliaEvent::RoutingUpdated { peer, .. } => {
                debug!("Routing table updated with peer: {}", peer);
                Some(NetworkEvent::BehaviorEvent {
                    peer_id: PeerId::from_bytes(&peer.to_bytes()).unwrap_or(PeerId::random()),
                    behavior_id: "kademlia".to_string(),
                    event: "RoutingUpdated".to_string(),
                })
            }
            KademliaEvent::UnroutablePeer { peer } => {
                warn!("Unroutable peer: {}", peer);
                Some(NetworkEvent::BehaviorEvent {
                    peer_id: PeerId::from_bytes(&peer.to_bytes()).unwrap_or(PeerId::random()),
                    behavior_id: "kademlia".to_string(),
                    event: "UnroutablePeer".to_string(),
                })
            }
            KademliaEvent::RoutablePeer { peer, address } => {
                debug!("Routable peer: {} at {}", peer, address);
                Some(NetworkEvent::BehaviorEvent {
                    peer_id: PeerId::from_bytes(&peer.to_bytes()).unwrap_or(PeerId::random()),
                    behavior_id: "kademlia".to_string(),
                    event: "RoutablePeer".to_string(),
                })
            }
            KademliaEvent::PendingRoutablePeer { peer, address } => {
                debug!("Pending routable peer: {} at {}", peer, address);
                None // Less important event
            }
            KademliaEvent::InboundRequest { request: _ } => {
                // Handle inbound requests
                None
            }
            KademliaEvent::ModeChanged { .. } => {
                debug!("Kademlia mode changed");
                None
            }
        }
    }

    /// Handle a query result.
    #[allow(dead_code)]
    fn handle_query_result(&mut self, query_id: String, result: QueryResult) -> Option<NetworkEvent> {
        if let Some(query_info) = self.active_queries.remove(&query_id) {
            let latency = query_info.started_at.elapsed();
            self.stats.active_queries = self.active_queries.len();

            // Update average latency
            let total_queries = self.stats.successful_queries + self.stats.failed_queries + 1;
            self.stats.avg_query_latency = (self.stats.avg_query_latency * (total_queries - 1) as u32 + latency) / total_queries as u32;

            match result {
                QueryResult::GetRecord(get_result) => {
                    match get_result {
                        Ok(GetRecordOk::FoundRecord(peer_record)) => {
                            self.stats.successful_queries += 1;
                            self.stats.records_retrieved += 1;

                            // Cache retrieved record
                            let record = peer_record.record;
                            self.local_records.insert(record.key.clone(), record.clone());

                            info!("Retrieved record for query {}", query_id);
                            Some(NetworkEvent::KademliaRecordFound {
                                key: query_info.target_key.map(|k| k.to_vec().into()).unwrap_or_default(),
                                data: record.value.into(),
                                peer_id: peer_record.peer.and_then(|p| PeerId::from_bytes(&p.to_bytes()).ok()),
                            })
                        }
                        Ok(GetRecordOk::FinishedWithNoAdditionalRecord { .. }) => {
                            self.stats.successful_queries += 1;
                            info!("Query {} finished with no additional records", query_id);
                            None
                        }
                        Err(e) => {
                            self.stats.failed_queries += 1;
                            warn!("Get record query {} failed: {:?}", query_id, e);
                            Some(NetworkEvent::KademliaQueryResult {
                                query_id,
                                result: crate::event::KademliaQueryResult::Error {
                                    error: format!("Get record failed: {:?}", e),
                                    key: query_info.target_key.map(|k| k.to_vec().into()),
                                },
                            })
                        }
                    }
                }
                QueryResult::PutRecord(put_result) => {
                    match put_result {
                        Ok(PutRecordOk { key }) => {
                            self.stats.successful_queries += 1;
                            self.stats.records_stored += 1;
                            info!("Successfully stored record for query {}", query_id);
                            Some(NetworkEvent::KademliaRecordStored {
                                key: key.to_vec().into(),
                                peer_id: PeerId::random(), // Would be the actual peer in real implementation
                            })
                        }
                        Err(e) => {
                            self.stats.failed_queries += 1;
                            warn!("Put record query {} failed: {:?}", query_id, e);
                            Some(NetworkEvent::KademliaQueryResult {
                                query_id,
                                result: crate::event::KademliaQueryResult::Error {
                                    error: format!("Put record failed: {:?}", e),
                                    key: query_info.target_key.map(|k| k.to_vec().into()),
                                },
                            })
                        }
                    }
                }
                QueryResult::GetClosestPeers(closest_result) => {
                    match closest_result {
                        Ok(closest_peers_result) => {
                            self.stats.successful_queries += 1;
                            let peer_ids: Vec<_> = closest_peers_result.peers.into_iter()
                                .filter_map(|p| PeerId::from_bytes(&p.peer_id.to_bytes()).ok())
                                .collect();
                            
                            info!("Found {} closest peers for query {}", peer_ids.len(), query_id);
                            Some(NetworkEvent::KademliaQueryResult {
                                query_id,
                                result: crate::event::KademliaQueryResult::GetClosestPeers {
                                    key: query_info.target_key.map(|k| k.to_vec().into()).unwrap_or_default(),
                                    peers: peer_ids,
                                },
                            })
                        }
                        Err(e) => {
                            self.stats.failed_queries += 1;
                            warn!("Get closest peers query {} failed: {:?}", query_id, e);
                            Some(NetworkEvent::KademliaQueryResult {
                                query_id,
                                result: crate::event::KademliaQueryResult::Error {
                                    error: format!("Get closest peers failed: {:?}", e),
                                    key: query_info.target_key.map(|k| k.to_vec().into()),
                                },
                            })
                        }
                    }
                }
                QueryResult::Bootstrap(bootstrap_result) => {
                    match bootstrap_result {
                        Ok(_) => {
                            self.stats.successful_queries += 1;
                            info!("Bootstrap query {} completed successfully", query_id);
                            Some(NetworkEvent::BehaviorEvent {
                                peer_id: PeerId::random(),
                                behavior_id: "kademlia".to_string(),
                                event: "BootstrapComplete".to_string(),
                            })
                        }
                        Err(e) => {
                            self.stats.failed_queries += 1;
                            warn!("Bootstrap query {} failed: {:?}", query_id, e);
                            Some(NetworkEvent::BehaviorEvent {
                                peer_id: PeerId::random(),
                                behavior_id: "kademlia".to_string(),
                                event: format!("BootstrapFailed: {:?}", e),
                            })
                        }
                    }
                }
                _ => {
                    debug!("Unhandled query result type for query {}", query_id);
                    None
                }
            }
        } else {
            warn!("Received result for unknown query: {}", query_id);
            None
        }
    }

    /// Cleanup timed out queries.
    pub fn cleanup_timed_out_queries(&mut self) {
        let mut timed_out_queries = Vec::new();

        for (query_id, query_info) in &self.active_queries {
            if query_info.is_timed_out() {
                timed_out_queries.push(query_id.clone());
            }
        }

        for query_id in timed_out_queries {
            if let Some(_query_info) = self.active_queries.remove(&query_id) {
                self.stats.query_timeouts += 1;
                self.stats.failed_queries += 1;
                self.stats.active_queries = self.active_queries.len();
                warn!("Query {} timed out", query_id);
            }
        }
    }

    /// Perform periodic maintenance.
    pub fn periodic_maintenance(&mut self) {
        if self.last_maintenance.elapsed() >= Duration::from_secs(60) {
            self.cleanup_timed_out_queries();
            self.cleanup_expired_records();
            self.last_maintenance = Instant::now();
            debug!("Performed periodic maintenance");
        }
    }

    /// Cleanup expired records from local cache.
    fn cleanup_expired_records(&mut self) {
        let now = Instant::now();
        let initial_count = self.local_records.len();
        
        self.local_records.retain(|_, record| {
            if let Some(expires) = record.expires {
                now < expires
            } else {
                true // Keep records without expiration
            }
        });

        let removed_count = initial_count - self.local_records.len();
        if removed_count > 0 {
            debug!("Cleaned up {} expired records", removed_count);
        }
    }

    /// Get behavior statistics.
    pub fn stats(&self) -> &KademliaStats {
        &self.stats
    }

    /// Get active query info.
    pub fn get_query_info(&self, query_id: &str) -> Option<&QueryInfo> {
        self.active_queries.get(query_id)
    }

    /// Get all active queries.
    pub fn get_active_queries(&self) -> &HashMap<String, QueryInfo> {
        &self.active_queries
    }

    /// Cancel a query.
    pub fn cancel_query(&mut self, query_id: &str) -> bool {
        if self.active_queries.remove(query_id).is_some() {
            self.stats.active_queries = self.active_queries.len();
            debug!("Cancelled query: {}", query_id);
            true
        } else {
            false
        }
    }

    /// Get local record count.
    pub fn local_record_count(&self) -> usize {
        self.local_records.len()
    }

    /// Check if a record exists locally.
    pub fn has_local_record(&self, key: &RecordKey) -> bool {
        self.local_records.contains_key(key)
    }
}

impl fmt::Debug for KademliaBehaviorWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KademliaBehaviorWrapper")
            .field("config", &self.config)
            .field("active_queries", &self.active_queries.len())
            .field("local_records", &self.local_records.len())
            .field("stats", &self.stats)
            .field("active", &self.active)
            .finish()
    }
}

#[async_trait]
impl BehaviorController for KademliaBehaviorWrapper {
    fn id(&self) -> String {
        "kademlia".to_string()
    }

    fn name(&self) -> &'static str {
        "kademlia"
    }

    async fn start(&mut self) -> Result<()> {
        self.active = true;
        info!("Started Kademlia behavior");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.active = false;
        
        // Cancel all active queries
        let query_ids: Vec<_> = self.active_queries.keys().cloned().collect();
        for query_id in query_ids {
            self.cancel_query(&query_id);
        }

        info!("Stopped Kademlia behavior");
        Ok(())
    }

    async fn health_check(&self) -> BehaviorHealth {
        if !self.active {
            return BehaviorHealth::Unhealthy("Behavior is inactive".to_string());
        }

        let active_query_count = self.active_queries.len();
        let max_queries = self.config.max_concurrent_queries;

        if active_query_count > max_queries * 9 / 10 {
            BehaviorHealth::Degraded(format!(
                "High query load: {}/{}", 
                active_query_count, 
                max_queries
            ))
        } else if self.stats.failed_queries > 0 && 
                 self.stats.failed_queries * 100 / (self.stats.successful_queries + self.stats.failed_queries + 1) > 50 {
            BehaviorHealth::Degraded(format!(
                "High failure rate: {}/{} queries failed",
                self.stats.failed_queries,
                self.stats.successful_queries + self.stats.failed_queries
            ))
        } else if self.local_records.len() > self.config.max_records * 9 / 10 {
            BehaviorHealth::Degraded(format!(
                "High record storage: {}/{}", 
                self.local_records.len(), 
                self.config.max_records
            ))
        } else {
            BehaviorHealth::Healthy
        }
    }

    async fn handle_event(&mut self, event: NetworkEvent) -> Result<Vec<BehaviorAction>> {
        if !self.active {
            return Ok(Vec::new());
        }

        // Perform periodic maintenance
        self.periodic_maintenance();

        let actions = Vec::new();

        match event {
            NetworkEvent::PeerConnected { peer_id, .. } => {
                // Could add peer to routing table
                debug!("Peer connected, could add to routing table: {}", peer_id);
            }
            NetworkEvent::PeerDisconnected { peer_id, .. } => {
                // Could remove peer from routing table
                debug!("Peer disconnected, could remove from routing table: {}", peer_id);
            }
            _ => {
                // Not interested in other events for basic Kademlia
            }
        }

        Ok(actions)
    }

    async fn handle_state_request(&mut self, request: StateRequest) -> StateResponse {
        match request {
            StateRequest::GetClosestPeers { target: _, count } => {
                // In a real implementation, we'd query the routing table
                let peers: Vec<_> = (0..count.min(self.config.replication_factor))
                    .map(|_| crate::types::PeerInfo {
                        peer_id: PeerId::random(),
                        addresses: crate::types::Addresses::new(),
                    })
                    .collect();
                
                StateResponse::ClosestPeers(peers)
            }
            StateRequest::GetRoutingTableStatus => {
                StateResponse::RoutingTableStatus {
                    peer_count: self.stats.routing_table_size,
                    bucket_count: 256, // Typical Kademlia bucket count
                }
            }
            StateRequest::Custom { request_type, data: _ } => {
                match request_type.as_str() {
                    "get_kademlia_stats" => {
                        StateResponse::Custom {
                            response_type: "kademlia_stats".to_string(),
                            data: format!(
                                "Records: {}, Queries: {}/{}, Active: {}",
                                self.local_records.len(),
                                self.stats.successful_queries,
                                self.stats.failed_queries,
                                self.stats.active_queries
                            ).into(),
                        }
                    }
                    "get_record_count" => {
                        StateResponse::Custom {
                            response_type: "record_count".to_string(),
                            data: self.local_records.len().to_string().into(),
                        }
                    }
                    _ => StateResponse::Error("Unknown request type".to_string()),
                }
            }
            _ => StateResponse::Error("Unsupported state request".to_string()),
        }
    }

    fn is_interested(&self, event: &NetworkEvent) -> bool {
        matches!(event,
            NetworkEvent::PeerConnected { .. } |
            NetworkEvent::PeerDisconnected { .. } |
            NetworkEvent::KademliaQueryResult { .. } |
            NetworkEvent::KademliaRecordStored { .. } |
            NetworkEvent::KademliaRecordFound { .. }
        )
    }

    fn config_keys(&self) -> Vec<String> {
        vec![
            "replication_factor".to_string(),
            "query_timeout".to_string(),
            "record_ttl".to_string(),
            "max_concurrent_queries".to_string(),
            "max_records".to_string(),
            "routing_table_refresh_interval".to_string(),
        ]
    }

    async fn update_config(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "replication_factor" => {
                let factor: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid replication_factor value".to_string()))?;
                self.config.replication_factor = factor;
                info!("Updated replication_factor to {}", factor);
            }
            "query_timeout" => {
                let timeout: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid timeout value".to_string()))?;
                self.config.query_timeout = Duration::from_secs(timeout);
                info!("Updated query_timeout to {} seconds", timeout);
            }
            "record_ttl" => {
                let ttl: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid TTL value".to_string()))?;
                self.config.record_ttl = Duration::from_secs(ttl);
                info!("Updated record_ttl to {} seconds", ttl);
            }
            "max_concurrent_queries" => {
                let max_queries: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid max_queries value".to_string()))?;
                self.config.max_concurrent_queries = max_queries;
                info!("Updated max_concurrent_queries to {}", max_queries);
            }
            "max_records" => {
                let max_records: usize = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid max_records value".to_string()))?;
                self.config.max_records = max_records;
                info!("Updated max_records to {}", max_records);
            }
            "routing_table_refresh_interval" => {
                let interval: u64 = value.parse()
                    .map_err(|_| AntNetError::Configuration("Invalid interval value".to_string()))?;
                self.config.routing_table_refresh_interval = Duration::from_secs(interval);
                info!("Updated routing_table_refresh_interval to {} seconds", interval);
            }
            _ => {
                return Err(AntNetError::Configuration(format!(
                    "Unknown configuration key: {}", key
                )));
            }
        }
        Ok(())
    }

    fn clone_controller(&self) -> Box<dyn BehaviorController> {
        // Note: We can't actually clone the Kademlia behavior, so this is a placeholder
        panic!("KademliaBehaviorWrapper cannot be cloned due to internal state constraints")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_kad::{Behaviour, Config as KademliaConfig, store::MemoryStore};
    use libp2p::PeerId as Libp2pPeerId;

    fn create_test_kademlia() -> Behaviour<MemoryStore> {
        let peer_id = Libp2pPeerId::random();
        let store = MemoryStore::new(peer_id);
        let config = KademliaConfig::default();
        Behaviour::with_config(peer_id, store, config)
    }

    #[tokio::test]
    async fn test_kademlia_behavior_lifecycle() {
        let kademlia = create_test_kademlia();
        let config = KademliaBehaviorConfig::default();
        let mut behavior = KademliaBehaviorWrapper::new(kademlia, config);

        // Test basic properties
        assert_eq!(behavior.id(), "kademlia");
        assert_eq!(behavior.name(), "kademlia");
        assert!(matches!(behavior.health_check().await, BehaviorHealth::Healthy));

        // Test lifecycle
        behavior.start().await.unwrap();
        assert!(behavior.active);

        behavior.stop().await.unwrap();
        assert!(!behavior.active);
    }

    #[tokio::test]
    async fn test_kademlia_record_operations() {
        let kademlia = create_test_kademlia();
        let mut behavior = KademliaBehaviorWrapper::with_default_config(kademlia);

        let key = RecordKey::new(&b"test_key");
        let value = Bytes::from("test_value");

        // Test put record
        let query_id = behavior.put_record(key.clone(), value.clone()).await.unwrap();
        assert!(!query_id.is_empty());
        assert_eq!(behavior.stats().active_queries, 1);

        // Test get record (should find in local cache)
        let get_query_id = behavior.get_record(key.clone()).await.unwrap();
        assert_eq!(get_query_id, "local_cache"); // Found in cache
        assert_eq!(behavior.stats().records_retrieved, 1);

        // Verify local record exists
        assert!(behavior.has_local_record(&key));
        assert_eq!(behavior.local_record_count(), 1);
    }

    #[tokio::test]
    async fn test_kademlia_bootstrap() {
        let kademlia = create_test_kademlia();
        let mut behavior = KademliaBehaviorWrapper::with_default_config(kademlia);

        let bootstrap_peer = PeerId::random();
        let bootstrap_addr = "/ip4/127.0.0.1/tcp/12345".parse().unwrap();
        
        behavior.add_bootstrap_peer(bootstrap_peer, bootstrap_addr);
        assert_eq!(behavior.bootstrap_peers.len(), 1);

        let query_id = behavior.bootstrap().await.unwrap();
        assert!(!query_id.is_empty());
        assert_eq!(behavior.stats().bootstrap_count, 1);
        assert_eq!(behavior.stats().active_queries, 1);
    }

    #[tokio::test]
    async fn test_kademlia_query_limits() {
        let kademlia = create_test_kademlia();
        let config = KademliaBehaviorConfig {
            max_concurrent_queries: 2,
            ..Default::default()
        };
        let mut behavior = KademliaBehaviorWrapper::new(kademlia, config);

        let key1 = RecordKey::new(&b"key1");
        let key2 = RecordKey::new(&b"key2");
        let key3 = RecordKey::new(&b"key3");

        // First two queries should succeed
        behavior.get_record(key1).await.unwrap();
        behavior.get_record(key2).await.unwrap();
        assert_eq!(behavior.active_queries.len(), 2);

        // Third query should fail due to limit
        assert!(behavior.get_record(key3).await.is_err());
        assert_eq!(behavior.active_queries.len(), 2);
    }

    #[tokio::test]
    async fn test_kademlia_query_timeout_cleanup() {
        let kademlia = create_test_kademlia();
        let config = KademliaBehaviorConfig {
            query_timeout: Duration::from_millis(10), // Very short timeout
            ..Default::default()
        };
        let mut behavior = KademliaBehaviorWrapper::new(kademlia, config);

        let key = RecordKey::new(&b"test_key");
        behavior.get_record(key).await.unwrap();
        assert_eq!(behavior.active_queries.len(), 1);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Cleanup should remove timed out query
        behavior.cleanup_timed_out_queries();
        assert_eq!(behavior.active_queries.len(), 0);
        assert_eq!(behavior.stats().query_timeouts, 1);
    }

    #[tokio::test]
    async fn test_kademlia_configuration() {
        let kademlia = create_test_kademlia();
        let mut behavior = KademliaBehaviorWrapper::with_default_config(kademlia);

        // Test configuration updates
        assert!(behavior.config_keys().contains(&"replication_factor".to_string()));
        
        behavior.update_config("replication_factor", "15").await.unwrap();
        assert_eq!(behavior.config.replication_factor, 15);

        behavior.update_config("max_records", "5000").await.unwrap();
        assert_eq!(behavior.config.max_records, 5000);

        // Test invalid configuration
        assert!(behavior.update_config("invalid_key", "value").await.is_err());
        assert!(behavior.update_config("replication_factor", "invalid").await.is_err());
    }

    #[tokio::test]
    async fn test_kademlia_state_requests() {
        let kademlia = create_test_kademlia();
        let mut behavior = KademliaBehaviorWrapper::with_default_config(kademlia);

        // Test routing table status request
        let routing_request = StateRequest::GetRoutingTableStatus;
        let response = behavior.handle_state_request(routing_request).await;
        match response {
            StateResponse::RoutingTableStatus { peer_count, bucket_count } => {
                assert_eq!(peer_count, 0); // Empty routing table
                assert_eq!(bucket_count, 256);
            }
            _ => panic!("Expected RoutingTableStatus response"),
        }

        // Test custom stats request
        let stats_request = StateRequest::Custom {
            request_type: "get_kademlia_stats".to_string(),
            data: Bytes::new(),
        };
        let response = behavior.handle_state_request(stats_request).await;
        match response {
            StateResponse::Custom { response_type, .. } => {
                assert_eq!(response_type, "kademlia_stats");
            }
            _ => panic!("Expected Custom response"),
        }
    }
}