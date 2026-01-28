# Eatsome Printer Daemon

🖨️ **Local printer service for Eatsome POS** - Automatic kitchen order printing with smart routing, offline support, and zero-config thermal printer support.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-blue)](https://github.com/eatsome-webapp/eatsome-printer-daemon/releases)
[![Built with Tauri](https://img.shields.io/badge/Built%20with-Tauri%202.0-orange)](https://tauri.app/)

## Features

✨ **Smart Routing** - Automatically routes orders to the right kitchen station (bar, grill, kitchen)
🔌 **Universal Connectivity** - Supports USB, Network (WiFi/Ethernet), and Bluetooth printers
💪 **Offline-First** - Queue jobs locally when internet is down, sync when back online
🔄 **Auto-Updates** - Seamless updates in the background without interrupting service
🛡️ **Backup Printing** - Automatic failover to backup printers if primary fails
⚡ **Zero Config** - Printers are automatically discovered, no manual setup required
🔐 **Secure** - JWT-based authentication per restaurant, no hardcoded credentials

## System Requirements

### macOS
- macOS 10.15 (Catalina) or newer
- Apple Silicon (M1/M2/M3) or Intel (x86_64)

### Windows
- Windows 10 or Windows 11
- 64-bit architecture

### Linux
- Ubuntu 20.04+, Debian 11+, Fedora 36+, or compatible distros
- systemd-based distributions
- USB access requires `lp` group membership

## Installation

### macOS

1. Download the latest DMG from [Releases](https://github.com/eatsome-webapp/eatsome-printer-daemon/releases/latest)
2. Open the DMG and drag "Eatsome Printer Service" to Applications
3. **Control + Click** on the app → "Open" (required for unsigned apps)
4. Follow the setup wizard and scan the QR code from your restaurant dashboard

### Windows

1. Download the installer `.exe` from [Releases](https://github.com/eatsome-webapp/eatsome-printer-daemon/releases/latest)
2. Run the installer (may show SmartScreen warning - click "More info" → "Run anyway")
3. The service starts automatically after installation
4. Follow the setup wizard and scan the QR code from your restaurant dashboard

### Linux

**Ubuntu/Debian:**
```bash
# Download .deb from releases
wget https://github.com/eatsome-webapp/eatsome-printer-daemon/releases/latest/download/eatsome-printer-service_amd64.deb

# Install
sudo dpkg -i eatsome-printer-service_amd64.deb

# Add user to lp group for USB access
sudo usermod -a -G lp $USER

# Log out and log back in for group membership to take effect
# Service starts automatically
```

**Fedora/RHEL:**
```bash
# Download .rpm from releases
wget https://github.com/eatsome-webapp/eatsome-printer-daemon/releases/latest/download/eatsome-printer-service.rpm

# Install
sudo dnf install eatsome-printer-service.rpm

# Add user to lp group
sudo usermod -a -G lp $USER

# Log out and log back in
```

## Building from Source

### Prerequisites

- **Node.js** 20.9.0 or newer
- **Rust** 1.70.0 or newer ([rustup](https://rustup.rs/))
- **pnpm** 9.0.0 or newer

### Build Steps

```bash
# Clone the repository
git clone https://github.com/eatsome-webapp/eatsome-printer-daemon.git
cd eatsome-printer-daemon

# Install dependencies
pnpm install

# Development mode
pnpm tauri dev

# Build for production
pnpm tauri build
```

**Build outputs:**
- macOS: `src-tauri/target/release/bundle/macos/*.dmg`
- Windows: `src-tauri/target/release/bundle/nsis/*.exe`
- Linux: `src-tauri/target/release/bundle/deb/*.deb` and `*.rpm`

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Eatsome POS (Web)                    │
│              https://pos.eatsome.nl                     │
└────────────────────┬────────────────────────────────────┘
                     │ Supabase Realtime (WebSocket)
                     │ JWT Authentication
                     ▼
┌─────────────────────────────────────────────────────────┐
│              Printer Daemon (Local Service)             │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐ │
│  │   Realtime  │  │     Queue    │  │    Printer     │ │
│  │   Client    │─▶│   Manager    │─▶│    Manager     │ │
│  │ (Supabase)  │  │  (SQLite)    │  │  (ESC/POS)     │ │
│  └─────────────┘  └──────────────┘  └────────────────┘ │
└────────────────────┬────────────────────────────────────┘
                     │ USB / Network / Bluetooth
                     ▼
┌─────────────────────────────────────────────────────────┐
│              Kitchen Thermal Printers                   │
│   🖨️ Bar        🖨️ Grill        🖨️ Kitchen            │
└─────────────────────────────────────────────────────────┘
```

## Technology Stack

- **Framework:** [Tauri 2.0](https://tauri.app/) (Rust + WebView)
- **Frontend:** React 19 + TypeScript
- **Queue:** better-queue + SQLite
- **Realtime:** Supabase Realtime (PostgreSQL LISTEN/NOTIFY)
- **Print Protocol:** ESC/POS (direct USB/Network communication)
- **State Management:** Zustand

## Development

### Project Structure

```
eatsome-printer-daemon/
├── src/                      # React frontend (setup wizard)
│   ├── App.tsx
│   └── components/
├── src-tauri/               # Rust backend
│   ├── src/
│   │   ├── main.rs          # Entry point
│   │   ├── realtime.rs      # Supabase Realtime client
│   │   ├── queue.rs         # Job queue management
│   │   ├── printer.rs       # Printer manager
│   │   ├── discovery.rs     # USB/Network/BT discovery
│   │   ├── routing.rs       # Kitchen routing logic
│   │   ├── escpos.rs        # ESC/POS command builder
│   │   ├── circuit_breaker.rs # Failure recovery
│   │   └── api.rs           # REST API server
│   ├── Cargo.toml
│   └── tauri.conf.json
└── package.json
```

### Running Tests

```bash
# Rust tests
cd src-tauri
cargo test

# Integration tests
cargo test --test '*' -- --nocapture
```

## Troubleshooting

### macOS: "App can't be opened"
- **Solution:** Control + Click → Open (bypasses Gatekeeper)
- Or remove quarantine: `xattr -d com.apple.quarantine /Applications/EatsomePrinterService.app`

### Windows: SmartScreen warning
- **Solution:** Click "More info" → "Run anyway"
- This warning appears for all new apps until reputation is established

### Linux: USB permission denied
- **Solution:** Add user to `lp` group: `sudo usermod -a -G lp $USER`
- Log out and log back in for changes to take effect

### Printer not detected
1. Check USB cable connection
2. Verify printer is powered on
3. For network printers: ensure same network as POS computer
4. Check firewall settings (allow UDP port 9100 for raw printing)

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

**Before submitting a PR:**
1. Run `cargo fmt` and `cargo clippy`
2. Ensure all tests pass (`cargo test`)
3. Update documentation if adding new features

## Security

- **JWT Authentication:** Each restaurant gets unique tokens, rotated daily
- **No Hardcoded Secrets:** All credentials stored in encrypted local config
- **PII Filtering:** Error reporting strips customer data before sending to Sentry
- **Service Role Never Exposed:** Backend only, never included in builds

Report security vulnerabilities to: security@eatsome.nl

## License

MIT License - see [LICENSE](LICENSE) for details

## Links

- 🌐 **Website:** [eatsome.nl](https://eatsome.nl)
- 📖 **Documentation:** [docs.eatsome.nl/printer-service](https://docs.eatsome.nl/printer-service)
- 💬 **Support:** support@eatsome.nl
- 🐛 **Issues:** [GitHub Issues](https://github.com/eatsome-webapp/eatsome-printer-daemon/issues)

---

Made with ❤️ by the Eatsome team
