# 🐧 buttre Linux — Bộ Gõ Tiếng Việt

**Phiên bản**: 0.9.2-beta
**Framework**: IBus, fcitx5 (addon) và Wayland-native `zwp_input_method_v2` (KDE Plasma/sway/Hyprland) — buttre tự nhận diện (`buttre --doctor`)
**Trạng thái**: ✅ Hoạt động — verify tự động trên CI (ibus-daemon thật + headless sway)

---

## 📦 Tính Năng

- ✅ **Mỗi kiểu gõ một input source** — "buttre Telex", "buttre VNI", "buttre Chữ Nôm" và từng bàn phím TOML, đổi bằng Super+Space như macOS và Windows (IBus; ADR-0005)
- ✅ **Composition thời gian thực** — preedit có gạch chân, dựng dần theo từng phím
- ✅ **Backspace thông minh** — thu gọn từ đang gõ theo raw key
- ✅ **Hỗ trợ Shift/CapsLock** — chữ hoa/thường (XKB xử lý trước khi tới engine)
- ✅ **Dấu câu/space** — commit từ rồi cho ký tự đi qua
- ✅ **Wayland-native** — `zwp_input_method_v2` cho sway/Hyprland/KDE; tự fallback IBus cho GNOME/X11
- ✅ **Ô mật khẩu** — bypass engine (không lọt vào composition/learning)
- ✅ **Chế độ Nôm** — cửa sổ chọn chữ (candidate) qua IBus (GNOME…), KDE Plasma và Wayland-native (sway, Hyprland, labwc, niri, COSMIC — popup `zwp_input_popup_surface_v2`, nét theo scale màn hình): Space/Enter chọn chữ đang sáng, 1–9 chọn theo số, ↑↓/PgUp/PgDn chuyển, Esc huỷ. Compositor không vẽ popup này thì các phím chọn vẫn chạy (Space ra chữ đầu danh sách)

> **Không tap phím toàn cục:** cả hai backend đều để OS/compositor định tuyến phím tới bộ gõ đang chọn — không theo dõi bàn phím toàn hệ thống, nên không bị nghi là keylogger.

---

## 🏗️ Kiến Trúc

```
┌─────────────────────────────┐
│    Ứng Dụng Linux           │
│  (gedit, Firefox, v.v.)     │
└──────────┬──────────────────┘
           │ GTK/Qt Input
           ▼
┌─────────────────────────────┐
│      IBus Daemon            │
│  - Quản lý input method     │
│  - Định tuyến key event     │
└──────────┬──────────────────┘
           │ D-Bus IPC
           ▼
┌─────────────────────────────┐
│  buttre (IBus engine)       │
│  ┌───────────────────────┐  │
│  │  Giao Diện D-Bus      │  │
│  └───────┬───────────────┘  │
│          │                  │
│  ┌───────▼───────────────┐  │
│  │  VietnameseEngine     │  │
│  │  (buttre-engine)      │  │
│  └───────────────────────┘  │
└─────────────────────────────┘
```

---

## 🚀 Cài Đặt

### Gói cài sẵn (.deb / .rpm)

Tải từ [Releases](https://github.com/dxsl-org/buttre/releases):

```bash
sudo apt install ./buttre-*-linux-x86_64.deb       # Debian/Ubuntu/Mint/Pop!_OS
sudo dnf install ./buttre-*-linux-x86_64.rpm       # Fedora/RHEL
sudo zypper install --allow-unsigned-rpm ./buttre-*-linux-x86_64.rpm   # openSUSE
```

Dùng fcitx5 (Kubuntu 24.04 mặc định dùng fcitx5): cài thêm gói addon, cùng lúc với gói chính:

```bash
sudo apt install ./buttre-*-linux-x86_64.deb ./fcitx5-buttre-*-linux-x86_64.deb
```

Cả `.deb` lẫn `.rpm` đều không bắt buộc IBus: máy đã có IBus hoặc fcitx5 thì giữ nguyên framework đó.
`.deb` gợi ý cài IBus khi máy chưa có framework nào; `.rpm` không tự cài IBus (`Suggests`), nên trên
KDE chưa có IBus/fcitx5 (openSUSE) thì chọn "buttre (Wayland IME)" trong Virtual Keyboard.

`ibus-daemon` đang chạy không tự thấy engine mới: **đăng xuất rồi đăng nhập lại** (hoặc
chạy `ibus restart`) trước khi thêm buttre ở bước [Cấu Hình](#️-cấu-hình). Gói không
cài tray trên GNOME/IBus — mỗi kiểu gõ là một input source của hệ thống; chạy `buttre`
(menu ứng dụng) để mở cửa sổ Cấu hình.

### Build từ mã nguồn

Distro không có gói sẵn (Arch, openSUSE, Void, Alpine, NixOS…) hoặc muốn bản mới nhất:

```bash
git clone https://github.com/dxsl-org/buttre
cd buttre
./scripts/install-linux-auto.sh   # chạy bằng user đang đăng nhập, không sudo
```

Gói cần cài trước cho từng distro, cách cài thủ công, gỡ cài đặt:
[docs/linux-build-from-source.md](docs/linux-build-from-source.md). Tự đóng gói `.deb`/`.rpm`:
`./installers/linux/build_packages.sh`.

---

## ⚙️ Cấu Hình

Chọn theo môi trường desktop của bạn.

### GNOME (Ubuntu mặc định) — qua IBus

GNOME **không** dùng `ibus-setup` để thêm engine; nó quản input source qua Settings:

1. **Settings → Keyboard → Input Sources → (+)** → **Vietnamese** → chọn kiểu gõ: **buttre Telex**,
   **buttre VNI**, **buttre Chữ Nôm** hoặc một bàn phím TOML. Thêm bao nhiêu kiểu tuỳ ý.
   - Hoặc qua CLI: thêm `('ibus', 'buttre')` (Telex), `('ibus', 'buttre-vni')`, `('ibus', 'buttre-nom')`
     vào `gsettings get org.gnome.desktop.input-sources sources`.
2. Đổi kiểu gõ / tắt tiếng Việt: **Super+Space** (mặc định của GNOME) — chọn bàn phím thường là tắt.
3. "Cấu hình buttre…" nằm trong menu input source trên thanh trên cùng.

Bản 0.9.2-beta trở về trước chỉ có một input source `buttre` với radio kiểu gõ trong menu; khi nâng cấp,
`buttre` trở thành "buttre Telex", VNI/Nôm cần thêm một lần. Bàn phím TOML thêm sau khi cài chỉ
hiện sau khi IBus dựng lại cache: `ibus write-cache && ibus restart` (hoặc đăng nhập lại).

### Cinnamon (Linux Mint) — qua IBus

Cinnamon quản input source giống GNOME (không dùng `ibus-setup`):

1. **System Settings → Keyboard → Layouts → (+)** → chọn **buttre Telex**, **buttre VNI**,
   **buttre Chữ Nôm** hoặc bàn phím dân tộc. CLI: thêm `('ibus', 'buttre')`, `('ibus', 'buttre-vni')`…
   vào `gsettings get org.cinnamon.desktop.input-sources sources`.
2. Đổi kiểu gõ: **Super+Space**, hoặc applet bàn phím trên panel (hiện TELEX / VNI / NÔM). Chọn
   "English (US)" là tắt.
3. "Cấu hình buttre…" nằm trong menu của applet bàn phím.

`ibus-setup` chỉ dùng trên các desktop không tự quản input source (XFCE, MATE…) hoặc khi chạy IBus độc lập.

### sway / Hyprland / KDE / COSMIC — Wayland-native (không cần IBus)

Trên các compositor hỗ trợ `zwp_input_method_v2`, chạy buttre trực tiếp làm input method — không cần ibus-daemon:

```bash
# Thêm vào config compositor để tự khởi động, ví dụ sway (~/.config/sway/config):
exec /usr/bin/buttre --ime
# Tray (đổi kiểu gõ, bật/tắt) — sway/Hyprland không chạy XDG autostart:
exec /usr/bin/buttre --autostart
```

Hyprland bản mới (đã kiểm 0.57) đọc `~/.config/hypr/hyprland.lua` (có file này thì `hyprland.conf` bị bỏ qua):

```lua
hl.on("hyprland.start", function ()
  hl.exec_cmd("/usr/bin/buttre --ime")
  hl.exec_cmd("/usr/bin/buttre --autostart")
end)
```

Hyprland bản cũ (`hyprland.conf`): `exec-once = /usr/bin/buttre --ime` và
`exec-once = /usr/bin/buttre --autostart`. Tray hiện trên waybar (module `tray`).

niri (`~/.config/niri/config.kdl`) — tray chạy theo autostart của gói nếu niri được mở qua
`niri-session` (systemd chạy XDG autostart); nếu không thì thêm dòng thứ hai. Tray hiện trên bar có
StatusNotifierItem (waybar module `tray`, Noctalia widget `tray`):

```kdl
spawn-at-startup "/usr/bin/buttre" "--ime"
spawn-at-startup "/usr/bin/buttre" "--autostart"
```

Phiên niri mở từ GDM trên Ubuntu 26.04 không đặt biến IM và không chạy ibus-daemon, nên buttre tự
nhận Wayland-native. Nếu `buttre --doctor` báo `env IM=Some(Ibus)` thì chạy `im-config -n none` như
COSMIC (bên dưới). Không có Xwayland (niri thiếu `xwayland-satellite` — Ubuntu chưa đóng gói) thì
tray không bắt phím toàn cục được; phím tắt Ctrl+Shift+… vẫn chạy khi đang gõ trong ô nhập.

`buttre --doctor` in môi trường buttre nhận diện được (IBus, fcitx5, KWin, Wayland-native) và
lý do. Lúc đăng nhập tray chờ tối đa 30 giây nếu phiên được cấu hình IBus/fcitx5 mà daemon chưa lên.

`--ime` tự dò: dùng Wayland-native nếu compositor hỗ trợ, tự fallback sang IBus nếu không (hoặc nếu một IME khác đã giữ seat).
Nếu phiên đang dùng IBus hoặc fcitx5 (vd. Hyprland có `exec-once = fcitx5 -d`), `--ime` tự thoát
để không giành input method với fcitx5 — gõ qua addon fcitx5-buttre, đổi kiểu gõ ở menu fcitx5.

**KDE Plasma:** KWin tự chạy `buttre --ime` theo `~/.config/kwinrc` (System Settings → Keyboard →
Virtual Keyboard → "buttre (Wayland IME)"). "Thoát" trên tray tắt gõ tiếng Việt (`Settings::enabled`
= tắt): từ Plasma 6.7 KWin không còn cách tắt tiến trình IME qua D-Bus nên engine vẫn chạy nền, chỉ
không gõ dấu. Mở lại tray rồi bấm "Bật bộ gõ" (hoặc Ctrl+Shift+Space) để gõ lại.

- Kubuntu (và distro dùng im-config) có cài fcitx5/IBus đặt sẵn `GTK_IM_MODULE`/`QT_IM_MODULE`
  = fcitx/ibus — app GTK/Qt khi đó gõ qua fcitx5/IBus, bỏ qua buttre của KWin. Khi đã chọn buttre ở
  Virtual Keyboard, chạy `im-config -n none` rồi đăng nhập lại.
- Plasma 5.27 (Kubuntu 24.04, Debian 12): phím tắt Ctrl+Shift+… cần System Settings → Applications
  → Legacy X11 App Support → "As above, plus any key typed while the Control, Alt, or Meta keys are
  pressed" (Plasma 6 đã mặc định như vậy).

**COSMIC (Pop!_OS 24.04):** dùng Wayland-native. Pop!_OS đặt sẵn `GTK_IM_MODULE=ibus` nhưng
ibus-daemon không tự chạy trong phiên COSMIC, và app riêng của COSMIC (cosmic-edit…) không nhận
IBus. Bỏ cấu hình IBus của im-config và cho phiên tự chạy `buttre --ime`, rồi đăng nhập lại:

```bash
im-config -n none
mkdir -p ~/.config/autostart
printf '[Desktop Entry]\nType=Application\nName=buttre IME\nTryExec=/usr/bin/buttre\nExec=/usr/bin/buttre --ime\nNoDisplay=true\n' \
  > ~/.config/autostart/buttre-ime.desktop
```

Tray do autostart của gói chạy. Phím tắt Ctrl+Shift+… hoạt động khi đang gõ trong một ô nhập.

### Các chế độ chạy (flags)

| Lệnh | Vai trò |
|------|---------|
| `buttre` | Tray (Wayland-native) — hoặc cửa sổ Cấu hình trên IBus/fcitx5 |
| `buttre --ibus` | Engine IBus, do `ibus-daemon` tự spawn theo component XML (không chạy tay) |
| `buttre --ibus-engines` | In danh sách engine IBus (mỗi kiểu gõ một engine) — `ibus-daemon` đọc khi dựng cache |
| `buttre --ime` | Engine tự-dò backend (Wayland-native → IBus) cho sway/Hyprland/KDE |

Trên IBus, kiểu gõ là input source đang chọn. Trên Wayland-native (không có danh sách input
source của OS), đổi kiểu gõ trong app tray tự áp dụng vào engine đang chạy (qua
`~/.config/buttre/method`) — không cần restart.

### Wayland cho app không-native

App GTK/Qt chạy trên Wayland thường nhận IME tự động. App chạy qua XWayland hoặc không hỗ trợ text-input-v3 cần biến môi trường (đặt trong `~/.profile`) — chỉ khi ibus-daemon có chạy; ở Wayland-native không có IBus, các biến này làm app GTK/Qt bỏ qua buttre:

```bash
export GTK_IM_MODULE=ibus
export QT_IM_MODULE=ibus
export XMODIFIERS=@im=ibus
```

### Kiểm Thử

```bash
# Mở gedit
gedit

# Chuyển sang buttre: Super+Space
# Gõ: hoaf
# Mong đợi: hoà ✨
```

---

## 🧪 Kiểm Thử

### Test Case

**Test 1: Telex Cơ Bản**
```
Gõ: hoaf
Mong đợi: hoà
```

**Test 2: Chữ Hoa**
```
Gõ: Shift+V i e e t
Mong đợi: Việt
```

**Test 3: Backspace**
```
Gõ: hoaf → hoà
Nhấn Backspace
Mong đợi: hoa (đã xóa dấu)
```

**Test 4: Nhiều Từ**
```
Gõ: tieesf vieets
Mong đợi: tiếng việts
```

### Ứng Dụng Đã Kiểm Thử

- ✅ **GTK** (gedit, ô nhập GTK3), **Qt6** (Wayland lẫn X11), **LibreOffice**, **VS Code**
- ✅ **Firefox** (gói distro và Flatpak), **Chromium** 156 (Wayland mặc định, không cần cờ)
- ✅ **Terminal**: foot, kitty
- ⚠️ **App chạy qua XWayland trên Wayland-native** (sway, Hyprland, labwc, COSMIC): Java/Swing
  (JetBrains), Qt ép `QT_QPA_PLATFORM=xcb`, Chromium `--ozone-platform=x11` — gõ ra chữ thô; giao
  thức input-method của Wayland không tới được XWayland. Chạy app ở chế độ Wayland, hoặc dùng IBus/fcitx5.
- ⚠️ **Java/Swing qua IBus** (JetBrains trên GNOME): mất ký tự khi gõ (vd. "đường" → "đng") — lỗi
  XIM của Java, ibus-unikey cũng bị y hệt.

---

## 🐛 Xử Lý Sự Cố

### Vấn Đề: buttre không trong danh sách input method

**Giải pháp**:
```bash
# Khởi động lại IBus
ibus restart

# Kiểm tra component đã đăng ký chưa
ls /usr/share/ibus/component/buttre.xml

# Xem logs
journalctl -f | grep buttre
```

### Vấn Đề: Không hiển thị composition

**Giải pháp**:
```bash
# Kiểm tra IBus đang chạy
ps aux | grep ibus

# Khởi động lại IBus daemon
killall ibus-daemon
ibus-daemon -drx
```

### Vấn Đề: Phím không hoạt động

**Giải pháp**:
```bash
# Kiểm tra engine đang chạy
ps aux | grep '[b]uttre'

# Chạy engine thủ công để debug (đúng flag cho từng backend)
RUST_LOG=debug /usr/bin/buttre --ibus   # IBus (GNOME/X11)
RUST_LOG=debug /usr/bin/buttre --ime    # Wayland-native (sway/Hyprland/KDE)
```

### Vấn Đề: Ctrl+Shift+Space / Ctrl+Shift+1…9 không ăn (Wayland-native)

Nếu bạn đặt **Ctrl+Shift để đổi layout bàn phím** (`grp:ctrl_shift_toggle`, vd. sway
`xkb_options grp:ctrl_shift_toggle`), XKB biến phím Shift thứ hai thành phím đổi layout
nên buttre không còn thấy tổ hợp Ctrl+Shift — và mỗi lần bấm layout bị đổi. Chọn phím
đổi layout khác (`grp:win_space_toggle`, `grp:alt_shift_toggle`, `grp:caps_toggle`…)
hoặc đổi kiểu gõ từ tray. Trên IBus/fcitx5 không có xung đột này (đổi kiểu gõ qua menu
của OS, Super+Space).

---

## 🗑️ Gỡ Cài Đặt

```bash
# Nếu cài qua package
sudo apt remove buttre fcitx5-buttre   # Debian/Ubuntu (fcitx5-buttre nếu có cài)
sudo dnf remove buttre                 # Fedora/RHEL
sudo zypper rm buttre                  # openSUSE

# Thủ công
sudo rm /usr/bin/buttre
sudo rm /usr/share/ibus/component/buttre.xml

# Khởi động lại IBus
ibus restart
```

Gói không chạm được file trong thư mục người dùng — dọn thêm nếu muốn sạch hẳn:

```bash
# KDE Plasma đang chọn buttre ở Virtual Keyboard: bỏ chọn (gõ phím vẫn bình
# thường nếu để nguyên — KWin chỉ không có bộ gõ)
kwriteconfig6 --file kwinrc --group Wayland --key InputMethod --delete
# Autostart, kiểu gõ đang chọn, từ đã học, gõ tắt
rm -f ~/.config/autostart/buttre.desktop ~/.config/autostart/buttre-ime.desktop
rm -rf ~/.config/buttre ~/.local/share/buttre ~/.cache/buttre
```

---

## 📊 So Sánh: Windows vs macOS vs Linux

| Khía Cạnh | Windows | macOS | Linux |
|-----------|---------|-------|-------|
| **Trạng thái** | ✅ Hoạt động (còn 1 lỗi separator, issue #4) | 🚧 Engine+FFI sẵn sàng; IMKit host đang phát triển | ✅ Hoạt động |
| **Framework** | TSF | IMKit (dự kiến) | IBus + Wayland `zwp_input_method_v2` |
| **Ngôn ngữ** | Rust | Rust + Swift/Obj-C | Rust |
| **IPC** | COM | FFI | D-Bus / Wayland |
| **Cài đặt** | Registry | ~/Library/Input Methods | /usr/share/ibus + compositor exec |
| **Composition** | ITfComposition | setMarkedText | UpdatePreeditText / set_preedit_string |

---

## 🔧 Phát Triển

### Cấu Trúc Dự Án

```
crates/buttre-platform/
├── src/shared/
│   └── engine_bridge.rs    # Semantics composition dùng chung mọi backend ⭐
├── src/platforms/linux/
│   ├── mod.rs              # run_engine_auto(): dò backend + fallback
│   ├── ibus.rs             # Adapter D-Bus (IBus.Engine) mỏng qua bridge
│   ├── ibus_bus.rs         # Kết nối private bus + Factory + lifecycle ⭐
│   ├── method_sync.rs      # Sync kiểu gõ tray↔engine (file + watcher)
│   └── wayland/            # Backend zwp_input_method_v2 (grab + xkb + virtual-kb) ⭐
└── Cargo.toml              # Metadata đóng gói deb/rpm

installers/linux/
├── buttre.xml              # Component XML: exec = /usr/bin/buttre --ibus ⭐
├── build_packages.sh       # Builder .deb / .rpm
└── debian/                 # Hook postinst

scripts/
├── test-ibus-scenarios.py  # 7 kịch bản gõ e2e qua InputContext thật
├── ci-ibus-smoke.sh        # CI: ibus-daemon thật + typed scenarios
└── ci-wayland-smoke.sh     # CI: headless sway binding + fallback
```

### Lệnh Build

```bash
# Build
cargo build --release -p buttre-platform

# Test
cargo test -p buttre-platform

# Cài đặt
sudo ./scripts/install-ibus.sh

# Gói distro
./installers/linux/build_packages.sh
```

### Chế Độ Debug

```bash
# Chạy với debug logging
RUST_LOG=debug /usr/bin/buttre

# Monitor D-Bus
dbus-monitor "interface='org.freedesktop.IBus.Engine'"
```

---

## 📚 Chi Tiết Kỹ Thuật

### Giao Diện D-Bus

**Service**: `org.freedesktop.IBus.buttre`
**Object**: `/org/freedesktop/IBus/Engine/buttre`
**Interface**: `org.freedesktop.IBus.Engine`

**Các Method**:
- `ProcessKeyEvent(keyval, keycode, state) → bool`
- `FocusIn()`
- `FocusOut()`
- `Enable()`
- `Disable()`
- `Reset()`
- `SetCursorLocation(x, y, w, h)`

### Bảng Phím

| GDK Keyval | Ký Tự |
|------------|-------|
| 0x0061-0x007a | a-z |
| 0x0041-0x005A | A-Z |
| 0x0020 | Dấu cách |
| 0xFF0D | Enter |
| 0xFF08 | Backspace |

---

## 🎯 Lộ Trình

### Giai Đoạn 1: MVP (Hiện Tại)
- ✅ IBus engine
- ✅ Telex tiếng Việt
- ✅ Hiển thị preedit
- ✅ Script cài đặt

### Giai Đoạn 2: Cải Tiến
- [ ] Chế độ VNI
- [ ] UI cài đặt
- [ ] Hỗ trợ Fcitx5
- [ ] Tối ưu Wayland

### Giai Đoạn 3: Nâng Cao
- [ ] Hỗ trợ Nôm
- [ ] Cửa sổ candidate
- [ ] Tự động hoàn thành
- [ ] Đồng bộ đám mây

---

## 📖 Tài Liệu Tham Khảo

- [IBus Developer Guide](https://github.com/ibus/ibus/wiki/DevGuide)
- [D-Bus Specification](https://dbus.freedesktop.org/doc/dbus-specification.html)
- [zbus Documentation](https://docs.rs/zbus/)
- [ibus-bamboo](https://github.com/BambooEngine/ibus-bamboo) — Tham chiếu

---

## 🤝 Đóng Góp

Chào mừng mọi đóng góp! Vui lòng:

1. Fork repository
2. Tạo feature branch
3. Thực hiện thay đổi
4. Kiểm thử trên nhiều distro
5. Submit pull request

---

## 📝 Giấy Phép

Mozilla Public License 2.0 — Xem file LICENSE

---

**Trạng thái**: ✅ Sẵn sàng build và kiểm thử trên Linux!

*Được viết bằng ❤️ sử dụng Rust + zbus + IBus*
