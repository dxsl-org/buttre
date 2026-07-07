#!/bin/bash
# Interactive Wayland IME test (run inside WSL2 Ubuntu with WSLg, or any
# Linux session that can nest sway).
#
# Opens a nested sway window containing a GTK text entry, with
# `buttre --ime` attached as the Wayland-native input method.
# Type `vieejt ` and expect `việt ` (preedit underline while composing).
set -eu
BIN="${BUTTRE_BIN:-target/debug/buttre}"

command -v sway >/dev/null || { echo "sway not installed"; exit 1; }
[ -x "$BIN" ] || { echo "buttre binary not found at $BIN (set BUTTRE_BIN or cargo build -p buttre-platform)"; exit 1; }

# Clean up leftovers from previous runs.
pkill -f "buttre --ime" 2>/dev/null || true
pkill -f buttre-gtk-entry.py 2>/dev/null || true

# GTK text entry as the text-input-v3 client (python3-gi ships with ibus).
ENTRY=/tmp/buttre-gtk-entry.py
cat > "$ENTRY" <<'EOF'
import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, Pango
win = Gtk.Window(title="buttre Wayland test")
win.set_default_size(640, 140)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin=16)
label = Gtk.Label(label="Gõ thử:  vieejt<space> → việt ·  hoaf. → hòa.  ·  viet65 → việt (VNI)")
entry = Gtk.Entry()
entry.modify_font(Pango.FontDescription("Sans 16"))
box.pack_start(label, False, False, 0)
box.pack_start(entry, False, False, 0)
win.add(box)
win.connect("destroy", Gtk.main_quit)
win.show_all()
Gtk.main()
EOF

SWAY_CFG=/tmp/buttre-sway-test.cfg
cat > "$SWAY_CFG" <<EOF
xwayland disable
output * bg #103050 solid_color
exec RUST_LOG=info $PWD/$BIN --ime > /tmp/buttre-ime.log 2>&1
exec GTK_IM_MODULE=wayland python3 $ENTRY
EOF

echo "─────────────────────────────────────────────────────────"
echo " Một cửa sổ sway (nền xanh đậm) sẽ mở, bên trong có ô nhập."
echo " Click vào ô nhập rồi gõ:  vieejt<space>  →  việt"
echo " Đóng cửa sổ sway (hoặc Ctrl+C ở đây) để kết thúc."
echo "─────────────────────────────────────────────────────────"
# WLR_RENDERER=pixman: software rendering — nested wlroots under WSLg often
# creates the window but presents nothing with the GLES/GPU path.
WLR_RENDERER=pixman sway --config "$SWAY_CFG" > /tmp/sway-nested.log 2>&1 || true

pkill -f "buttre --ime" 2>/dev/null || true
pkill -f buttre-gtk-entry.py 2>/dev/null || true
echo "=== engine log (/tmp/buttre-ime.log) ==="
tail -8 /tmp/buttre-ime.log 2>/dev/null || echo "(no engine log)"
