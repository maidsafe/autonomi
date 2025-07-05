// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Peer transport affinity tracking and learning
//! 
//! This module implements machine learning-inspired algorithms to track
//! which transport works best for each peer, learning from historical
//! performance data to make intelligent routing decisions.

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
};

/// Peer transport affinity tracker
/// 
/// Learns which transport performs best for each peer based on historical
/// performance data, connection success rates, and latency patterns.
pub struct PeerAffinityTracker {
    /// Per-peer affinity data
    peer_affinities: Arc<RwLock<HashMap<KadPeerId, PeerAffinity>>>,
    
    /// Global transport preferences
    global_preferences: Arc<RwLock<GlobalPreferences>>,
    
    /// Learning algorithm configuration
    learning_config: LearningConfig,
    
    /// Performance history for learning
    performance_history: Arc<RwLock<PerformanceHistory>>,
    
    /// Prediction models per transport
    prediction_models: Arc<RwLock<HashMap<TransportId, PredictionModel>>>,
}

/// Affinity data for a specific peer
#[derive(Debug, Clone)]
struct PeerAffinity {
    /// Peer ID
    peer_id: KadPeerId,
    
    /// Transport performance scores (0.0 to 1.0)
    transport_scores: HashMap<TransportId, f64>,
    
    /// Preferred transport based on learning
    preferred_transport: Option<TransportId>,
    
    /// Confidence in preference (0.0 to 1.0)
    preference_confidence: f64,
    
    /// Historical performance samples
    performance_samples: Vec<PerformanceSample>,
    
    /// Connection patterns
    connection_patterns: ConnectionPatterns,
    
    /// Last interaction timestamp
    last_updated: Instant,
    
    /// Learning metadata
    learning_metadata: LearningMetadata,
}

/// Performance sample for a peer-transport combination
#[derive(Debug, Clone)]
struct PerformanceSample {
    timestamp: Instant,
    transport: TransportId,
    latency: Duration,
    success: bool,
    operation_type: String,
    bytes_transferred: Option<u64>,
    connection_time: Option<Duration>,
}

/// Connection patterns for a peer
#[derive(Debug, Clone)]
struct ConnectionPatterns {
    /// Successful connections per transport
    successful_connections: HashMap<TransportId, u32>,
    /// Failed connections per transport
    failed_connections: HashMap<TransportId, u32>,
    /// Average connection establishment time
    avg_connection_time: HashMap<TransportId, Duration>,
    /// Peak performance times (hour of day)
    peak_performance_hours: Vec<u8>,
    /// Geographic/network locality hints
    locality_hints: LocalityHints,
}

/// Geographic and network locality hints
#[derive(Debug, Clone)]
struct LocalityHints {
    /// Estimated network distance
    network_distance: Option<u32>,
    /// NAT traversal capability
    nat_traversal_support: Option<bool>,
    /// Protocol version compatibility
    protocol_compatibility: HashMap<TransportId, ProtocolCompatibility>,
    /// Bandwidth characteristics
    bandwidth_profile: BandwidthProfile,
}

/// Protocol compatibility information
#[derive(Debug, Clone)]
struct ProtocolCompatibility {
    /// Supported protocol versions
    supported_versions: Vec<String>,
    /// Feature compatibility flags
    feature_support: HashMap<String, bool>,
    /// Negotiation success rate
    negotiation_success_rate: f64,
}

/// Bandwidth profile for a peer
#[derive(Debug, Clone)]
struct BandwidthProfile {
    /// Estimated available bandwidth (Mbps)
    estimated_bandwidth: Option<f64>,
    /// Bandwidth consistency (variance)
    bandwidth_variance: Option<f64>,
    /// Optimal transfer size ranges
    optimal_transfer_sizes: Vec<(usize, usize)>,
}

/// Learning metadata for tracking algorithm behavior
#[derive(Debug, Clone)]
struct LearningMetadata {
    /// Total samples collected
    sample_count: usize,
    /// Learning algorithm version
    algorithm_version: u32,
    /// Prediction accuracy history
    prediction_accuracy: Vec<f64>,
    /// Model training timestamps
    last_model_update: Option<Instant>,
    /// Feature importance weights
    feature_weights: HashMap<String, f64>,
}

/// Global transport preferences learned across all peers
#[derive(Debug, Clone)]
struct GlobalPreferences {
    /// Overall transport performance rankings
    transport_rankings: Vec<(TransportId, f64)>,
    /// Global success rates
    global_success_rates: HashMap<TransportId, f64>,
    /// Average latencies per transport
    global_avg_latencies: HashMap<TransportId, Duration>,
    /// Transport reliability scores
    reliability_scores: HashMap<TransportId, f64>,
    /// Seasonal/temporal patterns
    temporal_patterns: TemporalPatterns,
}

/// Temporal performance patterns
#[derive(Debug, Clone)]
struct TemporalPatterns {
    /// Performance by hour of day
    hourly_patterns: HashMap<u8, HashMap<TransportId, f64>>,
    /// Performance by day of week
    daily_patterns: HashMap<u8, HashMap<TransportId, f64>>,
    /// Trend analysis
    performance_trends: HashMap<TransportId, TrendAnalysis>,
}

/// Trend analysis data
#[derive(Debug, Clone)]
struct TrendAnalysis {
    /// Recent performance trend (improving/declining)
    trend_direction: TrendDirection,
    /// Trend magnitude
    trend_magnitude: f64,
    /// Trend confidence
    trend_confidence: f64,
}

/// Trend direction indicators
#[derive(Debug, Clone, Copy)]
enum TrendDirection {
    Improving,
    Stable,
    Declining,
}

/// Performance history for learning algorithms
#[derive(Debug)]
struct PerformanceHistory {
    /// Recent performance samples (limited size)
    recent_samples: Vec<PerformanceSample>,
    /// Aggregated performance summaries
    performance_summaries: HashMap<(KadPeerId, TransportId), PerformanceSummary>,
    /// Learning dataset for model training
    training_dataset: Vec<TrainingExample>,
}

/// Aggregated performance summary
#[derive(Debug, Clone)]
struct PerformanceSummary {
    peer_id: KadPeerId,
    transport: TransportId,
    sample_count: usize,
    avg_latency: Duration,
    success_rate: f64,
    avg_throughput: Option<f64>,
    reliability_score: f64,
    last_updated: Instant,
}

/// Training example for machine learning
#[derive(Debug, Clone)]
struct TrainingExample {
    /// Input features
    features: HashMap<String, f64>,
    /// Target transport performance score
    target_score: f64,
    /// Transport that was used
    actual_transport: TransportId,
    /// Observed performance
    observed_performance: f64,
}

/// Prediction model for transport selection
#[derive(Debug, Clone)]
struct PredictionModel {
    /// Model type identifier
    model_type: ModelType,
    /// Feature weights/coefficients
    weights: HashMap<String, f64>,
    /// Model accuracy metrics
    accuracy_metrics: AccuracyMetrics,
    /// Training history
    training_history: Vec<TrainingSession>,
    /// Model version
    version: u32,
}

/// Supported model types
#[derive(Debug, Clone)]
enum ModelType {
    /// Simple linear regression
    LinearRegression,
    /// Exponential weighted moving average
    ExponentialSmoothing,
    /// Simple Bayesian classifier
    NaiveBayes,
}

/// Model accuracy metrics
#[derive(Debug, Clone)]
struct AccuracyMetrics {
    /// Mean absolute error
    mae: f64,
    /// Root mean square error
    rmse: f64,
    /// Prediction accuracy (0.0 to 1.0)
    accuracy: f64,
    /// Confidence intervals
    confidence_interval: (f64, f64),
}

/// Training session record
#[derive(Debug, Clone)]
struct TrainingSession {
    timestamp: Instant,
    training_samples: usize,
    accuracy_improvement: f64,
    convergence_iterations: u32,
}

/// Learning algorithm configuration
#[derive(Debug, Clone)]
struct LearningConfig {
    /// Maximum samples per peer
    max_samples_per_peer: usize,
    /// Learning rate for model updates
    learning_rate: f64,
    /// Decay factor for old samples
    sample_decay_factor: f64,
    /// Minimum samples before making predictions
    min_samples_for_prediction: usize,
    /// Model retraining interval
    retraining_interval: Duration,
    /// Feature selection strategy
    feature_selection: FeatureSelectionStrategy,
}

/// Feature selection strategies
#[derive(Debug, Clone)]
enum FeatureSelectionStrategy {
    /// Use all available features
    All,
    /// Select top-k most important features
    TopK(usize),
    /// Use features above importance threshold
    Threshold(f64),
}

impl PeerAffinityTracker {
    /// Create a new peer affinity tracker
    pub async fn new(
        max_history_size: usize,
        evaluation_interval: Duration,
    ) -> DualStackResult<Self> {
        let learning_config = LearningConfig {
            max_samples_per_peer: max_history_size.min(1000),
            learning_rate: 0.1,
            sample_decay_factor: 0.95,
            min_samples_for_prediction: 10,
            retraining_interval: evaluation_interval,
            feature_selection: FeatureSelectionStrategy::TopK(10),
        };
        
        let peer_affinities = Arc::new(RwLock::new(HashMap::new()));
        let global_preferences = Arc::new(RwLock::new(GlobalPreferences::new()));
        let performance_history = Arc::new(RwLock::new(PerformanceHistory::new()));
        
        // Initialize prediction models
        let mut models = HashMap::new();
        for transport in [TransportId::LibP2P, TransportId::Iroh] {
            models.insert(transport, PredictionModel::new(ModelType::ExponentialSmoothing));
        }
        let prediction_models = Arc::new(RwLock::new(models));
        
        Ok(Self {
            peer_affinities,
            global_preferences,
            learning_config,
            performance_history,
            prediction_models,
        })
    }
    
    /// Record a transport selection for learning
    #[instrument(skip(self), fields(peer_id = %peer_id, transport = ?transport))]
    pub async fn record_selection(&self, peer_id: &KadPeerId, transport: TransportId) {
        debug!("Recording transport selection: {} -> {:?}", peer_id, transport);
        
        let mut affinities = self.peer_affinities.write().await;
        let affinity = affinities.entry(peer_id.clone()).or_insert_with(|| {
            PeerAffinity::new(peer_id.clone())
        });
        
        // Update selection count
        *affinity.connection_patterns.successful_connections
            .entry(transport)
            .or_insert(0) += 1;
        
        affinity.last_updated = Instant::now();
    }
    
    /// Record performance result for learning
    #[instrument(skip(self), fields(peer_id = %peer_id, transport = ?transport))]
    pub async fn record_result(
        &self,
        peer_id: &KadPeerId,
        transport: TransportId,
        latency: Duration,
        success: bool,
    ) {
        debug!("Recording performance result: {} {:?} latency={:?} success={}", 
               peer_id, transport, latency, success);
        
        let sample = PerformanceSample {
            timestamp: Instant::now(),
            transport,
            latency,
            success,
            operation_type: "generic".to_string(),
            bytes_transferred: None,
            connection_time: None,
        };
        
        // Update peer affinity
        {
            let mut affinities = self.peer_affinities.write().await;
            let affinity = affinities.entry(peer_id.clone()).or_insert_with(|| {
                PeerAffinity::new(peer_id.clone())
            });
            
            affinity.performance_samples.push(sample.clone());
            
            // Limit sample history
            if affinity.performance_samples.len() > self.learning_config.max_samples_per_peer {
                affinity.performance_samples.remove(0);
            }
            
            // Update connection patterns
            if success {
                *affinity.connection_patterns.successful_connections
                    .entry(transport)
                    .or_insert(0) += 1;
            } else {
                *affinity.connection_patterns.failed_connections
                    .entry(transport)
                    .or_insert(0) += 1;
            }
            
            affinity.last_updated = Instant::now();
            affinity.learning_metadata.sample_count += 1;
        }
        
        // Update global performance history
        {
            let mut history = self.performance_history.write().await;
            history.recent_samples.push(sample.clone());
            
            // Limit global history
            if history.recent_samples.len() > 10000 {
                history.recent_samples.drain(0..5000);
            }
            
            // Update performance summary
            let key = (peer_id.clone(), transport);
            let summary = history.performance_summaries.entry(key).or_insert_with(|| {
                PerformanceSummary {
                    peer_id: peer_id.clone(),
                    transport,
                    sample_count: 0,
                    avg_latency: Duration::from_millis(0),
                    success_rate: 0.0,
                    avg_throughput: None,
                    reliability_score: 0.0,
                    last_updated: Instant::now(),
                }
            });
            
            // Update running averages
            summary.sample_count += 1;
            summary.avg_latency = Duration::from_nanos(
                ((summary.avg_latency.as_nanos() as f64 * (summary.sample_count - 1) as f64) +
                 latency.as_nanos() as f64) as u64 / summary.sample_count as u64
            );
            
            let new_success = if success { 1.0 } else { 0.0 };
            summary.success_rate = 
                (summary.success_rate * (summary.sample_count - 1) as f64 + new_success) /
                summary.sample_count as f64;
            
            summary.last_updated = Instant::now();
        }
        
        // Trigger learning update if we have enough samples
        if self.should_update_models().await {
            self.update_prediction_models().await;
        }
    }
    
    /// Get preferred transport for a peer
    #[instrument(skip(self), fields(peer_id = %peer_id))]
    pub async fn get_preferred_transport(&self, peer_id: &KadPeerId) -> Option<TransportId> {
        let affinities = self.peer_affinities.read().await;
        
        if let Some(affinity) = affinities.get(peer_id) {
            // Return preference if confidence is high enough
            if affinity.preference_confidence > 0.7 {
                return affinity.preferred_transport;
            }
        }
        
        // Fall back to global preferences
        let global = self.global_preferences.read().await;
        if let Some((transport, _score)) = global.transport_rankings.first() {
            Some(*transport)
        } else {
            None
        }
    }
    
    /// Get transport preference score for a peer
    pub async fn get_preference_score(&self, peer_id: &KadPeerId, transport: TransportId) -> f64 {
        let affinities = self.peer_affinities.read().await;
        
        if let Some(affinity) = affinities.get(peer_id) {
            if let Some(&score) = affinity.transport_scores.get(&transport) {
                return score;
            }
        }
        
        // Fall back to global score
        let global = self.global_preferences.read().await;
        global.reliability_scores.get(&transport).copied().unwrap_or(0.5)
    }
    
    /// Predict optimal transport for a peer and operation
    pub async fn predict_optimal_transport(
        &self,
        peer_id: &KadPeerId,
        operation_type: &str,
        payload_size: Option<usize>,
    ) -> Option<TransportId> {
        // Extract features for prediction
        let features = self.extract_features(peer_id, operation_type, payload_size).await;
        
        let models = self.prediction_models.read().await;
        let mut best_transport = None;
        let mut best_score = 0.0;
        
        for (&transport, model) in models.iter() {
            if let Some(predicted_score) = model.predict(&features) {
                if predicted_score > best_score {
                    best_score = predicted_score;
                    best_transport = Some(transport);
                }
            }
        }
        
        debug!("Predicted optimal transport for peer {}: {:?} (score: {:.3})", 
               peer_id, best_transport, best_score);
        
        best_transport
    }
    
    /// Extract features for machine learning prediction
    async fn extract_features(
        &self,
        peer_id: &KadPeerId,
        operation_type: &str,
        payload_size: Option<usize>,
    ) -> HashMap<String, f64> {
        let mut features = HashMap::new();
        
        // Basic features
        features.insert("payload_size".to_string(), 
                       payload_size.unwrap_or(1024) as f64);
        features.insert("operation_type_hash".to_string(), 
                       self.hash_operation_type(operation_type));
        
        // Peer-specific features
        let affinities = self.peer_affinities.read().await;
        if let Some(affinity) = affinities.get(peer_id) {
            features.insert("sample_count".to_string(), 
                           affinity.learning_metadata.sample_count as f64);
            features.insert("last_seen_hours".to_string(),
                           affinity.last_updated.elapsed().as_secs() as f64 / 3600.0);
            
            // Success rates per transport
            for transport in [TransportId::LibP2P, TransportId::Iroh] {
                let successful = affinity.connection_patterns.successful_connections
                    .get(&transport).copied().unwrap_or(0) as f64;
                let failed = affinity.connection_patterns.failed_connections
                    .get(&transport).copied().unwrap_or(0) as f64;
                
                let success_rate = if successful + failed > 0.0 {
                    successful / (successful + failed)
                } else {
                    0.5 // Neutral
                };
                
                features.insert(format!("{:?}_success_rate", transport), success_rate);
            }
        }
        
        // Global features
        let global = self.global_preferences.read().await;
        for (&transport, &reliability) in &global.reliability_scores {
            features.insert(format!("{:?}_global_reliability", transport), reliability);
        }
        
        // Temporal features
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let hour_of_day = (now.as_secs() / 3600) % 24;
        features.insert("hour_of_day".to_string(), hour_of_day as f64);
        
        features
    }
    
    /// Hash operation type for feature extraction
    fn hash_operation_type(&self, operation_type: &str) -> f64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        operation_type.hash(&mut hasher);
        (hasher.finish() % 1000) as f64 / 1000.0
    }
    
    /// Check if models should be updated
    async fn should_update_models(&self) -> bool {
        let history = self.performance_history.read().await;
        
        // Update if we have enough new samples
        history.recent_samples.len() >= self.learning_config.min_samples_for_prediction * 2
    }
    
    /// Update prediction models with recent data
    async fn update_prediction_models(&self) {
        debug!("Updating prediction models with recent performance data");
        
        // Generate training examples from recent data
        let training_examples = self.generate_training_examples().await;
        
        if training_examples.is_empty() {
            return;
        }
        
        // Update each transport model
        let mut models = self.prediction_models.write().await;
        for (transport, model) in models.iter_mut() {
            let transport_examples: Vec<_> = training_examples.iter()
                .filter(|ex| ex.actual_transport == *transport)
                .collect();
            
            if !transport_examples.is_empty() {
                model.train(&transport_examples);
            }
        }
        
        // Update global preferences
        self.update_global_preferences(&training_examples).await;
    }
    
    /// Generate training examples from performance history
    async fn generate_training_examples(&self) -> Vec<TrainingExample> {
        let mut examples = Vec::new();
        let history = self.performance_history.read().await;
        
        for sample in &history.recent_samples {
            let features = self.extract_features(
                &sample.peer_id,
                &sample.operation_type,
                sample.bytes_transferred.map(|b| b as usize),
            ).await;
            
            // Calculate performance score
            let latency_score = 1.0 - (sample.latency.as_millis() as f64 / 5000.0).min(1.0);
            let success_score = if sample.success { 1.0 } else { 0.0 };
            let observed_performance = (latency_score + success_score) / 2.0;
            
            examples.push(TrainingExample {
                features,
                target_score: observed_performance,
                actual_transport: sample.transport,
                observed_performance,
            });
        }
        
        examples
    }
    
    /// Update global preferences based on training data
    async fn update_global_preferences(&self, examples: &[TrainingExample]) {
        let mut global = self.global_preferences.write().await;
        
        // Calculate transport rankings
        let mut transport_scores: HashMap<TransportId, Vec<f64>> = HashMap::new();
        
        for example in examples {
            transport_scores.entry(example.actual_transport)
                .or_insert_with(Vec::new)
                .push(example.observed_performance);
        }
        
        // Update rankings
        global.transport_rankings.clear();
        for (transport, scores) in transport_scores {
            let avg_score = scores.iter().sum::<f64>() / scores.len() as f64;
            global.transport_rankings.push((transport, avg_score));
        }
        
        // Sort by score (highest first)
        global.transport_rankings.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        debug!("Updated global transport rankings: {:?}", global.transport_rankings);
    }
    
    /// Get affinity statistics
    pub async fn get_affinity_stats(&self) -> AffinityStats {
        let affinities = self.peer_affinities.read().await;
        let global = self.global_preferences.read().await;
        let models = self.prediction_models.read().await;
        
        AffinityStats {
            tracked_peers: affinities.len(),
            total_samples: affinities.values()
                .map(|a| a.learning_metadata.sample_count)
                .sum(),
            transport_rankings: global.transport_rankings.clone(),
            model_accuracies: models.iter()
                .map(|(transport, model)| (*transport, model.accuracy_metrics.accuracy))
                .collect(),
        }
    }
}

impl PeerAffinity {
    fn new(peer_id: KadPeerId) -> Self {
        Self {
            peer_id,
            transport_scores: HashMap::new(),
            preferred_transport: None,
            preference_confidence: 0.0,
            performance_samples: Vec::new(),
            connection_patterns: ConnectionPatterns::new(),
            last_updated: Instant::now(),
            learning_metadata: LearningMetadata::new(),
        }
    }
}

impl ConnectionPatterns {
    fn new() -> Self {
        Self {
            successful_connections: HashMap::new(),
            failed_connections: HashMap::new(),
            avg_connection_time: HashMap::new(),
            peak_performance_hours: Vec::new(),
            locality_hints: LocalityHints::new(),
        }
    }
}

impl LocalityHints {
    fn new() -> Self {
        Self {
            network_distance: None,
            nat_traversal_support: None,
            protocol_compatibility: HashMap::new(),
            bandwidth_profile: BandwidthProfile::new(),
        }
    }
}

impl BandwidthProfile {
    fn new() -> Self {
        Self {
            estimated_bandwidth: None,
            bandwidth_variance: None,
            optimal_transfer_sizes: Vec::new(),
        }
    }
}

impl LearningMetadata {
    fn new() -> Self {
        Self {
            sample_count: 0,
            algorithm_version: 1,
            prediction_accuracy: Vec::new(),
            last_model_update: None,
            feature_weights: HashMap::new(),
        }
    }
}

impl GlobalPreferences {
    fn new() -> Self {
        Self {
            transport_rankings: Vec::new(),
            global_success_rates: HashMap::new(),
            global_avg_latencies: HashMap::new(),
            reliability_scores: HashMap::new(),
            temporal_patterns: TemporalPatterns::new(),
        }
    }
}

impl TemporalPatterns {
    fn new() -> Self {
        Self {
            hourly_patterns: HashMap::new(),
            daily_patterns: HashMap::new(),
            performance_trends: HashMap::new(),
        }
    }
}

impl PerformanceHistory {
    fn new() -> Self {
        Self {
            recent_samples: Vec::new(),
            performance_summaries: HashMap::new(),
            training_dataset: Vec::new(),
        }
    }
}

impl PredictionModel {
    fn new(model_type: ModelType) -> Self {
        Self {
            model_type,
            weights: HashMap::new(),
            accuracy_metrics: AccuracyMetrics {
                mae: 0.0,
                rmse: 0.0,
                accuracy: 0.5,
                confidence_interval: (0.0, 1.0),
            },
            training_history: Vec::new(),
            version: 1,
        }
    }
    
    /// Train the model with new examples
    fn train(&mut self, examples: &[&TrainingExample]) {
        if examples.is_empty() {
            return;
        }
        
        match self.model_type {
            ModelType::ExponentialSmoothing => {
                self.train_exponential_smoothing(examples);
            },
            ModelType::LinearRegression => {
                self.train_linear_regression(examples);
            },
            ModelType::NaiveBayes => {
                self.train_naive_bayes(examples);
            }
        }
        
        self.version += 1;
        self.training_history.push(TrainingSession {
            timestamp: Instant::now(),
            training_samples: examples.len(),
            accuracy_improvement: 0.0, // Would calculate actual improvement
            convergence_iterations: 1,
        });
    }
    
    /// Simple exponential smoothing training
    fn train_exponential_smoothing(&mut self, examples: &[&TrainingExample]) {
        let alpha = 0.3; // Smoothing factor
        
        for example in examples {
            for (feature, value) in &example.features {
                let current_weight = self.weights.get(feature).copied().unwrap_or(0.5);
                let new_weight = alpha * value + (1.0 - alpha) * current_weight;
                self.weights.insert(feature.clone(), new_weight);
            }
        }
    }
    
    /// Simple linear regression training (placeholder)
    fn train_linear_regression(&mut self, _examples: &[&TrainingExample]) {
        // Simplified implementation - would use proper linear regression
        // For now, just maintain equal weights
        for weight in self.weights.values_mut() {
            *weight = 0.5;
        }
    }
    
    /// Naive Bayes training (placeholder)
    fn train_naive_bayes(&mut self, _examples: &[&TrainingExample]) {
        // Simplified implementation - would use proper Naive Bayes
        // For now, just maintain equal weights
        for weight in self.weights.values_mut() {
            *weight = 0.5;
        }
    }
    
    /// Predict performance score for given features
    fn predict(&self, features: &HashMap<String, f64>) -> Option<f64> {
        if self.weights.is_empty() {
            return Some(0.5); // Default neutral prediction
        }
        
        let mut score = 0.0;
        let mut weight_sum = 0.0;
        
        for (feature, &value) in features {
            if let Some(&weight) = self.weights.get(feature) {
                score += weight * value;
                weight_sum += weight.abs();
            }
        }
        
        if weight_sum > 0.0 {
            Some((score / weight_sum).max(0.0).min(1.0))
        } else {
            Some(0.5)
        }
    }
}

/// Affinity tracking statistics
#[derive(Debug, Clone)]
pub struct AffinityStats {
    pub tracked_peers: usize,
    pub total_samples: usize,
    pub transport_rankings: Vec<(TransportId, f64)>,
    pub model_accuracies: HashMap<TransportId, f64>,
}