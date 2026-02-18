// Integration tests for circuit breaker pattern

mod common;

use common::MockPrinter;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

struct CircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    threshold: u32,
    timeout: Duration,
    last_failure_time: Option<std::time::Instant>,
}

impl CircuitBreaker {
    fn new(threshold: u32, timeout: Duration) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            threshold,
            timeout,
            last_failure_time: None,
        }
    }

    async fn execute<F, T, E>(&mut self, operation: F) -> Result<T, E>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Display + From<String>,
    {
        // Check if circuit should transition from OPEN to HALF_OPEN
        if matches!(self.state, CircuitState::Open) {
            if let Some(last_failure) = self.last_failure_time {
                if last_failure.elapsed() >= self.timeout {
                    self.state = CircuitState::HalfOpen;
                } else {
                    return Err(E::from("Circuit OPEN, printer disabled".to_string()));
                }
            }
        }

        // Execute operation
        match operation.await {
            Ok(result) => {
                // Success - reset on HALF_OPEN or CLOSED
                if matches!(self.state, CircuitState::HalfOpen) {
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                }
                Ok(result)
            }
            Err(err) => {
                // Failure - increment counter
                self.failure_count += 1;
                self.last_failure_time = Some(std::time::Instant::now());

                if self.failure_count >= self.threshold {
                    self.state = CircuitState::Open;
                }

                Err(err)
            }
        }
    }

    fn get_state(&self) -> CircuitState {
        self.state.clone()
    }

    fn get_failure_count(&self) -> u32 {
        self.failure_count
    }
}

#[tokio::test]
async fn test_circuit_opens_after_threshold_failures() {
    let printer = MockPrinter::new("usb_123", "Test Printer");
    let mut circuit = CircuitBreaker::new(5, Duration::from_secs(10));

    // Simulate printer offline
    printer.set_online(false).await;

    // Fail 5 times (threshold)
    for i in 0..5 {
        let result = circuit
            .execute(printer.print(vec![0x1B, 0x40]))
            .await;
        assert!(result.is_err());
        assert_eq!(circuit.get_failure_count(), i + 1);
    }

    // Circuit should be OPEN
    assert!(matches!(circuit.get_state(), CircuitState::Open));

    // Next attempt should fail immediately without trying printer
    let result = circuit
        .execute(printer.print(vec![0x1B, 0x40]))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_circuit_transitions_to_half_open_after_timeout() {
    let printer = MockPrinter::new("usb_456", "Test Printer 2");
    let mut circuit = CircuitBreaker::new(3, Duration::from_millis(100));

    // Open circuit
    printer.set_online(false).await;
    for _ in 0..3 {
        let _ = circuit.execute(printer.print(vec![0x1B, 0x40])).await;
    }
    assert!(matches!(circuit.get_state(), CircuitState::Open));

    // Wait for timeout
    sleep(Duration::from_millis(150)).await;

    // Bring printer online
    printer.set_online(true).await;

    // Next attempt should transition to HALF_OPEN and succeed
    let result = circuit.execute(printer.print(vec![0x1B, 0x40])).await;
    assert!(result.is_ok());
    assert!(matches!(circuit.get_state(), CircuitState::Closed));
}

#[tokio::test]
async fn test_circuit_closes_after_successful_half_open() {
    let printer = MockPrinter::new("net_789", "Network Printer");
    let mut circuit = CircuitBreaker::new(2, Duration::from_millis(100));

    // Open circuit
    printer.set_should_fail(true).await;
    for _ in 0..2 {
        let _ = circuit.execute(printer.print(vec![0x1B, 0x40])).await;
    }
    assert!(matches!(circuit.get_state(), CircuitState::Open));

    // Wait for timeout and fix printer
    sleep(Duration::from_millis(150)).await;
    printer.set_should_fail(false).await;

    // Successful attempt should close circuit
    let result = circuit.execute(printer.print(vec![0x1B, 0x40])).await;
    assert!(result.is_ok());
    assert!(matches!(circuit.get_state(), CircuitState::Closed));

    // Should continue working
    let result2 = circuit.execute(printer.print(vec![0x1B, 0x40])).await;
    assert!(result2.is_ok());
    assert_eq!(circuit.get_failure_count(), 0);
}

#[tokio::test]
async fn test_circuit_reopens_if_half_open_fails() {
    let printer = MockPrinter::new("ble_999", "Bluetooth Printer");
    let mut circuit = CircuitBreaker::new(2, Duration::from_millis(100));

    // Open circuit
    printer.set_online(false).await;
    for _ in 0..2 {
        let _ = circuit.execute(printer.print(vec![0x1B, 0x40])).await;
    }
    assert!(matches!(circuit.get_state(), CircuitState::Open));

    // Wait for timeout (printer still offline)
    sleep(Duration::from_millis(150)).await;

    // HALF_OPEN attempt fails -> circuit should stay OPEN
    let result = circuit.execute(printer.print(vec![0x1B, 0x40])).await;
    assert!(result.is_err());
    assert!(matches!(circuit.get_state(), CircuitState::Open));
}

#[tokio::test]
async fn test_multiple_printers_independent_circuits() {
    let printer1 = MockPrinter::new("p1", "Printer 1");
    let printer2 = MockPrinter::new("p2", "Printer 2");
    let mut circuit1 = CircuitBreaker::new(3, Duration::from_secs(10));
    let mut circuit2 = CircuitBreaker::new(3, Duration::from_secs(10));

    // Fail printer1 but not printer2
    printer1.set_online(false).await;
    for _ in 0..3 {
        let _ = circuit1.execute(printer1.print(vec![0x1B, 0x40])).await;
    }

    // Circuit1 should be OPEN
    assert!(matches!(circuit1.get_state(), CircuitState::Open));

    // Circuit2 should still be CLOSED
    assert!(matches!(circuit2.get_state(), CircuitState::Closed));

    // Printer2 should still work
    let result = circuit2.execute(printer2.print(vec![0x1B, 0x40])).await;
    assert!(result.is_ok());
}
