# Architecture

System design and technical architecture of the Eatsome Printer Service.

## Table of Contents

- [Overview](#overview)
- [System Architecture](#system-architecture)
- [Module Structure](#module-structure)
- [Communication Patterns](#communication-patterns)
- [Data Flow](#data-flow)
- [Error Handling](#error-handling)
- [Security](#security)

## Overview

The Eatsome Printer Service is a **daemon-first architecture** designed for 24/7 reliability in commercial restaurant environments. Built with Tauri (Rust + React), it provides sub-100ms latency thermal printing with fault tolerance and offline queueing.

### Design Principles

1. **Offline-First**: Queue persists across restarts, network outages
2. **Fault Isolation**: Circuit breakers prevent cascading failures
3. **Zero-Config**: Auto-discovery, automatic routing, minimal setup
4. **Privacy-First**: PII stripped before external transmission (Sentry)
5. **Resource-Efficient**: <40 MB memory, <1% CPU idle

## System Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         POS Application                         │
│                        (Next.js + React)                        │
└───────────────────┬─────────────────────────────────────────────┘
                    │
                    │ 1. HTTP POST /api/print (fallback)
                    │ 2. Supabase Realtime broadcast (primary)
                    │
                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Supabase Realtime Server                     │
│                  (WebSocket pub/sub channels)                   │
└───────────────────┬─────────────────────────────────────────────┘
                    │
                    │ restaurant:{id}:print-job (broadcast)
                    │
                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Eatsome Printer Daemon (Tauri)                │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────────────────────────────────────────────────┐  │
│  │               Realtime Client (WebSocket)                │  │
│  └────────────────────┬─────────────────────────────────────┘  │
│                       │                                         │
│                       ▼                                         │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │          JWT Validator + Kitchen Router               │  │
│  │  (Verifies token, routes by menu_items.routing_group_id) │  │
│  └────────────────────┬─────────────────────────────────────┘  │
│                       │                                         │
│                       ▼                                         │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │     Queue Manager (SQLite + better-queue pattern)        │  │
│  │   (Encrypted with sqlcipher, exponential backoff retry)  │  │
│  └────────────────────┬─────────────────────────────────────┘  │
│                       │                                         │
│                       ▼                                         │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              Printer Manager (Circuit Breaker)           │  │
│  │         (USB/Network/BLE discovery, fault isolation)     │  │
│  └────────────────────┬─────────────────────────────────────┘  │
│                       │                                         │
│          ┌────────────┼────────────┐                           │
│          ▼            ▼            ▼                           │
│   ┌──────────┐ ┌──────────┐ ┌──────────┐                      │
│   │   USB    │ │ Network  │ │   BLE    │                      │
│   │ Printer  │ │ Printer  │ │ Printer  │                      │
│   │(rusb lib)│ │(TCP/9100)│ │(btleplug)│                      │
│   └──────────┘ └──────────┘ └──────────┘                      │
└─────────────────────────────────────────────────────────────────┘
```

## Module Structure

### Rust Backend (`src-tauri/src/`)

#### `main.rs` - Entry Point & IPC

**Responsibilities:**

- Initialize Tauri app + system tray
- Register IPC handlers for frontend communication
- Spawn background tasks (queue processor, cleanup, telemetry)
- Start HTTP fallback API server (localhost:8043)
- Initialize auto-updater background task

**Key Functions:**

```rust
#[tauri::command]
async fn discover_printers(state: State<'_, AppState>) -> Result<Vec<Printer>>

#[tauri::command]
async fn save_config(config: AppConfig, app: AppHandle, state: State<'_, AppState>) -> Result<()>

#[tauri::command]
async fn connect_realtime(state: State<'_, AppState>) -> Result<()>
```

#### `config.rs` - Configuration Management

**Data Structures:**

- `AppConfig`: Top-level configuration (restaurant ID, auth token, printers)
- `PrinterConfig`: Individual printer settings (connection type, station, capabilities)
- `PrinterCapabilities`: Printer features (cutter, drawer, QR codes, max width)

**Methods:**

- `database_path()`: Platform-specific SQLite location
- Managed via `tauri-plugin-store` (no manual file I/O)

#### `escpos.rs` - ESC/POS Command Builder

**Command Generation:**

- Text formatting (bold, underline, alignment, size)
- Barcodes (EAN13, Code39, Code128)
- QR codes (via `qrcode` crate)
- Paper cutting (full/partial)
- Cash drawer kick

**Example:**

```rust
pub struct ESCPOSBuilder {
    commands: Vec<u8>,
}

impl ESCPOSBuilder {
    pub fn bold(mut self) -> Self {
        self.commands.extend_from_slice(&[0x1B, 0x45, 0x01]);
        self
    }

    pub fn cut(mut self) -> Self {
        self.commands.extend_from_slice(&[0x1D, 0x56, 0x00]);
        self
    }
}
```

#### `printer.rs` - Printer Manager

**Printer Abstraction:**

```rust
pub enum Printer {
    USB(USBPrinter),
    Network(NetworkPrinter),
    Bluetooth(BLEPrinter),
}

impl Printer {
    pub async fn print(&self, commands: &[u8]) -> Result<()>
    pub async fn is_online(&self) -> bool
}
```

**Circuit Breaker Integration:**

- Each printer has dedicated circuit breaker
- State: CLOSED (normal), OPEN (disabled), HALF_OPEN (testing)
- Threshold: 5 consecutive failures → OPEN
- Timeout: 5 minutes in OPEN before HALF_OPEN

#### `queue.rs` - SQLite Queue Manager

**Queue Operations:**

- `enqueue(job)`: Add job to SQLite queue
- `dequeue()`: Get next job (priority order)
- `retry(job_id)`: Increment retry count, reschedule
- `complete(job_id)`: Mark job as completed
- `get_stats()`: Queue depth, pending/failed counts

**Encryption:**

- SQLite encrypted with `sqlcipher`
- Key derived from restaurant ID via PBKDF2
- Protects order contents at rest

**Retry Strategy:**

```rust
pub struct RetryStrategy {
    max_retries: u32,      // 3
    base_delay: Duration,   // 2 seconds
    max_delay: Duration,    // 5 minutes
}

fn next_retry_delay(attempt: u32) -> Duration {
    min(base_delay * 2^attempt, max_delay)
}
// Delays: 2s, 4s, 8s, 16s, 32s, ...
```

#### `realtime.rs` - Supabase Realtime Client

**WebSocket Management:**

```rust
pub struct RealtimeClient {
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    channels: HashMap<String, ChannelConfig>,
}

impl RealtimeClient {
    pub async fn subscribe(&mut self, channel: String)
    pub async fn send(&mut self, channel: String, event: String, payload: Value)
    pub async fn listen(&mut self) -> Result<Message>
}
```

**Channel Structure:**

- `restaurant:{restaurant_id}:print-job` - POS → Daemon (job delivery)
- `restaurant:{restaurant_id}:printer-status` - Daemon → POS (printer online/offline)

**Heartbeat:**

- Sends ping every 30 seconds
- Reconnects on missed pong (5 consecutive failures)

#### `discovery.rs` - Multi-Protocol Discovery

**USB Discovery:**

```rust
pub fn discover_usb_printers() -> Vec<USBDevice> {
    rusb::devices().filter(|d| {
        KNOWN_VENDORS.contains(&d.vendor_id())
    }).collect()
}

const KNOWN_VENDORS: &[u16] = &[
    0x04b8, // Epson
    0x0519, // Star Micronics
    0x04f9, // Brother
    0x1d90, // Citizen
];
```

**Network Discovery (mDNS + SNMP):**

```rust
pub async fn discover_network_printers() -> Vec<NetworkDevice> {
    let mdns = mdns_sd::ServiceDaemon::new()?;
    mdns.browse("_ipp._tcp.local")?; // IPP printers
    mdns.browse("_printer._tcp.local")?; // Generic printers

    // Fallback: SNMP scan on local subnet
    snmp_scan("192.168.1.0/24", 161).await
}
```

**Bluetooth Discovery:**

```rust
pub async fn discover_ble_printers() -> Vec<BLEDevice> {
    let manager = btleplug::Manager::new().await?;
    let adapters = manager.adapters().await?;

    for adapter in adapters {
        adapter.start_scan(ScanFilter::default()).await?;
    }
}
```

#### `routing.rs` - Kitchen Router

**Routing Logic:**

```rust
pub struct KitchenRouter {
    station_printers: HashMap<String, Vec<PrinterId>>,
}

impl KitchenRouter {
    pub async fn route_order(&self, order: Order) -> Vec<PrintJob> {
        let mut jobs = Vec::new();

        // Group items by routing_group_id (from menu_items table)
        let groups = self.group_by_station(&order.items).await;

        for (station, items) in groups {
            let printers = self.get_printers_for_station(&station);
            jobs.push(PrintJob {
                station,
                printer_ids: printers,
                items,
                order_number: order.order_number,
            });
        }

        jobs
    }

    async fn group_by_station(&self, items: &[OrderItem]) -> HashMap<String, Vec<OrderItem>> {
        // Query Supabase: SELECT routing_group_id FROM menu_items WHERE id IN (...)
        // Group items by station (e.g., "bar", "grill", "kitchen")
    }
}
```

**Backup Routing:**

- If primary printer circuit is OPEN, route to backup
- If no backup available, log error to Sentry + notify POS

#### `circuit_breaker.rs` - Fault Isolation

**State Machine:**

```
        failure_count >= threshold
CLOSED  ─────────────────────────────▶  OPEN
  ▲                                       │
  │                                       │ timeout elapsed
  │         success                       │
  └─────────  HALF_OPEN  ◀────────────────┘
                  │
                  │ failure
                  │
                  ▼
                OPEN
```

**Implementation:**

```rust
pub struct CircuitBreaker {
    state: State,
    failure_count: u32,
    threshold: u32,           // 5
    timeout: Duration,         // 5 minutes
    last_failure_time: Option<Instant>,
}

impl CircuitBreaker {
    pub async fn execute<F>(&mut self, f: F) -> Result<()>
    where F: Future<Output = Result<()>>
    {
        match self.state {
            State::OPEN => {
                if self.last_failure_time.elapsed() > self.timeout {
                    self.state = State::HALF_OPEN;
                } else {
                    return Err("Circuit OPEN");
                }
            }
            _ => {}
        }

        match f.await {
            Ok(_) => {
                self.state = State::CLOSED;
                self.failure_count = 0;
                Ok(())
            }
            Err(e) => {
                self.failure_count += 1;
                if self.failure_count >= self.threshold {
                    self.state = State::OPEN;
                    self.last_failure_time = Some(Instant::now());
                }
                Err(e)
            }
        }
    }
}
```

#### `auth.rs` - JWT Authentication

**Token Validation:**

```rust
pub struct JWTManager {
    secret: String,
}

impl JWTManager {
    pub fn validate(&self, token: &str) -> Result<Claims> {
        let validation = Validation::new(Algorithm::HS256);
        let token_data = decode::<Claims>(token, &DecodingKey::from_secret(self.secret.as_ref()), &validation)?;

        // Check expiration
        if token_data.claims.exp < Utc::now().timestamp() {
            return Err("Token expired");
        }

        // Check permissions
        if !token_data.claims.permissions.contains("print") {
            return Err("Insufficient permissions");
        }

        Ok(token_data.claims)
    }
}
```

**Token Rotation:**

- POS generates new token daily
- Daemon accepts current + previous token (1 hour grace period)
- Prevents service interruption during rotation

#### `telemetry.rs` - Metrics Collection

**Metrics:**

- `print_jobs_total`: Total jobs processed
- `print_jobs_success`: Successfully printed jobs
- `print_jobs_failed`: Failed jobs (after retries exhausted)
- `queue_depth`: Current queue size
- `circuit_breaker_trips`: Number of circuit breaker activations
- `realtime_reconnects`: WebSocket reconnection count

**Reporter:**

- Logs metrics every 5 minutes
- Sends to Supabase `daemon_metrics` table (future enhancement)

#### `api.rs` - HTTP Fallback API

**Endpoints:**

```
POST /api/print
Authorization: Bearer <JWT>
Content-Type: application/json

{
  "restaurant_id": "rest_123",
  "order_id": "R001-20260127-0042",
  "station": "bar",
  "items": [...]
}
```

**Use Case:**

- Fallback when Supabase Realtime unreachable
- Local POS apps on same machine (no network required)
- Development/testing without Supabase

#### `updater.rs` - Auto-Update Manager

**Update Flow:**

1. Check for updates every 6 hours
2. Download update in background (delta patches when possible)
3. Wait for idle state (no print jobs for 5 minutes)
4. Install and restart daemon

**Idle Detection:**

```rust
async fn wait_for_idle(&self) {
    let mut idle_since: Option<Instant> = None;

    loop {
        let queue_depth = self.queue_manager.get_stats().await?.pending;

        if queue_depth == 0 {
            if idle_since.is_none() {
                idle_since = Some(Instant::now());
            } else if idle_since.unwrap().elapsed() >= MIN_IDLE_TIME {
                return; // Safe to update
            }
        } else {
            idle_since = None; // Reset if jobs appear
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
```

#### `sentry_init.rs` - Crash Reporting

**PII Stripping:**

```rust
fn strip_pii_from_message(message: &str) -> String {
    let mut cleaned = message.to_string();

    // Strip emails
    cleaned = email_regex.replace_all(&cleaned, "[EMAIL_REDACTED]");

    // Strip phone numbers
    cleaned = phone_regex.replace_all(&cleaned, "[PHONE_REDACTED]");

    // Strip UUIDs
    cleaned = uuid_regex.replace_all(&cleaned, "[UUID_REDACTED]");

    cleaned
}
```

**Context Tags:**

- `platform`: macOS/Windows/Linux
- `daemon_version`: 1.0.0
- `restaurant_id_hash`: MD5(restaurant_id) (anonymized)

### React Frontend (`src/`)

#### Setup Wizard Flow

```
WelcomeStep
    │
    ▼
AuthenticationStep (QR scanner OR manual JWT input)
    │
    ▼
DiscoveryStep (USB + Network + BLE scan, ~30s)
    │
    ▼
AssignmentStep (Drag-and-drop printers to stations)
    │
    ▼
CompleteStep (Minimize to tray)
```

#### IPC Communication

```typescript
import { invoke } from '@tauri-apps/api/core'

// Discover printers
const printers = await invoke<Printer[]>('discover_printers')

// Save configuration
await invoke('save_config', { config })

// Connect to Supabase Realtime
await invoke('connect_realtime')
```

## Communication Patterns

### POS → Daemon

**Primary: Supabase Realtime (WebSocket)**

```typescript
// POS sends print job
await supabase
  .channel(`restaurant:${restaurantId}:print-job`)
  .send({
    type: 'broadcast',
    event: 'new-job',
    payload: {
      job_id: 'job_123',
      order_id: 'R001-20260127-0042',
      station: 'bar',
      items: [...]
    }
  })
```

**Fallback: HTTP API**

```typescript
// If Realtime fails
await fetch('http://localhost:8043/api/print', {
  method: 'POST',
  headers: {
    Authorization: `Bearer ${jwt}`,
    'Content-Type': 'application/json',
  },
  body: JSON.stringify(printJob),
})
```

### Daemon → POS

**Printer Status Updates:**

```typescript
// Daemon broadcasts printer status
await supabase.channel(`restaurant:${restaurantId}:printer-status`).send({
  type: 'broadcast',
  event: 'status-update',
  payload: {
    printer_id: 'usb_04b8_0e15',
    status: 'online',
    circuit_breaker_state: 'CLOSED',
  },
})
```

## Data Flow

### Print Job Lifecycle

```
1. POS creates order
   └─▶ Order saved to database (orders table)

2. POS sends print job via Realtime
   └─▶ Daemon receives broadcast on restaurant:{id}:print-job

3. Daemon validates JWT
   └─▶ auth.rs: JWTManager::validate()

4. Daemon routes order
   └─▶ routing.rs: KitchenRouter::route_order()
   └─▶ Query Supabase for menu_items.routing_group_id

5. Daemon enqueues jobs (one per station)
   └─▶ queue.rs: QueueManager::enqueue()
   └─▶ SQLite: INSERT INTO print_jobs

6. Queue processor dequeues job
   └─▶ queue.rs: QueueManager::dequeue()

7. Printer manager prints job
   └─▶ printer.rs: Printer::print()
   └─▶ Circuit breaker protects against faults

8. On success:
   └─▶ queue.rs: QueueManager::complete()
   └─▶ telemetry.rs: increment print_jobs_success

9. On failure:
   └─▶ queue.rs: QueueManager::retry() (exponential backoff)
   └─▶ After 3 retries: mark failed, log to Sentry
```

## Error Handling

### Error Types

```rust
#[derive(thiserror::Error, Debug)]
pub enum DaemonError {
    #[error("Printer offline: {0}")]
    PrinterOffline(String),

    #[error("Circuit breaker open: {0}")]
    CircuitBreakerOpen(String),

    #[error("Queue full (depth: {0})")]
    QueueFull(usize),

    #[error("JWT validation failed: {0}")]
    AuthenticationFailed(String),

    #[error("Supabase Realtime error: {0}")]
    RealtimeError(String),
}
```

### Error Recovery

1. **Printer Offline**: Circuit breaker opens → route to backup → alert POS
2. **Queue Full**: Reject new jobs → alert POS → suggest increasing capacity
3. **JWT Expired**: Disconnect Realtime → show notification → user must refresh token
4. **Realtime Disconnected**: Switch to HTTP fallback → reconnect in background

## Security

### Threat Model

**Protected Against:**

- ✅ Unauthorized print job injection (JWT validation)
- ✅ Order content leakage (SQLite encryption)
- ✅ Man-in-the-middle (TLS for Realtime + HTTP)
- ✅ PII leakage to Sentry (automatic stripping)
- ✅ Token theft (short-lived JWTs, daily rotation)

**Not Protected Against:**

- ❌ Physical access to machine (SQLite key derivable from restaurant ID)
- ❌ Malicious POS app (daemon trusts authenticated POS)
- ❌ Printer firmware exploits (out of scope)

### Security Boundaries

```
┌──────────────────────────┐
│  Untrusted: Internet     │  ← TLS, JWT validation
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│  Trusted: Daemon         │  ← Encrypted SQLite
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│  Trusted: Local Printers │  ← Physical security only
└──────────────────────────┘
```

## Performance Characteristics

### Latency

- **P50**: < 50ms (order received → print starts)
- **P95**: < 100ms
- **P99**: < 500ms

### Throughput

- **Peak**: 50 orders/minute (tested)
- **Sustained**: 20 orders/minute (typical restaurant)

### Resource Usage

- **Memory**: 30-40 MB idle, 50-60 MB under load
- **CPU**: <1% idle, <5% under load
- **Disk**: SQLite database grows ~1 KB per order (with encryption overhead)
