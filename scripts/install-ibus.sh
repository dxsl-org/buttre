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

# Install a system-wide launcher so the tray/config app is reachable from the
# desktop's application menu (the IBus engine and the tray app are separate
# processes — ibus-daemon spawns `buttre --ibus` for typing, but nothing
# launches the tray/"Cấu hình" UI on its own).
echo "🚀 Installing application launcher..."
APPDIR="$PREFIX/share/applications"
mkdir -p "$APPDIR"
cat > "$APPDIR/buttre.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=buttre
Comment=Bộ gõ tiếng Việt
Exec="$BINDIR/buttre"
Icon=buttre
Terminal=false
Categories=Utility;
EOF
chmod 644 "$APPDIR/buttre.desktop"

# Enable "Tự động khởi động cùng Hệ điều hành" out of the box: create the XDG
# autostart entry so the tray app starts at every login. This runs under sudo,
# so $HOME is root's — the entry MUST land in the INVOKING user's home (via
# $SUDO_USER) and be owned by them, or the desktop session never reads it.
# The file is byte-identical to what `buttre_autostart::set_enabled(true)`
# writes, so the tray's own re-registration on launch is an idempotent rewrite.
REAL_USER="${SUDO_USER:-}"
if [ -n "$REAL_USER" ] && [ "$REAL_USER" != "root" ]; then
    USER_HOME="$(getent passwd "$REAL_USER" | cut -d: -f6)"
    if [ -n "$USER_HOME" ] && [ -d "$USER_HOME" ]; then
        echo "🔁 Enabling autostart for user '$REAL_USER'..."
        # Resolve the primary group rather than assuming it equals the username
        # (user-private-groups is common but not universal — a mismatch would
        # abort the whole installer via `set -e`).
        REAL_GROUP="$(id -gn "$REAL_USER")"
        AUTOSTART_DIR="$USER_HOME/.config/autostart"
        install -d -o "$REAL_USER" -g "$REAL_GROUP" "$AUTOSTART_DIR"
        cat > "$AUTOSTART_DIR/buttre.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=buttre
Comment=Bộ gõ tiếng Việt
Exec="$BINDIR/buttre"
X-GNOME-Autostart-enabled=true
EOF
        chmod 644 "$AUTOSTART_DIR/buttre.desktop"
        chown "$REAL_USER:$REAL_GROUP" "$AUTOSTART_DIR/buttre.desktop"
    else
        echo "⚠️  Could not resolve home for '$REAL_USER' — skipping autostart."
        echo "    Enable it later from the tray: Cấu hình → Tự động khởi động."
    fi
else
    echo "⚠️  Run via 'sudo' (not a root login) to auto-enable autostart per user."
    echo "    Otherwise enable it from the tray: Cấu hình → Tự động khởi động."
fi

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
echo "🖳  Tray & settings: the tray app (menu + 'Cấu hình') autostarts at your"
echo "    NEXT login. To use it right now without logging out, run: buttre &"
echo ""
echo "🔑 Switch input method: Super+Space (or configured hotkey)"
