# buttre

**Bộ gõ tiếng Việt mã nguồn mở, viết bằng Rust, chạy trên Windows, macOS và Linux.**

[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-brightgreen.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.9.2--beta-blue.svg)](CHANGELOG.md)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#nền-tảng-hỗ-trợ)

buttre gõ Telex, VNI, Chữ Nôm và chữ của nhiều dân tộc thiểu số Việt Nam. Bộ gõ tích hợp
theo cách mỗi hệ điều hành thiết kế cho bộ gõ — Text Services Framework trên Windows,
Input Method Kit trên macOS, IBus/fcitx5/Wayland trên Linux — nên không cần theo dõi bàn
phím toàn hệ thống và hoạt động như một bàn phím có sẵn của máy.

---

## Mục lục

- [Tính năng](#tính-năng)
- [Nền tảng hỗ trợ](#nền-tảng-hỗ-trợ)
- [Cài đặt](#cài-đặt)
- [Sử dụng](#sử-dụng)
- [Build từ mã nguồn](#build-từ-mã-nguồn)
- [Kiến trúc](#kiến-trúc)
- [Tài liệu](#tài-liệu)
- [Đóng góp](#đóng-góp)
- [Hỗ trợ dự án](#hỗ-trợ-dự-án)
- [Giấy phép](#giấy-phép)

---

## Tính năng

**Kiểu gõ**

- **Telex** và **VNI** — mỗi kiểu gõ là một bàn phím / input source riêng của hệ điều hành,
  đổi bằng phím tắt chuẩn của OS (Win+Space, Ctrl+Space trên macOS, Super+Space trên GNOME).
- **Chữ Nôm** — gõ âm Quốc ngữ, chọn chữ Hán Nôm trong cửa sổ ứng viên (Space/Enter chọn chữ
  đang sáng, 1–9 chọn theo số, mũi tên và PgUp/PgDn để chuyển trang).
- **Bàn phím dân tộc** đi kèm: Chăm, Chăm Akhar Thrah, Hmong, Khmer, Mường/Tày/Nùng,
  Tây Nguyên, Thái.
- **Bàn phím tự định nghĩa** bằng file TOML ([`keyboards/`](keyboards/)) — thả file vào thư
  mục bàn phím là có thêm một kiểu gõ.

**Gõ thông minh**

- Kiểm tra theo cấu trúc âm tiết tiếng Việt: từ tiếng Anh như `windows`, `rows` giữ nguyên.
- Đặt dấu thanh theo quy tắc chính tả, sửa lại khi gõ thêm chữ.
- `Ctrl+Shift+Z` đảo từ vừa gõ giữa bản có dấu và bản nguyên phím.
- Gõ tắt (macro) và học từ người dùng hay sửa, cấu hình trong cửa sổ Cấu hình.
- Ô mật khẩu đi thẳng, không qua bộ gõ.

**Riêng tư**

- Xử lý hoàn toàn trên máy, không gửi dữ liệu đi đâu, không telemetry.
- Cần quyền quản trị một lần khi cài để đăng ký bộ gõ; lúc gõ chạy với quyền người dùng.
- Mã nguồn mở toàn bộ theo GPL-3.0.

---

## Nền tảng hỗ trợ

| Hệ điều hành | Cách tích hợp | Gói cài | Hướng dẫn |
|---|---|---|---|
| **Windows** (x86_64) | Text Services Framework — mỗi kiểu gõ là một bàn phím Windows. Bản *Hook* (`buttre --hook`) cho ứng dụng không hỗ trợ TSF | `.msi`, `.zip` (Hook) | [QUICK-START.md](QUICK-START.md) |
| **macOS** (Apple Silicon và Intel) | Input Method Kit — mỗi kiểu gõ là một input source | `.pkg` (đã ký và notarize) | [MACOS_README.md](MACOS_README.md) |
| **Linux** | IBus, fcitx5 (addon), hoặc Wayland-native `zwp_input_method_v2` (KDE Plasma, sway, Hyprland). buttre tự nhận diện môi trường | `.deb`, `.rpm`, mã nguồn | [LINUX_README.md](LINUX_README.md) |

Trên Linux, buttre đã được kiểm trên Ubuntu (GNOME), Linux Mint (Cinnamon, XFCE), Fedora và
CachyOS (KDE Plasma), sway và Hyprland — với cả IBus lẫn fcitx5. Chi tiết:
[docs/linux-coverage-test-matrix.md](docs/linux-coverage-test-matrix.md).

---

## Cài đặt

Tải bản mới nhất ở trang [Releases](https://github.com/dxsl-org/buttre/releases).

### Windows

Chạy `buttre-<phiên bản>-windows-x86_64.msi`. Bước chọn bàn phím đã chọn sẵn Telex, VNI và
Chữ Nôm; cài xong là gõ được. Xem [QUICK-START.md](QUICK-START.md).

### macOS

Bấm đúp `buttre-<phiên bản>-macos.pkg`, chọn bàn phím ở nút **Customize**, rồi thêm input
source trong System Settings → Keyboard → Input Sources. Xem [MACOS_README.md](MACOS_README.md).

### Linux

```bash
sudo apt install ./buttre-*-linux-x86_64.deb     # Debian, Ubuntu, Linux Mint, Pop!_OS
sudo dnf install ./buttre-*-linux-x86_64.rpm     # Fedora
sudo zypper install --allow-unsigned-rpm ./buttre-*-linux-x86_64.rpm   # openSUSE Tumbleweed
```

Đăng xuất rồi đăng nhập lại, sau đó bật buttre theo desktop đang dùng:

| Desktop | Bật bộ gõ |
|---|---|
| GNOME | Settings → Keyboard → Input Sources → (+) → buttre Telex / VNI / Chữ Nôm |
| Cinnamon | System Settings → Keyboard → Layouts → (+) → buttre |
| KDE Plasma (Wayland) | System Settings → Keyboard → Virtual Keyboard → "buttre (Wayland IME)" |
| XFCE, MATE và desktop dùng IBus khác | `ibus-setup` → Input Method → Add → buttre |
| sway, Hyprland | Thêm `buttre --ime` và `buttre --autostart` vào config compositor |
| COSMIC (Pop!_OS) | `im-config -n none` và autostart `buttre --ime` — xem [LINUX_README.md](LINUX_README.md) |
| Dùng fcitx5 | Cài thêm `fcitx5-buttre-*.deb`, rồi thêm "Buttre" trong `fcitx5-configtool` |

Distro khác (Arch, Void, NixOS, Alpine…) và fcitx5 trên Fedora/openSUSE:
[build từ mã nguồn](docs/linux-build-from-source.md). Hướng dẫn đầy đủ và xử lý sự cố:
[LINUX_README.md](LINUX_README.md). Lệnh `buttre --doctor` in môi trường buttre nhận diện được.

---

## Sử dụng

| Gõ | Telex | VNI |
|---|---|---|
| `việt` | `vieejt` | `viet65` |
| `đường` | `dduowngf` | `d9uong72` |
| `hòa` | `hoaf` | `hoa2` |

- **Chọn kiểu gõ:** phím đổi bàn phím của hệ điều hành (Win+Space, Super+Space…). Trên Windows
  và Linux Wayland-native còn có `Ctrl+Shift+1` Telex, `Ctrl+Shift+2` VNI, `Ctrl+Shift+3` Chữ Nôm,
  `Ctrl+Shift+4…9` các bàn phím khác; dưới IBus/fcitx5 thì chọn trong menu bộ gõ của hệ thống.
- **Bật/tắt nhanh:** `` Ctrl+Shift+` `` trên Windows, `Ctrl+Shift+Space` trên Linux
  (Wayland-native).
- **Cấu hình:** mở ứng dụng **buttre** từ menu ứng dụng, hoặc menu chuột phải của icon buttre.

Phím tắt chi tiết theo từng hệ điều hành có trong hướng dẫn của nền tảng đó.

---

## Build từ mã nguồn

Cần [Rust](https://rustup.rs) stable (1.90 trở lên).

```bash
git clone https://github.com/dxsl-org/buttre.git
cd buttre
cargo build --release -p buttre-platform
```

| Nền tảng | Cài bản build | Tài liệu |
|---|---|---|
| Windows | `scripts\build-tsf.ps1 -Install` (PowerShell quyền Administrator) | [QUICK-START.md](QUICK-START.md) |
| macOS | `bash hosts/macos/build_app.sh <version>` | [MACOS_README.md](MACOS_README.md) |
| Linux | `./scripts/install-linux-auto.sh` (chạy bằng user đang đăng nhập) | [docs/linux-build-from-source.md](docs/linux-build-from-source.md) |

Kiểm tra trước khi gửi thay đổi:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features
cargo test --workspace
```

---

## Kiến trúc

Văn bản hiển thị là một hàm thuần của chuỗi phím đã gõ: mỗi lần nhấn phím, engine tính lại
toàn bộ từ đang gõ từ đầu, nên không có trạng thái ẩn nào làm sai kết quả về sau. Chi tiết:
[docs/PIPELINE_ARCHITECTURE.md](docs/PIPELINE_ARCHITECTURE.md).

| Thành phần | Vai trò |
|---|---|
| [`buttre-engine`](crates/buttre-engine) | Pipeline gõ 7 tầng: biến đổi phím, đặt dấu, kiểm tra âm tiết, Nôm |
| [`buttre-core`](crates/buttre-core) | Bảng kiểu gõ, bàn phím TOML, cấu hình, dữ liệu dùng chung |
| [`buttre-platform`](crates/buttre-platform) | Tích hợp hệ điều hành: TSF và Hook (Windows), IMKit (macOS), IBus và Wayland (Linux), tray |
| [`buttre-config`](crates/buttre-config) | Cửa sổ Cấu hình (Slint) |
| [`buttre-ffi`](crates/buttre-ffi) | C ABI cho addon fcitx5 |
| [`buttre-autostart`](crates/buttre-autostart) | Tự khởi động cùng hệ điều hành |
| [`addons/fcitx5-buttre`](addons/fcitx5-buttre) | Addon C++ cho fcitx5 |
| [`hosts/macos`](hosts/macos) | Ứng dụng IMKit cho macOS |

Các quyết định kiến trúc quan trọng được ghi trong [docs/adr/](docs/adr/).

---

## Tài liệu

| Tài liệu | Nội dung |
|---|---|
| [QUICK-START.md](QUICK-START.md) | Cài đặt và sử dụng trên Windows |
| [MACOS_README.md](MACOS_README.md) | Build, cài đặt, sử dụng trên macOS |
| [LINUX_README.md](LINUX_README.md) | Cài đặt, cấu hình theo desktop, xử lý sự cố trên Linux |
| [docs/linux-build-from-source.md](docs/linux-build-from-source.md) | Build và cài từ mã nguồn trên mọi distro Linux |
| [docs/PIPELINE_ARCHITECTURE.md](docs/PIPELINE_ARCHITECTURE.md) | Pipeline xử lý phím |
| [docs/01-architecture.md](docs/01-architecture.md) | Kiến trúc hệ thống |
| [docs/02-coding-guide.md](docs/02-coding-guide.md) | Quy ước code |
| [CHANGELOG.md](CHANGELOG.md) | Lịch sử thay đổi |

---

## Đóng góp

Mọi đóng góp đều được hoan nghênh: báo lỗi, sửa lỗi, tính năng, tài liệu, bàn phím mới.

1. Đọc [CONTRIBUTING.md](CONTRIBUTING.md) và [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
2. Báo lỗi qua [Issues](https://github.com/dxsl-org/buttre/issues) — ghi hệ điều hành,
   kiểu gõ, chuỗi phím đã gõ, kết quả mong đợi và kết quả thực tế. Trên Linux, đính kèm
   kết quả `buttre --doctor`.
3. Pull request cần qua `cargo fmt`, `cargo clippy` và `cargo test`. Thay đổi quy tắc gõ phải
   là thuật toán tổng quát (không hard-code một từ cụ thể) và được kiểm trên mọi kiểu gõ.

---

## Hỗ trợ dự án

buttre miễn phí và sẽ luôn miễn phí. Nếu buttre hữu ích với bạn:

- ⭐ Star repository để nhiều người biết đến buttre hơn.
- Báo lỗi, đóng góp code, tài liệu hoặc bàn phím mới.
- Ủng hộ qua [Ko-fi](https://ko-fi.com/dxsl_org).

**Dịch vụ cho tổ chức** — đào tạo, hỗ trợ triển khai hàng loạt, tuỳ chỉnh (từ điển chuyên
ngành, bàn phím riêng), cam kết hỗ trợ (SLA): **service@dxsl.org** hoặc **dichvu@dxsl.org**.

---

## Giấy phép

buttre là phần mềm tự do, phát hành theo [GNU General Public License v3.0](LICENSE).
Bạn được dùng cho mọi mục đích — cá nhân, doanh nghiệp, cơ quan nhà nước — được nghiên cứu,
sửa đổi và phân phối lại. Khi phân phối, bản phân phối (kể cả bản sửa đổi) phải giữ GPL-3.0
và kèm mã nguồn.

© DXSL Team
