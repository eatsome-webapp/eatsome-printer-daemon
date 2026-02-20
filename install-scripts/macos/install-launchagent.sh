#!/bin/bash

# macOS LaunchAgent Installation Script
# Installs and starts the Eatsome Printer Service as a user LaunchAgent

set -e

PLIST_NAME="com.eatsome.printer-service.plist"
LAUNCH_AGENTS_DIR="$HOME/Library/LaunchAgents"
PLIST_DEST="$LAUNCH_AGENTS_DIR/$PLIST_NAME"

echo "🚀 Installing Eatsome Printer Service LaunchAgent..."

# Create LaunchAgents directory if it doesn't exist
mkdir -p "$LAUNCH_AGENTS_DIR"

# Copy plist file
echo "📄 Copying LaunchAgent configuration..."
cp "$PLIST_NAME" "$PLIST_DEST"

# Set correct permissions
chmod 644 "$PLIST_DEST"

# Unload if already running (ignore errors)
echo "🔄 Stopping existing service (if running)..."
launchctl unload "$PLIST_DEST" 2>/dev/null || true

# Load the LaunchAgent
echo "✅ Loading LaunchAgent..."
launchctl load "$PLIST_DEST"

# Verify it's running
sleep 2
if launchctl list | grep -q "com.eatsome.printer-service"; then
    echo "✅ Eatsome Printer Service is now running!"
    echo "📊 Service will start automatically on login."
else
    echo "❌ Failed to start service. Check logs:"
    echo "   - /tmp/com.eatsome.printer-service.out.log"
    echo "   - /tmp/com.eatsome.printer-service.err.log"
    exit 1
fi

echo ""
echo "📝 Management commands:"
echo "  Stop service:    launchctl unload ~/Library/LaunchAgents/$PLIST_NAME"
echo "  Start service:   launchctl load ~/Library/LaunchAgents/$PLIST_NAME"
echo "  Check status:    launchctl list | grep eatsome"
echo "  View logs:       tail -f /tmp/com.eatsome.printer-service.out.log"
