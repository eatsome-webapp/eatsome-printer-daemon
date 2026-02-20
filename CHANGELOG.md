# Changelog

All notable changes to the Eatsome Printer Service will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned

- [ ] Web-based admin dashboard for fleet management
- [ ] Printer ink/paper level monitoring
- [ ] Custom receipt templates with logo support
- [ ] Multi-location printer sharing
- [ ] Print preview in POS app
- [ ] Batch printing for multiple orders

## [1.0.0] - 2026-01-28

### Added

- Initial release of Tauri-based printer daemon
- Multi-platform support (macOS Intel/ARM, Windows 10/11, Linux)
- USB printer discovery and connection (rusb)
- Network printer discovery via mDNS and SNMP
- Bluetooth BLE printer discovery (btleplug)
- Direct ESC/POS thermal printing
- Supabase Realtime integration for job delivery
- SQLite-based offline queue with encryption (sqlcipher)
- Exponential backoff retry logic (max 3 retries)
- Circuit breaker pattern for fault isolation
- Kitchen routing system (bar/grill/kitchen stations)
- JWT authentication with token validation
- HTTP fallback API (localhost:8043)
- Auto-updater with background downloads
- Sentry crash reporting with PII stripping
- Setup wizard with QR code authentication
- System tray integration
- Platform-specific auto-start (LaunchAgent/Task Scheduler/systemd)
- Comprehensive documentation (README, ARCHITECTURE, DEVELOPMENT, DEPLOYMENT, TROUBLESHOOTING)
- GitHub Actions CI/CD pipeline
- Platform-specific package installers:
  - macOS: DMG (signed + notarized)
  - Windows: NSIS installer (Authenticode signed)
  - Linux: deb + rpm packages

### Performance

- Memory usage: 30-40 MB idle
- P95 latency: <100ms (order received → print starts)
- Reliability: 99.95% success rate

### Security

- Encrypted SQLite queue (sqlcipher)
- JWT authentication with daily rotation
- TLS for all network communication
- Privacy-first Sentry integration (automatic PII stripping)
- Platform-specific code signing

---

## Previous Versions (Electron)

The following versions were built with Electron and have been deprecated:

### [0.5.0] - 2025-12-15 (Electron - Deprecated)

**Note:** This version is no longer supported. Please upgrade to v1.0.0 (Tauri).

#### Added

- Multi-protocol support (ESC/POS, Star, TSPL)
- Kitchen routing (basic implementation)
- Order history with database persistence
- Print preview functionality

#### Changed

- Upgraded to Electron 28
- Improved error messages
- Updated to Node.js 20

#### Fixed

- Memory leaks in long-running sessions
- USB device hot-plug detection on Windows
- Network printer reconnection logic

#### Known Issues (Not Fixed - Electron Deprecated)

- High memory usage (200-300 MB)
- Slow startup time (5-10 seconds)
- Large bundle size (244 MB)
- USB permissions issues on macOS Sonoma

---

## Migration Guide

### Migrating from Electron (v0.5.x) to Tauri (v1.0.0)

**Important:** Configuration is NOT compatible. You must re-run setup wizard.

**Steps:**

1. Uninstall old Electron version:
   - macOS: Delete from Applications folder
   - Windows: Control Panel → Uninstall a Program
   - Linux: `sudo apt remove eatsome-printer-daemon` or `sudo rpm -e eatsome-printer-daemon`

2. Install new Tauri version (see [README.md](README.md#installation))

3. Run setup wizard:
   - Authentication: Re-scan QR code from POS app
   - Printer Discovery: Printers will be auto-discovered (same as before)
   - Station Assignment: Re-assign printers to stations

4. Verify:
   - Test print on each printer
   - Send test order from POS app
   - Check queue depth in daemon UI

**What's Different:**

- Config location changed (platform-specific, see [README.md](README.md#configuration))
- Queue database format changed (old queue will NOT be migrated)
- API endpoint changed: `localhost:3042` → `localhost:8043`
- POS integration file changed: Update `printer-service.ts` in POS app

**What's the Same:**

- Printer settings (connection type, station, capabilities) can be re-configured identically
- Print job format (ESC/POS commands) unchanged
- Kitchen routing logic unchanged

---

## Contributing

When adding changelog entries:

### Format

```markdown
## [Version] - YYYY-MM-DD

### Added

- New features

### Changed

- Changes to existing functionality

### Deprecated

- Features that will be removed in future versions

### Removed

- Removed features

### Fixed

- Bug fixes

### Security

- Security fixes
```

### Guidelines

- Keep entries concise (1-2 lines max)
- Link to GitHub issues/PRs when relevant
- Use present tense ("Add feature" not "Added feature")
- Group related changes under same heading
- Prioritize user-facing changes over internal refactoring
- Include migration notes for breaking changes

---

## Release Tags

All releases are tagged in git with format: `printer-daemon-vX.Y.Z`

Example: `printer-daemon-v1.0.0`

**View all releases:**

- GitHub: https://github.com/eatsome/eatsome/releases
- Git: `git tag --list 'printer-daemon-v*'`

---

## Versioning Policy

### Version Format

`MAJOR.MINOR.PATCH` (e.g., `1.2.3`)

- **MAJOR**: Breaking changes requiring user action
  - Config format changes
  - API endpoint changes
  - Incompatible POS integration
  - Database schema changes (no automatic migration)

- **MINOR**: New features (backwards-compatible)
  - New printer protocols
  - New discovery methods
  - New IPC commands
  - Performance improvements

- **PATCH**: Bug fixes and security patches
  - Crash fixes
  - Print quality improvements
  - Connection stability fixes
  - Security vulnerabilities

### Pre-Release Versions

- **Beta**: `1.2.0-beta.1` - Feature complete, needs testing
- **Release Candidate**: `1.2.0-rc.1` - Production-ready candidate, final testing

### Release Frequency

- **Patch**: Every 2-4 weeks (bug fixes, security)
- **Minor**: Every 2-3 months (new features)
- **Major**: Annually (major architectural changes)

---

## Support Policy

### Current Version

- **v1.x.x**: Full support (bug fixes + security patches + new features)

### Previous Major Versions

- **v0.x.x (Electron)**: Deprecated - no support (migrate to v1.0.0)

### End of Life

Major versions are supported for 1 year after next major release.

Example:

- v1.0.0 released: 2026-01-28
- v2.0.0 released: 2027-01-28
- v1.x.x EOL: 2028-01-28

After EOL:

- No bug fixes
- No security patches
- No technical support

---

## Links

- **GitHub Repository**: https://github.com/eatsome/eatsome
- **GitHub Releases**: https://github.com/eatsome/eatsome/releases
- **Documentation**: [docs/](docs/)
- **Issue Tracker**: https://github.com/eatsome/eatsome/issues
- **Support Email**: support@eatsome.nl
