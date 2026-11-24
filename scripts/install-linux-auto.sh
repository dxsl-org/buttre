#!/bin/bash
# buttre Linux — build from source and install for the typing path this
# session actually uses.
#
# The decision is NOT made here: the freshly built binary answers
# `buttre --detect` with the same verdict the tray acts on (backend_detect.rs
# weighs KWin's Virtual Keyboard choice, the IM environment, a shell that
# starts IBus itself, the daemons on the session bus and the compositor's
# input-method protocol). This script only installs what that path needs:
#
#   ibus               files + IBus cache refresh, then add the source
#   fcitx5 / -no-addon files + fcitx5-buttre addon (built here), restart fcitx5
#   wayland-native     files; KWin already runs buttre, or sway/Hyprland
#                      config lines to add
#   kwin-unconfigured  files; pick buttre in Virtual Keyboard (kwinrc is the
#                      user's config — never edited here)
#   none               files; no IBus/fcitx5/Wayland IME — install one first
#
# Run from inside the graphical session as the DESKTOP USER (detection reads
# the session bus and environment); sudo is used only for system files.
# Same file layout as the .deb/.rpm ([package.metadata.deb] in
# crates/buttre-platform/Cargo.toml), so a later package install replaces it.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
BIN="$TARGET_DIR/release/buttre"

if [ "$EUID" -eq 0 ]; then
    echo "❌ Chạy bằng user đang đăng nhập, không dùng sudo — script tự gọi sudo khi cần."
    exit 1
fi

echo "📦 Build buttre..."
cargo build --release -p buttre-platform

SURFACE="$("$BIN" --detect)"
echo "🔎 Đường gõ của phiên này: $SURFACE   (chi tiết: $BIN --doctor)"

install_files() {
    echo "📥 Cài file vào /usr (cần sudo)..."
    sudo install -Dm755 "$BIN" /usr/bin/buttre
    if [ -f "$TARGET_DIR/release/buttre_nom.db" ]; then
        sudo install -Dm644 "$TARGET_DIR/release/buttre_nom.db" /usr/share/buttre/buttre_nom.db
    else
        echo "⚠️  Thiếu $TARGET_DIR/release/buttre_nom.db — chế độ Nôm sẽ không có dữ liệu."
    fi
    sudo install -Dm644 -t /usr/share/buttre/keyboards keyboards/*.toml
    sudo install -Dm644 installers/linux/buttre.xml /usr/share/ibus/component/buttre.xml
    sudo install -Dm644 -t /usr/share/applications \
        installers/linux/buttre.desktop installers/linux/buttre-ime.desktop
    sudo install -Dm644 installers/linux/buttre-autostart.desktop /etc/xdg/autostart/buttre.desktop
    local icons=/usr/share/icons/hicolor/128x128/apps
    sudo install -Dm644 crates/buttre-platform/icons/vietnamese.png "$icons/buttre.png"
    for m in english telex vni nom custom; do
        sudo install -Dm644 "crates/buttre-platform/icons/$m.png" "$icons/buttre-$m.png"
    done
    if command -v gtk-update-icon-cache >/dev/null; then
        sudo gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
    fi
    if command -v ibus >/dev/null; then
        sudo ibus write-cache --system >/dev/null 2>&1 || true
    fi
}

desktop_is() {
    case ":${XDG_CURRENT_DESKTOP:-}:" in
        *":$1:"*) return 0 ;;
    esac
    return 1
}

install_fcitx5_addon() {
    if ! command -v cmake >/dev/null || ! pkg-config --exists Fcitx5Core 2>/dev/null; then
        cat <<'EOF'
❌ Build addon fcitx5-buttre cần cmake, extra-cmake-modules, g++ và header fcitx5:
     Debian/Ubuntu/Mint: sudo apt install cmake extra-cmake-modules g++ libfcitx5core-dev
     Fedora:             sudo dnf install cmake extra-cmake-modules gcc-c++ fcitx5-devel
     Arch/CachyOS:       sudo pacman -S cmake extra-cmake-modules gcc fcitx5
   Cài xong chạy lại script này.
EOF
        exit 1
    fi
    echo "🔧 Build addon fcitx5-buttre..."
    cargo build --release -p buttre-ffi
    local build="$TARGET_DIR/fcitx5-buttre-build"
    cmake -S addons/fcitx5-buttre -B "$build" -DCMAKE_INSTALL_PREFIX=/usr \
        -DBUTTRE_PACKAGED=ON -DBUTTRE_FFI_LIB="$(realpath "$TARGET_DIR/release/libbuttre_ffi.so")" >/dev/null
    cmake --build "$build" -j"$(nproc)"
    sudo cmake --install "$build" >/dev/null
}

# KWin runs buttre: IM env vars (im-config on Kubuntu) would route GTK/Qt
# apps to fcitx5/IBus instead, and Plasma 5 keeps Ctrl+Shift chords away
# from the tray's X11 key grab.
kde_notes() {
    case "${GTK_IM_MODULE:-}${QT_IM_MODULE:-}" in
        *fcitx* | *ibus*)
            echo "   ⚠️  GTK_IM_MODULE/QT_IM_MODULE đang trỏ fcitx/ibus — app GTK/Qt sẽ bỏ qua buttre."
            echo "      Chạy: im-config -n none   (hoặc xoá các biến đó), rồi đăng nhập lại." ;;
    esac
    if plasmashell --version 2>/dev/null | grep -q ' 5\.'; then
        echo "   Plasma 5: để Ctrl+Shift+… chạy, System Settings → Applications → Legacy X11 App Support"
        echo "      → \"As above, plus any key typed while the Control, Alt, or Meta keys are pressed\"."
    fi
}

install_files

case "$SURFACE" in
    ibus)
        if command -v ibus >/dev/null; then
            ibus restart >/dev/null 2>&1 || true
        fi
        echo "✅ Xong (IBus). Thêm bộ gõ:"
        if desktop_is GNOME; then
            echo "   Settings → Keyboard → Input Sources → (+) → Vietnamese → buttre Telex/VNI/Chữ Nôm"
        elif desktop_is X-Cinnamon; then
            echo "   System Settings → Keyboard → Layouts → (+) → buttre Telex/VNI/Chữ Nôm"
        elif desktop_is KDE; then
            echo "   KWin đang dùng IBus: thêm buttre bằng ibus-setup → Input Method,"
            echo "   hoặc chọn \"buttre (Wayland IME)\" trong Virtual Keyboard để KWin chạy buttre trực tiếp."
        else
            echo "   ibus-setup → Input Method → Add → Vietnamese → buttre"
        fi
        echo "   Đổi kiểu gõ bằng Super+Space."
        ;;
    fcitx5 | fcitx5-no-addon)
        install_fcitx5_addon
        echo "✅ Xong (fcitx5). Đăng xuất/đăng nhập lại để fcitx5 nạp addon, rồi mở fcitx5-configtool → thêm"
        echo "   \"Buttre\" (bỏ chọn \"Only Show Current Language\" nếu không thấy)."
        ;;
    wayland-native)
        if desktop_is KDE; then
            echo "✅ Xong. KWin đang chạy buttre (Virtual Keyboard). Đăng xuất/đăng nhập lại để dùng bản vừa cài."
            kde_notes
        elif desktop_is COSMIC; then
            cat <<'EOF'
✅ Đã cài. COSMIC: bỏ cấu hình IBus của im-config, cho phiên tự chạy buttre --ime, rồi đăng nhập lại:
     im-config -n none
     mkdir -p ~/.config/autostart
     printf '[Desktop Entry]\nType=Application\nName=buttre IME\nTryExec=/usr/bin/buttre\nExec=/usr/bin/buttre --ime\nNoDisplay=true\n' \
       > ~/.config/autostart/buttre-ime.desktop
EOF
        else
            cat <<'EOF'
✅ Đã cài. Thêm vào config compositor rồi đăng nhập lại:
     sway (~/.config/sway/config):  exec /usr/bin/buttre --ime
                                    exec /usr/bin/buttre --autostart
     Hyprland mới (~/.config/hypr/hyprland.lua):
         hl.on("hyprland.start", function ()
           hl.exec_cmd("/usr/bin/buttre --ime")
           hl.exec_cmd("/usr/bin/buttre --autostart")
         end)
     Hyprland cũ (hyprland.conf):   exec-once = /usr/bin/buttre --ime
                                    exec-once = /usr/bin/buttre --autostart
EOF
        fi
        ;;
    kwin-unconfigured)
        echo "✅ Đã cài. System Settings → Keyboard → Virtual Keyboard → chọn \"buttre (Wayland IME)\" → Apply."
        kde_notes
        ;;
    none)
        echo "⚠️  Đã cài, nhưng phiên này chưa có IBus/fcitx5 và compositor không hỗ trợ input-method Wayland."
        echo "   Cài IBus (vd: sudo apt install ibus), đăng nhập lại, rồi chạy lại script này."
        exit 1
        ;;
    *)
        echo "❌ buttre --detect trả về \"$SURFACE\" — không biết đường cài này."
        exit 1
        ;;
esac
