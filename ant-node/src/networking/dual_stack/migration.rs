// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Migration orchestration for gradual libp2p to iroh transition
//! 
//! This module manages the gradual migration from libp2p to iroh transport,
//! providing safe rollout strategies, automatic rollback capabilities,
//! and canary deployment support.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::RwLock;
use tracing::{debug, error, info, warn, instrument};

use crate::networking::kad::transport::KadPeerId;

use super::{
    TransportId, DualStackError, DualStackResult,
    config::{MigrationConfig, MigrationStrategy, RollbackTriggers, CanaryConfig},
    utils,
};

/// Migration phases for gradual rollout
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhase {
    /// Not started (0% iroh usage)
    NotStarted,
    /// Conservative phase (0-25% iroh usage)
    Conservative,
    /// Validation phase (25-50% iroh usage)
    Validation,
    /// Optimization phase (50-75% iroh usage)
    Optimization,
    /// Completion phase (75-100% iroh usage)
    Completion,
    /// Fully migrated (100% iroh usage)
    Complete,
    /// Rollback in progress
    Rollback,
}

/// Migration policies for different rollout strategies
#[derive(Debug, Clone)]
pub enum MigrationPolicy {
    /// Percentage-based gradual rollout
    Percentage { current: f32, target: f32 },
    /// Cohort-based testing
    Cohort { active_cohorts: HashSet<u32>, total_cohorts: u32 },
    /// Geographic rollout
    Geographic { active_regions: HashSet<String> },
    /// Feature flag controlled
    FeatureFlag { enabled: bool },
}

/// Migration manager for orchestrating gradual transport transition
pub struct MigrationManager {
    /// Configuration for migration behavior
    config: MigrationConfig,
    
    /// Current migration state
    state: Arc<RwLock<MigrationState>>,
    
    /// Performance monitoring for rollback decisions
    performance_monitor: Arc<RwLock<PerformanceMonitor>>,
    
    /// Canary deployment manager
    canary_manager: Arc<RwLock<CanaryManager>>,
    
    /// Cohort assignments for peers
    cohort_assignments: Arc<RwLock<HashMap<KadPeerId, u32>>>,
    
    /// Migration metrics and statistics
    metrics: Arc<RwLock<MigrationMetrics>>,
}

/// Current migration state
#[derive(Debug, Clone)]
struct MigrationState {
    /// Current phase of migration
    phase: MigrationPhase,
    /// Current migration percentage
    percentage: f32,
    /// Target migration percentage
    target_percentage: f32,
    /// Migration start time
    started_at: Option<Instant>,
    /// Last rollout time
    last_rollout: Option<Instant>,
    /// Rollback state
    rollback_state: Option<RollbackState>,
    /// Active migration policy
    policy: MigrationPolicy,
}

/// Rollback state information
#[derive(Debug, Clone)]
struct RollbackState {
    /// Reason for rollback
    reason: String,
    /// Rollback started at
    started_at: Instant,
    /// Target percentage to rollback to
    target_percentage: f32,
    /// Original percentage before rollback
    original_percentage: f32,
}

/// Performance monitoring for rollback decisions
#[derive(Debug)]
struct PerformanceMonitor {
    /// Recent performance samples
    samples: Vec<PerformanceSample>,
    /// Baseline metrics before migration
    baseline_metrics: Option<BaselineMetrics>,
    /// Current evaluation window
    evaluation_window: Duration,
    /// Last evaluation time
    last_evaluation: Instant,
}

/// Performance sample for monitoring
#[derive(Debug, Clone)]
struct PerformanceSample {
    timestamp: Instant,
    transport: TransportId,
    latency_ms: f64,
    success_rate: f64,
    error_rate: f64,
    connection_failures: f64,
}

/// Baseline performance metrics
#[derive(Debug, Clone)]
struct BaselineMetrics {
    avg_latency_ms: f64,
    success_rate: f64,
    error_rate: f64,
    connection_failure_rate: f64,
    sample_count: usize,
}

/// Canary deployment manager
#[derive(Debug)]
struct CanaryManager {
    /// Current canary deployment
    current_canary: Option<CanaryDeployment>,
    /// Canary history
    history: Vec<CanaryResult>,
}

/// Active canary deployment
#[derive(Debug, Clone)]
struct CanaryDeployment {
    /// Canary percentage
    percentage: f32,
    /// Started at
    started_at: Instant,
    /// Evaluation duration
    duration: Duration,
    /// Peers in canary group
    canary_peers: HashSet<KadPeerId>,
    /// Success criteria
    success_criteria: CanarySuccessCriteria,
}

/// Success criteria for canary evaluation
#[derive(Debug, Clone)]
struct CanarySuccessCriteria {
    min_success_rate: f32,
    max_latency_increase: f32,
    min_operations: u64,
}

/// Result of canary deployment
#[derive(Debug, Clone)]
struct CanaryResult {
    started_at: Instant,
    completed_at: Instant,
    success: bool,
    metrics: CanaryMetrics,
    decision: String,
}

/// Metrics from canary deployment
#[derive(Debug, Clone)]
struct CanaryMetrics {
    success_rate: f64,
    latency_increase: f64,
    total_operations: u64,
    error_count: u64,
}

/// Migration metrics and statistics
#[derive(Debug, Clone)]
struct MigrationMetrics {
    /// Total peers migrated
    peers_migrated: u64,
    /// Total operations on new transport
    operations_on_iroh: u64,
    /// Total operations on old transport
    operations_on_libp2p: u64,
    /// Migration success rate
    migration_success_rate: f64,
    /// Rollback count
    rollback_count: u32,
    /// Performance improvement
    performance_improvement: f64,
}

impl MigrationManager {
    /// Create a new migration manager
    pub async fn new(config: MigrationConfig) -> DualStackResult<Self> {
        let initial_policy = match &config.strategy {
            MigrationStrategy::Percentage => MigrationPolicy::Percentage {
                current: 0.0,
                target: config.migration_percentage,
            },
            MigrationStrategy::Cohort { total_cohorts, active_cohorts } => {
                MigrationPolicy::Cohort {
                    active_cohorts: (0..*active_cohorts).collect(),
                    total_cohorts: *total_cohorts,
                }
            },
            MigrationStrategy::Geographic { regions } => MigrationPolicy::Geographic {
                active_regions: regions.iter().cloned().collect(),
            },
            MigrationStrategy::FeatureFlag { flag_name: _ } => MigrationPolicy::FeatureFlag {
                enabled: false,
            },
        };
        
        let state = Arc::new(RwLock::new(MigrationState {
            phase: MigrationPhase::NotStarted,
            percentage: 0.0,
            target_percentage: config.migration_percentage,
            started_at: None,
            last_rollout: None,
            rollback_state: None,
            policy: initial_policy,
        }));
        
        let performance_monitor = Arc::new(RwLock::new(PerformanceMonitor {
            samples: Vec::new(),
            baseline_metrics: None,
            evaluation_window: config.rollback_triggers.evaluation_window,
            last_evaluation: Instant::now(),
        }));
        
        let canary_manager = Arc::new(RwLock::new(CanaryManager {
            current_canary: None,
            history: Vec::new(),
        }));
        
        let cohort_assignments = Arc::new(RwLock::new(HashMap::new()));
        
        let metrics = Arc::new(RwLock::new(MigrationMetrics {
            peers_migrated: 0,
            operations_on_iroh: 0,
            operations_on_libp2p: 0,
            migration_success_rate: 0.0,
            rollback_count: 0,
            performance_improvement: 0.0,
        }));
        
        Ok(Self {
            config,
            state,
            performance_monitor,
            canary_manager,
            cohort_assignments,
            metrics,
        })
    }
    
    /// Apply migration policy to determine transport choice
    #[instrument(skip(self), fields(peer_id = %peer_id))]
    pub async fn apply_migration_policy(
        &self,
        peer_id: &KadPeerId,
        suggested_transport: TransportId,
    ) -> DualStackResult<TransportId> {
        if !self.config.enable_migration {
            return Ok(suggested_transport);
        }
        
        let state = self.state.read().await;
        
        // Check if we're in rollback mode
        if let Some(_rollback) = &state.rollback_state {
            debug!("Migration in rollback mode, using libp2p");
            return Ok(TransportId::LibP2P);
        }
        
        // Apply migration policy
        let should_use_iroh = match &state.policy {
            MigrationPolicy::Percentage { current, target: _ } => {
                self.percentage_based_decision(peer_id, *current).await
            },
            MigrationPolicy::Cohort { active_cohorts, total_cohorts } => {
                self.cohort_based_decision(peer_id, active_cohorts, *total_cohorts).await
            },
            MigrationPolicy::Geographic { active_regions } => {
                self.geographic_based_decision(peer_id, active_regions).await
            },
            MigrationPolicy::FeatureFlag { enabled } => *enabled,
        };
        
        let final_transport = if should_use_iroh {
            TransportId::Iroh
        } else {
            TransportId::LibP2P
        };
        
        // Check canary deployment
        if let Some(canary_transport) = self.check_canary_deployment(peer_id).await {
            return Ok(canary_transport);
        }
        
        debug!("Migration policy selected: {:?} for peer {}", final_transport, peer_id);
        Ok(final_transport)
    }
    
    /// Percentage-based migration decision
    async fn percentage_based_decision(&self, peer_id: &KadPeerId, percentage: f32) -> bool {
        // Use deterministic hash to ensure consistent decisions for same peer
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        peer_id.0.hash(&mut hasher);
        let hash = hasher.finish();
        
        let peer_percentage = (hash % 10000) as f32 / 100.0; // 0.00 to 99.99
        peer_percentage < percentage
    }
    
    /// Cohort-based migration decision
    async fn cohort_based_decision(
        &self,
        peer_id: &KadPeerId,
        active_cohorts: &HashSet<u32>,
        total_cohorts: u32,
    ) -> bool {
        // Get or assign cohort for peer
        let mut assignments = self.cohort_assignments.write().await;
        let cohort = assignments.entry(peer_id.clone()).or_insert_with(|| {
            utils::get_migration_cohort(peer_id, total_cohorts)
        });
        
        active_cohorts.contains(cohort)
    }
    
    /// Geographic-based migration decision (placeholder)
    async fn geographic_based_decision(
        &self,
        _peer_id: &KadPeerId,
        _active_regions: &HashSet<String>,
    ) -> bool {
        // Placeholder implementation - would need actual geographic data
        false
    }
    
    /// Check canary deployment for peer
    async fn check_canary_deployment(&self, peer_id: &KadPeerId) -> Option<TransportId> {
        let canary = self.canary_manager.read().await;
        
        if let Some(ref deployment) = canary.current_canary {
            if deployment.canary_peers.contains(peer_id) {
                return Some(TransportId::Iroh);
            }
        }
        
        None
    }
    
    /// Update migration progress (called periodically)
    pub async fn update_migration_progress(&self) -> DualStackResult<()> {
        if !self.config.enable_migration {
            return Ok(());
        }
        
        debug!("Updating migration progress");
        
        // Check for rollback triggers
        if self.should_trigger_rollback().await? {
            self.initiate_rollback("Performance degradation detected").await?;
            return Ok(());
        }
        
        // Update migration percentage
        let mut state = self.state.write().await;
        
        if state.rollback_state.is_none() {
            let now = Instant::now();
            
            // Check if it's time for next rollout step
            if let Some(last_rollout) = state.last_rollout {
                if now.duration_since(last_rollout) < self.config.rollout_interval {
                    return Ok(());
                }
            }
            
            // Increase migration percentage
            let new_percentage = (state.percentage + self.config.rollout_velocity)
                .min(state.target_percentage);
            
            if new_percentage > state.percentage {
                info!("Advancing migration from {:.1}% to {:.1}%", 
                      state.percentage * 100.0, new_percentage * 100.0);
                
                state.percentage = new_percentage;
                state.last_rollout = Some(now);
                
                // Update phase
                state.phase = self.calculate_phase(new_percentage);
                
                // Update policy
                if let MigrationPolicy::Percentage { current, target } = &mut state.policy {
                    *current = new_percentage;
                }
                
                // Start migration timer if first rollout
                if state.started_at.is_none() {
                    state.started_at = Some(now);
                }
            }
        }
        
        Ok(())
    }
    
    /// Check if rollback should be triggered
    async fn should_trigger_rollback(&self) -> DualStackResult<bool> {
        if !self.config.rollback_triggers.enabled {
            return Ok(false);
        }
        
        let monitor = self.performance_monitor.read().await;
        
        if let Some(baseline) = &monitor.baseline_metrics {
            let recent_samples: Vec<_> = monitor.samples
                .iter()
                .filter(|s| s.timestamp.elapsed() < monitor.evaluation_window)
                .collect();
            
            if recent_samples.len() < 10 {
                // Not enough data
                return Ok(false);
            }
            
            // Calculate current metrics
            let current_metrics = self.calculate_current_metrics(&recent_samples);
            
            // Check rollback triggers
            let error_rate_exceeded = current_metrics.error_rate > 
                self.config.rollback_triggers.error_rate_threshold as f64;
            
            let latency_degraded = (current_metrics.avg_latency_ms - baseline.avg_latency_ms) / 
                baseline.avg_latency_ms > self.config.rollback_triggers.latency_degradation_threshold as f64;
            
            let connection_failures_exceeded = current_metrics.connection_failure_rate > 
                self.config.rollback_triggers.connection_failure_threshold as f64;
            
            if error_rate_exceeded || latency_degraded || connection_failures_exceeded {
                warn!("Rollback triggers activated: error_rate={}, latency_degraded={}, connection_failures={}",
                      error_rate_exceeded, latency_degraded, connection_failures_exceeded);
                return Ok(true);
            }
        }
        
        Ok(false)
    }
    
    /// Calculate current performance metrics
    fn calculate_current_metrics(&self, samples: &[&PerformanceSample]) -> BaselineMetrics {
        if samples.is_empty() {
            return BaselineMetrics {
                avg_latency_ms: 0.0,
                success_rate: 0.0,
                error_rate: 0.0,
                connection_failure_rate: 0.0,
                sample_count: 0,
            };
        }
        
        let total_latency: f64 = samples.iter().map(|s| s.latency_ms).sum();
        let total_success_rate: f64 = samples.iter().map(|s| s.success_rate).sum();
        let total_error_rate: f64 = samples.iter().map(|s| s.error_rate).sum();
        let total_connection_failures: f64 = samples.iter().map(|s| s.connection_failures).sum();
        
        let count = samples.len() as f64;
        
        BaselineMetrics {
            avg_latency_ms: total_latency / count,
            success_rate: total_success_rate / count,
            error_rate: total_error_rate / count,
            connection_failure_rate: total_connection_failures / count,
            sample_count: samples.len(),
        }
    }
    
    /// Initiate rollback to previous state
    async fn initiate_rollback(&self, reason: &str) -> DualStackResult<()> {
        warn!("Initiating migration rollback: {}", reason);
        
        let mut state = self.state.write().await;
        let mut metrics = self.metrics.write().await;
        
        // Set rollback state
        state.rollback_state = Some(RollbackState {
            reason: reason.to_string(),
            started_at: Instant::now(),
            target_percentage: 0.0, // Rollback to 0%
            original_percentage: state.percentage,
        });
        
        // Update phase
        state.phase = MigrationPhase::Rollback;
        
        // Update metrics
        metrics.rollback_count += 1;
        
        info!("Rollback initiated: from {:.1}% to 0%", state.percentage * 100.0);
        
        Ok(())
    }
    
    /// Calculate migration phase based on percentage
    fn calculate_phase(&self, percentage: f32) -> MigrationPhase {
        match percentage {
            p if p <= 0.0 => MigrationPhase::NotStarted,
            p if p <= 0.25 => MigrationPhase::Conservative,
            p if p <= 0.50 => MigrationPhase::Validation,
            p if p <= 0.75 => MigrationPhase::Optimization,
            p if p < 1.0 => MigrationPhase::Completion,
            _ => MigrationPhase::Complete,
        }
    }
    
    /// Record performance sample for monitoring
    pub async fn record_performance_sample(
        &self,
        transport: TransportId,
        latency_ms: f64,
        success_rate: f64,
        error_rate: f64,
        connection_failures: f64,
    ) {
        let mut monitor = self.performance_monitor.write().await;
        
        let sample = PerformanceSample {
            timestamp: Instant::now(),
            transport,
            latency_ms,
            success_rate,
            error_rate,
            connection_failures,
        };
        
        monitor.samples.push(sample);
        
        // Limit sample history
        let max_samples = 10000;
        if monitor.samples.len() > max_samples {
            monitor.samples.drain(0..monitor.samples.len() - max_samples);
        }
        
        // Establish baseline if not set
        if monitor.baseline_metrics.is_none() && monitor.samples.len() >= 100 {
            let baseline_samples: Vec<_> = monitor.samples
                .iter()
                .filter(|s| s.transport == TransportId::LibP2P)
                .take(100)
                .collect();
            
            if !baseline_samples.is_empty() {
                monitor.baseline_metrics = Some(self.calculate_current_metrics(&baseline_samples));
                info!("Established performance baseline from {} samples", baseline_samples.len());
            }
        }
    }
    
    /// Start canary deployment
    pub async fn start_canary_deployment(&self, peers: HashSet<KadPeerId>) -> DualStackResult<()> {
        if !self.config.canary.enabled {
            return Ok(());
        }
        
        let mut canary = self.canary_manager.write().await;
        
        // Check if canary already running
        if canary.current_canary.is_some() {
            return Err(DualStackError::MigrationFailed {
                reason: "Canary deployment already in progress".to_string(),
            });
        }
        
        let deployment = CanaryDeployment {
            percentage: self.config.canary.canary_percentage,
            started_at: Instant::now(),
            duration: self.config.canary.evaluation_duration,
            canary_peers: peers.clone(),
            success_criteria: CanarySuccessCriteria {
                min_success_rate: self.config.canary.success_criteria.min_success_rate,
                max_latency_increase: self.config.canary.success_criteria.max_latency_increase,
                min_operations: self.config.canary.success_criteria.min_operations,
            },
        };
        
        canary.current_canary = Some(deployment);
        
        info!("Started canary deployment with {} peers for {:?}", 
              peers.len(), self.config.canary.evaluation_duration);
        
        Ok(())
    }
    
    /// Evaluate and complete canary deployment
    pub async fn evaluate_canary_deployment(&self) -> DualStackResult<bool> {
        let mut canary = self.canary_manager.write().await;
        
        let deployment = match canary.current_canary.take() {
            Some(d) => d,
            None => return Ok(false), // No canary running
        };
        
        let now = Instant::now();
        if now.duration_since(deployment.started_at) < deployment.duration {
            // Put it back, not ready yet
            canary.current_canary = Some(deployment);
            return Ok(false);
        }
        
        // Evaluate canary performance
        let metrics = self.evaluate_canary_metrics(&deployment).await?;
        
        let success = metrics.success_rate >= deployment.success_criteria.min_success_rate as f64 &&
                      metrics.latency_increase <= deployment.success_criteria.max_latency_increase as f64 &&
                      metrics.total_operations >= deployment.success_criteria.min_operations;
        
        let decision = if success {
            "Canary successful - proceeding with migration"
        } else {
            "Canary failed - aborting migration"
        };
        
        let result = CanaryResult {
            started_at: deployment.started_at,
            completed_at: now,
            success,
            metrics,
            decision: decision.to_string(),
        };
        
        canary.history.push(result);
        
        info!("Canary deployment completed: {} (success_rate={:.3}, latency_increase={:.3})",
              decision, metrics.success_rate, metrics.latency_increase);
        
        Ok(success)
    }
    
    /// Evaluate canary metrics (placeholder)
    async fn evaluate_canary_metrics(&self, _deployment: &CanaryDeployment) -> DualStackResult<CanaryMetrics> {
        // In a real implementation, this would:
        // 1. Collect metrics from canary peers
        // 2. Compare with control group
        // 3. Calculate statistical significance
        
        Ok(CanaryMetrics {
            success_rate: 0.98,
            latency_increase: 0.05,
            total_operations: 500,
            error_count: 10,
        })
    }
    
    /// Get current migration status
    pub async fn get_migration_status(&self) -> MigrationStatus {
        let state = self.state.read().await;
        let metrics = self.metrics.read().await;
        
        MigrationStatus {
            phase: state.phase,
            percentage: state.percentage,
            target_percentage: state.target_percentage,
            started_at: state.started_at,
            rollback_state: state.rollback_state.clone(),
            metrics: metrics.clone(),
        }
    }
    
    /// Shutdown migration manager
    pub async fn shutdown(&self) -> DualStackResult<()> {
        info!("Shutting down migration manager");
        
        // Complete any pending canary deployment
        if let Some(_) = self.canary_manager.read().await.current_canary {
            let _ = self.evaluate_canary_deployment().await;
        }
        
        Ok(())
    }
}

/// Current migration status
#[derive(Debug, Clone)]
pub struct MigrationStatus {
    pub phase: MigrationPhase,
    pub percentage: f32,
    pub target_percentage: f32,
    pub started_at: Option<Instant>,
    pub rollback_state: Option<RollbackState>,
    pub metrics: MigrationMetrics,
}