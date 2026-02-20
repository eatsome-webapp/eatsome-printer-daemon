#!/bin/bash
#
# RPM (Fedora/RHEL/CentOS) Postinstall Script
# Runs after RPM package installation
#
# Note: RPM postinstall scripts run differently than deb
# %post section in .spec file calls this script
#

set -e

# Package name
PACKAGE_NAME="eatsome-printer-service"

# Installation paths
INSTALL_DIR="/opt/eatsome-printer-service"
SERVICE_FILE="/usr/lib/systemd/user/eatsome-printer.service"
UDEV_RULES="/etc/udev/rules.d/60-eatsome-printer.rules"

# ============================================================================
# 1. Create udev Rules for USB Printer Access
# ============================================================================

echo "📝 Creating udev rules for USB printer access..."

cat > "$UDEV_RULES" <<'EOF'
# Eatsome Printer Service - USB Printer Access Rules
# Grants lp group access to USB thermal printers

# Epson Printers
SUBSYSTEM=="usb", ATTRS{idVendor}=="04b8", MODE="0666", GROUP="lp"

# Star Micronics Printers
SUBSYSTEM=="usb", ATTRS{idVendor}=="0519", MODE="0666", GROUP="lp"

# Brother Printers
SUBSYSTEM=="usb", ATTRS{idVendor}=="04f9", MODE="0666", GROUP="lp"

# Citizen Printers
SUBSYSTEM=="usb", ATTRS{idVendor}=="1d90", MODE="0666", GROUP="lp"

# Generic USB Printers (Class 7 = Printer)
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTRS{bDeviceClass}=="07", MODE="0666", GROUP="lp"
EOF

chmod 644 "$UDEV_RULES"
echo "✅ udev rules created"

# ============================================================================
# 2. Reload udev Rules
# ============================================================================

echo "🔄 Reloading udev rules..."
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules
    udevadm trigger --subsystem-match=usb
    echo "✅ udev rules reloaded"
else
    echo "⚠️  udevadm not found - rules will apply after reboot"
fi

# ============================================================================
# 3. Create Data Directory
# ============================================================================

DATA_DIR="/var/lib/eatsome-printer-service"
if [ ! -d "$DATA_DIR" ]; then
    echo "📁 Creating data directory..."
    mkdir -p "$DATA_DIR"
    chmod 755 "$DATA_DIR"
    echo "✅ Data directory created"
fi

# ============================================================================
# 4. Display Post-Install Instructions
# ============================================================================

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Eatsome Printer Service installed successfully!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "🎯 Next Steps (run as your user, NOT root):"
echo ""
echo "  1. Add yourself to the lp group:"
echo "     sudo usermod -a -G lp \$USER"
echo ""
echo "  2. Log out and log back in"
echo ""
echo "  3. Enable and start the service:"
echo "     systemctl --user enable eatsome-printer.service"
echo "     systemctl --user start eatsome-printer.service"
echo ""
echo "  4. Enable lingering (allows service to run without active session):"
echo "     loginctl enable-linger \$USER"
echo ""
echo "📖 Management commands:"
echo ""
echo "  Check status:  systemctl --user status eatsome-printer.service"
echo "  View logs:     journalctl --user -u eatsome-printer.service -f"
echo "  Restart:       systemctl --user restart eatsome-printer.service"
echo ""
echo "❓ Need help? Visit: https://github.com/eatsome/eatsome/issues"
echo ""

exit 0
