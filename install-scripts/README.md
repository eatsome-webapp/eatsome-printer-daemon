# Installation Scripts - Auto-Start Configuration

This directory contains platform-specific auto-start configurations for the Eatsome Printer Service daemon.

## Overview

The daemon must start automatically on system boot/user login to ensure printers are always available for the POS system. Each platform has its own mechanism:

| Platform    | Mechanism            | User/System | Location                     |
| ----------- | -------------------- | ----------- | ---------------------------- |
| **macOS**   | LaunchAgent          | User        | `~/Library/LaunchAgents/`    |
| **Windows** | Task Scheduler       | User        | Task Scheduler → `\Eatsome\` |
| **Linux**   | systemd user service | User        | `~/.config/systemd/user/`    |

## macOS (LaunchAgent)

### Installation

```bash
cd install-scripts/macos
./install-launchagent.sh
```

### Manual Installation

```bash
# Copy plist
cp com.eatsome.printer-service.plist ~/Library/LaunchAgents/

# Load service
launchctl load ~/Library/LaunchAgents/com.eatsome.printer-service.plist
```

### Management

```bash
# Status
launchctl list | grep eatsome

# Stop
launchctl unload ~/Library/LaunchAgents/com.eatsome.printer-service.plist

# Start
launchctl load ~/Library/LaunchAgents/com.eatsome.printer-service.plist

# View logs
tail -f /tmp/com.eatsome.printer-service.out.log
tail -f /tmp/com.eatsome.printer-service.err.log
```

### Behavior

- ✅ Starts automatically on user login
- ✅ Restarts automatically if crashes
- ✅ Runs in user context (access to user files/config)
- ✅ Throttled restart (max 1 restart per 10 seconds)
- ✅ Graceful shutdown with 30-second timeout

### Troubleshooting

**Service not starting:**

```bash
# Check logs
cat /tmp/com.eatsome.printer-service.err.log

# Validate plist syntax
plutil -lint ~/Library/LaunchAgents/com.eatsome.printer-service.plist

# Check for errors
launchctl load -w ~/Library/LaunchAgents/com.eatsome.printer-service.plist 2>&1
```

**Permission denied:**

```bash
# Fix permissions
chmod 644 ~/Library/LaunchAgents/com.eatsome.printer-service.plist
```

## Windows (Task Scheduler)

### Installation

```powershell
cd install-scripts\windows
.\install-task.ps1
```

### Manual Installation

```powershell
# Import XML
$xml = Get-Content .\EatsomePrinterService.xml -Raw
Register-ScheduledTask -Xml $xml -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"

# Start task
Start-ScheduledTask -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"
```

### Management

```powershell
# Status
Get-ScheduledTask -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"

# Start
Start-ScheduledTask -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"

# Stop
Stop-ScheduledTask -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"

# Uninstall
.\install-task.ps1 -Uninstall

# View task history
Get-WinEvent -LogName Microsoft-Windows-TaskScheduler/Operational | Where-Object {$_.Message -like "*Eatsome*"}
```

### Behavior

- ✅ Starts automatically on user login (10-second delay)
- ✅ Restarts automatically if crashes (max 3 attempts, 1-minute interval)
- ✅ Runs in user context (no admin privileges required)
- ✅ Ignores new instances (won't start duplicates)
- ✅ Below-normal priority (doesn't interfere with POS app)

### Troubleshooting

**Task not starting:**

```powershell
# Check task history
Get-ScheduledTaskInfo -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"

# View last run result
(Get-ScheduledTask -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\").State

# Test run manually
Start-ScheduledTask -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"
```

**Execution policy error:**

```powershell
# Set execution policy for current user
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
```

## Linux (systemd User Service)

### Installation

```bash
cd install-scripts/linux
./install-service.sh
```

### Manual Installation

```bash
# Create user service directory
mkdir -p ~/.config/systemd/user

# Copy service file
cp eatsome-printer.service ~/.config/systemd/user/

# Reload systemd
systemctl --user daemon-reload

# Enable and start
systemctl --user enable eatsome-printer.service
systemctl --user start eatsome-printer.service
```

### Management

```bash
# Status
systemctl --user status eatsome-printer.service

# Stop
systemctl --user stop eatsome-printer.service

# Start
systemctl --user start eatsome-printer.service

# Restart
systemctl --user restart eatsome-printer.service

# Disable auto-start
systemctl --user disable eatsome-printer.service

# View logs
journalctl --user -u eatsome-printer.service -f

# Last 50 logs
journalctl --user -u eatsome-printer.service -n 50
```

### Behavior

- ✅ Starts automatically on user login
- ✅ Restarts automatically if crashes (10-second delay)
- ✅ Runs in user context with `lp` group membership
- ✅ Graceful shutdown with 30-second timeout
- ✅ Resource limits (200MB RAM, 50% CPU)
- ✅ Security hardening (PrivateTmp, NoNewPrivileges, etc.)

### Prerequisites

**Add user to `lp` group** (for USB printer access):

```bash
sudo usermod -a -G lp $USER
# Log out and log back in for group changes to take effect
```

**Verify group membership:**

```bash
groups | grep -q lp && echo "✓ User is in lp group" || echo "✗ User NOT in lp group"
```

### Troubleshooting

**Service not starting:**

```bash
# Check systemd status
systemctl --user status eatsome-printer.service

# View detailed logs
journalctl --user -u eatsome-printer.service --no-pager

# Check for USB permission issues
ls -l /dev/bus/usb/*/*
```

**USB printer access denied:**

```bash
# Verify lp group membership
groups | grep lp

# Check udev rules
ls -l /etc/udev/rules.d/60-eatsome-printer.rules

# Reload udev rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

**Service fails after reboot:**

```bash
# Enable lingering (allows user services to run without active session)
loginctl enable-linger $USER

# Verify lingering enabled
loginctl show-user $USER | grep Linger
```

## Integration with Installers

These configurations are automatically integrated into platform-specific installers:

### macOS (.dmg)

- ✅ plist copied to `/Applications/Eatsome Printer Service.app/Contents/Resources/`
- ✅ Post-install script copies to `~/Library/LaunchAgents/` and loads
- ✅ Uninstaller removes plist and unloads service

### Windows (.msi)

- ✅ Task XML embedded in MSI
- ✅ Custom action imports task on install
- ✅ Custom action removes task on uninstall

### Linux (.deb / .rpm)

- ✅ Service file installed to `/usr/lib/systemd/user/eatsome-printer.service`
- ✅ Postinstall script enables service for current user
- ✅ Postrm script disables and removes service

## Security Considerations

### macOS

- ✅ Runs with user privileges (not root)
- ✅ Logs to user-writable `/tmp/` directory
- ✅ No elevated permissions required

### Windows

- ✅ Runs with user privileges (LeastPrivilege)
- ✅ No admin rights required
- ✅ Task isolated to user session

### Linux

- ✅ Runs with user privileges + `lp` group
- ✅ Security hardening via systemd directives
- ✅ Resource limits enforced
- ✅ Private `/tmp`, read-only system directories

## Testing

### Verify Auto-Start Works

**macOS:**

```bash
# Reboot and check
launchctl list | grep eatsome
```

**Windows:**

```powershell
# Reboot and check
Get-ScheduledTask -TaskName "Eatsome Printer Service" -TaskPath "\Eatsome\"
```

**Linux:**

```bash
# Reboot and check
systemctl --user is-active eatsome-printer.service
```

### Verify Crash Recovery

**Simulate crash:**

```bash
# macOS
pkill -9 eatsome-printer-service
# Wait 10 seconds, verify restarted
launchctl list | grep eatsome

# Windows
Stop-Process -Name eatsome-printer-service -Force
# Wait 1 minute, verify restarted
Get-Process eatsome-printer-service

# Linux
pkill -9 eatsome-printer-service
# Wait 10 seconds, verify restarted
systemctl --user is-active eatsome-printer.service
```

## Logging

Each platform logs to a different location:

| Platform    | Stdout Log                                 | Stderr Log                                 | Viewer              |
| ----------- | ------------------------------------------ | ------------------------------------------ | ------------------- |
| **macOS**   | `/tmp/com.eatsome.printer-service.out.log` | `/tmp/com.eatsome.printer-service.err.log` | `tail -f`           |
| **Windows** | Task Scheduler history                     | Event Viewer                               | Task Scheduler UI   |
| **Linux**   | systemd journal                            | systemd journal                            | `journalctl --user` |

## References

- [macOS LaunchAgent docs](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html)
- [Windows Task Scheduler XML Schema](https://docs.microsoft.com/en-us/windows/win32/taskschd/task-scheduler-schema)
- [systemd user services](https://www.freedesktop.org/software/systemd/man/systemd.service.html)
