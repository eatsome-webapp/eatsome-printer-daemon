use crate::errors::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{info, warn, error};

/// Circuit breaker states
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,   // Normal operation
    Open,     // Failures detected, blocking requests
    HalfOpen, // Testing if service recovered
}

/// Per-printer circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit (default: 5)
    pub failure_threshold: usize,
    /// Timeout before attempting recovery (default: 5 minutes)
    pub timeout: Duration,
    /// Tracking window for failures (default: 10 minutes)
    pub tracking_window: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            timeout: Duration::from_secs(5 * 60),      // 5 minutes
            tracking_window: Duration::from_secs(10 * 60), // 10 minutes
        }
    }
}

/// Circuit breaker for a single printer
pub struct CircuitBreaker {
    printer_id: String,
    config: CircuitBreakerConfig,
    state: Arc<Mutex<CircuitBreakerState>>,
}

#[derive(Debug)]
struct CircuitBreakerState {
    current_state: CircuitState,
    failure_timestamps: Vec<Instant>,
    last_failure_time: Option<Instant>,
    total_failures: u64,
    circuit_open_count: u64,
    recovery_count: u64,
}

impl CircuitBreaker {
    pub fn new(printer_id: String, config: CircuitBreakerConfig) -> Self {
        Self {
            printer_id,
            config,
            state: Arc::new(Mutex::new(CircuitBreakerState {
                current_state: CircuitState::Closed,
                failure_timestamps: Vec::new(),
                last_failure_time: None,
                total_failures: 0,
                circuit_open_count: 0,
                recovery_count: 0,
            })),
        }
    }

    /// Execute a print operation with circuit breaker protection
    pub async fn execute<F, Fut>(&self, operation: F) -> Result<()>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut state = self.state.lock().await;

        // Check if circuit is open
        if state.current_state == CircuitState::Open {
            // Check if timeout has passed
            if let Some(last_failure) = state.last_failure_time {
                if last_failure.elapsed() >= self.config.timeout {
                    // Transition to HALF_OPEN state for testing
                    info!("Circuit breaker for printer {} transitioning to HALF_OPEN (testing recovery)", self.printer_id);
                    state.current_state = CircuitState::HalfOpen;
                } else {
                    // Circuit still open, reject request
                    return Err(crate::errors::DaemonError::PrintJob(
                        format!("Circuit breaker OPEN for printer {}", self.printer_id)
                    ));
                }
            }
        }

        // Release lock before executing operation
        drop(state);

        // Execute operation
        let result = operation().await;

        // Update state based on result
        let mut state = self.state.lock().await;

        match result {
            Ok(_) => {
                // Success - reset or close circuit
                if state.current_state == CircuitState::HalfOpen {
                    // Recovery successful!
                    info!("Circuit breaker for printer {} recovered - transitioning to CLOSED", self.printer_id);
                    state.current_state = CircuitState::Closed;
                    state.failure_timestamps.clear();
                    state.recovery_count += 1;
                }
                Ok(())
            }
            Err(e) => {
                // Failure - record and check threshold
                let now = Instant::now();
                state.failure_timestamps.push(now);
                state.last_failure_time = Some(now);
                state.total_failures += 1;

                // Clean up old failures outside tracking window
                state.failure_timestamps.retain(|&timestamp| {
                    now.duration_since(timestamp) <= self.config.tracking_window
                });

                warn!(
                    "Print failure for printer {} - {} failures in tracking window",
                    self.printer_id,
                    state.failure_timestamps.len()
                );

                // Check if threshold exceeded
                if state.failure_timestamps.len() >= self.config.failure_threshold {
                    if state.current_state != CircuitState::Open {
                        // Open the circuit
                        error!(
                            "Circuit breaker OPEN for printer {} after {} failures",
                            self.printer_id, state.failure_timestamps.len()
                        );
                        state.current_state = CircuitState::Open;
                        state.circuit_open_count += 1;

                        // TODO: Alert POS app about circuit breaker opened
                    }
                }

                Err(e)
            }
        }
    }

    /// Get current circuit breaker status
    pub async fn get_status(&self) -> CircuitBreakerStatus {
        let state = self.state.lock().await;
        CircuitBreakerStatus {
            printer_id: self.printer_id.clone(),
            state: state.current_state.clone(),
            failure_count: state.failure_timestamps.len(),
            total_failures: state.total_failures,
            circuit_open_count: state.circuit_open_count,
            recovery_count: state.recovery_count,
            last_failure_time: state.last_failure_time,
        }
    }

    /// Manually reset circuit breaker (admin function)
    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        info!("Manually resetting circuit breaker for printer {}", self.printer_id);
        state.current_state = CircuitState::Closed;
        state.failure_timestamps.clear();
        state.last_failure_time = None;
    }
}

/// Circuit breaker status for reporting
#[derive(Debug, Clone, serde::Serialize)]
pub struct CircuitBreakerStatus {
    pub printer_id: String,
    pub state: CircuitState,
    pub failure_count: usize,
    pub total_failures: u64,
    pub circuit_open_count: u64,
    pub recovery_count: u64,
    #[serde(skip)]
    pub last_failure_time: Option<Instant>,
}

// Implement Serialize for CircuitState
impl serde::Serialize for CircuitState {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            CircuitState::Closed => "closed",
            CircuitState::Open => "open",
            CircuitState::HalfOpen => "half_open",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_failures() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 3; // Lower threshold for testing
        config.timeout = Duration::from_millis(100);

        let cb = CircuitBreaker::new("test_printer".to_string(), config);

        // Simulate 3 failures
        for i in 0..3 {
            let result = cb.execute(|| async {
                Err(crate::errors::DaemonError::PrintJob(format!("Test failure {}", i)))
            }).await;
            assert!(result.is_err());
        }

        // Circuit should be open now
        let status = cb.get_status().await;
        assert_eq!(status.state, CircuitState::Open);

        // Next request should be rejected immediately
        let result = cb.execute(|| async { Ok(()) }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_circuit_breaker_recovery() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 2;
        config.timeout = Duration::from_millis(50);

        let cb = CircuitBreaker::new("test_printer".to_string(), config);

        // Trigger failures to open circuit
        for _ in 0..2 {
            let _ = cb.execute(|| async {
                Err(crate::errors::DaemonError::PrintJob("Test failure".to_string()))
            }).await;
        }

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Next request should test recovery (HALF_OPEN)
        let result = cb.execute(|| async { Ok(()) }).await;
        assert!(result.is_ok());

        // Circuit should be closed again
        let status = cb.get_status().await;
        assert_eq!(status.state, CircuitState::Closed);
    }
}
