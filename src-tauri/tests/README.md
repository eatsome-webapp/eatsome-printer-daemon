# Integration Tests

Comprehensive test suite for the Eatsome Printer Service.

## Test Structure

```
tests/
├── common/
│   └── mod.rs              # Test utilities and fixtures
├── queue_persistence_test.rs    # Queue & SQLite persistence tests
├── circuit_breaker_test.rs      # Circuit breaker pattern tests
├── auth_jwt_test.rs             # JWT authentication tests
├── print_flow_test.rs           # End-to-end print flow tests
└── escpos_commands_test.rs      # ESC/POS command generation tests
```

## Running Tests

### All Tests

```bash
cd src-tauri
cargo test
```

### Specific Test File

```bash
cargo test --test queue_persistence_test
cargo test --test circuit_breaker_test
cargo test --test auth_jwt_test
cargo test --test print_flow_test
cargo test --test escpos_commands_test
```

### With Output

```bash
cargo test -- --nocapture
```

### With Debug Logs

```bash
RUST_LOG=debug cargo test
```

## Test Coverage

### Queue Persistence (queue_persistence_test.rs)

- ✅ Jobs persist across daemon restarts
- ✅ Failed jobs retry with exponential backoff
- ✅ Completed jobs cleanup after 7 days
- ✅ Corrupted data handled gracefully

### Circuit Breaker (circuit_breaker_test.rs)

- ✅ Circuit opens after threshold failures (5)
- ✅ Circuit transitions to HALF_OPEN after timeout
- ✅ Circuit closes after successful HALF_OPEN attempt
- ✅ Circuit reopens if HALF_OPEN attempt fails
- ✅ Multiple printers have independent circuits

### Authentication (auth_jwt_test.rs)

- ✅ Valid JWT tokens pass validation
- ✅ Expired tokens fail
- ✅ Invalid signatures fail
- ✅ Missing permissions detected
- ✅ Malformed JWTs fail
- ✅ Token rotation with grace period
- ✅ Restaurant ID mismatch detection

### Print Flow (print_flow_test.rs)

- ✅ Successful print flow
- ✅ Print fails when printer offline
- ✅ Job routing to correct station
- ✅ Backup printer routing
- ✅ Concurrent print jobs (10 simultaneous)
- ✅ Orders with modifiers
- ✅ Special characters (UTF-8)
- ✅ Large orders (50+ items)
- ✅ Retry after transient failure

### ESC/POS Commands (escpos_commands_test.rs)

- ✅ Initialize printer command
- ✅ Bold text commands
- ✅ Text alignment (left/center/right)
- ✅ Paper cut commands (full/partial)
- ✅ Line feed commands
- ✅ Text size commands
- ✅ Drawer kick command
- ✅ QR code commands
- ✅ Barcode commands
- ✅ Character encoding
- ✅ Full receipt sequence
- ✅ UTF-8 encoding
- ✅ Print width calculation
- ✅ Price formatting alignment

## Test Utilities (common/mod.rs)

### MockPrinter

Simulates printer behavior for testing:

```rust
let printer = MockPrinter::new("usb_123", "Test Printer");

// Control behavior
printer.set_online(false).await;  // Simulate offline
printer.set_should_fail(true).await;  // Simulate failure

// Verify results
let count = printer.get_print_count().await;
let last_cmd = printer.get_last_command().await;
```

### TestConfigBuilder

Creates test configuration:

```rust
let config = TestConfigBuilder::new()
    .with_restaurant_id("rest_123")
    .with_location_id("loc_456")
    .with_temp_dir(temp_dir)
    .build();

let db_path = config.get_db_path();
```

### Helper Functions

```rust
// Generate test JWT
let token = generate_test_jwt("rest_123", "loc_456", "secret");

// Create test print job
let job = create_test_print_job("ORDER_001", "bar");
```

## Mock vs Real Printers

**Unit/Integration Tests** (these tests):

- Use `MockPrinter` for fast, deterministic testing
- No real hardware required
- Can simulate failures, offline states

**Manual Testing** (see DEVELOPMENT.md):

- Use real USB/Network/BLE printers
- Test actual ESC/POS output
- Verify paper quality, cutting, drawer kick

## Continuous Integration

Tests run automatically on:

- Git pre-commit hook (via GitHub Actions locally)
- Pull requests (GitHub Actions)
- Main branch pushes (GitHub Actions)

See `.github/workflows/printer-daemon-release.yml` for CI configuration.

## Test Dependencies

From `Cargo.toml`:

```toml
[dev-dependencies]
tokio-test = "0.4"        # Async testing utilities
mockall = "0.12"          # Mocking framework
wiremock = "0.6"          # HTTP mocking
tempfile = "3.8"          # Temporary file/directory
pretty_assertions = "1.4" # Better assertion output
serial_test = "3.0"       # Serial test execution
```

## Writing New Tests

### Unit Test (in module)

```rust
// src-tauri/src/my_module.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        let result = my_function();
        assert_eq!(result, expected);
    }
}
```

### Integration Test (in tests/)

```rust
// src-tauri/tests/my_test.rs

mod common;

use common::MockPrinter;

#[tokio::test]
async fn test_my_feature() {
    let printer = MockPrinter::new("p1", "Test");
    // ... test logic
}
```

## Performance Testing

Not included in this suite. Use:

- **k6** for HTTP API load testing (see DEVELOPMENT.md)
- **Manual testing** with 50+ concurrent orders
- **Profiling** with `cargo flamegraph` for bottlenecks

## Test Metrics

Target coverage: **>80%** for core modules

Priority modules:

1. queue.rs - Job persistence
2. circuit_breaker.rs - Fault tolerance
3. auth.rs - JWT validation
4. routing.rs - Kitchen routing
5. escpos.rs - Command generation

## Debugging Failed Tests

```bash
# Run specific failing test with output
cargo test test_name -- --nocapture

# Run with backtrace
RUST_BACKTRACE=1 cargo test test_name

# Run single-threaded for debugging
cargo test -- --test-threads=1
```

## Known Issues

None currently. Report issues to: https://github.com/eatsome/eatsome/issues
