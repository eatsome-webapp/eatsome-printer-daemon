# Auto-Updater Setup Guide

This guide explains how to set up automatic updates for the Eatsome Printer Service using Tauri's built-in updater and GitHub Releases.

## Overview

The auto-updater:

- ✅ Checks for updates every 6 hours
- ✅ Downloads updates in the background
- ✅ Waits for system idle (no print jobs for 5 minutes)
- ✅ Installs and restarts automatically
- ✅ Supports delta updates (smaller downloads)
- ✅ Cryptographically signed with private key

## 1. Generate Signing Keys

The updater uses Ed25519 signatures to verify update authenticity.

### First Time Setup

```bash
cd apps/printer-daemon-tauri

# Generate new key pair (run ONCE, save securely!)
pnpm tauri signer generate -- -w ~/.tauri/eatsome-printer-service.key

# This creates:
# - Private key: ~/.tauri/eatsome-printer-service.key (SECRET - add to .gitignore!)
# - Public key: printed to console (add to tauri.conf.json)
```

**⚠️ CRITICAL:** Save the private key in a secure location (1Password, GitHub Secrets). If lost, you cannot publish signed updates!

### Update tauri.conf.json

Copy the public key from the output and update `src-tauri/tauri.conf.json`:

```json
{
  "plugins": {
    "updater": {
      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaYWJjZGVmZ2hpamtsbW5vcA=="
    }
  }
}
```

## 2. GitHub Release Workflow

### Manual Release Process

1. **Increment version** in `Cargo.toml`:

   ```toml
   [package]
   version = "1.0.1"  # Bump version
   ```

2. **Build signed binaries**:

   ```bash
   pnpm tauri build
   ```

3. **Create GitHub release**:

   ```bash
   gh release create v1.0.1 \
     src-tauri/target/release/bundle/macos/*.dmg \
     src-tauri/target/release/bundle/msi/*.msi \
     src-tauri/target/release/bundle/deb/*.deb \
     src-tauri/target/release/bundle/rpm/*.rpm \
     --title "Release v1.0.1" \
     --notes "Bug fixes and improvements"
   ```

4. **Generate update manifest**:

   ```bash
   pnpm tauri signer sign --key ~/.tauri/eatsome-printer-service.key \
     src-tauri/target/release/bundle/**/* \
     --output latest.json

   # Upload latest.json to release
   gh release upload v1.0.1 latest.json
   ```

### Automated Release (GitHub Actions)

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*.*.*'

jobs:
  release:
    strategy:
      matrix:
        platform:
          - os: macos-latest
            target: aarch64-apple-darwin
          - os: macos-latest
            target: x86_64-apple-darwin
          - os: windows-latest
            target: x86_64-pc-windows-msvc
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu

    runs-on: ${{ matrix.platform.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.platform.target }}

      - name: Setup pnpm
        uses: pnpm/action-setup@v2
        with:
          version: 8

      - name: Install dependencies
        run: pnpm install

      - name: Build Tauri app
        run: pnpm tauri build --target ${{ matrix.platform.target }}
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PASSWORD }}

      - name: Upload release assets
        uses: softprops/action-gh-release@v1
        with:
          files: |
            src-tauri/target/${{ matrix.platform.target }}/release/bundle/dmg/*.dmg
            src-tauri/target/${{ matrix.platform.target }}/release/bundle/msi/*.msi
            src-tauri/target/${{ matrix.platform.target }}/release/bundle/deb/*.deb
            src-tauri/target/${{ matrix.platform.target }}/release/bundle/rpm/*.rpm
            src-tauri/target/${{ matrix.platform.target }}/release/bundle/appimage/*.AppImage
```

### GitHub Secrets Setup

Add to repository secrets (Settings → Secrets → Actions):

```bash
# Private key content
TAURI_SIGNING_PRIVATE_KEY: <content of ~/.tauri/eatsome-printer-service.key>

# Password (if key is encrypted)
TAURI_SIGNING_PASSWORD: <your password or leave empty>
```

## 3. Update Manifest Format

The `latest.json` file served at the updater endpoint has this structure:

```json
{
  "version": "1.0.1",
  "date": "2026-01-28T12:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUREZ...",
      "url": "https://github.com/eatsome/eatsome/releases/download/v1.0.1/EatsomePrinterService_aarch64.dmg"
    },
    "darwin-x86_64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUREZ...",
      "url": "https://github.com/eatsome/eatsome/releases/download/v1.0.1/EatsomePrinterService_x64.dmg"
    },
    "windows-x86_64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUREZ...",
      "url": "https://github.com/eatsome/eatsome/releases/download/v1.0.1/EatsomePrinterService_x64-setup.exe"
    },
    "linux-x86_64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUREZ...",
      "url": "https://github.com/eatsome/eatsome/releases/download/v1.0.1/eatsome-printer-service_1.0.1_amd64.deb"
    }
  },
  "notes": "Bug fixes and performance improvements"
}
```

## 4. Testing Updates

### Test in Development

```typescript
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'

// Check for update
const update = await check()

if (update?.available) {
  console.log(`Update available: ${update.version}`)

  // Download and install
  await update.downloadAndInstall((event) => {
    if (event.event === 'Started') {
      console.log(`Downloading: ${event.data.contentLength} bytes`)
    } else if (event.event === 'Progress') {
      console.log(`Progress: ${event.data.chunkLength} bytes`)
    }
  })

  // Restart app
  await relaunch()
}
```

### Test with Local Server

1. Build a test release:

   ```bash
   pnpm tauri build
   pnpm tauri signer sign src-tauri/target/release/bundle/**/* -o test-latest.json
   ```

2. Serve locally:

   ```bash
   python3 -m http.server 8080
   ```

3. Update `tauri.conf.json` temporarily:

   ```json
   {
     "plugins": {
       "updater": {
         "endpoints": ["http://localhost:8080/test-latest.json"]
       }
     }
   }
   ```

4. Run app and check for updates

## 5. Rollback Strategy

If an update causes issues:

1. **Emergency rollback**:

   ```bash
   # Delete bad release
   gh release delete v1.0.1 --yes

   # Update latest.json to point to previous version
   # Upload corrected latest.json
   ```

2. **Backup system** - Before updating `latest.json`, daemon creates backup:
   - `config.json.backup`
   - `print-queue.db.backup`

3. **Crash detection** - If daemon crashes within 5 minutes of update:
   - Restores backup
   - Reverts to previous version (not yet implemented)

## 6. Monitoring

Monitor update success/failure rates:

```typescript
// In updater.rs TelemetryEvent
UpdateCheckStarted { version: String },
UpdateAvailable { current: String, latest: String },
UpdateDownloading { bytes_total: u64 },
UpdateInstalled { version: String, duration_ms: u64 },
UpdateFailed { error: String, stage: String },
```

View metrics in Sentry/telemetry dashboard.

## 7. Security Considerations

- ✅ **Signature verification** - Public key in binary, updates must be signed
- ✅ **HTTPS only** - GitHub Releases served over TLS
- ✅ **No user intervention** - Prevents social engineering attacks
- ⚠️ **Keep private key secure** - Compromise = ability to push malicious updates
- ✅ **Idle detection** - Won't interrupt active print jobs

## Troubleshooting

### Update check fails

**Symptoms:** Logs show "Update check failed: ..." every 6 hours

**Causes:**

- GitHub Releases endpoint unreachable (network issue)
- `latest.json` malformed or signature invalid
- Public key mismatch in `tauri.conf.json`

**Solution:**

```bash
# Test endpoint manually
curl https://github.com/eatsome/eatsome/releases/latest/download/latest.json

# Verify signature
pnpm tauri signer verify --key <pubkey> latest.json
```

### Update downloads but doesn't install

**Symptoms:** "Update downloaded" event fires but no restart

**Causes:**

- Queue not idle (active print jobs)
- Permissions issue (can't write to install directory)

**Solution:**

- Check logs for idle detection messages
- Ensure daemon has write permissions to install directory

### Update installed but reverted

**Symptoms:** Version number increases then decreases

**Causes:**

- Crash within 5 minutes of update (triggers rollback)
- Backup restoration logic activated

**Solution:**

- Check crash logs for errors in new version
- Fix underlying issue and release patched version

## Resources

- [Tauri Updater Docs](https://v2.tauri.app/plugin/updater/)
- [Signing Guide](https://v2.tauri.app/plugin/updater/#signing-updates)
- [GitHub Releases API](https://docs.github.com/en/rest/releases)
