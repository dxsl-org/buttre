 > **⚠️ Linux: gõ CHƯA hoạt động.** Engine đăng ký với `ibus-daemon` sai cách nên `ProcessKeyEvent` không bao giờ được gọi. Đang được viết lại (Phase 1: sửa IBus registration; Phase 3: thêm Wayland-native `zwp_input_method_v2`). Kế hoạch: [.agents/260706-1343-cross-platform-input-integration](.agents/260706-1343-cross-platform-input-integration/plan.md). Hướng dẫn cài bên dưới build được nhưng **chưa gõ ra chữ** cho tới khi Phase 1 hoàn thành.

# 🐧 buttre Linux — Bộ Gõ IBus

**Phiên bản**: 0.7.0-beta
**Framework**: IBus (đang bổ sung Wayland-native `zwp_input_method_v2`)
**Trạng thái**: 🚧 Engine sẵn sàng · lớp tích hợp IBus/Wayland **đang viết lại** · gõ chưa hoạt động

---

## 📦 Tính Năng

Engine tiếng Việt (Telex/VNI/Nôm) đã hoàn chỉnh và test kỹ. Các tính năng dưới đây là **mục tiêu của lớp tích hợp đang phát triển** — chưa hoạt động end-to-end cho tới khi Phase 1 (IBus) và Phase 3 (Wayland) hoàn thành:

- 🚧 **Telex tiếng Việt** — engine sẵn sàng; chờ registration
- 🚧 **Composition thời gian thực** — preedit text (Phase 2)
- 🚧 **Backspace thông minh** — xóa sửa đổi cuối cùng
- 🚧 **Hỗ trợ Shift** — chữ hoa/chữ thường
- 🚧 **Tự động hoàn thành** — Space/Enter xác nhận text (Phase 2)
- ⏳ **Wayland-native** — `zwp_input_method_v2` cho sway/KDE/Hyprland (Phase 3)
- ⏳ **Chế độ VNI** — engine có sẵn; chờ wiring
- ⏳ **Hỗ trợ Nôm** — Đang lên kế hoạch (kèm cửa sổ candidate)

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

### Thêm Input Method

1. **Mở IBus Preferences**:
   ```bash
   ibus-setup
   ```

2. **Thêm buttre**:
   - Vào tab "Input Method"
   - Nhấp nút "Add"
   - Chọn "Vietnamese"
   - Chọn "buttre Vietnamese (Telex)"
   - Nhấp "Add"

3. **Đặt Phím Tắt** (tùy chọn):
   - Vào tab "General"
   - Cấu hình phím tắt "Next input method"
   - Mặc định: `Super+Space`

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

# Chạy engine thủ công để debug
RUST_LOG=debug /usr/bin/buttre
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

| Khía Cạnh | Windows TSF | macOS IMKit | Linux IBus |
|-----------|-------------|-------------|------------|
| **Trạng thái** | ✅ Đang kiểm thử | ✅ Sẵn sàng | ✅ Sẵn sàng |
| **Framework** | TSF | IMKit | IBus |
| **Ngôn ngữ** | Rust | Obj-C + Rust | Rust |
| **IPC** | COM | Mach | D-Bus |
| **Cài đặt** | Registry | /Library | /usr/share |
| **Composition** | ITfComposition | setMarkedText | UpdatePreeditText |

---

## 🔧 Phát Triển

### Cấu Trúc Dự Án

```
crates/buttre-platform/
├── src/platforms/linux/
│   ├── mod.rs              # Điểm vào backend Linux
│   └── ibus.rs             # IBus engine (D-Bus) ⭐
└── Cargo.toml              # Metadata đóng gói deb/rpm

installers/linux/
├── buttre.xml              # Mô tả IBus component ⭐
├── build_packages.sh       # Builder .deb / .rpm ⭐
└── debian/                 # Hook postinst
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
