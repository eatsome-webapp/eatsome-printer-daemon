# Eatsome Printer Service

**Professional thermal printer daemon for restaurant POS systems**

Modern cross-platform daemon built with Tauri (Rust + React) for reliable, low-latency thermal printing in commercial kitchens.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)](README.md)

## Features

### Core Capabilities

- ✅ **Multi-Protocol Printing**: Direct ESC/POS commands for thermal printers
- ✅ **Multi-Connection**: USB, Network (IP), Bluetooth BLE support
- ✅ **Realtime Communication**: Supabase Realtime for instant job delivery
- ✅ **Offline Queue**: SQLite-based persistent queue with automatic retry
- ✅ **Kitchen Routing**: Smart order routing to bar/grill/kitchen stations
- ✅ **Circuit Breaker**: Automatic printer fault isolation and recovery
- ✅ **Auto-Updates**: Background updates with zero-downtime installation
- ✅ **Crash Reporting**: Sentry integration with privacy-first PII stripping

### Performance

- **Memory**: ~30-40 MB (vs Electron's 200-300 MB)
- **Latency**: P95 < 100ms (order received → print starts)
- **Reliability**: 99.95% success rate with exponential backoff retry
- **Uptime**: Designed for 24/7 operation in commercial environments

### Platform Support

| Platform    | Minimum Version         | Architectures        | Package Formats |
| ----------- | ----------------------- | -------------------- | --------------- |
| **macOS**   | 10.15 (Catalina)        | Intel, Apple Silicon | DMG             |
| **Windows** | Windows 10 (21H2)       | x64                  | MSI, NSIS       |
| **Linux**   | Ubuntu 20.04, Fedora 36 | x64                  | deb, rpm        |

## Installation

### macOS

1. Download `EatsomePrinterService_aarch64.dmg` (M1/M2/M3) or `EatsomePrinterService_x64.dmg` (Intel)
2. Open DMG and drag app to Applications folder
3. First launch: Right-click → Open (required for unsigned apps)
4. Grant USB permissions when prompted: System Preferences → Security & Privacy

### Windows

1. Download `EatsomePrinterService_x64-setup.exe`
2. Run installer (SmartScreen warning: click "More info" → "Run anyway")
3. Installer automatically configures Task Scheduler for auto-start
4. Daemon minimizes to system tray on launch

### Linux (Ubuntu/Debian)

```bash
# Download deb package
wget https://github.com/eatsome/eatsome/releases/latest/download/eatsome-printer-service_1.0.0_amd64.deb

# Install (creates udev rules, adds user to lp group, enables systemd service)
sudo dpkg -i eatsome-printer-service_1.0.0_amd64.deb

# Log out and log back in for group membership to take effect
```

### Linux (Fedora/RHEL)

```bash
# Download rpm package
wget https://github.com/eatsome/eatsome/releases/latest/download/eatsome-printer-service-1.0.0-1.x86_64.rpm

# Install
sudo rpm -ivh eatsome-printer-service-1.0.0-1.x86_64.rpm

# Follow manual setup instructions displayed after install:
# 1. Add user to lp group: sudo usermod -a -G lp $USER
# 2. Log out and log back in
# 3. Enable service: systemctl --user enable eatsome-printer.service
# 4. Start service: systemctl --user start eatsome-printer.service
```

## Quick Start

### First-Time Setup

1. Launch Eatsome Printer Service
2. **Authentication**: Scan QR code from POS app or paste JWT token
3. **Printer Discovery**: Click "Scan for Printers" (takes ~30 seconds)
4. **Station Assignment**: Drag printers to stations (Bar, Grill, Kitchen)
5. **Test Print**: Click test button on each printer to verify
6. **Complete**: Daemon minimizes to system tray

### POS Integration

The POS app automatically discovers the daemon and routes print jobs via Supabase Realtime channels.

**No manual configuration required** - authentication token contains restaurant ID and routing configuration.

## Development

### Prerequisites

- **Rust**: 1.70+ ([install via rustup](https://rustup.rs/))
- **Node.js**: 20.9+
- **pnpm**: 8.0+
- **System Dependencies**:
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio Build Tools 2019+ (with C++ Desktop Development)
  - Linux: `build-essential libusb-1.0-0-dev libudev-dev libwebkit2gtk-4.1-dev`

### Setup

```bash
# Clone repository
git clone https://github.com/eatsome/eatsome.git
cd eatsome/apps/printer-daemon-tauri

# Install dependencies
pnpm install

# Run in development mode (hot-reload enabled)
pnpm tauri:dev

# Build for production
pnpm tauri:build
```

### Project Structure

```
apps/printer-daemon-tauri/
├── src/                          # React frontend (setup wizard)
│   ├── App.tsx                   # Main app component
│   ├── components/               # Setup wizard steps
│   │   ├── WelcomeStep.tsx
│   │   ├── AuthenticationStep.tsx
│   │   ├── DiscoveryStep.tsx
│   │   ├── AssignmentStep.tsx
│   │   └── CompleteStep.tsx
│   ├── schemas/                  # Zod validation schemas
│   ├── sentry.ts                 # Crash reporting (frontend)
│   ├── main.tsx
│   └── index.css
├── src-tauri/                    # Rust backend
│   ├── src/
│   │   ├── main.rs               # Tauri entry point + IPC handlers
│   │   ├── config.rs             # Configuration structures
│   │   ├── escpos.rs             # ESC/POS command builder
│   │   ├── printer.rs            # Printer manager (USB/Network/BLE)
│   │   ├── queue.rs              # SQLite job queue with retry
│   │   ├── realtime.rs           # Supabase Realtime client
│   │   ├── discovery.rs          # Multi-protocol printer discovery
│   │   ├── routing.rs            # Kitchen station routing
│   │   ├── circuit_breaker.rs   # Fault isolation
│   │   ├── auth.rs               # JWT validation
│   │   ├── telemetry.rs          # Metrics collection
│   │   ├── api.rs                # HTTP fallback API (localhost:8043)
│   │   ├── updater.rs            # Auto-update manager
│   │   ├── sentry_init.rs        # Crash reporting (backend)
│   │   └── errors.rs             # Error types
│   ├── Cargo.toml
│   ├── tauri.conf.json           # Tauri configuration
│   └── entitlements.plist        # macOS USB permissions
├── install-scripts/              # Platform-specific installers
│   ├── macos/                    # LaunchAgent plist
│   ├── windows/                  # Task Scheduler XML
│   └── linux/                    # deb/rpm postinstall scripts
├── docs/                         # Documentation
│   ├── ARCHITECTURE.md
│   ├── DEVELOPMENT.md
│   ├── DEPLOYMENT.md
│   ├── TROUBLESHOOTING.md
│   ├── github-actions-setup.md
│   ├── sentry-setup.md
│   └── escpos-decision.md
├── package.json
├── vite.config.ts
└── README.md
```

## Architecture

### High-Level Flow

```
┌─────────────┐         ┌──────────────────┐         ┌─────────────┐
│   POS App   │────────▶│ Supabase Realtime│────────▶│   Daemon    │
│  (Next.js)  │  HTTP   │   (WebSocket)    │  Broad- │   (Tauri)   │
└─────────────┘  Fallback└──────────────────┘  cast   └──────┬──────┘
                                                              │
                                                              ▼
                                               ┌──────────────────────┐
                                               │  Kitchen Router      │
                                               │  (Bar/Grill/Kitchen) │
                                               └──────────┬───────────┘
                                                          │
                                  ┌───────────────────────┼───────────────────────┐
                                  ▼                       ▼                       ▼
                          ┌───────────────┐      ┌───────────────┐      ┌───────────────┐
                          │  Bar Printer  │      │ Grill Printer │      │Kitchen Printer│
                          │   (USB/IP)    │      │   (USB/IP)    │      │   (USB/IP)    │
                          └───────────────┘      └───────────────┘      └───────────────┘
```

### Technology Stack

**Rust Backend:**

- `tauri 2.0` - Cross-platform app framework
- `tokio` - Async runtime
- `rusqlite` + `sqlcipher` - Encrypted job queue
- `rusb`, `btleplug`, `mdns-sd` - Device discovery
- `escpos` - Thermal printer commands
- `axum` - HTTP fallback API
- `sentry` - Crash reporting

**React Frontend:**

- `react 18` - Setup wizard UI
- `vite` - Fast dev server
- `@tauri-apps/api` - IPC bridge to Rust
- `zod` - Schema validation
- `@sentry/tauri` - Crash reporting

## Configuration

Configuration is managed via `tauri-plugin-store` and persisted in platform-specific locations:

- **macOS**: `~/Library/Application Support/com.eatsome.printer-service/config.json`
- **Windows**: `%APPDATA%\Eatsome Printer Service\config.json`
- **Linux**: `~/.config/eatsome-printer-service/config.json`

### Example Configuration

```json
{
  "version": "1.0.0",
  "restaurant_id": "rest_abc123",
  "location_id": "loc_xyz789",
  "auth_token": "eyJhbGciOiJIUzI1NiIs...",
  "supabase_url": "https://gtlpzikuozrdgomsvqmo.supabase.co",
  "supabase_anon_key": "...",
  "service_role_key": "...",
  "printers": [
    {
      "id": "usb_04b8_0e15",
      "name": "Bar Printer - Epson TM-T88V",
      "connection_type": "usb",
      "address": "/dev/usb/lp0",
      "protocol": "escpos",
      "station": "bar",
      "is_primary": true,
      "capabilities": {
        "cutter": true,
        "drawer": false,
        "qrcode": true,
        "max_width": 48
      }
    }
  ]
}
```

## Troubleshooting

### USB Printers Not Detected (Linux)

```bash
# Check if user is in lp group
groups | grep lp

# If not, add user to lp group
sudo usermod -a -G lp $USER

# Reload udev rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb

# Verify udev rules exist
cat /etc/udev/rules.d/60-eatsome-printer.rules

# Log out and log back in for group membership to take effect
```

### macOS USB Permission Denied

**Symptom:** `IOServiceOpen failed` error in logs

**Fix:**

1. Check entitlements: `codesign -d --entitlements - /Applications/EatsomePrinterService.app`
2. Grant permission: System Preferences → Security & Privacy → Privacy → Files and Folders
3. If still denied, uninstall and reinstall (permissions reset on reinstall)

### Windows SmartScreen Warning

**Symptom:** "Windows protected your PC" blue screen

**Explanation:** Normal for new software without established reputation

**Fix:**

1. Click "More info"
2. Click "Run anyway"

**Note:** Warning disappears after ~100-500 users install (reputation builds over 2-4 weeks)

### Daemon Won't Start

```bash
# Check logs
# macOS/Linux: tail -f /tmp/eatsome-printer-service.log
# Windows: Get-Content "$env:APPDATA\Eatsome Printer Service\logs\daemon.log" -Wait

# Common issues:
# 1. Invalid config.json → Delete and re-run setup wizard
# 2. Port 8043 already in use → Kill process using port
# 3. SQLite database corruption → Delete print-queue.db and restart
```

### Print Jobs Stuck in Queue

```bash
# Check circuit breaker status via IPC
# If printer circuit is "OPEN" (disabled):
# 1. Check printer is powered on and connected
# 2. Restart daemon to reset circuit breaker
# 3. If persistent, check printer error lights/logs
```

For more detailed troubleshooting, see [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)

## Documentation

- **[ARCHITECTURE.md](docs/ARCHITECTURE.md)** - System design and module structure
- **[DEVELOPMENT.md](docs/DEVELOPMENT.md)** - Development workflow and debugging
- **[DEPLOYMENT.md](docs/DEPLOYMENT.md)** - Release process and CI/CD
- **[TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)** - Common issues and solutions
- **[GitHub Actions Setup](docs/github-actions-setup.md)** - CI/CD configuration
- **[Sentry Setup](docs/sentry-setup.md)** - Crash reporting configuration
- **[ESC/POS Decision](docs/escpos-decision.md)** - Why we chose direct ESC/POS

## Building for Production

### macOS Universal Binary

```bash
# Apple Silicon (M1/M2/M3)
rustup target add aarch64-apple-darwin
pnpm tauri build --target aarch64-apple-darwin

# Intel (x86_64)
rustup target add x86_64-apple-darwin
pnpm tauri build --target x86_64-apple-darwin

# Outputs:
# src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/EatsomePrinterService_aarch64.dmg
# src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/EatsomePrinterService_x64.dmg
```

### Windows

```bash
pnpm tauri build --target x86_64-pc-windows-msvc

# Output:
# src-tauri/target/release/bundle/nsis/EatsomePrinterService_x64-setup.exe
# src-tauri/target/release/bundle/msi/EatsomePrinterService_x64.msi
```

### Linux

```bash
# deb (Ubuntu/Debian)
pnpm tauri build --target x86_64-unknown-linux-gnu

# rpm (Fedora/RHEL) - requires rpmbuild
pnpm tauri build --target x86_64-unknown-linux-gnu --bundles rpm

# Outputs:
# src-tauri/target/release/bundle/deb/eatsome-printer-service_1.0.0_amd64.deb
# src-tauri/target/release/bundle/rpm/eatsome-printer-service-1.0.0-1.x86_64.rpm
```

## Contributing

We welcome contributions! Please see [DEVELOPMENT.md](docs/DEVELOPMENT.md) for development setup and coding standards.

### Reporting Issues

- **Bugs**: Use GitHub Issues with `bug` label
- **Feature Requests**: Use GitHub Issues with `enhancement` label
- **Security**: Email security@eatsome.nl (do not create public issues)

## License

MIT © 2024-2026 Eatsome B.V.

## Support

- **Documentation**: [docs/](docs/)
- **GitHub Issues**: https://github.com/eatsome/eatsome/issues
- **Email**: support@eatsome.nl
