#!/bin/bash

# Linux systemd User Service Installation Script
# Installs and starts the Eatsome Printer Service as a user service

set -e

SERVICE_NAME="eatsome-printer.service"
USER_SERVICE_DIR="$HOME/.config/systemd/user"
SERVICE_DEST="$USER_SERVICE_DIR/$SERVICE_NAME"

echo "🚀 Installing Eatsome Printer Service..."

# Create user service directory
mkdir -p "$USER_SERVICE_DIR"

# Copy service file
echo "📄 Copying service file..."
cp "$SERVICE_NAME" "$SERVICE_DEST"

# Reload systemd
echo "🔄 Reloading systemd..."
systemctl --user daemon-reload

# Enable service (start on login)
echo "✅ Enabling service..."
systemctl --user enable "$SERVICE_NAME"

# Start service immediately
echo "▶️  Starting service..."
systemctl --user start "$SERVICE_NAME"

# Wait a moment for service to start
sleep 2

# Check status
if systemctl --user is-active --quiet "$SERVICE_NAME"; then
    echo "✅ Eatsome Printer Service is now running!"
    echo "📊 Service will start automatically on login."
else
    echo "❌ Service failed to start. Checking status..."
    systemctl --user status "$SERVICE_NAME" || true
    exit 1
fi

echo ""
echo "📝 Management commands:"
echo "  Status:       systemctl --user status $SERVICE_NAME"
echo "  Stop:         systemctl --user stop $SERVICE_NAME"
echo "  Start:        systemctl --user start $SERVICE_NAME"
echo "  Restart:      systemctl --user restart $SERVICE_NAME"
echo "  Disable:      systemctl --user disable $SERVICE_NAME"
echo "  View logs:    journalctl --user -u $SERVICE_NAME -f"
echo "  Last 50 logs: journalctl --user -u $SERVICE_NAME -n 50"

# Add user to lp group if not already a member
if ! groups | grep -q '\blp\b'; then
    echo ""
    echo "⚠️  You are not in the 'lp' group (required for USB printer access)"
    echo "   Run: sudo usermod -a -G lp \$USER"
    echo "   Then log out and log back in for group changes to take effect"
fi

# Check for udev rules
if [ ! -f "/etc/udev/rules.d/60-eatsome-printer.rules" ]; then
    echo ""
    echo "⚠️  udev rules not found. USB printer access may not work."
    echo "   Install the deb/rpm package for automatic udev rule setup"
fi
