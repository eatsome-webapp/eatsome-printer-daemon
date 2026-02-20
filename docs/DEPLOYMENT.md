# Deployment Guide

Complete guide for deploying the Eatsome Printer Service to production.

## Table of Contents

- [Overview](#overview)
- [Versioning Strategy](#versioning-strategy)
- [Release Process](#release-process)
- [CI/CD Pipeline](#cicd-pipeline)
- [Platform-Specific Signing](#platform-specific-signing)
- [Auto-Updater Configuration](#auto-updater-configuration)
- [Rollback Procedures](#rollback-procedures)
- [Monitoring & Alerts](#monitoring--alerts)

## Overview

The deployment process is fully automated via GitHub Actions and follows semantic versioning. Releases trigger multi-platform builds, code signing, notarization (macOS), and automatic update manifest generation.

**Timeline:** ~30-40 minutes from tag push to published release

## Versioning Strategy

### Semantic Versioning (SemVer)

Format: `MAJOR.MINOR.PATCH` (e.g., `1.2.3`)

- **MAJOR**: Breaking changes (e.g., config format changes, incompatible POS API)
- **MINOR**: New features (backwards-compatible)
- **PATCH**: Bug fixes, security patches

### Version Bumping

```bash
# Update version in all files
cd apps/printer-daemon-tauri

# Patch (1.0.0 → 1.0.1)
npm version patch

# Minor (1.0.1 → 1.1.0)
npm version minor

# Major (1.1.0 → 2.0.0)
npm version major

# This updates:
# - package.json
# - src-tauri/Cargo.toml
# - src-tauri/tauri.conf.json
```

### Pre-Release Versions

For beta testing:

```bash
# Beta (1.2.0-beta.1)
npm version 1.2.0-beta.1

# Release Candidate (1.2.0-rc.1)
npm version 1.2.0-rc.1
```

**Auto-Updater Behavior:**

- **Stable channel**: Only updates to stable releases (1.x.y)
- **Beta channel**: Updates to beta and RC releases (requires manual opt-in)

## Release Process

### Prerequisites

Before your first release, complete:

1. **Generate Tauri Signing Keys** (see [github-actions-setup.md](github-actions-setup.md))
2. **Obtain Code Signing Certificates** (macOS + Windows)
3. **Configure GitHub Secrets** (all required secrets)
4. **Test Build Locally** (verify all platforms build successfully)

### Manual Release (Recommended)

```bash
# 1. Update version
npm version minor  # Or patch/major

# 2. Update CHANGELOG.md
# Add release notes for new version

# 3. Commit version bump
git add .
git commit -m "chore: release v1.1.0"
git push origin main

# 4. Create and push tag
git tag printer-daemon-v1.1.0
git push origin printer-daemon-v1.1.0

# 5. GitHub Actions automatically:
#    - Builds for all platforms
#    - Signs binaries
#    - Creates GitHub Release (draft)
#    - Uploads artifacts

# 6. Review draft release
#    - Check all artifacts uploaded
#    - Review release notes
#    - Test download links

# 7. Publish release
#    - Click "Publish release" button
#    - Auto-updater starts distributing to users
```

### Automated Release (GitHub Web UI)

1. Go to GitHub → Actions → Printer Daemon Release
2. Click "Run workflow"
3. Enter version (e.g., `1.1.0`)
4. Click "Run workflow"
5. Wait for build completion (~30-40 minutes)
6. Review and publish draft release

### What Gets Built

For version `1.1.0`:

```
├── macOS/
│   ├── EatsomePrinterService_aarch64.dmg (Apple Silicon, signed + notarized)
│   └── EatsomePrinterService_x64.dmg (Intel, signed + notarized)
├── Windows/
│   └── EatsomePrinterService_x64-setup.exe (NSIS, signed with Authenticode)
├── Linux/
│   ├── eatsome-printer-service_1.1.0_amd64.deb (Debian/Ubuntu)
│   └── eatsome-printer-service-1.1.0-1.x86_64.rpm (Fedora/RHEL)
└── latest.json (Auto-updater manifest)
```

## CI/CD Pipeline

### Workflow File

`.github/workflows/printer-daemon-release.yml`

### Pipeline Stages

#### 1. Create Release (ubuntu-latest)

**Duration:** ~1 minute

**Actions:**

- Parse version from tag or manual input
- Create draft GitHub Release
- Generate release notes template
- Output `upload_url` for artifact uploads

#### 2. Build macOS (macos-latest, matrix: arm64 + x64)

**Duration:** ~15-20 minutes per architecture

**Actions:**

- Setup Rust + pnpm
- Import code signing certificate to keychain
- Build Tauri app for target architecture
- Sign DMG with Developer ID Application certificate
- Submit DMG for notarization (Apple servers)
- Wait for notarization (5-15 minutes)
- Staple notarization ticket to DMG
- Upload to GitHub Release

**Parallel Execution:** ARM and Intel builds run simultaneously

#### 3. Build Windows (windows-latest)

**Duration:** ~10-15 minutes

**Actions:**

- Setup Rust + pnpm
- Build Tauri app (NSIS installer)
- Sign installer with Authenticode certificate
- Upload to GitHub Release

#### 4. Build Linux (ubuntu-latest)

**Duration:** ~10-15 minutes

**Actions:**

- Setup Rust + pnpm
- Install system dependencies (webkit2gtk, libusb, etc.)
- Create symlinks for postinstall/postrm scripts
- Build deb and rpm packages
- Rename packages to standard names
- Upload to GitHub Release

#### 5. Generate Updater Manifest (ubuntu-latest)

**Duration:** ~1 minute

**Actions:**

- Create `latest.json` with version + download URLs
- Sign manifest with Tauri signing keys
- Upload to GitHub Release

**Manifest Format:**

```json
{
  "version": "v1.1.0",
  "notes": "https://github.com/eatsome/eatsome/releases/tag/printer-daemon-v1.1.0",
  "pub_date": "2026-01-28T12:34:56Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6...",
      "url": "https://github.com/eatsome/eatsome/releases/download/printer-daemon-v1.1.0/EatsomePrinterService_aarch64.dmg"
    },
    "windows-x86_64": {
      "signature": "...",
      "url": "..."
    }
  }
}
```

#### 6. Publish Release (ubuntu-latest)

**Duration:** ~1 minute

**Actions:**

- Change release from "draft" to "published"
- Triggers auto-update checks in deployed daemons

### Pipeline Visualization

```
┌─────────────────┐
│ Create Release  │ (1 min)
└────────┬────────┘
         │
    ┌────┴────┬────────────┬────────────┐
    ▼         ▼            ▼            ▼
┌──────┐  ┌──────┐  ┌──────────┐  ┌──────────┐
│macOS │  │macOS │  │ Windows  │  │  Linux   │
│ ARM  │  │Intel │  │   x64    │  │   x64    │
└───┬──┘  └───┬──┘  └─────┬────┘  └─────┬────┘
    │         │            │             │
    └─────────┴────────────┴─────────────┘
                     │
             ┌───────▼────────┐
             │ Generate       │
             │ Updater        │
             │ Manifest       │
             └───────┬────────┘
                     │
             ┌───────▼────────┐
             │ Publish        │
             │ Release        │
             └────────────────┘
```

## Platform-Specific Signing

### macOS Code Signing

**Certificate Type:** Developer ID Application

**Process:**

1. Export certificate from Keychain as .p12 (password-protected)
2. Base64 encode: `base64 -i certificate.p12 | pbcopy`
3. Add to GitHub Secrets:
   - `MACOS_CERTIFICATE`: Base64-encoded .p12
   - `MACOS_CERTIFICATE_PASSWORD`: Password
   - `APPLE_SIGNING_IDENTITY`: "Developer ID Application: Eatsome B.V. (TEAM_ID)"

**Notarization:**

- Required for macOS 10.15+ (Catalina and later)
- Automated via `xcrun notarytool`
- Requires:
  - `APPLE_ID`: Apple ID email
  - `APPLE_APP_PASSWORD`: App-specific password (generate at appleid.apple.com)
  - `APPLE_TEAM_ID`: 10-character team ID

**Verification:**

```bash
# Check signature
codesign -dv --verbose=4 /Applications/EatsomePrinterService.app

# Check notarization
spctl -a -vv /Applications/EatsomePrinterService.app

# Expected: "accepted"
```

### Windows Code Signing

**Certificate Type:** Code Signing Certificate (Standard or EV)

**Process:**

1. Export certificate as .pfx (password-protected)
2. Base64 encode: `certutil -encode certificate.pfx certificate.b64`
3. Add to GitHub Secrets:
   - `WINDOWS_CERTIFICATE`: Base64-encoded .pfx
   - `WINDOWS_CERTIFICATE_PASSWORD`: Password

**SignTool Command:**

```powershell
signtool sign /f certificate.pfx /p PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 installer.exe
```

**Verification:**

```powershell
# Check signature
signtool verify /pa /v installer.exe

# Expected: "Successfully verified"
```

**SmartScreen Reputation:**

- EV certificates DO NOT bypass SmartScreen (as of March 2024)
- Reputation requires ~100-500 downloads over 2-4 weeks
- Monitor via Microsoft Partner Center

### Linux Package Signing (Optional)

**Debian/Ubuntu:**

```bash
# Sign with dpkg-sig
dpkg-sig --sign builder package.deb

# Verify
dpkg-sig --verify package.deb
```

**Fedora/RHEL:**

```bash
# Sign with rpm
rpm --addsign package.rpm

# Verify
rpm --checksig package.rpm
```

**Note:** Linux package signing is optional for Eatsome (low risk, no distribution via official repos)

## Auto-Updater Configuration

### Tauri Updater Settings

**File:** `src-tauri/tauri.conf.json`

```json
{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": ["https://github.com/eatsome/eatsome/releases/latest/download/latest.json"],
      "dialog": false,
      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6...",
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

### Update Flow

```
1. Daemon checks for updates every 6 hours
   └─▶ HTTP GET latest.json

2. Compare current version with latest.version
   └─▶ If newer version available, proceed

3. Download update in background
   └─▶ Delta patches used when possible (smaller downloads)

4. Wait for idle state (no print jobs for 5 minutes)
   └─▶ Prevent interruption during busy periods

5. Install update and restart daemon
   └─▶ Windows: "passive" mode (no user interaction)
   └─▶ macOS/Linux: Background replacement
```

### Testing Updates

```bash
# 1. Install version 1.0.0
# 2. Release version 1.0.1 via workflow
# 3. Monitor daemon logs

# macOS/Linux
tail -f /tmp/eatsome-printer-service.log | grep -i update

# Windows
Get-Content "$env:APPDATA\Eatsome Printer Service\logs\daemon.log" -Wait | Select-String "update"

# Expected logs:
# [INFO] Checking for updates...
# [INFO] Update available: v1.0.0 -> v1.0.1
# [INFO] Waiting for idle state...
# [INFO] Downloading update...
# [INFO] Update installed - restarting
```

### Forcing Manual Update

```bash
# Option 1: Trigger check via IPC
# (Requires exposing check_for_updates command in UI)

# Option 2: Delete cached update info
# macOS: rm ~/Library/Caches/com.eatsome.printer-service/updater
# Windows: del %LOCALAPPDATA%\Eatsome Printer Service\updater
# Linux: rm ~/.cache/eatsome-printer-service/updater
```

## Rollback Procedures

### Scenario 1: Bad Release (Pre-Publish)

**Symptom:** Build artifacts fail tests, critical bug discovered during QA

**Action:**

1. Delete draft release: GitHub → Releases → Draft → Delete
2. Delete tag: `git push origin :printer-daemon-v1.1.0`
3. Fix bug, increment patch version
4. Re-release: `git tag printer-daemon-v1.1.1 && git push origin printer-daemon-v1.1.1`

**Impact:** Zero (release never published, no users affected)

### Scenario 2: Bad Release (Post-Publish)

**Symptom:** Critical bug reported by users after publish

**Action:**

1. **Immediate:** Unpublish release (GitHub → Releases → Edit → Unpublish)
   - Stops new downloads
   - Does NOT affect already-installed daemons
2. **Fix:** Create hotfix version `v1.1.2`
3. **Release:** Follow standard release process
4. **Notify:** Email affected users (if identifiable via Sentry)

**Impact:** Users who downloaded before unpublish remain affected until they manually update or auto-updater installs v1.1.2

### Scenario 3: Auto-Updater Causing Crashes

**Symptom:** Daemon crashes after auto-update to v1.1.0

**Action:**

1. Unpublish v1.1.0 release immediately
2. Create emergency patch v1.1.1 with:
   - Rollback changes from v1.1.0
   - OR Fix crash bug
3. Publish v1.1.1 with `CRITICAL UPDATE` in release notes
4. Monitor Sentry for crash resolution

**Recovery for Affected Users:**

```bash
# Users can manually downgrade (emergency only)
# macOS/Windows: Uninstall v1.1.0, install v1.0.0 from GitHub Releases
# Linux: sudo apt install eatsome-printer-service=1.0.0
```

### Preventing Rollbacks

**Pre-Release Checklist:**

- [ ] Build artifacts pass CI tests
- [ ] Manual QA on 3 platforms (macOS/Windows/Linux)
- [ ] Test print on 3 printer types (USB/Network/BLE)
- [ ] Load test (50 orders/min for 5 minutes)
- [ ] Auto-updater test (v1.0.0 → v1.1.0)
- [ ] Sentry integration test (trigger error, verify in dashboard)

## Monitoring & Alerts

### Build Status Monitoring

**Slack Integration:**

1. Go to GitHub → Settings → Webhooks
2. Add Slack webhook URL
3. Subscribe to workflow events:
   - `workflow_run` (started, completed, failed)

**Expected Notifications:**

- ✅ "Printer Daemon Release v1.1.0: Build started"
- ✅ "Printer Daemon Release v1.1.0: Build completed (35m 12s)"
- ❌ "Printer Daemon Release v1.1.0: Build failed (macOS notarization timeout)"

### Sentry Crash Monitoring

**Post-Release Alerts:**

1. Sentry dashboard → Alerts → Create Alert
2. Condition: "New issue created" OR "Error rate > 10 events/5min"
3. Action: Email team + Slack notification
4. Filter: `release:1.1.0`

**Typical Alerts After Release:**

- 🔴 "Crash on startup (v1.1.0, macOS 15.3)"
- 🔴 "Print job failure rate 25% (v1.1.0, USB printers)"

### Download Metrics

**GitHub Release Stats:**

- Total downloads per asset
- Downloads per day
- Platform distribution

**Track via GitHub API:**

```bash
curl -H "Authorization: token $GITHUB_TOKEN" \
  https://api.github.com/repos/eatsome/eatsome/releases/tags/printer-daemon-v1.1.0

# Returns: download_count for each asset
```

**Goal:** 100+ downloads within 2 weeks (builds SmartScreen reputation)

## Cost Breakdown

| Item                               | Provider         | Cost         | Frequency  | Notes                                       |
| ---------------------------------- | ---------------- | ------------ | ---------- | ------------------------------------------- |
| Apple Developer Program            | Apple            | $99          | Annual     | Code signing + notarization                 |
| Code Signing Certificate (Windows) | DigiCert/Sectigo | $300-500     | Annual     | EV or Standard                              |
| Sentry (Team Plan)                 | Sentry           | $26          | Monthly    | If exceeding free tier (5,000 events/month) |
| GitHub Actions                     | GitHub           | $0           | -          | Free for public repos                       |
| **Total**                          |                  | **$399-599** | **Annual** |                                             |

**Free Alternative:**

- Skip Windows signing (accept SmartScreen warnings)
- Use Sentry free tier (sufficient for <100 restaurants)

## Security Checklist

Before every release:

- [ ] Rotate Tauri signing keys annually
- [ ] Verify code signing certificates not expired
- [ ] Review Sentry PII stripping (test with sample data)
- [ ] Scan dependencies for vulnerabilities (`cargo audit`, `npm audit`)
- [ ] Check for hardcoded secrets (`secretlint`)
- [ ] Verify SQLite encryption enabled
- [ ] Test JWT validation with expired/invalid tokens

## References

- [GitHub Actions: Create Release](https://github.com/actions/create-release)
- [Tauri Updater Guide](https://v2.tauri.app/plugin/updater/)
- [Apple Notarization Guide](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
- [Windows Code Signing](https://docs.microsoft.com/en-us/windows/win32/seccrypto/signtool)
- [Semantic Versioning](https://semver.org/)
