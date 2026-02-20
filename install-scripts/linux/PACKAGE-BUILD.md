# Linux Package Build Integration

This guide explains how to integrate the postinstall/postrm scripts into deb and rpm package builds using Tauri's bundler.

## Overview

Tauri's Linux bundler supports both deb and rpm package formats. Postinstall scripts handle:

- ✅ Creating udev rules for USB printer access
- ✅ Adding user to `lp` group
- ✅ Creating data directories
- ✅ Enabling systemd user service
- ✅ Cleanup on uninstall

## Debian/Ubuntu (.deb) Packages

### File Structure

```
src-tauri/
├── tauri.conf.json
└── debian/
    ├── postinst          # Symlink to install-scripts/linux/postinst
    └── postrm            # Symlink to install-scripts/linux/postrm
```

### Setup

1. **Create debian directory:**

   ```bash
   cd apps/printer-daemon-tauri/src-tauri
   mkdir -p debian
   ```

2. **Create symlinks to scripts:**

   ```bash
   cd debian
   ln -s ../../install-scripts/linux/postinst postinst
   ln -s ../../install-scripts/linux/postrm postrm
   chmod +x postinst postrm
   ```

3. **Update tauri.conf.json:**

   ```json
   {
     "bundle": {
       "linux": {
         "deb": {
           "depends": ["libc6", "libusb-1.0-0", "systemd"],
           "files": {
             "/etc/udev/rules.d/60-eatsome-printer.rules": "../install-scripts/linux/60-eatsome-printer.rules",
             "/usr/lib/systemd/user/eatsome-printer.service": "../install-scripts/linux/eatsome-printer.service"
           }
         }
       }
     }
   }
   ```

### Build

```bash
pnpm tauri build --target x86_64-unknown-linux-gnu
```

### Test Package

```bash
# Extract and inspect
dpkg-deb -c target/release/bundle/deb/eatsome-printer-service_1.0.0_amd64.deb

# Install
sudo dpkg -i target/release/bundle/deb/eatsome-printer-service_1.0.0_amd64.deb

# Verify postinst ran
systemctl --user status eatsome-printer.service
cat /etc/udev/rules.d/60-eatsome-printer.rules
groups | grep lp

# Uninstall
sudo apt remove eatsome-printer-service

# Purge (removes data)
sudo apt purge eatsome-printer-service
```

## Fedora/RHEL (.rpm) Packages

### File Structure

```
src-tauri/
├── tauri.conf.json
└── rpm/
    ├── eatsome-printer-service.spec   # RPM spec file
    └── scripts/
        ├── postinstall.sh             # Symlink to install-scripts/linux/rpm-postinstall.sh
        └── postuninstall.sh           # Symlink to install-scripts/linux/rpm-postuninstall.sh
```

### Setup

1. **Create rpm directory:**

   ```bash
   cd apps/printer-daemon-tauri/src-tauri
   mkdir -p rpm/scripts
   ```

2. **Create symlinks:**

   ```bash
   cd rpm/scripts
   ln -s ../../../install-scripts/linux/rpm-postinstall.sh postinstall.sh
   ln -s ../../../install-scripts/linux/rpm-postuninstall.sh postuninstall.sh
   chmod +x postinstall.sh postuninstall.sh
   ```

3. **Create RPM spec file** (`rpm/eatsome-printer-service.spec`):

   ```spec
   Name:           eatsome-printer-service
   Version:        1.0.0
   Release:        1%{?dist}
   Summary:        Eatsome Printer Service - Thermal printer daemon

   License:        MIT
   URL:            https://github.com/eatsome/eatsome
   Source0:        %{name}-%{version}.tar.gz

   Requires:       glibc
   Requires:       libusb1
   Requires:       systemd

   %description
   Background daemon for managing thermal printers in Eatsome restaurant POS system.

   %prep
   %setup -q

   %build
   # Built by Tauri

   %install
   mkdir -p %{buildroot}/opt/eatsome-printer-service
   mkdir -p %{buildroot}/usr/lib/systemd/user
   mkdir -p %{buildroot}/etc/udev/rules.d

   # Install binary
   install -m 755 eatsome-printer-service %{buildroot}/opt/eatsome-printer-service/

   # Install systemd service
   install -m 644 eatsome-printer.service %{buildroot}/usr/lib/systemd/user/

   # Install udev rules (will be created by postinstall)
   # Placeholder - actual rules created in %post

   %files
   /opt/eatsome-printer-service/eatsome-printer-service
   /usr/lib/systemd/user/eatsome-printer.service

   %post
   # Run postinstall script
   /bin/bash %{_datadir}/%{name}/scripts/postinstall.sh

   %postun
   # Run postuninstall script
   /bin/bash %{_datadir}/%{name}/scripts/postuninstall.sh $1

   %changelog
   * Tue Jan 28 2026 Eatsome B.V. <support@eatsome.nl> - 1.0.0-1
   - Initial release
   ```

4. **Update tauri.conf.json:**

   ```json
   {
     "bundle": {
       "linux": {
         "rpm": {
           "depends": ["glibc", "libusb1", "systemd"],
           "files": {
             "/etc/udev/rules.d/60-eatsome-printer.rules": "../install-scripts/linux/60-eatsome-printer.rules",
             "/usr/lib/systemd/user/eatsome-printer.service": "../install-scripts/linux/eatsome-printer.service"
           }
         }
       }
     }
   }
   ```

### Build

```bash
pnpm tauri build --target x86_64-unknown-linux-gnu
```

### Test Package

```bash
# Install
sudo rpm -ivh target/release/bundle/rpm/eatsome-printer-service-1.0.0-1.x86_64.rpm

# Verify postinstall ran
systemctl --user status eatsome-printer.service
cat /etc/udev/rules.d/60-eatsome-printer.rules

# Uninstall
sudo rpm -e eatsome-printer-service

# Check cleanup
ls /etc/udev/rules.d/ | grep eatsome
```

## Script Behavior

### postinst / rpm-postinstall.sh

**What it does:**

1. Creates udev rules in `/etc/udev/rules.d/60-eatsome-printer.rules`
2. Reloads udev to apply rules immediately
3. Adds current user to `lp` group
4. Creates data directory `/var/lib/eatsome-printer-service`
5. Enables systemd user service (deb only, RPM requires manual enable)
6. Displays next steps to user

**User sees:**

```
📝 Creating udev rules for USB printer access...
✅ udev rules created at /etc/udev/rules.d/60-eatsome-printer.rules
🔄 Reloading udev rules...
✅ udev rules reloaded
👤 Adding user to 'lp' group...
✅ User added to lp group
⚠️  IMPORTANT: Log out and log back in for group changes to take effect!
📁 Creating data directory...
✅ Data directory created at /var/lib/eatsome-printer-service
🚀 Enabling systemd user service for user...
✅ Service enabled and started for user

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
✅ Eatsome Printer Service installed successfully!
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### postrm / rpm-postuninstall.sh

**What it does:**

1. Stops systemd user service
2. Disables systemd user service
3. Removes udev rules
4. Reloads udev
5. On purge/full removal: deletes data directory
6. On purge/full removal: deletes user config directory

**Difference between remove and purge (deb only):**

- `apt remove`: Keeps config/data (upgrade-safe)
- `apt purge`: Deletes everything (clean uninstall)

## Testing Checklist

### Pre-Build

- [ ] Symlinks created correctly
- [ ] Scripts are executable (`chmod +x`)
- [ ] Script syntax valid (`bash -n script.sh`)

### Post-Build

- [ ] Package contains correct files (`dpkg-deb -c` / `rpm -qlp`)
- [ ] Package metadata correct (version, dependencies)

### Post-Install

- [ ] udev rules created in `/etc/udev/rules.d/`
- [ ] User added to `lp` group (`groups | grep lp`)
- [ ] Data directory created in `/var/lib/`
- [ ] Systemd service enabled and running (deb)
- [ ] Systemd service files present (rpm)

### USB Printer Test

- [ ] Plug in USB thermal printer
- [ ] Check device appears: `ls /dev/bus/usb/*/*`
- [ ] Check permissions: `ls -l /dev/bus/usb/*/xxx` (should be group `lp`)
- [ ] Daemon can access printer (check daemon logs)

### Post-Removal

- [ ] udev rules removed
- [ ] Systemd service stopped
- [ ] Config preserved (on remove)
- [ ] Config deleted (on purge)

## Troubleshooting

### postinst fails

**Symptom:** Package installation fails with postinst error

**Debug:**

```bash
# Extract package without running scripts
dpkg --unpack package.deb

# Run postinst manually
sudo /var/lib/dpkg/info/eatsome-printer-service.postinst configure

# Check logs
journalctl -xe
```

### udev rules not working

**Symptom:** USB printer access denied

**Debug:**

```bash
# Check rules exist
cat /etc/udev/rules.d/60-eatsome-printer.rules

# Reload udev manually
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb

# Check device permissions
lsusb  # Find printer
ls -l /dev/bus/usb/XXX/YYY  # Check permissions

# Test rules
sudo udevadm test $(udevadm info -q path -n /dev/bus/usb/XXX/YYY)
```

### systemd service not starting

**Symptom:** Service fails to start after install

**Debug:**

```bash
# Check service file exists
ls /usr/lib/systemd/user/eatsome-printer.service

# Reload systemd
systemctl --user daemon-reload

# Check service status
systemctl --user status eatsome-printer.service

# View logs
journalctl --user -u eatsome-printer.service -n 50
```

### User not in lp group

**Symptom:** postinst says user added, but `groups` doesn't show `lp`

**Cause:** Group membership requires logout/login

**Solution:**

```bash
# Verify user is in lp group (system level)
id $USER | grep lp

# Force apply without logout (temporary, doesn't persist)
newgrp lp

# Proper solution: logout and login
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Build Linux Packages

on:
  release:
    types: [created]

jobs:
  build-linux:
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Setup pnpm
        uses: pnpm/action-setup@v2

      - name: Install dependencies
        run: pnpm install

      - name: Create symlinks for package scripts
        run: |
          cd apps/printer-daemon-tauri/src-tauri
          mkdir -p debian
          ln -s ../../install-scripts/linux/postinst debian/postinst
          ln -s ../../install-scripts/linux/postrm debian/postrm
          chmod +x debian/postinst debian/postrm

      - name: Build packages
        run: pnpm tauri build

      - name: Test deb package
        run: |
          sudo dpkg -i apps/printer-daemon-tauri/src-tauri/target/release/bundle/deb/*.deb
          systemctl --user status eatsome-printer.service || true
          sudo dpkg -r eatsome-printer-service

      - name: Upload artifacts
        uses: actions/upload-artifact@v3
        with:
          name: linux-packages
          path: |
            apps/printer-daemon-tauri/src-tauri/target/release/bundle/deb/*.deb
            apps/printer-daemon-tauri/src-tauri/target/release/bundle/rpm/*.rpm
```

## References

- [Debian Policy Manual - Maintainer Scripts](https://www.debian.org/doc/debian-policy/ch-maintainerscripts.html)
- [RPM Packaging Guide](https://rpm-packaging-guide.github.io/)
- [Tauri Linux Bundler](https://v2.tauri.app/reference/config/#linuxconfig)
- [systemd User Services](https://www.freedesktop.org/software/systemd/man/systemd.service.html)
- [udev Rules Writing](https://www.reactivated.net/writing_udev_rules.html)
