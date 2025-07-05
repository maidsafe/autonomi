// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Core Kademlia DHT behavior implementation.
//! 
//! This module provides the main Kademlia behavior that orchestrates routing table
//! management, query processing, and record storage.

#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{
    sync::{mpsc, oneshot, RwLock},
    task::JoinHandle,
    time::interval,
};
use tracing::{debug, info, trace, warn};

use crate::networking::kad::{
    transport::{
        KademliaTransport, KadPeerId,
        KadEvent, KadError, KadConfig, KadStats, QueryId, RecordKey, Record, PeerInfo,
        QueryResult, RoutingAction,
    },
    kbucket::{KBucket, KBucketEntry, KBucketKey, KBucketConfig},
    query::{QueryPool, QueryType, QueryConfig},
    record_store::{RecordStore, MemoryRecordStore, RecordStoreConfig},
};


#[cfg(test)]
use crate::networking::kad::transport::{KadAddress, KadMessage, KadResponse};

#[cfg(test)]
use crate::networking::kad::query::Query;

/// Commands that can be sent to the Kademlia behavior
#[derive(Debug)]
pub enum KadCommand {
    /// Start a FindNode query
    FindNode {
        target: KadPeerId,
        response_tx: oneshot::Sender<Result<Vec<PeerInfo>, KadError>>,
    },
    /// Start a FindValue query
    FindValue {
        key: RecordKey,
        response_tx: oneshot::Sender<Result<Option<Record>, KadError>>,
    },
    /// Store a record
    PutRecord {
        record: Record,
        response_tx: oneshot::Sender<Result<(), KadError>>,
    },
    /// Get providers for a key
    GetProviders {
        key: RecordKey,
        response_tx: oneshot::Sender<Result<Vec<PeerInfo>, KadError>>,
    },
    /// Bootstrap the routing table
    Bootstrap {
        response_tx: oneshot::Sender<Result<(), KadError>>,
    },
    /// Add a peer to the routing table
    AddPeer {
        peer: PeerInfo,
    },
    /// Remove a peer from the routing table
    RemovePeer {
        peer_id: KadPeerId,
    },
    /// Get routing table information
    GetRoutingTable {
        response_tx: oneshot::Sender<RoutingTableInfo>,
    },
    /// Get statistics
    GetStats {
        response_tx: oneshot::Sender<KadStats>,
    },
    /// Shutdown the behavior
    Shutdown,
}

/// Information about the current routing table state
#[derive(Debug, Clone)]
pub struct RoutingTableInfo {
    /// Number of peers in each bucket
    pub bucket_sizes: Vec<usize>,
    /// Total number of peers
    pub total_peers: usize,
    /// Local peer ID
    pub local_peer_id: KadPeerId,
    /// Closest peers to local ID
    pub closest_peers: Vec<PeerInfo>,
}

/// Main Kademlia DHT behavior
pub struct Kademlia<T, S> 
where
    T: KademliaTransport,
    S: RecordStore,
{
    /// Transport layer
    transport: Arc<T>,
    /// Local peer ID
    local_peer_id: KadPeerId,
    /// Local key for distance calculations
    local_key: KBucketKey,
    /// Routing table (k-buckets)
    routing_table: Vec<KBucket>,
    /// Query pool for managing active queries
    query_pool: QueryPool,
    /// Record store for local data
    record_store: Arc<RwLock<S>>,
    /// Configuration
    config: KadConfig,
    /// K-bucket configuration
    bucket_config: KBucketConfig,
    /// Statistics
    stats: KadStats,
    /// Command receiver
    command_rx: mpsc::UnboundedReceiver<KadCommand>,
    /// Command sender (for cloning)
    command_tx: mpsc::UnboundedSender<KadCommand>,
    /// Event sender
    event_tx: mpsc::UnboundedSender<KadEvent>,
    /// Bootstrap peers
    bootstrap_peers: Vec<PeerInfo>,
    /// Whether the behavior is running
    running: bool,
    /// Background task handles
    tasks: Vec<JoinHandle<()>>,
    /// Last bootstrap time
    last_bootstrap: Option<Instant>,
    /// Known providers for keys
    providers: HashMap<RecordKey, HashSet<KadPeerId>>,
}

impl<T, S> Kademlia<T, S>
where
    T: KademliaTransport + 'static,
    S: RecordStore + 'static,
{
    /// Create a new Kademlia behavior
    pub fn new(
        transport: Arc<T>,
        record_store: Arc<RwLock<S>>,
        config: KadConfig,
    ) -> Result<Self, KadError> {
        let local_peer_id = transport.local_peer_id();
        let local_key = KBucketKey::from_peer_id(&local_peer_id);
        
        let bucket_config = KBucketConfig {
            k_value: config.k_value,
            peer_timeout: config.query_timeout,
            allow_replacement: true,
            replacement_cache_size: 5,
        };
        
        let query_config = QueryConfig {
            alpha: config.alpha,
            query_timeout: config.query_timeout,
            request_timeout: config.request_timeout,
            max_peers: 100,
            max_retries: 2,
            min_peers: config.replication_factor.min(config.k_value),
        };
        
        let query_pool = QueryPool::new(query_config, config.max_concurrent_queries);
        
        // Initialize routing table with empty buckets
        let max_buckets = 256; // For 256-bit key space
        let mut routing_table = Vec::with_capacity(max_buckets);
        for _ in 0..max_buckets {
            routing_table.push(KBucket::new(bucket_config.clone()));
        }
        
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, _) = mpsc::unbounded_channel();
        
        Ok(Self {
            transport,
            local_peer_id,
            local_key,
            routing_table,
            query_pool,
            record_store,
            config,
            bucket_config,
            stats: KadStats::default(),
            command_rx,
            command_tx,
            event_tx,
            bootstrap_peers: Vec::new(),
            running: false,
            tasks: Vec::new(),
            last_bootstrap: None,
            providers: HashMap::new(),
        })
    }

    /// Get a handle to send commands to this Kademlia instance
    pub fn handle(&self) -> KademliaHandle {
        KademliaHandle {
            command_tx: self.command_tx.clone(),
        }
    }

    /// Add bootstrap peers
    pub fn add_bootstrap_peers(&mut self, peers: Vec<PeerInfo>) {
        self.bootstrap_peers.extend(peers);
    }

    /// Start the Kademlia behavior
    pub async fn start(&mut self) -> Result<(), KadError> {
        if self.running {
            return Ok(());
        }

        self.running = true;
        info!("Starting Kademlia DHT with local peer ID: {}", self.local_peer_id);

        // Start background tasks
        self.start_background_tasks().await?;

        // Initial bootstrap if we have bootstrap peers
        if !self.bootstrap_peers.is_empty() {
            info!("Performing initial bootstrap with {} peers", self.bootstrap_peers.len());
            self.perform_bootstrap().await?;
        }

        Ok(())
    }

    /// Stop the Kademlia behavior
    pub async fn stop(&mut self) {
        if !self.running {
            return;
        }

        info!("Stopping Kademlia DHT");
        self.running = false;

        // Cancel background tasks
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }

    /// Main event loop
    pub async fn run(mut self) -> Result<(), KadError> {
        self.start().await?;

        while self.running {
            tokio::select! {
                // Handle commands
                Some(command) = self.command_rx.recv() => {
                    self.handle_command(command).await;
                }
                
                // Process query timeouts and completion
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    self.process_queries().await;
                }
                
                else => {
                    debug!("Event loop shutting down");
                    break;
                }
            }
        }

        self.stop().await;
        Ok(())
    }

    /// Start background tasks
    async fn start_background_tasks(&mut self) -> Result<(), KadError> {
        // Periodic bootstrap task
        if let Some(interval) = self.config.bootstrap_interval {
            let handle = self.handle();
            let task = tokio::spawn(async move {
                let mut timer = tokio::time::interval(interval);
                loop {
                    timer.tick().await;
                    let (tx, _rx) = oneshot::channel();
                    let _ = handle.bootstrap(tx).await;
                }
            });
            self.tasks.push(task);
        }

        // Record store cleanup task
        let record_store = self.record_store.clone();
        let task = tokio::spawn(async move {
            let mut timer = interval(Duration::from_secs(300)); // 5 minutes
            loop {
                timer.tick().await;
                if let Ok(mut store) = record_store.try_write() {
                    if let Ok(removed) = store.cleanup().await {
                        if removed > 0 {
                            debug!("Cleaned up {} expired records", removed);
                        }
                    }
                }
            }
        });
        self.tasks.push(task);

        Ok(())
    }

    /// Handle incoming commands
    async fn handle_command(&mut self, command: KadCommand) {
        match command {
            KadCommand::FindNode { target, response_tx } => {
                let result = self.find_node(target).await;
                let _ = response_tx.send(result);
            }

            KadCommand::FindValue { key, response_tx } => {
                let result = self.find_value(key).await;
                let _ = response_tx.send(result);
            }

            KadCommand::PutRecord { record, response_tx } => {
                let result = self.put_record(record).await;
                let _ = response_tx.send(result);
            }

            KadCommand::GetProviders { key, response_tx } => {
                let result = self.get_providers(key).await;
                let _ = response_tx.send(result);
            }

            KadCommand::Bootstrap { response_tx } => {
                let result = self.perform_bootstrap().await;
                let _ = response_tx.send(result);
            }

            KadCommand::AddPeer { peer } => {
                self.add_peer_to_routing_table(peer).await;
            }

            KadCommand::RemovePeer { peer_id } => {
                self.remove_peer_from_routing_table(&peer_id).await;
            }

            KadCommand::GetRoutingTable { response_tx } => {
                let info = self.get_routing_table_info();
                let _ = response_tx.send(info);
            }

            KadCommand::GetStats { response_tx } => {
                let _ = response_tx.send(self.stats.clone());
            }

            KadCommand::Shutdown => {
                self.running = false;
            }
        }
    }

    /// Perform a FindNode query
    async fn find_node(&mut self, target: KadPeerId) -> Result<Vec<PeerInfo>, KadError> {
        debug!("Starting FindNode query for target: {}", target);
        
        let query_type = QueryType::FindNode { target: target.clone() };
        let query_id = self.query_pool.add_query(query_type)?;
        
        // Get initial peers from routing table
        let target_key = KBucketKey::from_peer_id(&target);
        let initial_peers = self.get_closest_peers(&target_key, self.config.alpha);
        
        if let Some(query) = self.query_pool.get_query_mut(&query_id) {
            query.start(initial_peers);
        }

        self.stats.queries_initiated += 1;
        
        // Process the query
        match self.process_single_query(query_id).await? {
            QueryResult::GetClosestPeers { peers, .. } => Ok(peers),
            _ => Err(KadError::QueryFailed { 
                reason: "Unexpected query result type for find_node".to_string() 
            }),
        }
    }

    /// Perform a FindValue query
    async fn find_value(&mut self, key: RecordKey) -> Result<Option<Record>, KadError> {
        debug!("Starting FindValue query for key: {:?}", key);
        
        // First check local store
        {
            let store = self.record_store.read().await;
            if let Ok(Some(record)) = store.get(&key).await {
                debug!("Found record locally");
                return Ok(Some(record));
            }
        }
        
        let query_type = QueryType::FindValue { key: key.clone() };
        let query_id = self.query_pool.add_query(query_type)?;
        
        // Get initial peers from routing table
        let target_key = KBucketKey::from_peer_id(&key.to_kad_peer_id());
        let initial_peers = self.get_closest_peers(&target_key, self.config.alpha);
        
        if let Some(query) = self.query_pool.get_query_mut(&query_id) {
            query.start(initial_peers);
        }

        self.stats.queries_initiated += 1;
        
        // Process the query and extract the record
        match self.process_single_query(query_id).await {
            Ok(QueryResult::GetRecord { record, .. }) => Ok(record),
            Ok(_) => Err(KadError::QueryFailed { 
                reason: "Unexpected query result type".to_string() 
            }),
            Err(e) => Err(e),
        }
    }

    /// Store a record in the DHT
    async fn put_record(&mut self, record: Record) -> Result<(), KadError> {
        debug!("Starting PutRecord for key: {:?}", record.key);
        
        // Store locally first
        {
            let mut store = self.record_store.write().await;
            store.put(record.clone()).await
                .map_err(|e| KadError::Storage(e.to_string()))?;
        }
        
        let query_type = QueryType::PutValue { record: record.clone() };
        let query_id = self.query_pool.add_query(query_type)?;
        
        // Get closest peers for replication
        let target_key = KBucketKey::from_peer_id(&record.key.to_kad_peer_id());
        let initial_peers = self.get_closest_peers(&target_key, self.config.replication_factor);
        
        if let Some(query) = self.query_pool.get_query_mut(&query_id) {
            query.start(initial_peers);
        }

        self.stats.queries_initiated += 1;
        
        // Process the query
        match self.process_single_query(query_id).await {
            Ok(QueryResult::PutRecord { success, .. }) => {
                if success {
                    Ok(())
                } else {
                    Err(KadError::QueryFailed { 
                        reason: "Failed to replicate record".to_string() 
                    })
                }
            }
            Ok(_) => Err(KadError::QueryFailed { 
                reason: "Unexpected query result type".to_string() 
            }),
            Err(e) => Err(e),
        }
    }

    /// Get providers for a key
    async fn get_providers(&mut self, key: RecordKey) -> Result<Vec<PeerInfo>, KadError> {
        debug!("Starting GetProviders query for key: {:?}", key);
        
        // Check local providers first
        if let Some(local_providers) = self.providers.get(&key) {
            let mut providers = Vec::new();
            for provider_id in local_providers {
                if let Some(peer_info) = self.get_peer_info(provider_id) {
                    providers.push(peer_info);
                }
            }
            if !providers.is_empty() {
                return Ok(providers);
            }
        }
        
        let query_type = QueryType::GetProviders { key: key.clone() };
        let query_id = self.query_pool.add_query(query_type)?;
        
        let target_key = KBucketKey::from_peer_id(&key.to_kad_peer_id());
        let initial_peers = self.get_closest_peers(&target_key, self.config.alpha);
        
        if let Some(query) = self.query_pool.get_query_mut(&query_id) {
            query.start(initial_peers);
        }

        self.stats.queries_initiated += 1;
        
        match self.process_single_query(query_id).await {
            Ok(QueryResult::GetProviders { providers, .. }) => Ok(providers),
            Ok(_) => Err(KadError::QueryFailed { 
                reason: "Unexpected query result type".to_string() 
            }),
            Err(e) => Err(e),
        }
    }

    /// Perform bootstrap operation
    async fn perform_bootstrap(&mut self) -> Result<(), KadError> {
        debug!("Starting bootstrap operation");
        
        if self.bootstrap_peers.is_empty() {
            return Err(KadError::NoPeers);
        }
        
        // Add bootstrap peers to routing table
        let bootstrap_peers = self.bootstrap_peers.clone();
        for peer in &bootstrap_peers {
            self.add_peer_to_routing_table(peer.clone()).await;
        }
        
        let query_type = QueryType::Bootstrap;
        let query_id = self.query_pool.add_query(query_type)?;
        
        let initial_peers = self.bootstrap_peers.clone();
        
        if let Some(query) = self.query_pool.get_query_mut(&query_id) {
            query.start(initial_peers);
        }

        self.stats.queries_initiated += 1;
        self.last_bootstrap = Some(Instant::now());
        
        match self.process_single_query(query_id).await {
            Ok(QueryResult::Bootstrap { .. }) => {
                info!("Bootstrap completed successfully");
                Ok(())
            }
            Ok(_) => Err(KadError::QueryFailed { 
                reason: "Unexpected query result type".to_string() 
            }),
            Err(e) => {
                warn!("Bootstrap failed: {}", e);
                Err(e)
            }
        }
    }

    /// Process queries until completion
    async fn process_queries(&mut self) {
        let finished_queries = self.query_pool.collect_finished();
        
        for (query_id, query) in finished_queries {
            match query.state {
                crate::networking::kad::query::QueryState::Succeeded => {
                    self.stats.queries_completed += 1;
                    if let Some(result) = &query.result {
                        self.emit_event(KadEvent::QueryCompleted {
                            query_id,
                            result: result.clone(),
                            duration: query.duration(),
                        });
                    }
                }
                _ => {
                    self.stats.queries_failed += 1;
                    let error = query.errors.first()
                        .cloned()
                        .unwrap_or_else(|| KadError::QueryFailed { 
                            reason: "Unknown error".to_string() 
                        });
                    
                    self.emit_event(KadEvent::QueryFailed {
                        query_id,
                        error,
                        duration: query.duration(),
                    });
                }
            }
        }
        
        // Process active queries
        let active_query_ids: Vec<_> = self.query_pool.active_query_ids().cloned().collect();
        
        for query_id in active_query_ids {
            self.process_query_step(query_id).await;
        }
    }

    /// Process a single step of a query
    async fn process_query_step(&mut self, query_id: QueryId) {
        let peers_to_contact = if let Some(query) = self.query_pool.get_query_mut(&query_id) {
            query.next_peers_to_contact()
        } else {
            return;
        };

        for peer_id in peers_to_contact {
            if let Some(query) = self.query_pool.get_query(&query_id) {
                let message = query.query_type.create_message(self.local_peer_id.clone());
                
                match self.transport.send_request(&peer_id, message).await {
                    Ok(response) => {
                        if let Some(query) = self.query_pool.get_query_mut(&query_id) {
                            query.handle_response(&peer_id, response);
                        }
                    }
                    Err(e) => {
                        let kad_error = KadError::Transport(e.to_string());
                        if let Some(query) = self.query_pool.get_query_mut(&query_id) {
                            query.handle_error(&peer_id, kad_error);
                        }
                    }
                }
            }
        }
    }

    /// Process a single query to completion
    async fn process_single_query(&mut self, query_id: QueryId) -> Result<QueryResult, KadError> {
        let timeout = self.config.query_timeout;
        let start_time = Instant::now();
        
        loop {
            if start_time.elapsed() > timeout {
                self.query_pool.remove_query(&query_id);
                return Err(KadError::Timeout { duration: timeout });
            }
            
            self.process_query_step(query_id).await;
            
            if let Some(query) = self.query_pool.get_query(&query_id) {
                if query.is_finished() {
                    let completed_query = self.query_pool.remove_query(&query_id).unwrap();
                    
                    match completed_query.result {
                        Some(result) => return Ok(result),
                        None => {
                            let error = completed_query.errors.first()
                                .cloned()
                                .unwrap_or_else(|| KadError::QueryFailed { 
                                    reason: "Query completed without result".to_string() 
                                });
                            return Err(error);
                        }
                    }
                }
            }
            
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Add a peer to the routing table
    async fn add_peer_to_routing_table(&mut self, peer: PeerInfo) {
        let peer_key = KBucketKey::from_peer_id(&peer.peer_id);
        
        if let Some(bucket_index) = peer_key.bucket_index(&self.local_key) {
            let bucket_index = bucket_index as usize;
            
            if bucket_index < self.routing_table.len() {
                let entry = KBucketEntry::new(peer.peer_id.clone(), peer.addresses);
                let result = self.routing_table[bucket_index].insert(entry);
                
                match result {
                    crate::networking::kad::kbucket::InsertResult::Inserted => {
                        debug!("Added peer {} to bucket {}", peer.peer_id, bucket_index);
                        self.emit_event(KadEvent::RoutingUpdated {
                            peer: peer.peer_id,
                            action: RoutingAction::Added,
                            bucket: bucket_index as u32,
                        });
                    }
                    crate::networking::kad::kbucket::InsertResult::Updated => {
                        debug!("Updated peer {} in bucket {}", peer.peer_id, bucket_index);
                        self.emit_event(KadEvent::RoutingUpdated {
                            peer: peer.peer_id,
                            action: RoutingAction::Updated,
                            bucket: bucket_index as u32,
                        });
                    }
                    _ => {
                        // Bucket full or other reasons
                        trace!("Could not add peer {} to bucket {}", peer.peer_id, bucket_index);
                    }
                }
            }
        }
    }

    /// Remove a peer from the routing table
    async fn remove_peer_from_routing_table(&mut self, peer_id: &KadPeerId) {
        let peer_key = KBucketKey::from_peer_id(peer_id);
        
        if let Some(bucket_index) = peer_key.bucket_index(&self.local_key) {
            let bucket_index = bucket_index as usize;
            
            if bucket_index < self.routing_table.len() {
                if let Some(_removed) = self.routing_table[bucket_index].remove(peer_id) {
                    debug!("Removed peer {} from bucket {}", peer_id, bucket_index);
                    self.emit_event(KadEvent::RoutingUpdated {
                        peer: peer_id.clone(),
                        action: RoutingAction::Removed,
                        bucket: bucket_index as u32,
                    });
                }
            }
        }
    }

    /// Get the closest peers to a target key
    fn get_closest_peers(&self, target: &KBucketKey, count: usize) -> Vec<PeerInfo> {
        let mut all_peers = Vec::new();
        
        // Collect peers from all buckets
        for bucket in &self.routing_table {
            for entry in bucket.active_peers() {
                all_peers.push(PeerInfo {
                    peer_id: entry.peer_id.clone(),
                    addresses: entry.addresses.clone(),
                    connection_status: entry.status,
                    last_seen: entry.last_seen.elapsed().ok().map(|elapsed| Instant::now() - elapsed),
                });
            }
        }
        
        // Sort by distance to target
        all_peers.sort_by(|a, b| {
            let dist_a = KBucketKey::from_peer_id(&a.peer_id).distance(target);
            let dist_b = KBucketKey::from_peer_id(&b.peer_id).distance(target);
            dist_a.cmp(&dist_b)
        });
        
        all_peers.into_iter().take(count).collect()
    }

    /// Get peer info by ID
    fn get_peer_info(&self, peer_id: &KadPeerId) -> Option<PeerInfo> {
        let peer_key = KBucketKey::from_peer_id(peer_id);
        
        if let Some(bucket_index) = peer_key.bucket_index(&self.local_key) {
            let bucket_index = bucket_index as usize;
            
            if bucket_index < self.routing_table.len() {
                if let Some(entry) = self.routing_table[bucket_index].get(peer_id) {
                    return Some(PeerInfo {
                        peer_id: entry.peer_id.clone(),
                        addresses: entry.addresses.clone(),
                        connection_status: entry.status,
                        last_seen: entry.last_seen.elapsed().ok().map(|elapsed| Instant::now() - elapsed),
                    });
                }
            }
        }
        
        None
    }

    /// Get routing table information
    fn get_routing_table_info(&self) -> RoutingTableInfo {
        let bucket_sizes: Vec<usize> = self.routing_table.iter()
            .map(|bucket| bucket.len())
            .collect();
        
        let total_peers = bucket_sizes.iter().sum();
        
        let local_key = KBucketKey::from_peer_id(&self.local_peer_id);
        let closest_peers = self.get_closest_peers(&local_key, 20);
        
        RoutingTableInfo {
            bucket_sizes,
            total_peers,
            local_peer_id: self.local_peer_id.clone(),
            closest_peers,
        }
    }

    /// Emit an event
    fn emit_event(&self, event: KadEvent) {
        let _ = self.event_tx.send(event);
    }
}

/// Handle for sending commands to a Kademlia instance
#[derive(Clone, Debug)]
pub struct KademliaHandle {
    command_tx: mpsc::UnboundedSender<KadCommand>,
}

impl KademliaHandle {
    /// Find the closest peers to a target
    pub async fn find_node(&self, target: KadPeerId) -> Result<Vec<PeerInfo>, KadError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(KadCommand::FindNode { target, response_tx: tx })
            .map_err(|_| KadError::QueryFailed { reason: "Command channel closed".to_string() })?;
        
        rx.await.map_err(|_| KadError::QueryFailed { 
            reason: "Response channel closed".to_string() 
        })?
    }

    /// Find a value by key
    pub async fn find_value(&self, key: RecordKey) -> Result<Option<Record>, KadError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(KadCommand::FindValue { key, response_tx: tx })
            .map_err(|_| KadError::QueryFailed { reason: "Command channel closed".to_string() })?;
        
        rx.await.map_err(|_| KadError::QueryFailed { 
            reason: "Response channel closed".to_string() 
        })?
    }

    /// Store a record
    pub async fn put_record(&self, record: Record) -> Result<(), KadError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(KadCommand::PutRecord { record, response_tx: tx })
            .map_err(|_| KadError::QueryFailed { reason: "Command channel closed".to_string() })?;
        
        rx.await.map_err(|_| KadError::QueryFailed { 
            reason: "Response channel closed".to_string() 
        })?
    }

    /// Bootstrap the DHT
    pub async fn bootstrap(&self, _response_tx: oneshot::Sender<Result<(), KadError>>) -> Result<(), KadError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(KadCommand::Bootstrap { response_tx: tx })
            .map_err(|_| KadError::QueryFailed { reason: "Command channel closed".to_string() })?;
        
        rx.await.map_err(|_| KadError::QueryFailed { 
            reason: "Response channel closed".to_string() 
        })?
    }

    /// Add a peer
    pub async fn add_peer(&self, peer: PeerInfo) -> Result<(), KadError> {
        self.command_tx.send(KadCommand::AddPeer { peer })
            .map_err(|_| KadError::QueryFailed { reason: "Command channel closed".to_string() })?;
        Ok(())
    }

    /// Get statistics
    pub async fn get_stats(&self) -> Result<KadStats, KadError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send(KadCommand::GetStats { response_tx: tx })
            .map_err(|_| KadError::QueryFailed { reason: "Command channel closed".to_string() })?;
        
        rx.await.map_err(|_| KadError::QueryFailed { 
            reason: "Response channel closed".to_string() 
        })
    }
}

/// Convenience constructor for Kademlia with default record store
impl<T> Kademlia<T, MemoryRecordStore>
where
    T: KademliaTransport + 'static,
{
    /// Create a new Kademlia instance with memory record store
    pub fn with_memory_store(
        transport: Arc<T>,
        config: KadConfig,
    ) -> Result<Self, KadError> {
        let record_store_config = RecordStoreConfig {
            max_records: 1000,
            max_record_size: 1024 * 1024, // 1MB
            max_total_size: 100 * 1024 * 1024, // 100MB
            ..Default::default()
        };
        
        let record_store = Arc::new(RwLock::new(
            MemoryRecordStore::new(record_store_config)
        ));
        
        Self::new(transport, record_store, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::kad::transport::KadAddress;
    use std::collections::HashMap;
    use async_trait::async_trait;

    // Mock transport for testing
    #[derive(Debug)]
    struct MockTransport {
        local_peer_id: KadPeerId,
        peers: HashMap<KadPeerId, PeerInfo>,
    }

    impl MockTransport {
        fn new(id: u8) -> Self {
            Self {
                local_peer_id: KadPeerId::new(vec![id]),
                peers: HashMap::new(),
            }
        }
    }

    #[async_trait]
    impl KademliaTransport for MockTransport {
        type Error = KadError;

        fn local_peer_id(&self) -> KadPeerId {
            self.local_peer_id.clone()
        }

        fn listen_addresses(&self) -> Vec<KadAddress> {
            vec![KadAddress::new("mock".to_string(), "127.0.0.1:0".to_string())]
        }

        async fn send_request(&self, _peer: &KadPeerId, _message: KadMessage) -> Result<KadResponse, Self::Error> {
            // Mock successful response
            Ok(KadResponse::Nodes {
                closer_peers: vec![],
                requester: self.local_peer_id.clone(),
            })
        }

        async fn send_message(&self, _peer: &KadPeerId, _message: KadMessage) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn is_connected(&self, _peer: &KadPeerId) -> bool {
            true
        }

        async fn dial_peer(&self, _peer: &KadPeerId, _addresses: &[KadAddress]) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn add_peer_addresses(&self, _peer: &KadPeerId, _addresses: Vec<KadAddress>) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn remove_peer(&self, _peer: &KadPeerId) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn peer_info(&self, peer: &KadPeerId) -> Option<PeerInfo> {
            self.peers.get(peer).cloned()
        }

        async fn connected_peers(&self) -> Vec<KadPeerId> {
            self.peers.keys().cloned().collect()
        }

        async fn start_listening(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_kademlia_creation() {
        let transport = Arc::new(MockTransport::new(1));
        let config = KadConfig::default();
        
        let kademlia = Kademlia::with_memory_store(transport, config);
        assert!(kademlia.is_ok());
    }

    #[tokio::test]
    async fn test_kademlia_handle() {
        let transport = Arc::new(MockTransport::new(1));
        let config = KadConfig::default();
        
        let kademlia = Kademlia::with_memory_store(transport, config).unwrap();
        let handle = kademlia.handle();
        
        // Test that handle can be cloned
        let _handle2 = handle.clone();
    }
}