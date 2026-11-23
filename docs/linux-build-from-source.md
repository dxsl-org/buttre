# Build và cài buttre từ mã nguồn trên Linux

Dành cho distro không có gói `.deb`/`.rpm` của buttre (Arch, CachyOS, Manjaro, openSUSE,
Void, Gentoo, Alpine, NixOS…), cho người dùng fcitx5 trên Fedora, và cho ai muốn chạy bản
mới nhất trên nhánh `main`.

- [1. Cài công cụ build](#1-cài-công-cụ-build)
- [2. Build và cài](#2-build-và-cài)
- [3. Bật bộ gõ](#3-bật-bộ-gõ)
- [4. Cài thủ công](#4-cài-thủ-công)
- [5. Ghi chú theo distro](#5-ghi-chú-theo-distro)
- [6. Cập nhật và gỡ cài đặt](#6-cập-nhật-và-gỡ-cài-đặt)
- [7. Xử lý sự cố](#7-xử-lý-sự-cố)

## 1. Cài công cụ build

buttre cần:

- **Rust 1.90 trở lên.** Nếu rust của distro cũ hơn (Debian 13 có 1.85, Ubuntu 24.04 cũ hơn
  nữa), cài bằng [rustup](https://rustup.rs):
  `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Trình biên dịch C, `pkg-config`.
- Thư viện phát triển: glib, pango, cairo (kèm cairo-gobject), libxkbcommon, libX11, libXtst.

Mỗi lệnh dưới đây đã được kiểm bằng một lần build sạch `cargo build --release -p
buttre-platform` trong container của distro đó (2026-11):

| Distro | Lệnh cài | Rust của distro |
|---|---|---|
| Debian 13, Ubuntu 24.04+, Mint, Pop!_OS | `sudo apt install build-essential pkg-config curl libglib2.0-dev libpango1.0-dev libcairo2-dev libxkbcommon-dev libx11-dev libxtst-dev` | Cũ — dùng rustup |
| Fedora | `sudo dnf install gcc pkgconf-pkg-config cargo glib2-devel pango-devel cairo-devel cairo-gobject-devel libxkbcommon-devel libX11-devel libXtst-devel` | Đủ mới |
| Arch, CachyOS, Manjaro, EndeavourOS | `sudo pacman -S --needed base-devel rust glib2 pango cairo libxkbcommon libx11 libxtst` | Đủ mới |
| openSUSE Tumbleweed | `sudo zypper in gcc pkgconf cargo glib2-devel pango-devel cairo-devel libxkbcommon-devel libX11-devel libXtst-devel` | Đủ mới |
| Void (glibc) | `sudo xbps-install -S base-devel pkg-config cargo glib-devel pango-devel cairo-devel libxkbcommon-devel libX11-devel libXtst-devel` | Đủ mới |
| Alpine (musl) | `sudo apk add build-base pkgconf cargo glib-dev pango-dev cairo-dev libxkbcommon-dev libx11-dev libxtst-dev` | Đủ mới |
| NixOS | `nix-shell -p cargo rustc gcc pkg-config glib pango cairo libxkbcommon xorg.libX11 xorg.libXtst` | Đủ mới |

**Dùng fcitx5?** Addon `fcitx5-buttre` là C++ và cần thêm:

| Distro | Lệnh cài thêm |
|---|---|
| Debian, Ubuntu, Mint | `sudo apt install cmake extra-cmake-modules g++ libfcitx5core-dev` |
| Fedora | `sudo dnf install cmake extra-cmake-modules gcc-c++ fcitx5-devel` |
| Arch, CachyOS | `sudo pacman -S --needed cmake extra-cmake-modules fcitx5` |

Lúc chạy, buttre cần một trong các đường gõ: **IBus**, **fcitx5** (kèm addon), hoặc một
compositor Wayland hỗ trợ input-method (KDE Plasma, sway, Hyprland). Chế độ Chữ Nôm cần một
font có chữ Hán Nôm, ví dụ Noto Sans CJK.

## 2. Build và cài

```bash
git clone https://github.com/dxsl-org/buttre.git
cd buttre
./scripts/install-linux-auto.sh
```

Chạy script **bằng user đang đăng nhập, bên trong phiên đồ hoạ, không thêm `sudo`**: script
cần biến môi trường và D-Bus của phiên để nhận diện, và tự gọi `sudo` khi chép file vào
`/usr`. Script làm lần lượt:

1. Build `buttre` bản release (lần đầu mất vài phút).
2. Hỏi chính binary vừa build: `buttre --detect` in đường gõ của phiên — `ibus`, `fcitx5`,
   `fcitx5-no-addon`, `wayland-native`, `kwin-unconfigured` hoặc `none`. Đây cũng là kết luận
   tray dùng khi chạy thật, nên script không tự đoán.
3. Chép bộ file giống gói `.deb`/`.rpm` vào `/usr` và `/etc/xdg/autostart`.
4. Phiên dùng fcitx5: build và cài addon `fcitx5-buttre`.
5. In bước bật bộ gõ đúng desktop đang dùng.

Muốn xem chi tiết vì sao buttre chọn đường gõ đó: `buttre --doctor`.

## 3. Bật bộ gõ

| Môi trường | Bước bật |
|---|---|
| GNOME | Đăng xuất/đăng nhập lại → Settings → Keyboard → Input Sources → (+) → buttre Telex / VNI / Chữ Nôm |
| Cinnamon | System Settings → Keyboard → Layouts → (+) → buttre |
| XFCE, MATE, LXQt (IBus) | `ibus-setup` → Input Method → Add → buttre |
| KDE Plasma Wayland | System Settings → Keyboard → Virtual Keyboard → "buttre (Wayland IME)" → Apply |
| fcitx5 | Đăng xuất/đăng nhập lại → `fcitx5-configtool` → thêm "Buttre" |
| sway | Thêm vào `~/.config/sway/config`: `exec /usr/bin/buttre --ime` và `exec /usr/bin/buttre --autostart` |
| Hyprland | Thêm vào `~/.config/hypr/hyprland.lua` khối `hl.on("hyprland.start", …)` gọi `hl.exec_cmd` cho hai lệnh trên (bản cũ dùng `hyprland.conf`: `exec-once = …`) — xem [LINUX_README.md](../LINUX_README.md) |

Desktop dùng IBus nhưng IBus chưa chạy (thường gặp trên Void, Gentoo, Alpine): đặt biến môi
trường trong `~/.xprofile` hoặc `/etc/environment` rồi cho `ibus-daemon -drx` tự khởi động
cùng phiên:

```bash
GTK_IM_MODULE=ibus
QT_IM_MODULE=ibus
XMODIFIERS=@im=ibus
```

## 4. Cài thủ công

Khi không dùng được script (không có `sudo`, đóng gói cho distro, cài vào prefix khác):

```bash
cargo build --release -p buttre-platform
```

| File nguồn | Đích |
|---|---|
| `target/release/buttre` | `/usr/bin/buttre` |
| `target/release/buttre_nom.db` | `/usr/share/buttre/buttre_nom.db` |
| `keyboards/*.toml` | `/usr/share/buttre/keyboards/` |
| `installers/linux/buttre.xml` | `/usr/share/ibus/component/buttre.xml` (IBus) |
| `installers/linux/buttre.desktop` | `/usr/share/applications/` |
| `installers/linux/buttre-ime.desktop` | `/usr/share/applications/` (KDE Virtual Keyboard) |
| `installers/linux/buttre-autostart.desktop` | `/etc/xdg/autostart/buttre.desktop` |
| `crates/buttre-platform/icons/vietnamese.png` | `/usr/share/icons/hicolor/128x128/apps/buttre.png` |
| `crates/buttre-platform/icons/{english,telex,vni,nom,custom}.png` | `/usr/share/icons/hicolor/128x128/apps/buttre-<tên>.png` |

`buttre.xml`, `buttre-ime.desktop` và `buttre-autostart.desktop` gọi `/usr/bin/buttre`;
cài vào chỗ khác thì sửa đường dẫn trong ba file này. Sau khi chép: `sudo ibus write-cache
--system` (nếu dùng IBus) và đăng xuất/đăng nhập lại.

Addon fcitx5:

```bash
cargo build --release -p buttre-ffi
cmake -S addons/fcitx5-buttre -B build-fcitx5 -DCMAKE_INSTALL_PREFIX=/usr \
      -DBUTTRE_PACKAGED=ON -DBUTTRE_FFI_LIB="$PWD/target/release/libbuttre_ffi.so"
cmake --build build-fcitx5
sudo cmake --install build-fcitx5
```

buttre tìm dữ liệu Nôm cạnh file chạy, trong `~/.local/share/buttre/` rồi
`/usr/share/buttre/`; tìm bàn phím trong `~/.local/share/buttre/keyboards/` (ưu tiên) và
`/usr/share/buttre/keyboards/`.

## 5. Ghi chú theo distro

| Distro | Trạng thái | Ghi chú |
|---|---|---|
| Debian, Ubuntu, Mint, Fedora | Có gói sẵn | Chỉ cần build từ mã nguồn khi muốn bản mới nhất, hoặc addon fcitx5 trên Fedora |
| Arch, CachyOS | Build và chạy đã kiểm trên máy thật (KDE, sway, Hyprland, fcitx5) | Chưa có gói AUR |
| openSUSE Tumbleweed | Build đã kiểm | Chưa chạy thử trên máy thật |
| Void (glibc) | Build đã kiểm | Không có systemd: log của buttre nằm trong `~/.xsession-errors`. IBus phải tự cấu hình (mục 3) |
| Alpine, Void musl | Build đã kiểm trên Alpine (musl) | Chưa chạy thử trên máy thật |
| NixOS | Build đã kiểm bằng `nix-shell` | Chưa có gói Nix: IBus/fcitx5 trên NixOS chỉ nạp engine khai báo trong cấu hình hệ thống (`i18n.inputMethod`), nên script không cài được vào đó |

## 6. Cập nhật và gỡ cài đặt

Cập nhật: `git pull` rồi chạy lại `./scripts/install-linux-auto.sh`, sau đó đăng xuất/đăng
nhập lại.

Gỡ cài đặt (bản cài bằng script):

```bash
sudo rm -f /usr/bin/buttre /usr/share/ibus/component/buttre.xml \
    /usr/share/applications/buttre.desktop /usr/share/applications/buttre-ime.desktop \
    /etc/xdg/autostart/buttre.desktop /usr/share/icons/hicolor/128x128/apps/buttre*.png
sudo rm -rf /usr/share/buttre
# addon fcitx5 (nếu đã cài)
sudo rm -f /usr/lib*/fcitx5/libbuttre-fcitx5.so /usr/lib/*/fcitx5/libbuttre-fcitx5.so \
    /usr/share/fcitx5/addon/buttre.conf /usr/share/fcitx5/inputmethod/buttre.conf
sudo rm -rf /usr/lib*/buttre /usr/lib/*/buttre
```

Cấu hình người dùng nằm ở `~/.config/buttre/` và `~/.local/share/buttre/`.

## 7. Xử lý sự cố

| Triệu chứng | Nguyên nhân thường gặp |
|---|---|
| `error: rustc X is not supported by the following packages` | Rust của distro cũ hơn 1.90 — cài rustup |
| `The system library 'cairo-gobject' required by crate 'cairo-sys-rs' was not found` | Thiếu gói dev của cairo-gobject (Fedora: `cairo-gobject-devel`) |
| `buttre --detect` in `none` | Phiên chưa chạy IBus/fcitx5 và compositor không hỗ trợ input-method Wayland — xem mục 3 |
| `buttre --detect` in `fcitx5-no-addon` | Phiên dùng fcitx5 nhưng chưa có addon — cài công cụ fcitx5 ở mục 1 rồi chạy lại script |
| Chữ Nôm hiện ô vuông | Thiếu font Hán Nôm |

Khi báo lỗi, đính kèm kết quả `buttre --doctor`.
