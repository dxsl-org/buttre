#!/bin/bash
# buttre IBus - Installation Script
# Run with sudo

set -e

echo "🐧 Installing buttre IBus Engine..."

# Configuration
PREFIX="${PREFIX:-/usr}"
BINDIR="$PREFIX/bin"
COMPONENTDIR="$PREFIX/share/ibus/component"
# hicolor 128x128 backs the <icon>buttre</icon> the component XML advertises.
ICONDIR="$PREFIX/share/icons/hicolor/128x128/apps"
PIXMAPDIR="$PREFIX/share/pixmaps"
# Honour CARGO_TARGET_DIR: on a VirtualBox shared folder (vboxsf) build-script
# outputs land empty, so builds are commonly redirected to a real filesystem.
TARGET_DIR="${CARGO_TARGET_DIR:-target}"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "❌ Please run as root (sudo ./install.sh)"
    exit 1
fi

# Build release binary
echo "📦 Building release binary..."
cargo build --release -p buttre-platform

# Create directories
echo "📁 Creating directories..."
mkdir -p "$BINDIR"
mkdir -p "$COMPONENTDIR"
mkdir -p "$ICONDIR"
mkdir -p "$PIXMAPDIR"

# Install binary (component XML expects /usr/bin/buttre)
echo "📥 Installing binary..."
install -m 755 "$TARGET_DIR/release/buttre" "$BINDIR/"

# Install component XML
echo "📄 Installing component..."
install -m 644 installers/linux/buttre.xml "$COMPONENTDIR/buttre.xml"

# Install engine icon: hicolor for GNOME/IBus, pixmaps as a legacy fallback.
# Resolves <icon>buttre</icon> in buttre.xml (else the switcher shows only text).
echo "🎨 Installing icon..."
install -m 644 crates/buttre-platform/icons/vietnamese.png "$ICONDIR/buttre.png"
install -m 644 crates/buttre-platform/icons/vietnamese.png "$PIXMAPDIR/buttre.png"
gtk-update-icon-cache -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true

# Restart IBus
echo "🔄 Restarting IBus..."
if command -v ibus-daemon &> /dev/null; then
    killall ibus-daemon 2>/dev/null || true
    sleep 1
    ibus-daemon -drx &
fi

echo "✅ Installation complete!"
echo ""
echo "📝 Next steps:"
echo "1. Open IBus Preferences: ibus-setup"
echo "2. Go to 'Input Method' tab"
echo "3. Click 'Add' button"
echo "4. Select 'Vietnamese' → 'buttre Vietnamese (Telex)'"
echo "5. Test in any application (gedit, Firefox, etc.)"
echo ""
echo "🔑 Switch input method: Super+Space (or configured hotkey)"
