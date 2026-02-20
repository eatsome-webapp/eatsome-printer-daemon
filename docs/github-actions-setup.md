# GitHub Actions CI/CD Setup

Complete guide for setting up automated builds and releases for the Eatsome Printer Service.

## Overview

The `printer-daemon-release.yml` workflow automates:

- Multi-platform builds (macOS ARM/Intel, Windows x64, Linux x64)
- Code signing (macOS + Windows)
- Notarization (macOS)
- Package creation (dmg, exe, deb, rpm)
- Auto-updater manifest generation
- GitHub Releases publishing

## Required Secrets

Configure these in GitHub repository settings → Secrets and variables → Actions:

### Tauri Signing (All Platforms)

**TAURI_SIGNING_PRIVATE_KEY** - Ed25519 private key for update signing

```bash
# Generate signing keys
cd apps/printer-daemon-tauri
pnpm tauri signer generate

# Output:
# Private key: dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5...
# Public key: dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaYWJjZGVmZ2hpamtsbW5vcA==

# Add private key to GitHub Secrets as TAURI_SIGNING_PRIVATE_KEY
# Add public key to src-tauri/tauri.conf.json:
# "updater": { "pubkey": "<public key here>" }
```

**TAURI_SIGNING_PRIVATE_KEY_PASSWORD** - Password for signing key (if set during generation)

### macOS Signing & Notarization

**MACOS_CERTIFICATE** - Developer ID Application certificate (base64-encoded .p12)

```bash
# Export certificate from Keychain Access
# File → Export Items → Save as .p12 with password

# Encode to base64
base64 -i certificate.p12 | pbcopy

# Paste into GitHub Secrets as MACOS_CERTIFICATE
```

**MACOS_CERTIFICATE_PASSWORD** - Password used when exporting .p12

**APPLE_SIGNING_IDENTITY** - Certificate name (e.g., "Developer ID Application: Eatsome B.V. (TEAM_ID)")

```bash
# Find your identity
security find-identity -v -p codesigning

# Example output:
# 1) ABC123... "Developer ID Application: Eatsome B.V. (XYZ456)"

# Use the full quoted string as APPLE_SIGNING_IDENTITY
```

**APPLE_ID** - Apple ID email for notarization (e.g., `developer@eatsome.nl`)

**APPLE_APP_PASSWORD** - App-specific password for notarization

```bash
# Generate at https://appleid.apple.com/account/manage
# Account → Security → App-Specific Passwords → Generate

# Use the generated password (xxxx-xxxx-xxxx-xxxx)
```

**APPLE_TEAM_ID** - Apple Developer Team ID (10-character alphanumeric)

```bash
# Find at https://developer.apple.com/account
# Membership → Team ID

# Example: XYZ456ABCD
```

### Windows Signing (Optional but Recommended)

**WINDOWS_CERTIFICATE** - Code signing certificate (base64-encoded .pfx)

```bash
# Export certificate as .pfx with password

# Encode to base64
certutil -encode certificate.pfx certificate.b64
# OR on macOS/Linux:
base64 -i certificate.pfx | pbcopy

# Paste into GitHub Secrets as WINDOWS_CERTIFICATE
```

**WINDOWS_CERTIFICATE_PASSWORD** - Password used when exporting .pfx

> **Note:** Windows signing is optional. Without it, SmartScreen warnings will appear until reputation is built (~100 downloads). See [Platform-Specific Notes](#windows-smartscreen-reputation) below.

## Triggering Releases

### Method 1: Git Tag (Recommended)

```bash
# Update version in package.json and tauri.conf.json
cd apps/printer-daemon-tauri
npm version 1.0.0  # Or patch/minor/major

# Create and push tag
git tag printer-daemon-v1.0.0
git push origin printer-daemon-v1.0.0

# Workflow triggers automatically
```

### Method 2: Manual Workflow Dispatch

1. Go to GitHub → Actions → Printer Daemon Release
2. Click "Run workflow"
3. Enter version (e.g., `1.0.0`)
4. Click "Run workflow"

## Build Output

For version `1.0.0`, the workflow creates:

### macOS

- `EatsomePrinterService_aarch64.dmg` - Apple Silicon (M1/M2/M3)
- `EatsomePrinterService_x64.dmg` - Intel

### Windows

- `EatsomePrinterService_x64-setup.exe` - Windows 10/11 installer

### Linux

- `eatsome-printer-service_1.0.0_amd64.deb` - Debian/Ubuntu
- `eatsome-printer-service-1.0.0-1.x86_64.rpm` - Fedora/RHEL

### Auto-Updater

- `latest.json` - Update manifest for Tauri updater

## Workflow Steps

### 1. Create Release

- Parses version from tag or manual input
- Creates draft GitHub Release
- Generates release notes template

### 2. Build macOS (Parallel: ARM + Intel)

- Sets up Node.js 20, pnpm, Rust
- Imports code signing certificate to temporary keychain
- Builds Tauri app for target architecture
- Submits DMG for notarization (Apple servers)
- Staples notarization ticket to DMG
- Uploads to GitHub Release

**Duration:** ~15-20 minutes per architecture (notarization wait)

### 3. Build Windows

- Sets up Node.js 20, pnpm, Rust
- Builds Tauri app (NSIS installer)
- Signs installer with Authenticode (if certificate provided)
- Uploads to GitHub Release

**Duration:** ~10-15 minutes

### 4. Build Linux

- Sets up Node.js 20, pnpm, Rust
- Installs system dependencies (webkit2gtk, libusb, etc.)
- Creates symlinks for postinstall scripts
- Builds both deb and rpm packages
- Uploads to GitHub Release

**Duration:** ~10-15 minutes

### 5. Generate Updater Manifest

- Creates `latest.json` with download URLs
- Includes version, release notes URL, signatures
- Uploads to GitHub Release

**Duration:** ~1 minute

### 6. Publish Release

- Changes release from "draft" to "published"
- Triggers auto-update checks in deployed daemons

**Duration:** ~1 minute

**Total Duration:** ~30-40 minutes (macOS notarization is bottleneck)

## Platform-Specific Notes

### macOS Notarization

Notarization is **required** for macOS 10.15+ (Catalina and later). Without it, users see:

> "EatsomePrinterService.app" cannot be opened because the developer cannot be verified.

**Notarization Process:**

1. App is submitted to Apple servers
2. Apple scans for malware (5-15 minutes)
3. Notarization ticket is issued
4. Ticket is "stapled" to DMG
5. DMG can be opened without warnings

**Cost:** Included with Apple Developer Program ($99/year)

### Windows SmartScreen Reputation

**With Code Signing Certificate:**

- Users see publisher name instead of "Unknown publisher"
- SmartScreen warnings still appear until reputation is built
- Reputation requires ~100-500 downloads over 2-4 weeks
- **No instant bypass** (changed March 2024)

**Without Code Signing Certificate:**

- SmartScreen shows "Windows protected your PC" (blue screen)
- Users must click "More info" → "Run anyway"
- EV certificates no longer bypass SmartScreen

**Recommendation:** Purchase code signing certificate ($300-500/year) for professional appearance, but expect SmartScreen warnings regardless until reputation is built.

### Linux Package Signing

Linux packages (deb/rpm) are **not signed** by default in this workflow. Package signing is optional:

**Debian/Ubuntu (deb):**

```bash
# Sign with dpkg-sig
dpkg-sig --sign builder package.deb
```

**Fedora/RHEL (rpm):**

```bash
# Sign with rpm --addsign
rpm --addsign package.rpm
```

Most users install packages without signature verification, so signing is less critical than on macOS/Windows.

## Auto-Updater Configuration

### 1. Update tauri.conf.json

```json
{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": ["https://github.com/eatsome/eatsome/releases/latest/download/latest.json"],
      "dialog": false,
      "pubkey": "<YOUR_PUBLIC_KEY_HERE>",
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

### 2. Verify Update Flow

**Daemon Behavior:**

1. Checks for updates every 6 hours
2. Downloads update in background if available
3. Waits for 5 minutes of idle (no print jobs)
4. Installs update and restarts

**User Experience:**

- No manual download required
- No interruption during active printing
- Seamless background updates

### 3. Testing Updates

```bash
# 1. Install version 1.0.0
# 2. Release version 1.0.1 via workflow
# 3. Wait up to 6 hours OR trigger manual check:

# On macOS/Linux:
tail -f /tmp/eatsome-printer-service.log | grep -i update

# On Windows:
Get-Content C:\Users\<USER>\AppData\Local\eatsome-printer-service\logs\daemon.log -Wait | Select-String "update"

# Expected log:
# [INFO] Checking for updates...
# [INFO] Update available: v1.0.0 -> v1.0.1
# [INFO] Waiting for idle state...
# [INFO] Downloading update...
# [INFO] Update installed - restarting
```

## Troubleshooting

### macOS Notarization Fails

**Error:** `The uploaded file is invalid or corrupted`

**Cause:** Binary not properly signed before submission

**Fix:**

1. Verify MACOS_CERTIFICATE secret is correct base64
2. Check APPLE_SIGNING_IDENTITY matches certificate name exactly
3. Ensure certificate is "Developer ID Application" (not "Developer ID Installer")

### Windows Signing Fails

**Error:** `SignTool Error: No certificates were found that met all the given criteria`

**Cause:** Certificate not imported correctly

**Fix:**

1. Verify WINDOWS_CERTIFICATE is base64-encoded .pfx
2. Check WINDOWS_CERTIFICATE_PASSWORD is correct
3. Ensure certificate is for "Code Signing" purpose

### Linux Build Fails: Missing Dependencies

**Error:** `error: failed to run custom build command for 'webkit2gtk-sys'`

**Cause:** Missing system dependencies

**Fix:** Already handled in workflow (`apt-get install` step). If running locally:

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  libssl-dev \
  libgtk-3-dev \
  libusb-1.0-0-dev \
  libudev-dev
```

### Auto-Update Not Working

**Symptom:** Daemon never updates even when new version is released

**Debug:**

1. Check `latest.json` exists at configured endpoint
2. Verify `pubkey` in `tauri.conf.json` matches generated public key
3. Check daemon logs for update check errors
4. Test network connectivity to GitHub from daemon

## Security Best Practices

### 1. Rotate Signing Keys Annually

```bash
# Generate new keys
pnpm tauri signer generate

# Update GitHub Secret: TAURI_SIGNING_PRIVATE_KEY
# Update tauri.conf.json pubkey
# Commit and release

# Old versions will stop receiving updates (can't verify new signatures)
# Acceptable for annual rotation
```

### 2. Protect Private Keys

- **NEVER** commit private keys to git
- Store in GitHub Secrets only
- Use password-protected keys when possible
- Rotate immediately if compromised

### 3. Code Signing Certificate Security

- Store certificates in password-protected files
- Use different passwords for each certificate
- Renew before expiration (macOS: 1 year, Windows: 1-3 years)
- Monitor for unauthorized usage

## Cost Summary

| Item                               | Provider         | Cost         | Frequency  |
| ---------------------------------- | ---------------- | ------------ | ---------- |
| Apple Developer Program            | Apple            | $99          | Annual     |
| Code Signing Certificate (Windows) | DigiCert/Sectigo | $300-500     | Annual     |
| **Total**                          |                  | **$399-599** | **Annual** |

**Free Alternatives:**

- Skip Windows signing (accept SmartScreen warnings)
- Use self-signed certificates for Linux (not recommended for production)

## References

- [Tauri Updater Guide](https://v2.tauri.app/plugin/updater/)
- [GitHub Actions: Create Release](https://github.com/actions/create-release)
- [Apple Notarization Guide](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
- [Windows Code Signing Best Practices](https://docs.microsoft.com/en-us/windows/win32/seccrypto/signtool)
