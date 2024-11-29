use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    max_failures: u32,
    reset_timeout: Duration,
    min_backoff: Duration,
    max_backoff: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            max_failures: 5,
            reset_timeout: Duration::from_secs(60),
            min_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
struct EndpointState {
    failures: u32,
    last_failure: Instant,
    last_attempt: Instant,
    backoff_duration: Duration,
}

impl EndpointState {
    fn new(min_backoff: Duration) -> Self {
        Self {
            failures: 0,
            last_failure: Instant::now(),
            last_attempt: Instant::now(),
            backoff_duration: min_backoff,
        }
    }

    fn record_failure(&mut self, max_backoff: Duration) {
        self.failures += 1;
        self.last_failure = Instant::now();
        self.last_attempt = Instant::now();
        // Exponential backoff with max limit
        self.backoff_duration = std::cmp::min(self.backoff_duration * 2, max_backoff);
    }

    fn record_success(&mut self, min_backoff: Duration) {
        self.failures = 0;
        self.backoff_duration = min_backoff;
    }

    fn is_open(&self, max_failures: u32, reset_timeout: Duration) -> bool {
        if self.failures >= max_failures {
            // Check if we've waited long enough since the last failure
            if self.last_failure.elapsed() > reset_timeout {
                false // Circuit is half-open, allow retry
            } else {
                true // Circuit is open, block requests
            }
        } else {
            false // Circuit is closed, allow requests
        }
    }

    fn should_retry(&self) -> bool {
        self.last_attempt.elapsed() >= self.backoff_duration
    }
}

#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    states: Arc<RwLock<HashMap<String, EndpointState>>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            config: CircuitBreakerConfig::default(),
        }
    }

    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn check_endpoint(&self, endpoint: &str) -> bool {
        let mut states = self.states.write().await;
        let state = states
            .entry(endpoint.to_string())
            .or_insert_with(|| EndpointState::new(self.config.min_backoff));

        !(state.is_open(self.config.max_failures, self.config.reset_timeout) && !state.should_retry())
    }

    pub async fn record_success(&self, endpoint: &str) {
        let mut states = self.states.write().await;
        if let Some(state) = states.get_mut(endpoint) {
            state.record_success(self.config.min_backoff);
        }
    }

    pub async fn record_failure(&self, endpoint: &str) {
        let mut states = self.states.write().await;
        let state = states
            .entry(endpoint.to_string())
            .or_insert_with(|| EndpointState::new(self.config.min_backoff));
        state.record_failure(self.config.max_backoff);
    }

    pub async fn get_backoff_duration(&self, endpoint: &str) -> Duration {
        let states = self.states.read().await;
        states
            .get(endpoint)
            .map(|state| state.backoff_duration)
            .unwrap_or(self.config.min_backoff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    fn test_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            max_failures: 3,
            reset_timeout: Duration::from_millis(100), // Much shorter for testing
            min_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(100),
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_basic() {
        let cb = CircuitBreaker::with_config(test_config());
        let endpoint = "http://test.endpoint";

        // Initially should allow requests
        assert!(cb.check_endpoint(endpoint).await);

        // Record failures
        for _ in 0..test_config().max_failures {
            cb.record_failure(endpoint).await;
        }

        // Circuit should be open
        assert!(!cb.check_endpoint(endpoint).await);

        // Record success should reset
        cb.record_success(endpoint).await;
        assert!(cb.check_endpoint(endpoint).await);
    }

    #[tokio::test]
    async fn test_backoff_duration() {
        let config = test_config();
        let cb = CircuitBreaker::with_config(config.clone());
        let endpoint = "http://test.endpoint";

        assert_eq!(cb.get_backoff_duration(endpoint).await, config.min_backoff);

        // Record a failure
        cb.record_failure(endpoint).await;
        assert_eq!(
            cb.get_backoff_duration(endpoint).await,
            config.min_backoff * 2
        );

        // Record another failure
        cb.record_failure(endpoint).await;
        assert_eq!(
            cb.get_backoff_duration(endpoint).await,
            config.min_backoff * 4
        );

        // Success should reset backoff
        cb.record_success(endpoint).await;
        assert_eq!(cb.get_backoff_duration(endpoint).await, config.min_backoff);
    }

    #[tokio::test]
    async fn test_circuit_half_open() {
        let config = test_config();
        let cb = CircuitBreaker::with_config(config.clone());
        let endpoint = "http://test.endpoint";

        // Open the circuit
        for _ in 0..config.max_failures {
            cb.record_failure(endpoint).await;
        }
        assert!(!cb.check_endpoint(endpoint).await);

        // Wait for reset timeout
        sleep(config.reset_timeout + Duration::from_millis(10)).await;

        // Circuit should be half-open now
        assert!(cb.check_endpoint(endpoint).await);
    }
}
