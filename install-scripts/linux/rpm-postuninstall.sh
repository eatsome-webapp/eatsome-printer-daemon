#!/bin/bash
#
# RPM (Fedora/RHEL/CentOS) Post-Uninstall Script
# Runs after RPM package removal
#
# %postun section in .spec file calls this script
#

set -e

# Installation paths
UDEV_RULES="/etc/udev/rules.d/60-eatsome-printer.rules"
DATA_DIR="/var/lib/eatsome-printer-service"

# ============================================================================
# 1. Remove udev Rules
# ============================================================================

if [ -f "$UDEV_RULES" ]; then
    echo "🗑️  Removing udev rules..."
    rm -f "$UDEV_RULES"

    # Reload udev
    if command -v udevadm >/dev/null 2>&1; then
        udevadm control --reload-rules
        udevadm trigger --subsystem-match=usb
    fi

    echo "✅ udev rules removed"
fi

# ============================================================================
# 2. Data Directory Cleanup
# ============================================================================

# RPM doesn't have "purge" concept, so we clean up on full removal
# $1 = number of instances remaining (0 = full removal)
REMAINING_INSTANCES="${1:-0}"

if [ "$REMAINING_INSTANCES" -eq 0 ]; then
    echo "🗑️  Cleaning up data directory..."

    if [ -d "$DATA_DIR" ]; then
        rm -rf "$DATA_DIR"
        echo "✅ Data directory removed"
    fi

    # Also remove user's config directories (all users)
    for user_home in /home/*; do
        USER_CONFIG="$user_home/.config/eatsome-printer-service"
        if [ -d "$USER_CONFIG" ]; then
            rm -rf "$USER_CONFIG"
            echo "✅ Removed config for $(basename $user_home)"
        fi
    done
else
    echo "📦 Package being upgraded - data preserved"
fi

# ============================================================================
# 3. Display Post-Removal Information
# ============================================================================

if [ "$REMAINING_INSTANCES" -eq 0 ]; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "✅ Eatsome Printer Service removed successfully"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "💡 To stop the service for your user, run:"
    echo "   systemctl --user stop eatsome-printer.service"
    echo "   systemctl --user disable eatsome-printer.service"
    echo ""
fi

exit 0
