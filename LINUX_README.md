# 🐧 buttre Linux — Bộ Gõ Tiếng Việt

**Phiên bản**: 0.7.0-beta
**Framework**: IBus (GNOME/X11) + Wayland-native `zwp_input_method_v2` (sway/KDE/Hyprland)
**Trạng thái**: ✅ Hoạt động — verify tự động trên CI (ibus-daemon thật + headless sway)

---

## 📦 Tính Năng

- ✅ **Telex + VNI** — chuyển kiểu gõ từ tray áp dụng ngay vào engine đang chạy (không cần restart)
- ✅ **Composition thời gian thực** — preedit có gạch chân, dựng dần theo từng phím
- ✅ **Backspace thông minh** — thu gọn từ đang gõ theo raw key
- ✅ **Hỗ trợ Shift/CapsLock** — chữ hoa/thường (XKB xử lý trước khi tới engine)
- ✅ **Dấu câu/space** — commit từ rồi cho ký tự đi qua
- ✅ **Wayland-native** — `zwp_input_method_v2` cho sway/Hyprland/KDE; tự fallback IBus cho GNOME/X11
- ✅ **Ô mật khẩu** — bypass engine (không lọt vào composition/learning)
- ⏳ **Chế độ Nôm** — chạy được nhưng chưa có cửa sổ candidate qua IBus (đang phát triển)

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

### Yêu Cầu

```bash
# Debian/Ubuntu
sudo apt install ibus libibus-1.0-dev build-essential

# Fedora/RHEL
sudo dnf install ibus ibus-devel gcc

# Arch
sudo pacman -S ibus base-devel

# Rust (nếu chưa cài)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Build & Cài Đặt

```bash
# Clone repository
git clone https://github.com/dxsl-org/buttre
cd buttre

# Build và cài đặt (từ thư mục gốc)
sudo ./scripts/install-ibus.sh

# Hoặc build gói distro (.deb / .rpm)
./installers/linux/build_packages.sh
```

### Build Thủ Công

```bash
# Chỉ build
cargo build --release -p buttre-platform

# Cài đặt thủ công
sudo install -m 755 target/release/buttre /usr/bin/
sudo install -m 644 installers/linux/buttre.xml /usr/share/ibus/component/buttre.xml

# Khởi động lại IBus
ibus restart
```

---

## ⚙️ Cấu Hình

Chọn theo môi trường desktop của bạn.

### GNOME (Ubuntu mặc định) — qua IBus

GNOME **không** dùng `ibus-setup` để thêm engine; nó quản input source qua Settings:

1. **Settings → Keyboard → Input Sources → (+)** → **Vietnamese** → **buttre Vietnamese**.
   - Hoặc qua CLI: `gsettings get org.gnome.desktop.input-sources sources` rồi thêm `('ibus', 'buttre')`.
2. Chuyển bộ gõ: **Super+Space** (mặc định của GNOME).

`ibus-setup` chỉ dùng trên các desktop không-GNOME (XFCE, MATE…) hoặc khi chạy IBus độc lập.

### sway / Hyprland / KDE — Wayland-native (không cần IBus)

Trên các compositor hỗ trợ `zwp_input_method_v2`, chạy buttre trực tiếp làm input method — không cần ibus-daemon:

```bash
# Thêm vào config compositor để tự khởi động, ví dụ sway (~/.config/sway/config):
exec /usr/bin/buttre --ime
```

`--ime` tự dò: dùng Wayland-native nếu compositor hỗ trợ, tự fallback sang IBus nếu không (hoặc nếu một IME khác đã giữ seat).

### Các chế độ chạy (flags)

| Lệnh | Vai trò |
|------|---------|
| `buttre` | App khay hệ thống (tray) — chọn Telex/VNI/Nôm |
| `buttre --ibus` | Engine IBus, do `ibus-daemon` tự spawn theo component XML (không chạy tay) |
| `buttre --ime` | Engine tự-dò backend (Wayland-native → IBus) cho sway/Hyprland/KDE |

Đổi kiểu gõ trong app tray sẽ tự áp dụng vào engine đang chạy (qua `~/.config/buttre/method`) — không cần restart. Bật/tắt tiếng Việt dùng bộ chuyển input source của OS (Super+Space), không phải app tray.

### Wayland cho app không-native

App GTK/Qt chạy trên Wayland thường nhận IME tự động. App chạy qua XWayland hoặc không hỗ trợ text-input-v3 cần biến môi trường (đặt trong `~/.profile`):

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

- ✅ **gedit** — Hỗ trợ đầy đủ
- ✅ **Firefox** — Hỗ trợ đầy đủ
- ✅ **VS Code** — Hỗ trợ đầy đủ
- ✅ **LibreOffice** — Hỗ trợ đầy đủ
- ✅ **Terminal** — Hỗ trợ đầy đủ

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

---

## 🗑️ Gỡ Cài Đặt

```bash
# Nếu cài qua package
sudo apt remove buttre        # Debian/Ubuntu
sudo dnf remove buttre        # Fedora/RHEL

# Thủ công
sudo rm /usr/bin/buttre
sudo rm /usr/share/ibus/component/buttre.xml

# Khởi động lại IBus
ibus restart
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
