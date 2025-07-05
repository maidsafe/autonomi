// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Intelligent transport routing for dual-stack operations
//! 
//! This module implements sophisticated routing logic that selects the optimal
//! transport (libp2p or iroh) for each operation based on various factors
//! including peer capabilities, performance metrics, and load balancing.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::RwLock;
use tracing::{debug, info, instrument};

use crate::networking::kad::transport::KadPeerId;

use super::{
    TransportId, DualStackError, DualStackResult,
    config::{RoutingConfig, LoadBalancingStrategy},
    utils,
};

/// Transport selection result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportChoice {
    /// Use libp2p transport
    LibP2P,
    /// Use iroh transport  
    Iroh,
    /// Either transport is acceptable (let load balancer decide)
    Either,
}

impl From<TransportChoice> for TransportId {
    fn from(choice: TransportChoice) -> Self {
        match choice {
            TransportChoice::LibP2P => TransportId::LibP2P,
            TransportChoice::Iroh => TransportId::Iroh,
            TransportChoice::Either => TransportId::LibP2P, // Default fallback
        }
    }
}

/// Routing policies for transport selection
#[derive(Debug, Clone)]
pub enum RoutingPolicy {
    /// Always use specific transport
    Fixed(TransportId),
    /// Performance-based selection
    PerformanceBased,
    /// Load balancing between transports
    LoadBalanced,
    /// Peer capability-based selection
    CapabilityBased,
    /// Hybrid policy combining multiple factors
    Hybrid {
        performance_weight: f32,
        capability_weight: f32,
        load_weight: f32,
    },
}

/// Transport router for intelligent selection
pub struct TransportRouter {
    /// Configuration for routing behavior
    config: RoutingConfig,
    
    /// Performance metrics for transport selection
    performance_metrics: Arc<RwLock<PerformanceMetrics>>,
    
    /// Load balancing state
    load_balancer: Arc<RwLock<LoadBalancer>>,
    
    /// Peer capability cache
    peer_capabilities: Arc<RwLock<PeerCapabilityCache>>,
    
    /// Routing decision cache
    decision_cache: Arc<RwLock<DecisionCache>>,
}

/// Performance metrics for transport comparison
#[derive(Debug, Clone)]
struct PerformanceMetrics {
    /// Per-transport performance data
    transport_metrics: HashMap<TransportId, TransportPerformance>,
    /// Last update timestamp
    last_updated: Instant,
}

/// Performance data for a specific transport
#[derive(Debug, Clone)]
struct TransportPerformance {
    /// Average latency in milliseconds
    avg_latency_ms: f64,
    /// Success rate (0.0 to 1.0)
    success_rate: f64,
    /// Bandwidth utilization
    bandwidth_mbps: f64,
    /// Connection establishment time
    connection_time_ms: f64,
    /// Total operations processed
    total_operations: u64,
    /// Recent operation samples
    recent_samples: Vec<OperationSample>,
}

/// Sample from a recent operation
#[derive(Debug, Clone)]
struct OperationSample {
    timestamp: Instant,
    latency: Duration,
    success: bool,
    operation_type: String,
}

/// Load balancer state
#[derive(Debug)]
struct LoadBalancer {
    /// Current load per transport
    current_load: HashMap<TransportId, f32>,
    /// Round-robin counter
    round_robin_counter: u64,
    /// Weighted selection state
    weighted_state: WeightedState,
}

/// State for weighted load balancing
#[derive(Debug)]
struct WeightedState {
    libp2p_weight: f32,
    iroh_weight: f32,
    accumulated_weight: f32,
}

/// Peer capability cache
#[derive(Debug)]
struct PeerCapabilityCache {
    /// Known peer capabilities
    capabilities: HashMap<KadPeerId, PeerCapabilities>,
    /// Cache insertion times for TTL
    insertion_times: HashMap<KadPeerId, Instant>,
    /// Cache TTL
    ttl: Duration,
}

/// Capabilities of a specific peer
#[derive(Debug, Clone)]
struct PeerCapabilities {
    /// Supports libp2p protocol
    supports_libp2p: bool,
    /// Supports iroh protocol
    supports_iroh: bool,
    /// Last successful transport used
    last_successful_transport: Option<TransportId>,
    /// Preferred transport based on performance
    preferred_transport: Option<TransportId>,
    /// NAT traversal capability
    nat_traversal_capable: bool,
}

/// Routing decision cache for efficiency
#[derive(Debug)]
struct DecisionCache {
    /// Cached routing decisions
    decisions: HashMap<CacheKey, CachedDecision>,
    /// Cache insertion times
    insertion_times: HashMap<CacheKey, Instant>,
    /// Cache TTL
    ttl: Duration,
}

/// Cache key for routing decisions
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    peer_id: KadPeerId,
    operation_type: String,
}

/// Cached routing decision
#[derive(Debug, Clone)]
struct CachedDecision {
    transport: TransportId,
    confidence: f32,
    reason: String,
}

impl TransportRouter {
    /// Create a new transport router
    pub async fn new(config: RoutingConfig) -> DualStackResult<Self> {
        let performance_metrics = Arc::new(RwLock::new(PerformanceMetrics {
            transport_metrics: HashMap::new(),
            last_updated: Instant::now(),
        }));
        
        let load_balancer = Arc::new(RwLock::new(LoadBalancer {
            current_load: HashMap::new(),
            round_robin_counter: 0,
            weighted_state: WeightedState {
                libp2p_weight: 1.0,
                iroh_weight: 1.0,
                accumulated_weight: 0.0,
            },
        }));
        
        let peer_capabilities = Arc::new(RwLock::new(PeerCapabilityCache {
            capabilities: HashMap::new(),
            insertion_times: HashMap::new(),
            ttl: Duration::from_hours(1),
        }));
        
        let decision_cache = Arc::new(RwLock::new(DecisionCache {
            decisions: HashMap::new(),
            insertion_times: HashMap::new(),
            ttl: Duration::from_minutes(5),
        }));
        
        Ok(Self {
            config,
            performance_metrics,
            load_balancer,
            peer_capabilities,
            decision_cache,
        })
    }
    
    /// Select optimal transport for a peer and operation
    #[instrument(skip(self), fields(peer_id = %peer_id, operation = %operation_type))]
    pub async fn select_transport(
        &self,
        peer_id: &KadPeerId,
        available_transports: &[TransportId],
        operation_type: &str,
    ) -> DualStackResult<TransportId> {
        debug!("Selecting transport for peer {} operation {}", peer_id, operation_type);
        
        // Check cache first
        if let Some(cached) = self.get_cached_decision(peer_id, operation_type).await {
            debug!("Using cached decision: {:?} (confidence: {:.2})", 
                   cached.transport, cached.confidence);
            return Ok(cached.transport);
        }
        
        // Select based on configuration and policies
        let choice = match &self.config.load_balancing {
            LoadBalancingStrategy::RoundRobin => {
                self.round_robin_selection(available_transports).await
            },
            LoadBalancingStrategy::LeastLoaded => {
                self.least_loaded_selection(available_transports).await
            },
            LoadBalancingStrategy::PerformanceBased => {
                self.performance_based_selection(peer_id, available_transports, operation_type).await
            },
            LoadBalancingStrategy::Weighted { libp2p_weight, iroh_weight } => {
                self.weighted_selection(available_transports, *libp2p_weight, *iroh_weight).await
            },
            LoadBalancingStrategy::PreferredWithFallback { preferred } => {
                self.preferred_with_fallback_selection(available_transports, *preferred).await
            },
        };
        
        let selected_transport = choice?;
        
        // Cache the decision
        self.cache_decision(peer_id, operation_type, selected_transport, "strategy").await;
        
        debug!("Selected transport: {:?}", selected_transport);
        Ok(selected_transport)
    }
    
    /// Round-robin transport selection
    async fn round_robin_selection(&self, available_transports: &[TransportId]) -> DualStackResult<TransportId> {
        if available_transports.is_empty() {
            return Err(DualStackError::Routing("No available transports".to_string()));
        }
        
        let mut balancer = self.load_balancer.write().await;
        let index = (balancer.round_robin_counter as usize) % available_transports.len();
        balancer.round_robin_counter += 1;
        
        Ok(available_transports[index])
    }
    
    /// Least loaded transport selection
    async fn least_loaded_selection(&self, available_transports: &[TransportId]) -> DualStackResult<TransportId> {
        if available_transports.is_empty() {
            return Err(DualStackError::Routing("No available transports".to_string()));
        }
        
        let balancer = self.load_balancer.read().await;
        
        let least_loaded = available_transports
            .iter()
            .min_by(|&a, &b| {
                let load_a = balancer.current_load.get(a).unwrap_or(&0.0);
                let load_b = balancer.current_load.get(b).unwrap_or(&0.0);
                load_a.partial_cmp(load_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .unwrap_or(available_transports[0]);
        
        Ok(least_loaded)
    }
    
    /// Performance-based transport selection
    async fn performance_based_selection(
        &self,
        peer_id: &KadPeerId,
        available_transports: &[TransportId],
        operation_type: &str,
    ) -> DualStackResult<TransportId> {
        if available_transports.is_empty() {
            return Err(DualStackError::Routing("No available transports".to_string()));
        }
        
        // Get peer capabilities
        let peer_capabilities = self.get_peer_capabilities(peer_id).await;
        
        // Get performance metrics
        let metrics = self.performance_metrics.read().await;
        
        let mut best_transport = available_transports[0];
        let mut best_score = 0.0;
        
        for &transport in available_transports {
            let mut score = 0.0;
            
            // Base score from performance metrics
            if let Some(perf) = metrics.transport_metrics.get(&transport) {
                score = utils::calculate_preference_score(
                    perf.avg_latency_ms,
                    perf.success_rate,
                    perf.bandwidth_mbps,
                );
            }
            
            // Adjust based on peer capabilities
            if let Some(caps) = &peer_capabilities {
                match transport {
                    TransportId::LibP2P => {
                        if !caps.supports_libp2p {
                            score *= 0.1; // Heavy penalty
                        }
                        if caps.preferred_transport == Some(TransportId::LibP2P) {
                            score *= 1.2; // Bonus for preference
                        }
                    },
                    TransportId::Iroh => {
                        if !caps.supports_iroh {
                            score *= 0.1; // Heavy penalty
                        }
                        if caps.preferred_transport == Some(TransportId::Iroh) {
                            score *= 1.2; // Bonus for preference
                        }
                        if caps.nat_traversal_capable {
                            score *= 1.1; // Bonus for NAT traversal
                        }
                    },
                }
            }
            
            // Prefer modern transport if configured
            if self.config.prefer_modern_transport && transport.is_modern() {
                score *= 1.1;
            }
            
            // Operation-specific preferences
            match operation_type {
                "bootstrap" => {
                    // Prefer more reliable transport for bootstrap
                    if let Some(perf) = metrics.transport_metrics.get(&transport) {
                        score *= perf.success_rate;
                    }
                },
                "put_record" | "find_value" => {
                    // Prefer faster transport for data operations
                    if transport == TransportId::Iroh {
                        score *= 1.1; // iroh often faster for data transfer
                    }
                },
                _ => {
                    // Default scoring
                }
            }
            
            if score > best_score {
                best_score = score;
                best_transport = transport;
            }
        }
        
        debug!("Performance-based selection: {:?} (score: {:.3})", best_transport, best_score);
        Ok(best_transport)
    }
    
    /// Weighted transport selection
    async fn weighted_selection(
        &self,
        available_transports: &[TransportId],
        libp2p_weight: f32,
        iroh_weight: f32,
    ) -> DualStackResult<TransportId> {
        if available_transports.is_empty() {
            return Err(DualStackError::Routing("No available transports".to_string()));
        }
        
        let mut balancer = self.load_balancer.write().await;
        
        // Normalize weights
        let total_weight = libp2p_weight + iroh_weight;
        if total_weight <= 0.0 {
            return Ok(available_transports[0]);
        }
        
        let norm_libp2p = libp2p_weight / total_weight;
        let norm_iroh = iroh_weight / total_weight;
        
        // Accumulate weight and select transport
        balancer.weighted_state.accumulated_weight += 1.0;
        
        let threshold = balancer.weighted_state.accumulated_weight % 1.0;
        
        let selected = if threshold < norm_libp2p && available_transports.contains(&TransportId::LibP2P) {
            TransportId::LibP2P
        } else if available_transports.contains(&TransportId::Iroh) {
            TransportId::Iroh
        } else {
            available_transports[0] // Fallback
        };
        
        Ok(selected)
    }
    
    /// Preferred transport with fallback selection
    async fn preferred_with_fallback_selection(
        &self,
        available_transports: &[TransportId],
        preferred: TransportId,
    ) -> DualStackResult<TransportId> {
        if available_transports.is_empty() {
            return Err(DualStackError::Routing("No available transports".to_string()));
        }
        
        // Use preferred if available
        if available_transports.contains(&preferred) {
            Ok(preferred)
        } else {
            // Fallback to first available
            Ok(available_transports[0])
        }
    }
    
    /// Get cached routing decision
    async fn get_cached_decision(&self, peer_id: &KadPeerId, operation_type: &str) -> Option<CachedDecision> {
        let cache = self.decision_cache.read().await;
        let key = CacheKey {
            peer_id: peer_id.clone(),
            operation_type: operation_type.to_string(),
        };
        
        if let Some(decision) = cache.decisions.get(&key) {
            if let Some(&insertion_time) = cache.insertion_times.get(&key) {
                if insertion_time.elapsed() < cache.ttl {
                    return Some(decision.clone());
                }
            }
        }
        
        None
    }
    
    /// Cache a routing decision
    async fn cache_decision(
        &self,
        peer_id: &KadPeerId,
        operation_type: &str,
        transport: TransportId,
        reason: &str,
    ) {
        let mut cache = self.decision_cache.write().await;
        let key = CacheKey {
            peer_id: peer_id.clone(),
            operation_type: operation_type.to_string(),
        };
        
        let decision = CachedDecision {
            transport,
            confidence: 0.8, // Default confidence
            reason: reason.to_string(),
        };
        
        cache.decisions.insert(key.clone(), decision);
        cache.insertion_times.insert(key, Instant::now());
        
        // Cleanup old entries periodically
        if cache.decisions.len() > 1000 {
            self.cleanup_decision_cache(&mut cache).await;
        }
    }
    
    /// Cleanup old cache entries
    async fn cleanup_decision_cache(&self, cache: &mut DecisionCache) {
        let now = Instant::now();
        let expired_keys: Vec<_> = cache.insertion_times
            .iter()
            .filter(|(_, &time)| now.duration_since(time) > cache.ttl)
            .map(|(key, _)| key.clone())
            .collect();
        
        for key in expired_keys {
            cache.decisions.remove(&key);
            cache.insertion_times.remove(&key);
        }
    }
    
    /// Get peer capabilities with caching
    async fn get_peer_capabilities(&self, peer_id: &KadPeerId) -> Option<PeerCapabilities> {
        let mut cache = self.peer_capabilities.write().await;
        
        // Check if we have cached capabilities
        if let Some(caps) = cache.capabilities.get(peer_id) {
            if let Some(&insertion_time) = cache.insertion_times.get(peer_id) {
                if insertion_time.elapsed() < cache.ttl {
                    return Some(caps.clone());
                }
            }
        }
        
        // Discover peer capabilities (simplified logic)
        let capabilities = self.discover_peer_capabilities(peer_id).await;
        
        // Cache the result
        if let Some(caps) = &capabilities {
            cache.capabilities.insert(peer_id.clone(), caps.clone());
            cache.insertion_times.insert(peer_id.clone(), Instant::now());
        }
        
        capabilities
    }
    
    /// Discover peer capabilities (placeholder implementation)
    async fn discover_peer_capabilities(&self, peer_id: &KadPeerId) -> Option<PeerCapabilities> {
        // In a real implementation, this would:
        // 1. Check protocol negotiation results
        // 2. Query peer discovery services
        // 3. Use heuristics based on peer ID patterns
        // 4. Check historical connection data
        
        // For now, use simple heuristics
        let supports_dual_stack = utils::peer_supports_dual_stack(peer_id);
        
        Some(PeerCapabilities {
            supports_libp2p: true, // Assume all peers support libp2p
            supports_iroh: supports_dual_stack,
            last_successful_transport: None,
            preferred_transport: if supports_dual_stack {
                Some(TransportId::Iroh)
            } else {
                Some(TransportId::LibP2P)
            },
            nat_traversal_capable: supports_dual_stack, // iroh has better NAT traversal
        })
    }
    
    /// Update performance metrics from operation results
    pub async fn update_performance_metrics(
        &self,
        transport: TransportId,
        latency: Duration,
        success: bool,
        operation_type: &str,
    ) {
        let mut metrics = self.performance_metrics.write().await;
        
        let transport_perf = metrics.transport_metrics
            .entry(transport)
            .or_insert_with(|| TransportPerformance {
                avg_latency_ms: 0.0,
                success_rate: 1.0,
                bandwidth_mbps: 0.0,
                connection_time_ms: 0.0,
                total_operations: 0,
                recent_samples: Vec::new(),
            });
        
        // Add new sample
        let sample = OperationSample {
            timestamp: Instant::now(),
            latency,
            success,
            operation_type: operation_type.to_string(),
        };
        
        transport_perf.recent_samples.push(sample);
        transport_perf.total_operations += 1;
        
        // Limit sample history
        if transport_perf.recent_samples.len() > 100 {
            transport_perf.recent_samples.remove(0);
        }
        
        // Update aggregated metrics
        self.recalculate_metrics(transport_perf);
        
        metrics.last_updated = Instant::now();
    }
    
    /// Recalculate aggregated metrics from samples
    fn recalculate_metrics(&self, perf: &mut TransportPerformance) {
        if perf.recent_samples.is_empty() {
            return;
        }
        
        let recent_window = Duration::from_minutes(10);
        let now = Instant::now();
        
        // Filter recent samples
        let recent: Vec<_> = perf.recent_samples
            .iter()
            .filter(|s| now.duration_since(s.timestamp) < recent_window)
            .collect();
        
        if recent.is_empty() {
            return;
        }
        
        // Calculate average latency
        let total_latency: Duration = recent.iter().map(|s| s.latency).sum();
        perf.avg_latency_ms = total_latency.as_millis() as f64 / recent.len() as f64;
        
        // Calculate success rate
        let successes = recent.iter().filter(|s| s.success).count();
        perf.success_rate = successes as f64 / recent.len() as f64;
        
        // Placeholder for bandwidth calculation
        perf.bandwidth_mbps = if perf.success_rate > 0.8 { 100.0 } else { 50.0 };
    }
    
    /// Update load balancing metrics
    pub async fn update_load_metrics(&self, transport: TransportId, load: f32) {
        let mut balancer = self.load_balancer.write().await;
        balancer.current_load.insert(transport, load);
    }
    
    /// Get current routing statistics
    pub async fn get_routing_stats(&self) -> RoutingStats {
        let metrics = self.performance_metrics.read().await;
        let balancer = self.load_balancer.read().await;
        let cache = self.decision_cache.read().await;
        
        RoutingStats {
            cache_size: cache.decisions.len(),
            cache_hit_rate: 0.0, // Would need to track hits/misses
            transport_metrics: metrics.transport_metrics.clone(),
            current_load: balancer.current_load.clone(),
        }
    }
}

/// Routing statistics
#[derive(Debug, Clone)]
pub struct RoutingStats {
    pub cache_size: usize,
    pub cache_hit_rate: f32,
    pub transport_metrics: HashMap<TransportId, TransportPerformance>,
    pub current_load: HashMap<TransportId, f32>,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            transport_metrics: HashMap::new(),
            last_updated: Instant::now(),
        }
    }
}

impl Default for TransportPerformance {
    fn default() -> Self {
        Self {
            avg_latency_ms: 100.0, // Default assumption
            success_rate: 0.95,    // Optimistic default
            bandwidth_mbps: 10.0,  // Conservative default
            connection_time_ms: 1000.0,
            total_operations: 0,
            recent_samples: Vec::new(),
        }
    }
}

impl Default for LoadBalancer {
    fn default() -> Self {
        Self {
            current_load: HashMap::new(),
            round_robin_counter: 0,
            weighted_state: WeightedState {
                libp2p_weight: 1.0,
                iroh_weight: 1.0,
                accumulated_weight: 0.0,
            },
        }
    }
}