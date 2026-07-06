> **⚠️ macOS: gõ CHƯA hoạt động.** Hiện chỉ có engine tiếng Việt (Rust) + lớp FFI. Bộ gõ macOS thật sự — một **IMKit input source** — đang được phát triển, chưa hoàn thành. Không có installer cho người dùng cuối. Artifact release `libbuttre_platform.dylib` chỉ là thư viện dành cho developer muốn tự xây host app. Kế hoạch triển khai: [.agents/260706-1343-cross-platform-input-integration](.agents/260706-1343-cross-platform-input-integration/plan.md).

# 🍎 buttre macOS — Trạng Thái Phát Triển

## 📦 Thông Tin

**Phiên bản engine**: 0.7.0-beta
**Trạng thái macOS**: 🚧 Engine + FFI sẵn sàng · IMKit input source **đang phát triển** (Phase 7) · gõ chưa hoạt động
**Kiến trúc mục tiêu**: IMKit (IMKServer + IMKInputController) + Rust FFI

> Cảnh báo trung thực: các phiên bản README trước ghi "✅ Code Hoàn Chỉnh, Sẵn Sàng Build" và hướng dẫn cài `build/macos/buttre.app`. Điều đó **không đúng** — chưa có bước build nào tạo ra app đó, và backend macOS hiện tại (`platforms/macos/mod.rs`) mới chỉ là stub. Nội dung dưới đây phản ánh đúng những gì tồn tại trong repo.

---

## ✅ Đã có (thật) vs ⏳ Chưa có

| Thành phần | Trạng thái |
|-----------|-----------|
| `buttre-engine` (Telex/VNI/Nôm, Rust) | ✅ Hoạt động, test đầy đủ |
| FFI surface (`platforms/macos/ffi.rs`) | ✅ Export C ABI (`buttre_engine_*`) |
| Universal dylib build (`build_dylib.sh`) | ✅ Tạo `libbuttre_platform.dylib` (arm64 + x86_64) |
| IMKit input source (host app) | ⏳ Đang phát triển — Phase 6 (FFI v2) → Phase 7 (IMKit) |
| Cài đặt cho người dùng cuối | ❌ Chưa có |
| Gõ tiếng Việt trong app macOS | ❌ Chưa hoạt động |

Vì sao đi hướng **IMKit** (không phải CGEventTap): IMKit để hệ điều hành định tuyến phím tới input source đã chọn — **không cần quyền Accessibility**, không theo dõi phím toàn cục, nên không bị macOS/người dùng nghi là keylogger. Đây là chuẩn hợp lệ cho bộ gõ trên macOS.

---

## 🏗️ Kiến Trúc Mục Tiêu (khi Phase 7 hoàn thành)

```
┌─────────────────────────────┐
│    Ứng Dụng macOS           │
│  (TextEdit, Notes, v.v.)    │
└──────────┬──────────────────┘
           │ OS định tuyến phím tới input source đang chọn
    ┌──────▼──────┐
    │  IMKServer  │
    └──────┬──────┘
    ┌──────▼──────┐
    │IMKInputCtrl │  setMarkedText / insertText
    └──────┬──────┘
           │ FFI (ButtreKeyResult)
    ┌──────▼──────┐
    │buttre-engine│  (Rust)
    └─────────────┘
```

---

## 🚀 Build thư viện developer (dylib)

Đây là phần **thật sự chạy được** hôm nay — tạo ra dylib để tích hợp vào host app của riêng bạn.

### Yêu Cầu

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

### Build

```bash
# Dylib cho kiến trúc host (dev nhanh)
cargo build -p buttre-platform --release
# → target/release/libbuttre_platform.dylib

# Universal release ZIP (arm64 + x86_64)
./installers/macos/build_dylib.sh <version>
# → target/macos/buttre-<version>-macos-universal.zip
```

Hướng dẫn tích hợp dylib vào host app: xem [installers/macos/ARTIFACT_README.md](installers/macos/ARTIFACT_README.md).

---

## 🔍 FFI Surface (hiện tại)

Định nghĩa trong `crates/buttre-platform/src/platforms/macos/ffi.rs`:

```c
uint64_t     buttre_engine_new(void);
void         buttre_engine_free(uint64_t engine_id);
const char*  buttre_engine_process_key(uint64_t engine_id, uint16_t keycode, bool shift, bool capslock);
const char*  buttre_engine_process_backspace(uint64_t engine_id);
void         buttre_engine_reset(uint64_t engine_id);
bool         buttre_engine_set_method(uint64_t engine_id, uint8_t method); // 0=telex 1=vni 2=nom
void         buttre_engine_set_enabled(uint64_t engine_id, bool enabled);
```

> Lưu ý: Phase 6 sẽ thay contract trả về bằng struct `ButtreKeyResult { kind, backspace_count, text }` để host phân biệt được composition (preedit) vs commit. Bảng này sẽ cập nhật khi Phase 6 xong.

---

## 📁 Cấu Trúc Liên Quan

```
crates/buttre-platform/src/platforms/macos/
├── mod.rs   # backend macOS — hiện là stub (chưa nhận keystroke)
└── ffi.rs   # C ABI export cho host app tương lai

installers/macos/
├── build_dylib.sh      # universal dylib release ZIP
└── ARTIFACT_README.md  # hướng dẫn tích hợp dylib
```

---

## 🐛 Xử Lý Sự Cố (cho phần dylib)

**Build thất bại**: kiểm tra Xcode CLT (`xcode-select -p`), Rust toolchain (`rustup show`), target (`rustup target list --installed`).

**Gatekeeper / quarantine** khi tải dylib từ web:
```bash
xattr -dr com.apple.quarantine libbuttre_platform.dylib
```

---

## 📚 Tham Khảo (cho việc xây IMKit input source — Phase 7)

- [Input Method Kit — IMKInputController](https://developer.apple.com/documentation/inputmethodkit/imkinputcontroller)
- Reference implementations (xem `research/imkit-references.md` trong thư mục plan): marixdev/vnkey (GPL-3.0, Rust-FFI→ObjC), xmannv/xkey (MIT, Swift), vChewing/IMKSwift (MIT, Swift 6)

---

*buttre — Rust + (sắp có) Swift/Objective-C IMKit*
