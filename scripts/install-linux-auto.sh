#!/bin/bash
# buttre Linux — auto backend picker (plan: fcitx-backend-auto-priority P1).
#
# Detects the machine's IME stack and routes to the right install path with
# the SAME fixed priority the app itself uses (backend_detect.rs):
#
#   fcitx5  →  no addon yet (Phase 3): print guidance, fall through
#   ibus    →  scripts/install-ibus.sh (component XML + engine)
#   wayland →  KDE Plasma: point kwinrc [Wayland] InputMethod at buttre-ime
#              (manual steps printed — kwinrc is user config, never edited
#              by a root installer)
#
# One machine must serve buttre through ONE path: two live registrations
# would both write ~/.config/buttre/{method,enabled} and fight each other.
#
# Run as the DESKTOP USER first (detection needs the session bus); it
# re-invokes sudo only where installation actually requires root.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

has_name() {
    busctl --user list 2>/dev/null | grep -q "^$1 " ||
        busctl --user status "$1" >/dev/null 2>&1
}

FCITX5=no
IBUS=no
WAYLAND=no
has_name org.fcitx.Fcitx5 && FCITX5=yes
if has_name org.freedesktop.IBus || has_name org.freedesktop.portal.IBus || pgrep -x ibus-daemon >/dev/null; then
    IBUS=yes
fi
[ -n "$WAYLAND_DISPLAY" ] && WAYLAND=yes

echo "🔎 probes: fcitx5=$FCITX5 ibus=$IBUS wayland=$WAYLAND"

if [ "$FCITX5" = yes ]; then
    cat <<'EOF'
⚠️  fcitx5 đang chạy. buttre CHƯA có addon fcitx5 (đang trong kế hoạch —
    xem plan-fcitx-backend-auto-priority.md). Hai lựa chọn:
      1. Tắt fcitx5 rồi chạy lại script này (sẽ dùng ibus hoặc wayland), hoặc
      2. Giữ fcitx5 và KHÔNG cài buttre song song (tránh tranh nguồn gõ).
EOF
    exit 1
fi

if [ "$IBUS" = yes ]; then
    echo "➡️  Dùng đường IBus: scripts/install-ibus.sh (cần sudo)"
    exec sudo "$SCRIPT_DIR/install-ibus.sh"
fi

if [ "$WAYLAND" = yes ]; then
    cat <<EOF
➡️  Không có daemon IME nào chạy — dùng đường Wayland (compositor quản lý).
    Trên KDE Plasma:
      1. sudo install -m755 <binary> /usr/bin/buttre
      2. Cài desktop file buttre-ime (xem installers/linux/README.md, mục KWin)
      3. System Settings → Virtual Keyboard → chọn "buttre", hoặc thêm vào
         ~/.config/kwinrc:  [Wayland]\nInputMethod=<đường dẫn buttre-ime.desktop>
      4. Kiểm tra: buttre --doctor
EOF
    exit 0
fi

echo "❌ Không phát hiện fcitx5/ibus/wayland — session X11 không có daemon IME?"
echo "   Cài ibus trước (vd: sudo apt install ibus) rồi chạy lại."
exit 1
