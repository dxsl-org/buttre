#!/bin/bash
# Interactive IBus IME test for WSL2/WSLg (or any Linux session).
#
# Renders a GTK text entry DIRECTLY on the host display (WSLg renders plain
# GTK apps fine — only nested compositors have presentation quirks) and
# routes its input through ibus-daemon → the buttre engine.
# Type `vieejt ` and expect `việt ` (preedit underline while composing).
set -eu
BIN="${BUTTRE_BIN:-target/debug/buttre}"
[ -x "$BIN" ] || { echo "buttre binary not found at $BIN (cargo build -p buttre-platform)"; exit 1; }

# --- register the component (user level, no sudo) ---
COMP_DIR="$HOME/.local/share/ibus/component"
mkdir -p "$COMP_DIR"
sed "s|<exec>/usr/bin/buttre --ibus</exec>|<exec>$PWD/$BIN --ibus</exec>|" \
    installers/linux/buttre.xml > "$COMP_DIR/buttre.xml"
rm -f ~/.cache/ibus/bus/registry*   # cache must be invalidated to see XML changes
export IBUS_COMPONENT_PATH=/usr/share/ibus/component:$COMP_DIR

# --- session bus + ibus-daemon ---
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    eval "$(dbus-launch --sh-syntax)"
fi
pkill -f "buttre --ibus" 2>/dev/null || true
ibus-daemon -dr 2>/dev/null
sleep 2
ibus engine buttre >/dev/null 2>&1 || true
echo "Active IBus engine: $(ibus engine 2>/dev/null)"

# --- GTK entry on the host display, input routed through IBus ---
ENTRY=/tmp/buttre-gtk-entry.py
cat > "$ENTRY" <<'EOF'
import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, Pango
win = Gtk.Window(title="buttre IBus test")
win.set_default_size(640, 140)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin=16)
label = Gtk.Label(label="Gõ thử:  vieejt<space> → việt  ·  hoaf. → hòa.  ·  backspace giữa từ")
entry = Gtk.Entry()
entry.override_font(Pango.FontDescription("Sans 16"))
box.pack_start(label, False, False, 0)
box.pack_start(entry, False, False, 0)
win.add(box)
win.connect("destroy", Gtk.main_quit)
win.show_all()
Gtk.main()
EOF

echo "─────────────────────────────────────────────────────────"
echo " Cửa sổ 'buttre IBus test' sẽ mở. Click vào ô nhập và gõ:"
echo "   vieejt<space>  →  việt"
echo " Đóng cửa sổ để kết thúc."
echo "─────────────────────────────────────────────────────────"
GTK_IM_MODULE=ibus python3 "$ENTRY" 2>/dev/null || true

pkill -f "buttre --ibus" 2>/dev/null || true
echo "done."
