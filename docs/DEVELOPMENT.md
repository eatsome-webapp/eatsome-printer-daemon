# Development Guide

Complete guide for developing the Eatsome Printer Service locally.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Debugging](#debugging)
- [Testing](#testing)
- [Code Style](#code-style)
- [Common Development Tasks](#common-development-tasks)
- [Troubleshooting Development Issues](#troubleshooting-development-issues)

## Getting Started

### Prerequisites

#### System Dependencies

**macOS:**

```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install Homebrew (if not already installed)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Windows:**

```powershell
# Install Rust
# Download and run: https://win.rustup.rs/

# Install Visual Studio Build Tools 2019+
# Download from: https://visualstudio.microsoft.com/downloads/
# Select "C++ Desktop Development" workload
```

**Linux (Ubuntu/Debian):**

```bash
# Install system dependencies
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libusb-1.0-0-dev \
  libudev-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Development Tools

```bash
# Install Node.js 20+
# macOS: brew install node@20
# Windows: https://nodejs.org/en/download/
# Linux: https://github.com/nodesource/distributions

# Install pnpm
npm install -g pnpm

# Verify installations
rustc --version  # Should be 1.70+
node --version   # Should be 20.9+
pnpm --version   # Should be 8.0+
```

### Clone Repository

```bash
git clone https://github.com/eatsome/eatsome.git
cd eatsome/apps/printer-daemon-tauri
```

### Install Dependencies

```bash
# Install Node.js dependencies
pnpm install

# Dependencies are automatically installed for:
# - React frontend (src/)
# - Tauri CLI (build tool)
# Rust dependencies are fetched automatically on first build
```

### Initial Build

```bash
# Build Rust backend + React frontend
pnpm tauri build

# First build takes 5-10 minutes (compiles all Rust dependencies)
# Subsequent builds are incremental (~30 seconds)
```

## Development Workflow

### Running Development Server

```bash
# Start development server with hot-reload
pnpm tauri:dev

# This starts:
# 1. Vite dev server (React frontend, port 1420)
# 2. Tauri app (Rust backend with React WebView)
# 3. File watchers (auto-reload on changes)
```

**Hot-Reload Behavior:**

- **React changes**: Instant hot-module replacement (no restart)
- **Rust changes**: Full rebuild + restart (~5-10 seconds)
- **Config changes** (tauri.conf.json): Requires restart

### Project Structure

```
apps/printer-daemon-tauri/
├── src/                      # React frontend
│   ├── App.tsx               # Main component (setup wizard router)
│   ├── components/           # Setup wizard steps
│   ├── schemas/              # Zod validation (IPC data)
│   ├── sentry.ts             # Frontend crash reporting
│   └── main.tsx              # Entry point
├── src-tauri/                # Rust backend
│   ├── src/
│   │   ├── main.rs           # Entry point + IPC handlers
│   │   ├── [module].rs       # Core modules (see ARCHITECTURE.md)
│   │   └── lib.rs            # (optional) Library exports
│   ├── Cargo.toml            # Rust dependencies
│   ├── tauri.conf.json       # Tauri configuration
│   ├── icons/                # App icons (all platforms)
│   └── build.rs              # Build script
├── install-scripts/          # Platform-specific installers
├── docs/                     # Documentation
├── package.json              # Node.js dependencies
├── vite.config.ts            # Vite configuration
└── .env.example              # Environment variables template
```

### Development Environment Variables

Create `.env` file (not committed to git):

```bash
# Sentry Crash Reporting (Optional for development)
SENTRY_DSN=https://your-dsn@o123456.ingest.sentry.io/789012
SENTRY_ENVIRONMENT=development
SENTRY_TRACES_SAMPLE_RATE=1.0

VITE_SENTRY_DSN=https://your-dsn@o123456.ingest.sentry.io/789012
VITE_SENTRY_ENVIRONMENT=development
VITE_SENTRY_TRACES_SAMPLE_RATE=1.0

# Supabase (Use test project for development)
VITE_SUPABASE_URL=https://your-project.supabase.co
VITE_SUPABASE_ANON_KEY=your-anon-key
```

## Debugging

### Rust Backend Debugging

#### VS Code (Recommended)

**Install Extension:**

- CodeLLDB (vadimcn.vscode-lldb)

**Launch Configuration** (`.vscode/launch.json`):

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Tauri Development Debug",
      "cargo": {
        "args": ["build", "--manifest-path=./src-tauri/Cargo.toml", "--no-default-features"]
      },
      "args": [],
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

**Usage:**

1. Set breakpoints in Rust code
2. Press F5 to start debugging
3. Debugger pauses at breakpoints
4. Inspect variables, step through code

#### Command Line Debugging

```bash
# Run with debug logs
RUST_LOG=debug pnpm tauri:dev

# Log levels: trace, debug, info, warn, error
# Filter by module: RUST_LOG=eatsome_printer_daemon::printer=debug

# Pretty print with colors
RUST_LOG=debug pnpm tauri:dev 2>&1 | bunyan
```

#### Common Debugging Tasks

**Check USB devices:**

```rust
// In src-tauri/src/discovery.rs
pub fn list_usb_devices() -> Result<()> {
    for device in rusb::devices()?.iter() {
        let desc = device.device_descriptor()?;
        println!("Device: {:04x}:{:04x}", desc.vendor_id(), desc.product_id());
    }
    Ok(())
}

// Call from main.rs temporarily
```

**Test ESC/POS commands:**

```rust
// In src-tauri/src/escpos.rs
let commands = ESCPOSBuilder::new()
    .bold()
    .text("TEST PRINT")
    .cut()
    .build();

std::fs::write("test.bin", &commands)?;
// Send test.bin to printer via: cat test.bin > /dev/usb/lp0
```

### React Frontend Debugging

#### Browser DevTools

```bash
# Start dev server
pnpm tauri:dev

# Open DevTools in Tauri window:
# macOS: Cmd+Option+I
# Windows/Linux: Ctrl+Shift+I
```

**Useful Panels:**

- **Console**: View logs, errors, warnings
- **Network**: Monitor IPC calls (appear as internal:// requests)
- **React DevTools**: Inspect component state (requires extension)

#### Debugging IPC Calls

```typescript
// Log all IPC calls
import { invoke } from '@tauri-apps/api/core'

const originalInvoke = invoke
invoke = async (cmd: string, args?: any) => {
  console.log('[IPC]', cmd, args)
  const result = await originalInvoke(cmd, args)
  console.log('[IPC Result]', cmd, result)
  return result
}
```

### Debugging Print Jobs

```bash
# Enable print job logging
RUST_LOG=eatsome_printer_daemon::queue=debug,eatsome_printer_daemon::printer=debug pnpm tauri:dev

# Check SQLite queue
sqlite3 ~/.config/eatsome-printer-service/print-queue.db
> SELECT * FROM print_jobs WHERE status = 'pending';
> SELECT * FROM print_jobs WHERE status = 'failed';
```

## Testing

### Rust Unit Tests

```bash
# Run all tests
cd src-tauri
cargo test

# Run specific module tests
cargo test queue::tests

# Run with output
cargo test -- --nocapture

# Run with debug logs
RUST_LOG=debug cargo test
```

**Example Test:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_queue_enqueue_dequeue() {
        let queue = QueueManager::new(":memory:", None).await.unwrap();

        let job = PrintJob {
            id: "job_123".to_string(),
            order_id: "R001-20260127-0042".to_string(),
            station: "bar".to_string(),
            items: vec![],
        };

        queue.enqueue(job.clone()).await.unwrap();

        let dequeued = queue.dequeue().await.unwrap();
        assert_eq!(dequeued.id, job.id);
    }
}
```

### Integration Tests

```bash
# Create test file: src-tauri/tests/integration_test.rs

use eatsome_printer_daemon::*;

#[tokio::test]
async fn test_end_to_end_print() {
    // 1. Initialize daemon
    // 2. Enqueue print job
    // 3. Verify job processed
    // 4. Check printer received commands
}
```

### Manual Testing

**Test Print Flow:**

```bash
# 1. Start daemon in dev mode
pnpm tauri:dev

# 2. Open setup wizard
# 3. Authenticate with test JWT
# 4. Discover test printers
# 5. Assign to stations
# 6. Send test print job via Supabase Realtime or HTTP API

# Example: HTTP API test
curl -X POST http://localhost:8043/api/print \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "restaurant_id": "rest_123",
    "order_id": "TEST_001",
    "station": "bar",
    "items": [{"name": "Test Item", "quantity": 1}]
  }'
```

**Load Testing:**

```bash
# Install k6
brew install k6  # macOS
# Or: https://k6.io/docs/getting-started/installation/

# Create load test script: load-test.js
import http from 'k6/http';
export let options = {
  vus: 10,        // 10 virtual users
  duration: '30s', // Run for 30 seconds
};
export default function () {
  http.post('http://localhost:8043/api/print', JSON.stringify({
    restaurant_id: 'rest_123',
    order_id: `TEST_${Date.now()}`,
    station: 'bar',
    items: [{ name: 'Test Item', quantity: 1 }]
  }), {
    headers: {
      'Authorization': 'Bearer JWT_TOKEN_HERE',
      'Content-Type': 'application/json',
    },
  });
}

# Run load test
k6 run load-test.js
```

## Code Style

### Rust

**Formatter:**

```bash
# Format all Rust code
cd src-tauri
cargo fmt

# Check formatting (CI)
cargo fmt -- --check
```

**Linter:**

```bash
# Run Clippy (Rust linter)
cargo clippy -- -D warnings

# Fix auto-fixable issues
cargo clippy --fix
```

**Style Guide:**

- Use `snake_case` for functions, variables, modules
- Use `CamelCase` for types, structs, enums
- Prefer `Result<T>` over panics
- Document public functions with `///` doc comments

### TypeScript/React

**Formatter:**

```bash
# Format all TypeScript/React code
pnpm prettier --write src/

# Check formatting (CI)
pnpm prettier --check src/
```

**Linter:**

```bash
# Run ESLint
pnpm eslint src/

# Fix auto-fixable issues
pnpm eslint --fix src/
```

**Style Guide:**

- Use `camelCase` for functions, variables
- Use `PascalCase` for components, types
- Prefer functional components over class components
- Use Zod schemas for IPC data validation

## Common Development Tasks

### Adding a New IPC Command

1. **Define Rust handler** (`src-tauri/src/main.rs`):

```rust
#[tauri::command]
async fn my_new_command(arg: String, state: State<'_, AppState>) -> Result<String, String> {
    // Implementation
    Ok(format!("Received: {}", arg))
}
```

2. **Register handler**:

```rust
.invoke_handler(tauri::generate_handler![
    get_config,
    // ... existing commands
    my_new_command,  // Add here
])
```

3. **Call from React**:

```typescript
import { invoke } from '@tauri-apps/api/core'

const result = await invoke<string>('my_new_command', { arg: 'test' })
```

### Adding a New Printer Protocol

1. **Create printer struct** (`src-tauri/src/printer.rs`):

```rust
pub struct MyProtocolPrinter {
    address: String,
}

impl MyProtocolPrinter {
    pub async fn print(&self, commands: &[u8]) -> Result<()> {
        // Implementation
    }
}
```

2. **Add to Printer enum**:

```rust
pub enum Printer {
    USB(USBPrinter),
    Network(NetworkPrinter),
    Bluetooth(BLEPrinter),
    MyProtocol(MyProtocolPrinter),  // Add here
}
```

3. **Update discovery** (`src-tauri/src/discovery.rs`):

```rust
pub async fn discover_my_protocol_printers() -> Vec<MyProtocolPrinter> {
    // Implementation
}
```

### Database Migration

```bash
# Create migration file
cd src-tauri
echo "ALTER TABLE print_jobs ADD COLUMN new_field TEXT;" > migrations/0002_add_new_field.sql

# Apply migration (automatic on next run)
pnpm tauri:dev
```

### Updating Dependencies

```bash
# Update Rust dependencies
cd src-tauri
cargo update

# Update Node.js dependencies
cd ..
pnpm update

# Check for outdated dependencies
cargo outdated
pnpm outdated
```

## Troubleshooting Development Issues

### "Cannot find Rust toolchain"

```bash
# Verify Rust installed
rustc --version

# If not installed:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add to PATH (if needed)
source $HOME/.cargo/env
```

### "WebKit2GTK not found" (Linux)

```bash
# Install webkit2gtk
sudo apt-get install libwebkit2gtk-4.1-dev

# Or for older systems:
sudo apt-get install libwebkit2gtk-4.0-dev
```

### "USB device access denied" (Development)

**macOS:**

```bash
# Grant Terminal USB permissions
# System Preferences → Security & Privacy → Privacy → Files and Folders
# Enable USB for Terminal.app
```

**Linux:**

```bash
# Add user to lp group
sudo usermod -a -G lp $USER

# Create temporary udev rules
sudo cp install-scripts/linux/60-eatsome-printer.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules

# Log out and log back in
```

### "Port 1420 already in use"

```bash
# Kill process using port
lsof -ti:1420 | xargs kill -9

# Or change port in vite.config.ts:
export default defineConfig({
  server: { port: 1421 }
})
```

### Rust Build Fails with "linker error"

**macOS:**

```bash
# Install Xcode Command Line Tools
xcode-select --install
```

**Windows:**

```powershell
# Ensure Visual Studio Build Tools installed with C++ workload
```

**Linux:**

```bash
# Install build essentials
sudo apt-get install build-essential
```

### Hot-Reload Not Working

```bash
# Clear Tauri cache
rm -rf target/

# Clear Vite cache
rm -rf node_modules/.vite

# Reinstall dependencies
pnpm install

# Restart dev server
pnpm tauri:dev
```

## Contributing

Before submitting a pull request:

1. **Run tests**: `cargo test && pnpm test`
2. **Format code**: `cargo fmt && pnpm prettier --write src/`
3. **Lint code**: `cargo clippy && pnpm eslint src/`
4. **Build successfully**: `pnpm tauri build`
5. **Update documentation** if adding features
6. **Add changelog entry** in CHANGELOG.md

## Additional Resources

- [Tauri Documentation](https://v2.tauri.app/)
- [Rust Book](https://doc.rust-lang.org/book/)
- [React Documentation](https://react.dev/)
- [SQLite Documentation](https://www.sqlite.org/docs.html)
- [ESC/POS Command Reference](https://reference.epson-biz.com/modules/ref_escpos/)
