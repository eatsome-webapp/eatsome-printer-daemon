# Troubleshooting Guide

Solutions for common issues with the Eatsome Printer Service.

## Table of Contents

- [Installation Issues](#installation-issues)
- [USB Printer Issues](#usb-printer-issues)
- [Network Printer Issues](#network-printer-issues)
- [Print Job Issues](#print-job-issues)
- [Connection Issues](#connection-issues)
- [Performance Issues](#performance-issues)
- [Platform-Specific Issues](#platform-specific-issues)

## Installation Issues

### macOS: "Cannot open because developer cannot be verified"

**Symptom:**

> "EatsomePrinterService.app" cannot be opened because the developer cannot be verified.

**Cause:** macOS Gatekeeper blocking unsigned or unnotarized app

**Solution:**

```bash
# Method 1: Right-click → Open
# 1. Right-click EatsomePrinterService.app
# 2. Click "Open"
# 3. Click "Open" in confirmation dialog

# Method 2: Command line override
xattr -d com.apple.quarantine /Applications/EatsomePrinterService.app

# Method 3: System Preferences
# System Preferences → Security & Privacy → General
# Click "Open Anyway" next to blocked app message
```

**Prevention:** Download from official GitHub Releases (notarized builds)

### Windows: "Windows protected your PC" SmartScreen Warning

**Symptom:**

> Windows protected your PC. Microsoft Defender SmartScreen prevented an unrecognized app from starting.

**Cause:** New application without established reputation

**Solution:**

```
1. Click "More info"
2. Click "Run anyway"
```

**Note:** Warning disappears after ~100-500 users install (2-4 weeks)

**Alternative:** Contact support@eatsome.nl for pre-signed beta builds

### Linux: "dpkg: dependency problems"

**Symptom:**

```
dpkg: dependency problems prevent configuration of eatsome-printer-service:
 eatsome-printer-service depends on libusb-1.0-0; however:
  Package libusb-1.0-0 is not installed.
```

**Solution:**

```bash
# Install missing dependencies first
sudo apt-get install -f

# Or manually install dependencies
sudo apt-get install libusb-1.0-0 libwebkit2gtk-4.1-0 systemd

# Then retry installation
sudo dpkg -i eatsome-printer-service_1.0.0_amd64.deb
```

## USB Printer Issues

### macOS: "IOServiceOpen failed: (e00002c1)"

**Symptom:** Daemon logs show `IOServiceOpen failed` when accessing USB printer

**Cause:** Missing USB device entitlements or permissions

**Solution:**

```bash
# Check entitlements
codesign -d --entitlements - /Applications/EatsomePrinterService.app

# Expected output should include:
# <key>com.apple.security.device.usb</key>
# <true/>

# If missing, app needs to be re-signed with correct entitlements

# Grant permissions manually:
# System Preferences → Security & Privacy → Privacy → Files and Folders
# Enable USB for EatsomePrinterService
```

**Workaround:** Restart app after granting permissions

### Windows: "Access to USB device denied"

**Symptom:** Printer discovery finds USB printer but cannot connect

**Cause:** Windows driver not installed or device locked by another process

**Solution:**

```powershell
# Check if printer driver installed
Get-WmiObject Win32_PnPEntity | Where-Object { $_.Name -like "*printer*" }

# If driver missing, install vendor driver:
# Epson: Download from https://epson.com/Support/Printers/
# Star: Download from https://www.starmicronics.com/support/

# Kill processes locking device
# Device Manager → Printers → Right-click printer → Disable → Enable
```

### Linux: "Permission denied: /dev/bus/usb/001/005"

**Symptom:** Daemon cannot access USB printer despite discovery

**Cause:** User not in `lp` group or udev rules not applied

**Solution:**

```bash
# Check if user in lp group
groups | grep lp

# If not, add user to lp group
sudo usermod -a -G lp $USER

# Verify udev rules exist
ls /etc/udev/rules.d/60-eatsome-printer.rules

# If missing, create manually
sudo nano /etc/udev/rules.d/60-eatsome-printer.rules

# Paste content from install-scripts/linux/60-eatsome-printer.rules

# Reload udev rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb

# CRITICAL: Log out and log back in for group membership to take effect
```

### USB Printer Not Discovered

**Symptom:** Printer discovery completes but USB printer not listed

**Cause:** Printer vendor ID not in known vendor list

**Solution:**

```bash
# Find printer vendor/product ID
lsusb  # Linux
system_profiler SPUSBDataType  # macOS
Get-WmiObject Win32_PnPEntity | Where-Object { $_.Name -like "*printer*" }  # Windows

# Example output:
# Bus 001 Device 005: ID 04b8:0e15 Epson Corp. TM-T88V

# Vendor ID: 04b8
# Product ID: 0e15

# Report missing vendor to support@eatsome.nl
# Temporary workaround: Use network printing instead
```

## Network Printer Issues

### Network Printer Not Discovered

**Symptom:** mDNS discovery does not find network printer

**Cause:** Firewall blocking mDNS (port 5353) or printer not advertising mDNS

**Solution:**

```bash
# Check firewall allows mDNS
# macOS: System Preferences → Security & Privacy → Firewall → Firewall Options
#        Ensure "Block all incoming connections" is OFF

# Linux: sudo ufw allow 5353/udp

# Windows: Windows Firewall → Advanced Settings → Inbound Rules
#          Create rule for UDP port 5353

# Test mDNS manually
# macOS: dns-sd -B _ipp._tcp .
# Linux: avahi-browse -at
# Windows: Install Bonjour Print Services

# If printer not advertising mDNS, add manually via IP
# Daemon UI → Add Printer → Manual IP: 192.168.1.100:9100
```

### "Connection refused" to Network Printer

**Symptom:** Daemon finds network printer but cannot connect (port 9100)

**Cause:** Printer not configured for raw TCP printing or firewall blocking

**Solution:**

```bash
# Test connection manually
telnet 192.168.1.100 9100

# If connection refused:
# 1. Check printer web UI → Network → Raw TCP enabled (port 9100)
# 2. Check printer firewall allows port 9100
# 3. Try alternative port 515 (LPR) or 631 (IPP)

# Update printer config in daemon:
# Settings → Printers → Edit → Address: 192.168.1.100:515
```

### Slow Network Printing

**Symptom:** Print jobs take >5 seconds to complete over network

**Cause:** Network latency or printer processing slow

**Solution:**

```bash
# Measure network latency
ping -c 10 192.168.1.100

# If latency >50ms:
# 1. Use wired connection instead of Wi-Fi
# 2. Move printer closer to router
# 3. Upgrade to managed switch (QoS for printer traffic)

# Check printer firmware up to date
# Printer web UI → Settings → Firmware → Check for updates
```

## Print Job Issues

### Print Jobs Stuck in Queue

**Symptom:** Queue depth increases but jobs never print

**Cause:** Circuit breaker OPEN (printer offline/failing)

**Solution:**

```bash
# Check circuit breaker status
# Daemon UI → Printers → View Status

# If circuit breaker OPEN:
# 1. Check printer powered on
# 2. Check printer USB/network connection
# 3. Restart printer (power cycle)
# 4. Restart daemon to reset circuit breaker

# Check daemon logs
# macOS/Linux: tail -f /tmp/eatsome-printer-service.log | grep -i circuit
# Windows: Get-Content "$env:APPDATA\Eatsome Printer Service\logs\daemon.log" -Wait | Select-String "circuit"

# Expected log: "Circuit breaker transitioned to HALF_OPEN, retrying..."
```

### Print Job Fails with "Printer offline"

**Symptom:** Job fails immediately with "Printer offline" error

**Cause:** Printer not responding to status queries

**Solution:**

```bash
# Test printer manually
# USB: echo "TEST" > /dev/usb/lp0  # Linux
#      echo "TEST" | lpr  # macOS
# Network: echo "TEST" | nc 192.168.1.100 9100

# If manual test works but daemon fails:
# 1. Restart daemon
# 2. Check daemon logs for specific error
# 3. Try removing and re-adding printer in daemon UI

# If manual test also fails:
# 1. Check printer error lights (paper out, cover open, etc.)
# 2. Restart printer
# 3. Check USB cable / network cable
```

### Garbled Print Output

**Symptom:** Printer prints random characters or symbols instead of order

**Cause:** Incorrect ESC/POS encoding or printer in wrong mode

**Solution:**

```bash
# Reset printer to default settings
# Printer menu → Settings → Reset → Factory Reset

# Ensure printer in ESC/POS mode (not STAR mode or other)
# Printer menu → Settings → Emulation → ESC/POS

# Check character encoding
# Daemon UI → Printer Settings → Encoding → UTF-8 (default)

# If still garbled:
# 1. Update printer firmware
# 2. Report to support@eatsome.nl with photo of garbled output
```

### Paper Cutter Not Working

**Symptom:** Print completes but paper not cut

**Cause:** Cutter capability not detected or cutter disabled

**Solution:**

```bash
# Check printer capabilities
# Daemon UI → Printers → View Details → Capabilities

# If cutter: false:
# 1. Edit printer settings
# 2. Enable "Paper Cutter" capability
# 3. Save and retry

# If cutter enabled but still not cutting:
# 1. Check cutter blade not jammed (paper debris)
# 2. Clean cutter mechanism
# 3. Replace cutter blade (if dull)
```

## Connection Issues

### Supabase Realtime Disconnected

**Symptom:** Daemon logs "Realtime connection lost" repeatedly

**Cause:** Network firewall blocking WebSocket or Supabase outage

**Solution:**

```bash
# Test WebSocket connection
curl -i -N -H "Connection: Upgrade" -H "Upgrade: websocket" \
  https://gtlpzikuozrdgomsvqmo.supabase.co/realtime/v1/websocket

# Expected: HTTP 101 Switching Protocols

# If connection blocked:
# 1. Check corporate firewall allows WebSocket (port 443)
# 2. Check proxy settings
# 3. Whitelist *.supabase.co in firewall

# Fallback: Use HTTP API instead
# Daemon automatically switches to localhost:8043 fallback
# POS app should detect and use HTTP instead of Realtime
```

### "JWT validation failed"

**Symptom:** Daemon rejects print jobs with "JWT validation failed"

**Cause:** Token expired or invalid secret

**Solution:**

```bash
# Check token expiration
# Decode JWT at https://jwt.io
# Check "exp" claim (Unix timestamp)

# If expired:
# 1. POS app → Settings → Printer Service → Reconnect
# 2. Scan new QR code or paste new token

# If token valid but still failing:
# 1. Check restaurant_id matches daemon config
# 2. Check token contains "print" permission
# 3. Restart daemon to reload config
```

### HTTP Fallback API Not Responding

**Symptom:** `curl localhost:8043/api/print` returns connection refused

**Cause:** Daemon not started or port 8043 already in use

**Solution:**

```bash
# Check daemon running
# macOS: ps aux | grep EatsomePrinterService
# Linux: systemctl --user status eatsome-printer.service
# Windows: tasklist | findstr EatsomePrinterService

# If not running, start daemon
# Check logs for startup errors

# Check port 8043 available
lsof -ti:8043  # macOS/Linux
netstat -ano | findstr :8043  # Windows

# If port in use, kill process:
kill -9 $(lsof -ti:8043)  # macOS/Linux
# Or change port in daemon config (not recommended)
```

## Performance Issues

### High Memory Usage (>100 MB)

**Symptom:** Daemon uses >100 MB memory

**Cause:** Queue database too large or memory leak

**Solution:**

```bash
# Check queue depth
sqlite3 ~/.config/eatsome-printer-service/print-queue.db \
  "SELECT COUNT(*) FROM print_jobs WHERE status = 'completed';"

# If >10,000 completed jobs:
# 1. Run cleanup: Daemon UI → Settings → Cleanup Queue
# 2. Or manually: DELETE FROM print_jobs WHERE status = 'completed' AND created_at < datetime('now', '-7 days');

# If still high memory:
# 1. Restart daemon (clears memory)
# 2. Report to support@eatsome.nl (possible memory leak)
```

### High CPU Usage (>10%)

**Symptom:** Daemon uses >10% CPU when idle

**Cause:** Excessive logging or tight polling loop

**Solution:**

```bash
# Check log level
# Daemon logs should NOT show debug/trace in production

# If debug logs enabled:
# 1. Edit config: RUST_LOG=info (not debug)
# 2. Restart daemon

# Check for infinite loop in logs:
# tail -f /tmp/eatsome-printer-service.log | grep -c "Checking for updates"
# Should be ~1 per 6 hours, not 100s per second

# If high CPU persists:
# 1. Restart daemon
# 2. Update to latest version (may contain performance fix)
```

### Slow Print Job Processing

**Symptom:** Jobs take >5 seconds to print after appearing in queue

**Cause:** Slow printer, network latency, or queue backlog

**Solution:**

```bash
# Check queue depth
# Daemon UI → Queue → View Stats

# If queue depth >50:
# 1. Circuit breaker may be rate-limiting to protect printer
# 2. Wait for queue to drain
# 3. Consider adding backup printer

# Check printer processing time
# Daemon logs: "Job job_123 printed in 4523ms"

# If >3000ms per job:
# 1. Reduce print density (Settings → Print Quality)
# 2. Disable unnecessary features (QR codes, logos)
# 3. Upgrade to faster printer model
```

## Platform-Specific Issues

### macOS: Daemon Won't Start After Update

**Symptom:** Daemon fails to launch after auto-update

**Cause:** Code signature invalidated or Gatekeeper re-blocking

**Solution:**

```bash
# Check quarantine attribute
xattr /Applications/EatsomePrinterService.app

# If com.apple.quarantine present:
xattr -d com.apple.quarantine /Applications/EatsomePrinterService.app

# Check code signature
codesign --verify --deep --verbose /Applications/EatsomePrinterService.app

# If signature invalid, re-download and install from GitHub Releases
```

### macOS: "This will damage your computer" Warning

**Symptom:** macOS shows "will damage your computer" message

**Cause:** App downloaded from untrusted source or damaged during download

**Solution:**

```bash
# ONLY download from official GitHub Releases
# https://github.com/eatsome/eatsome/releases

# Verify download integrity
shasum -a 256 EatsomePrinterService_aarch64.dmg
# Compare with SHA256 in GitHub Release notes

# If checksum matches, remove quarantine:
xattr -d com.apple.quarantine EatsomePrinterService_aarch64.dmg
```

### Windows: Daemon Doesn't Auto-Start on Login

**Symptom:** Daemon must be manually started after each login

**Cause:** Task Scheduler not configured or disabled

**Solution:**

```powershell
# Check Task Scheduler
taskschd.msc

# Navigate to: Task Scheduler Library → EatsomePrinterService
# Verify task exists and is enabled

# If missing, recreate:
$action = New-ScheduledTaskAction -Execute "C:\Program Files\Eatsome Printer Service\EatsomePrinterService.exe" -Argument "--silent"
$trigger = New-ScheduledTaskTrigger -AtLogOn
Register-ScheduledTask -TaskName "EatsomePrinterService" -Action $action -Trigger $trigger

# Test task
Start-ScheduledTask -TaskName "EatsomePrinterService"
```

### Linux: systemd Service Fails to Start

**Symptom:** `systemctl --user status eatsome-printer.service` shows "failed"

**Cause:** Missing dependencies or incorrect ExecStart path

**Solution:**

```bash
# View service logs
journalctl --user -u eatsome-printer.service -n 50

# Common errors:
# 1. "Executable not found"
#    → Verify: ls /opt/eatsome-printer-service/eatsome-printer-service
#    → Fix: Reinstall package

# 2. "Permission denied"
#    → Check ownership: ls -l /opt/eatsome-printer-service/
#    → Fix: sudo chown root:root /opt/eatsome-printer-service/eatsome-printer-service

# 3. "Failed to connect to bus"
#    → Run as user (not sudo): systemctl --user start eatsome-printer.service

# Reload systemd after fix
systemctl --user daemon-reload
systemctl --user restart eatsome-printer.service
```

## Getting Help

If none of these solutions work:

1. **Check Sentry:** Errors automatically reported to engineering team
2. **GitHub Issues:** https://github.com/eatsome/eatsome/issues
3. **Email Support:** support@eatsome.nl
4. **Include:**
   - Platform (macOS/Windows/Linux version)
   - Daemon version (Help → About)
   - Printer model
   - Relevant log excerpt (last 50 lines)
   - Steps to reproduce

**Log Locations:**

- macOS: `/tmp/eatsome-printer-service.log`
- Windows: `%APPDATA%\Eatsome Printer Service\logs\daemon.log`
- Linux: `~/.local/share/eatsome-printer-service/logs/daemon.log` or `journalctl --user -u eatsome-printer.service`
