// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Failover and redundancy controller for dual-stack operations
//! 
//! This module implements circuit breaker patterns, health monitoring,
//! and automatic failover between libp2p and iroh transports to ensure
//! high availability and resilience.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::RwLock;
use tracing::{debug, error, info, warn, instrument};

use crate::networking::kad::transport::KadError;

use super::{
    TransportId, DualStackError, DualStackResult,
    config::{FailoverConfig, CircuitBreakerConfig, HealthCheckConfig, RetryPolicyConfig},
};

/// Failover controller managing transport health and circuit breaker patterns
pub struct FailoverController {
    /// Configuration for failover behavior
    config: FailoverConfig,
    
    /// Circuit breakers per transport
    circuit_breakers: Arc<RwLock<HashMap<TransportId, CircuitBreaker>>>,
    
    /// Health status tracking
    health_tracker: Arc<RwLock<HealthTracker>>,
    
    /// Failure rate monitoring
    failure_monitor: Arc<RwLock<FailureMonitor>>,
    
    /// Retry state tracking
    retry_tracker: Arc<RwLock<RetryTracker>>,
}

/// Circuit breaker implementation
#[derive(Debug, Clone)]
struct CircuitBreaker {
    /// Current state of the circuit breaker
    state: CircuitState,
    
    /// Configuration for this circuit breaker
    config: CircuitBreakerConfig,
    
    /// Failure tracking window
    failure_window: FailureWindow,
    
    /// State transition timestamps
    state_history: Vec<StateTransition>,
    
    /// Last state change
    last_state_change: Instant,
    
    /// Half-open request counter
    half_open_requests: u32,
}

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CircuitState {
    /// Circuit is closed, allowing requests
    Closed,
    /// Circuit is open, rejecting requests
    Open,
    /// Circuit is half-open, testing recovery
    HalfOpen,
}

/// State transition record
#[derive(Debug, Clone)]
struct StateTransition {
    from_state: CircuitState,
    to_state: CircuitState,
    timestamp: Instant,
    reason: String,
}

/// Failure tracking window
#[derive(Debug, Clone)]
struct FailureWindow {
    /// Total requests in window
    total_requests: u32,
    /// Failed requests in window
    failed_requests: u32,
    /// Window start time
    window_start: Instant,
    /// Individual failure records
    failures: Vec<FailureRecord>,
}

/// Individual failure record
#[derive(Debug, Clone)]
struct FailureRecord {
    timestamp: Instant,
    error_type: String,
    recoverable: bool,
}

/// Health tracking for transports
#[derive(Debug)]
struct HealthTracker {
    /// Health status per transport
    health_status: HashMap<TransportId, HealthStatus>,
    /// Health check history
    health_history: Vec<HealthCheckResult>,
}

/// Health status for a transport
#[derive(Debug, Clone)]
struct HealthStatus {
    /// Is transport currently healthy
    is_healthy: bool,
    /// Consecutive failure count
    consecutive_failures: u32,
    /// Consecutive success count
    consecutive_successes: u32,
    /// Last health check time
    last_check: Instant,
    /// Health score (0.0 to 1.0)
    health_score: f64,
}

/// Health check result
#[derive(Debug, Clone)]
struct HealthCheckResult {
    transport: TransportId,
    timestamp: Instant,
    success: bool,
    latency: Option<Duration>,
    error: Option<String>,
}

/// Failure rate monitoring
#[derive(Debug)]
struct FailureMonitor {
    /// Failure rates per transport
    failure_rates: HashMap<TransportId, FailureRate>,
    /// Recent failure events
    recent_failures: Vec<FailureEvent>,
}

/// Failure rate tracking
#[derive(Debug, Clone)]
struct FailureRate {
    /// Current failure rate (0.0 to 1.0)
    rate: f64,
    /// Total operations in window
    total_operations: u64,
    /// Failed operations in window
    failed_operations: u64,
    /// Window start time
    window_start: Instant,
    /// Failure trend (increasing/decreasing)
    trend: FailureTrend,
}

/// Failure trend indicators
#[derive(Debug, Clone, Copy)]
enum FailureTrend {
    Stable,
    Increasing,
    Decreasing,
}

/// Failure event record
#[derive(Debug, Clone)]
struct FailureEvent {
    transport: TransportId,
    timestamp: Instant,
    error_type: String,
    severity: FailureSeverity,
}

/// Failure severity levels
#[derive(Debug, Clone, Copy)]
enum FailureSeverity {
    Low,      // Transient errors
    Medium,   // Connection issues
    High,     // Protocol errors
    Critical, // Complete transport failure
}

/// Retry tracking state
#[derive(Debug)]
struct RetryTracker {
    /// Active retry attempts
    active_retries: HashMap<String, RetryAttempt>,
    /// Retry statistics
    retry_stats: HashMap<TransportId, RetryStats>,
}

/// Individual retry attempt
#[derive(Debug, Clone)]
struct RetryAttempt {
    operation_id: String,
    transport: TransportId,
    attempt_count: u32,
    max_attempts: u32,
    next_attempt: Instant,
    backoff_delay: Duration,
    original_error: String,
}

/// Retry statistics per transport
#[derive(Debug, Clone)]
struct RetryStats {
    /// Total retry attempts
    total_attempts: u64,
    /// Successful retries
    successful_retries: u64,
    /// Failed retries (exhausted)
    failed_retries: u64,
    /// Average attempts per operation
    avg_attempts: f64,
}

impl FailoverController {
    /// Create a new failover controller
    pub async fn new(config: FailoverConfig) -> DualStackResult<Self> {
        let circuit_breakers = Arc::new(RwLock::new(HashMap::new()));
        
        // Initialize circuit breakers for each transport
        {
            let mut breakers = circuit_breakers.write().await;
            for transport in [TransportId::LibP2P, TransportId::Iroh] {
                breakers.insert(transport, CircuitBreaker::new(config.circuit_breaker.clone()));
            }
        }
        
        let health_tracker = Arc::new(RwLock::new(HealthTracker {
            health_status: [TransportId::LibP2P, TransportId::Iroh]
                .iter()
                .map(|&transport| (transport, HealthStatus::new()))
                .collect(),
            health_history: Vec::new(),
        }));
        
        let failure_monitor = Arc::new(RwLock::new(FailureMonitor {
            failure_rates: [TransportId::LibP2P, TransportId::Iroh]
                .iter()
                .map(|&transport| (transport, FailureRate::new()))
                .collect(),
            recent_failures: Vec::new(),
        }));
        
        let retry_tracker = Arc::new(RwLock::new(RetryTracker {
            active_retries: HashMap::new(),
            retry_stats: [TransportId::LibP2P, TransportId::Iroh]
                .iter()
                .map(|&transport| (transport, RetryStats::new()))
                .collect(),
        }));
        
        Ok(Self {
            config,
            circuit_breakers,
            health_tracker,
            failure_monitor,
            retry_tracker,
        })
    }
    
    /// Check if a transport is available for requests
    #[instrument(skip(self), fields(transport = ?transport))]
    pub async fn is_transport_available(&self, transport: TransportId) -> bool {
        if !self.config.enabled {
            return true; // Failover disabled, assume available
        }
        
        // Check circuit breaker state
        let breakers = self.circuit_breakers.read().await;
        if let Some(breaker) = breakers.get(&transport) {
            match breaker.state {
                CircuitState::Open => {
                    debug!("Transport {:?} circuit breaker is OPEN", transport);
                    return false;
                },
                CircuitState::HalfOpen => {
                    // Allow limited requests in half-open state
                    if breaker.half_open_requests >= breaker.config.half_open_requests {
                        debug!("Transport {:?} half-open request limit reached", transport);
                        return false;
                    }
                },
                CircuitState::Closed => {
                    // Circuit closed, check health status
                }
            }
        }
        
        // Check health status
        let health = self.health_tracker.read().await;
        if let Some(status) = health.health_status.get(&transport) {
            if !status.is_healthy {
                debug!("Transport {:?} is marked unhealthy", transport);
                return false;
            }
        }
        
        true
    }
    
    /// Record a successful operation
    pub async fn record_success(&self, transport: TransportId) {
        if !self.config.enabled {
            return;
        }
        
        debug!("Recording success for transport {:?}", transport);
        
        // Update circuit breaker
        {
            let mut breakers = self.circuit_breakers.write().await;
            if let Some(breaker) = breakers.get_mut(&transport) {
                breaker.record_success().await;
            }
        }
        
        // Update health status
        {
            let mut health = self.health_tracker.write().await;
            if let Some(status) = health.health_status.get_mut(&transport) {
                status.record_success();
            }
        }
        
        // Update failure monitoring
        {
            let mut monitor = self.failure_monitor.write().await;
            if let Some(rate) = monitor.failure_rates.get_mut(&transport) {
                rate.record_success();
            }
        }
    }
    
    /// Record a failed operation
    pub async fn record_failure(&self, transport: TransportId, error: &KadError) {
        if !self.config.enabled {
            return;
        }
        
        warn!("Recording failure for transport {:?}: {}", transport, error);
        
        let error_type = self.classify_error(error);
        let severity = self.determine_severity(error);
        
        // Update circuit breaker
        {
            let mut breakers = self.circuit_breakers.write().await;
            if let Some(breaker) = breakers.get_mut(&transport) {
                breaker.record_failure(error_type.clone()).await;
            }
        }
        
        // Update health status
        {
            let mut health = self.health_tracker.write().await;
            if let Some(status) = health.health_status.get_mut(&transport) {
                status.record_failure();
            }
        }
        
        // Update failure monitoring
        {
            let mut monitor = self.failure_monitor.write().await;
            if let Some(rate) = monitor.failure_rates.get_mut(&transport) {
                rate.record_failure();
            }
            
            // Add to failure events
            monitor.recent_failures.push(FailureEvent {
                transport,
                timestamp: Instant::now(),
                error_type,
                severity,
            });
            
            // Limit failure history
            if monitor.recent_failures.len() > 1000 {
                monitor.recent_failures.drain(0..500);
            }
        }
    }
    
    /// Record a timeout
    pub async fn record_timeout(&self, transport: TransportId) {
        if !self.config.enabled {
            return;
        }
        
        warn!("Recording timeout for transport {:?}", transport);
        
        // Treat timeout as a failure
        let timeout_error = KadError::QueryFailed {
            reason: "Operation timeout".to_string(),
        };
        
        self.record_failure(transport, &timeout_error).await;
    }
    
    /// Get health score for a transport (0.0 to 1.0)
    pub async fn get_health_score(&self, transport: TransportId) -> f64 {
        let health = self.health_tracker.read().await;
        health.health_status
            .get(&transport)
            .map(|status| status.health_score)
            .unwrap_or(0.5) // Default neutral score
    }
    
    /// Get failure rate for a transport
    pub async fn get_failure_rate(&self, transport: TransportId) -> f64 {
        let monitor = self.failure_monitor.read().await;
        monitor.failure_rates
            .get(&transport)
            .map(|rate| rate.rate)
            .unwrap_or(0.0)
    }
    
    /// Get circuit breaker state
    pub async fn get_circuit_state(&self, transport: TransportId) -> CircuitState {
        let breakers = self.circuit_breakers.read().await;
        breakers.get(&transport)
            .map(|breaker| breaker.state)
            .unwrap_or(CircuitState::Closed)
    }
    
    /// Force circuit breaker state (for testing/debugging)
    pub async fn force_circuit_state(&self, transport: TransportId, state: CircuitState) -> DualStackResult<()> {
        let mut breakers = self.circuit_breakers.write().await;
        if let Some(breaker) = breakers.get_mut(&transport) {
            let old_state = breaker.state;
            breaker.state = state;
            breaker.last_state_change = Instant::now();
            
            breaker.state_history.push(StateTransition {
                from_state: old_state,
                to_state: state,
                timestamp: Instant::now(),
                reason: "Manual override".to_string(),
            });
            
            info!("Forced circuit breaker state change: {:?} {:?} -> {:?}", 
                  transport, old_state, state);
            
            Ok(())
        } else {
            Err(DualStackError::Configuration("Transport not found".to_string()))
        }
    }
    
    /// Get failover statistics
    pub async fn get_failover_stats(&self) -> FailoverStats {
        let breakers = self.circuit_breakers.read().await;
        let health = self.health_tracker.read().await;
        let monitor = self.failure_monitor.read().await;
        let retry = self.retry_tracker.read().await;
        
        FailoverStats {
            circuit_states: breakers.iter()
                .map(|(transport, breaker)| (*transport, breaker.state))
                .collect(),
            health_scores: health.health_status.iter()
                .map(|(transport, status)| (*transport, status.health_score))
                .collect(),
            failure_rates: monitor.failure_rates.iter()
                .map(|(transport, rate)| (*transport, rate.rate))
                .collect(),
            retry_stats: retry.retry_stats.clone(),
        }
    }
    
    /// Classify error type for failure tracking
    fn classify_error(&self, error: &KadError) -> String {
        match error {
            KadError::Timeout => "timeout".to_string(),
            KadError::ConnectionFailed { .. } => "connection_failed".to_string(),
            KadError::QueryFailed { .. } => "query_failed".to_string(),
            KadError::InvalidPeerId => "invalid_peer".to_string(),
            KadError::RecordNotFound { .. } => "record_not_found".to_string(),
            KadError::StorageFailed { .. } => "storage_failed".to_string(),
            KadError::NetworkError { .. } => "network_error".to_string(),
        }
    }
    
    /// Determine failure severity
    fn determine_severity(&self, error: &KadError) -> FailureSeverity {
        match error {
            KadError::Timeout => FailureSeverity::Low,
            KadError::RecordNotFound { .. } => FailureSeverity::Low,
            KadError::ConnectionFailed { .. } => FailureSeverity::Medium,
            KadError::NetworkError { .. } => FailureSeverity::Medium,
            KadError::QueryFailed { .. } => FailureSeverity::High,
            KadError::StorageFailed { .. } => FailureSeverity::High,
            KadError::InvalidPeerId => FailureSeverity::Critical,
        }
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            config,
            failure_window: FailureWindow::new(),
            state_history: Vec::new(),
            last_state_change: Instant::now(),
            half_open_requests: 0,
        }
    }
    
    /// Record a successful operation
    async fn record_success(&mut self) {
        self.failure_window.total_requests += 1;
        
        match self.state {
            CircuitState::HalfOpen => {
                // Success in half-open state, close circuit
                self.transition_to(CircuitState::Closed, "Half-open success").await;
                self.half_open_requests = 0;
            },
            CircuitState::Closed => {
                // Success in closed state, maintain
            },
            CircuitState::Open => {
                // Should not receive success in open state
                warn!("Received success in open circuit state");
            }
        }
    }
    
    /// Record a failed operation
    async fn record_failure(&mut self, error_type: String) {
        self.failure_window.total_requests += 1;
        self.failure_window.failed_requests += 1;
        
        self.failure_window.failures.push(FailureRecord {
            timestamp: Instant::now(),
            error_type,
            recoverable: true, // Default assumption
        });
        
        // Check if we should open the circuit
        if self.state == CircuitState::Closed && self.should_open_circuit() {
            self.transition_to(CircuitState::Open, "Failure threshold exceeded").await;
        } else if self.state == CircuitState::HalfOpen {
            // Failure in half-open state, go back to open
            self.transition_to(CircuitState::Open, "Half-open failure").await;
            self.half_open_requests = 0;
        }
    }
    
    /// Check if circuit should be opened
    fn should_open_circuit(&self) -> bool {
        if !self.config.enabled {
            return false;
        }
        
        if self.failure_window.total_requests < self.config.min_requests {
            return false; // Not enough data
        }
        
        let failure_rate = self.failure_window.failed_requests as f32 / 
                          self.failure_window.total_requests as f32;
        
        failure_rate >= self.config.failure_rate_threshold
    }
    
    /// Transition to a new state
    async fn transition_to(&mut self, new_state: CircuitState, reason: &str) {
        let old_state = self.state;
        
        if old_state != new_state {
            self.state = new_state;
            self.last_state_change = Instant::now();
            
            self.state_history.push(StateTransition {
                from_state: old_state,
                to_state: new_state,
                timestamp: self.last_state_change,
                reason: reason.to_string(),
            });
            
            // Limit history size
            if self.state_history.len() > 100 {
                self.state_history.drain(0..50);
            }
            
            debug!("Circuit breaker state transition: {:?} -> {:?} ({})", 
                   old_state, new_state, reason);
            
            // Reset counters on state changes
            match new_state {
                CircuitState::Closed => {
                    self.failure_window = FailureWindow::new();
                    self.half_open_requests = 0;
                },
                CircuitState::Open => {
                    self.half_open_requests = 0;
                },
                CircuitState::HalfOpen => {
                    self.half_open_requests = 0;
                    self.failure_window = FailureWindow::new();
                }
            }
        }
    }
    
    /// Check if circuit should transition from open to half-open
    pub async fn update_state(&mut self) {
        if self.state == CircuitState::Open {
            if self.last_state_change.elapsed() >= self.config.recovery_timeout {
                self.transition_to(CircuitState::HalfOpen, "Recovery timeout elapsed").await;
            }
        }
    }
}

impl FailureWindow {
    fn new() -> Self {
        Self {
            total_requests: 0,
            failed_requests: 0,
            window_start: Instant::now(),
            failures: Vec::new(),
        }
    }
}

impl HealthStatus {
    fn new() -> Self {
        Self {
            is_healthy: true,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check: Instant::now(),
            health_score: 1.0,
        }
    }
    
    fn record_success(&mut self) {
        self.consecutive_successes += 1;
        self.consecutive_failures = 0;
        self.last_check = Instant::now();
        
        // Gradually increase health score
        self.health_score = (self.health_score + 0.1).min(1.0);
        self.is_healthy = true;
    }
    
    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.consecutive_successes = 0;
        self.last_check = Instant::now();
        
        // Decrease health score based on failure count
        let penalty = 0.1 + (self.consecutive_failures as f64 * 0.05);
        self.health_score = (self.health_score - penalty).max(0.0);
        
        // Mark unhealthy after threshold
        if self.consecutive_failures >= 3 {
            self.is_healthy = false;
        }
    }
}

impl FailureRate {
    fn new() -> Self {
        Self {
            rate: 0.0,
            total_operations: 0,
            failed_operations: 0,
            window_start: Instant::now(),
            trend: FailureTrend::Stable,
        }
    }
    
    fn record_success(&mut self) {
        self.total_operations += 1;
        self.update_rate();
    }
    
    fn record_failure(&mut self) {
        self.total_operations += 1;
        self.failed_operations += 1;
        self.update_rate();
    }
    
    fn update_rate(&mut self) {
        if self.total_operations > 0 {
            let new_rate = self.failed_operations as f64 / self.total_operations as f64;
            
            // Update trend
            if new_rate > self.rate * 1.1 {
                self.trend = FailureTrend::Increasing;
            } else if new_rate < self.rate * 0.9 {
                self.trend = FailureTrend::Decreasing;
            } else {
                self.trend = FailureTrend::Stable;
            }
            
            self.rate = new_rate;
        }
        
        // Reset window if too old
        if self.window_start.elapsed() > Duration::from_minutes(10) {
            self.total_operations = 0;
            self.failed_operations = 0;
            self.window_start = Instant::now();
            self.rate = 0.0;
            self.trend = FailureTrend::Stable;
        }
    }
}

impl RetryStats {
    fn new() -> Self {
        Self {
            total_attempts: 0,
            successful_retries: 0,
            failed_retries: 0,
            avg_attempts: 0.0,
        }
    }
}

/// Failover statistics
#[derive(Debug, Clone)]
pub struct FailoverStats {
    pub circuit_states: HashMap<TransportId, CircuitState>,
    pub health_scores: HashMap<TransportId, f64>,
    pub failure_rates: HashMap<TransportId, f64>,
    pub retry_stats: HashMap<TransportId, RetryStats>,
}