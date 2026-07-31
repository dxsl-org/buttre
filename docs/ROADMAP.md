# Lộ Trình Dự Án buttre

> Kế hoạch chiến lược cho sự phát triển buttre trên các nền tảng và tính năng

**Cập nhật lần cuối**: 2026-07-31 (method-sync-tri-surface pha 01-05 đã hoàn thành, chờ merge PR #21 + feat/learning-everywhere)
**Phiên bản**: 0.7.0-beta
**Trạng thái**: Windows Core & Installer Hoàn Thành, Đang Mở Rộng Đa Nền Tảng

---

## Tầm Nhìn

Xây dựng một **bộ gõ tiếng Việt hiện đại, hiệu năng cao, đa nền tảng** với:
- Xử lý phím bấm dưới mili-giây
- Hỗ trợ tất cả nền tảng chính (Windows, macOS, Linux)
- Các phương thức nhập linh hoạt (Telex, VNI, VIQR, Hán Nôm)
- Bảo vệ tuyệt đối quyền riêng tư (zero telemetry)
- Khuyến khích đóng góp cộng đồng qua mã nguồn mở

---

## Trạng Thái Hiện Tại

### ✅ Đã Hoàn Thành (v0.7.0-beta)

**Core Engine**:
- [x] Pipeline xử lý 7 giai đoạn (config-driven, recompute-from-raw)
- [x] Phương thức nhập Telex (hỗ trợ đầy đủ)
- [x] Phương thức nhập VNI (hỗ trợ đầy đủ)
- [x] Quy tắc chính tả tiếng Việt (tuân thủ 100%)
- [x] Vị trí dấu thanh (phong cách Old & New)
- [x] Chế độ tiếng Anh fallback (xử lý undo)
- [x] Gõ linh hoạt (hỗ trợ permutation)
- [x] 600+ integration test
- [x] Tối ưu hiệu năng (xử lý dưới ms)

**Nền Tảng Windows**:
- [x] Cài đặt TSF (Text Services Framework)
- [x] Đăng ký COM DLL (sửa lỗi CLSID)
- [x] Hỗ trợ composition string
- [x] Xử lý key event
- [x] Hướng dẫn kiểm thử thủ công

**Installer Đa Nền Tảng** (v0.7.0-beta):
- [x] Windows MSI qua cargo-wix (perMachine, đăng ký CLSID)
- [x] Linux .deb + .rpm qua cargo-deb & cargo-generate-rpm (tích hợp IBus)
- [x] macOS dylib artifact (phiên bản developer, unsigned)
- [x] Windows hook-only ZIP (exe portable, không cần cài đặt)
- [x] GitHub Actions ma trận release 4 nền tảng (softprops/action-gh-release@v2)
- [x] Sửa CLSID: đăng ký TSF DLL đồng bộ trên các nền tảng

**Hạ Tầng**:
- [x] Cài đặt Cargo workspace
- [x] Kiến trúc multi-crate (engine, core, platform, test)
- [x] Clippy lint & kiểm tra chất lượng code
- [x] Tối ưu release (LTO, tối ưu kích thước)
- [x] Artifact release trên GitHub (Windows MSI, Linux .deb/.rpm, macOS dylib, Windows hook ZIP)

---

## Lộ Trình Theo Phase

### Phase 1: Installer & Ổn Định Windows (Q1–Q2 2026)

**Mục tiêu**: Artifact release đa nền tảng không ký số

**Nhiệm Vụ Đã Hoàn Thành** (v0.7.0-beta):
- [x] Sửa lỗi CLSID mismatch (E6B8A6C0-1234-5678-9ABC-DEF012345678)
- [x] Windows MSI qua cargo-wix (perMachine, đăng ký CLSID + profile)
- [x] Linux .deb + .rpm qua cargo-deb/cargo-generate-rpm (tích hợp IBus)
- [x] macOS dylib artifact (unsigned, chỉ dành developer)
- [x] GitHub Actions ma trận 4 nền tảng (windows/ubuntu/macos, song song)
- [x] Cập nhật CHANGELOG.md với tất cả entries installer

**Event-Sourcing-Completion (Q2 2026)** — ✅ HOÀN THÀNH:
- [x] **Phase 2**: Evidence-based un-latch (temp_english_mode derived, không phải latch một chiều)
- [x] **Phase 3**: Word-boundary repair (compose_closed) — TSF + Hook
- [x] **Phase 4-5**: Coda-k & personal learning store (learning.toml)
- [x] **Phase 6**: Word-toggle hotkey (Ctrl+Shift+Z) + raw backspace_mode
- [x] **Phase 8**: Purity audit (purity_audit.rs, check-purity.ps1) — phân loại ghi chú

**Nhiệm Vụ Còn Lại** (Q2 2026):
- [ ] Sửa lỗi test đã biết
  - [ ] `test_find_best_permutation_thuwowfngf` (xử lý trùng lặp 'w')
  - [ ] `test_telex_settings` / `test_vni_settings` (lỗi ToneStyle mismatch)
- [ ] Kiểm thử thủ công & sửa lỗi
  - [ ] Kiểm thử trong Notepad, Word, VS Code, trình duyệt
  - [ ] Kiểm thử .deb/.rpm trên Ubuntu 22.04+
  - [ ] Sửa edge case phát hiện trong sử dụng thực tế
- [ ] Cập nhật tài liệu
  - [ ] WINDOWS_README.md (cách bỏ qua cảnh báo SmartScreen)
  - [ ] LINUX_README.md (cập nhật cache IBus)
  - [ ] MACOS_README.md (cách bỏ quarantine)
  - [ ] Hướng dẫn sử dụng (tiếng Việt)

**Deliverable**: buttre 0.7.0-beta với installer đa nền tảng; buttre 1.0 cho Windows (Q2 2026)

---

### Phase 2: Cài Đặt macOS (Q2 2026)

**Mục tiêu**: Bộ gõ macOS native

**Kiến Trúc**:
```
┌─────────────────────────────────────┐
│     Ứng Dụng macOS                  │
└────────────┬────────────────────────┘
             │ Text Input
             ▼
┌─────────────────────────────────────┐
│  Text Input Management (TIM)        │
└────────────┬────────────────────────┘
             │ IMKServer Protocol
             ▼
┌─────────────────────────────────────┐
│      buttre.app (Bundle)             │
│  ┌───────────────────────────────┐  │
│  │  IMKServer (Obj-C)            │  │
│  │  ├─ IMKInputController        │  │
│  │  └─ Rust Core (FFI)           │  │
│  │      └─ buttre-engine          │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

**Nhiệm Vụ**:
- [ ] Nghiên cứu framework IMKit
  - [ ] Nghiên cứu tài liệu Apple
  - [ ] Phân tích bộ gõ hiện có (v.d. GoTiengViet)
  - [ ] Xác định chiến lược FFI
- [ ] Tạo crate `buttre-macos`
  - [ ] Bridge Objective-C (dùng crate `objc`)
  - [ ] Cài đặt IMKServer
  - [ ] Wrapper IMKInputController
  - [ ] Xử lý key event
- [ ] Tích hợp với `buttre-engine`
  - [ ] Ánh xạ action (Replace → setMarkedText)
  - [ ] Cửa sổ candidate (cho Hán Nôm)
- [ ] Build & đóng gói
  - [ ] Tạo .app bundle
  - [ ] Ký code (Developer ID)
  - [ ] Notarization cho Gatekeeper
- [ ] Kiểm thử
  - [ ] Kiểm thử trong TextEdit, Notes, Safari, Chrome
  - [ ] Kiểm thử hiệu năng
- [ ] Phân phối
  - [ ] Installer DMG
  - [ ] Homebrew cask (tùy chọn)

**Deliverable**: buttre 1.0 cho macOS

---

### Phase 3: Cài Đặt Linux (Q3 2026)

**Mục tiêu**: Phương thức nhập IBus cho Linux

**Kiến Trúc**:
```
┌─────────────────────────────────────┐
│     Ứng Dụng Linux                  │
└────────────┬────────────────────────┘
             │ GTK/Qt Input Context
             ▼
┌─────────────────────────────────────┐
│     IBus Daemon (ibus-daemon)       │
└────────────┬────────────────────────┘
             │ D-Bus IPC
             ▼
┌─────────────────────────────────────┐
│      buttre IBus Engine              │
│  ┌───────────────────────────────┐  │
│  │   Giao diện D-Bus (Rust)      │  │
│  │   └─ buttre-engine             │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

**Nhiệm Vụ**:
- [ ] Nghiên cứu kiến trúc IBus
  - [ ] Nghiên cứu giao thức IBus
  - [ ] Phân tích ibus-bamboo (tham chiếu Go)
  - [ ] Giao tiếp D-Bus trong Rust (dùng `zbus`)
- [ ] Tạo crate `buttre-linux`
  - [ ] Cài đặt giao diện D-Bus
  - [ ] Xử lý key event process
  - [ ] Quản lý preedit text
  - [ ] Cửa sổ candidate (cho Hán Nôm)
- [ ] Tích hợp
  - [ ] Tích hợp buttre-engine
  - [ ] Ánh xạ action (Replace → update_preedit_text)
- [ ] Build & đóng gói
  - [ ] Biên dịch shared object (.so)
  - [ ] Tạo desktop file
  - [ ] IBus component XML
- [ ] Phân phối
  - [ ] .deb package (Ubuntu/Debian)
  - [ ] .rpm package (Fedora/RHEL)
  - [ ] AUR package (Arch Linux)
  - [ ] Flatpak (tùy chọn)
- [ ] Kiểm thử
  - [ ] Kiểm thử trong gedit, LibreOffice, Firefox
  - [ ] Xác minh hỗ trợ Wayland
  - [ ] Kiểm thử X11 fallback

**Deliverable**: buttre 1.0 cho Linux (IBus)

**Tương lai**: Hỗ trợ Fcitx5 (Phase 3.5)

---

### Phase 4: Hỗ Trợ Hán Nôm (Q4 2026)

**Mục tiêu**: Phương thức nhập Hán Nôm (chữ Nôm) đầy đủ

**Tính Năng**:
- [ ] Nhập dựa trên từ điển
  - [ ] Cơ sở dữ liệu 48.510 ký tự Hán Nôm (từ rime-han-nom-data)
  - [ ] Full-text search SQLite FTS5
  - [ ] Tra cứu theo keyword
  - [ ] Index tối ưu (kích thước tối thiểu)
- [ ] Cửa sổ candidate
  - [ ] Hiện nhiều candidate
  - [ ] Điều hướng bằng phím mũi tên / phím số
  - [ ] Xem trước chi tiết ký tự (nghĩa Nôm, Hán Việt)
- [ ] Chế độ nhập
  - [ ] Nhập theo phiên âm tiếng Việt (v.d. "người" → 𠊛)
  - [ ] Nhập Hán Việt (v.d. "nhân" → 人)
  - [ ] Tìm kiếm keyword (v.d. "người" → 人, 𠊛)
- [ ] Tích hợp pipeline
  - [ ] Giai đoạn 11: Tra Cứu Từ Điển
  - [ ] Giai đoạn 12: Tạo Đầu Ra (candidates)
- [ ] Kiểm thử & tài liệu
  - [ ] Dữ liệu test từ văn bản cổ
  - [ ] Hướng dẫn sử dụng nhập Hán Nôm

**Deliverable**: buttre 1.5 với hỗ trợ Hán Nôm

---

### Phase 5: Tính Năng Nâng Cao (2027)

**Mục tiêu**: Nâng cao trải nghiệm người dùng với các tính năng nâng cao

**Tính Năng Đang Cân Nhắc**:
- [ ] **Tự động hoàn thành**
  - [ ] Dự đoán cấp từ
  - [ ] Gợi ý cấp cụm từ
  - [ ] Học từ điển người dùng
- [ ] **Sửa chính tả**
  - [ ] Fuzzy matching cho lỗi gõ sai
  - [ ] Xếp hạng gợi ý
- [ ] **Tùy chỉnh người dùng**
  - [ ] Phím tắt tùy chỉnh
  - [ ] Quy tắc biến đổi tùy chỉnh
  - [ ] Từ điển tùy chỉnh
- [ ] **Hỗ trợ đa ngôn ngữ** (giao diện)
  - [ ] Giao diện tiếng Anh
  - [ ] Giao diện tiếng Việt
- [ ] **Ngôn ngữ dân tộc thiểu số** (mục tiêu mở rộng)
  - [ ] Chữ Tày-Nùng
  - [ ] Chữ Chăm
  - [ ] Chữ Hmông
- [ ] **Đồng bộ đám mây** (tùy chọn)
  - [ ] Đồng bộ từ điển người dùng trên các thiết bị
  - [ ] Bảo vệ quyền riêng tư (mã hóa)

**Lưu ý**: Các tính năng này **đang được thảo luận**. Cài đặt phụ thuộc vào:
- Nhu cầu cộng đồng
- Thời gian của team
- Khả thi kỹ thuật
- Cân nhắc quyền riêng tư

---

## Ma Trận Ưu Tiên Nền Tảng

| Nền Tảng | Ưu Tiên | Trạng Thái | Mục Tiêu |
|----------|---------|-----------|----------|
| Windows  | Cao     | ✅ Hoàn thành (TSF) | 1.0 (Q1 2026) |
| macOS    | Cao     | Đang lên kế hoạch (IMKit) | 1.0 (Q2 2026) |
| Linux    | Cao     | Đang lên kế hoạch (IBus) | 1.0 (Q3 2026) |
| ChromeOS | Thấp    | Tương lai | TBD |
| Android  | Thấp    | Tương lai | TBD |
| iOS      | Thấp    | Tương lai | TBD |

**Lưu ý**:
- Nền tảng desktop (Windows/macOS/Linux) là **ưu tiên hàng đầu**
- Nền tảng mobile (Android/iOS) yêu cầu kiến trúc khác (bàn phím ảo vs IME)
- ChromeOS có thể tái sử dụng cài đặt Linux (IBus)

---

## Nợ Kỹ Thuật & Tái Cấu Trúc

### Vấn Đề Đã Biết

**Lỗi Test Có Sẵn**:
1. `test_find_best_permutation_thuwowfngf` (stage6_permutation.rs)
   - **Vấn đề**: Xử lý transform mark trùng lặp thêm 'w' thừa
   - **Ưu tiên**: Trung bình (ảnh hưởng edge case)
   - **Cách sửa**: Cải thiện phát hiện trùng lặp trong permutation

2. `test_telex_settings` / `test_vni_settings` (presets.rs)
   - **Vấn đề**: Test expect ToneStyle::New nhưng preset dùng ToneStyle::Old
   - **Ưu tiên**: Thấp (không khớp giữa test và preset)
   - **Cách sửa**: Đồng bộ expectation test với default preset

**Cải Tiến Kiến Trúc** (Tương lai):
- [ ] **Xử lý lỗi**: Thay `anyhow` bằng kiểu lỗi tùy chỉnh trong library code
- [ ] **Logging**: Thay ghi file debug bằng tích hợp `tracing` đúng cách
- [ ] **Cấu hình**: Quản lý config tập trung (file TOML + UI)
- [ ] **Modular**: Tách các component UI độc lập nền tảng

---

## Cộng Đồng & Hệ Sinh Thái

### Chiến Lược Mã Nguồn Mở

**Mục tiêu**:
- Xây dựng **cộng đồng sôi nổi** xung quanh buttre
- Khuyến khích **đóng góp** từ developer và nhà ngôn ngữ học
- Cung cấp **tài liệu** cho contributor
- Duy trì **tiêu chuẩn chất lượng code** cao

**Sáng Kiến Cộng Đồng**:
- [ ] **Hướng Dẫn Đóng Góp** (CONTRIBUTING.md)
  - [ ] Cách build từ source
  - [ ] Cách chạy test
  - [ ] Quy trình review code
  - [ ] Hướng dẫn PR
- [ ] **Template issue**
  - [ ] Template báo lỗi
  - [ ] Template yêu cầu tính năng
  - [ ] Template Q&A
- [ ] **GitHub Discussions**
  - [ ] Thảo luận chung
  - [ ] Đề xuất tính năng
  - [ ] Showcase (dự án người dùng)
- [ ] **Trang tài liệu**
  - [ ] Hướng dẫn người dùng
  - [ ] Hướng dẫn developer
  - [ ] Tài liệu API

### Giấy Phép

**Hiện tại**: GNU General Public License v3.0 (GPL-3.0-only)

**Tại sao GPL-3.0?**
- ✅ **Copyleft cho sửa đổi**: Thay đổi code buttre phải mã nguồn mở
- ✅ **Tương thích với proprietary**: Có thể tích hợp vào ứng dụng proprietary
- ✅ **Copyleft cấp file**: Chỉ cần chia sẻ file đã sửa đổi, không cần toàn bộ dự án
- ✅ **Cấp phép bằng sáng chế**: Bảo vệ trước các yêu cầu bằng sáng chế

**Giấy phép không thay đổi**: Không có kế hoạch thay đổi giấy phép

---

## Tóm Tắt Mốc Thời Gian

| Quý | Trọng Tâm | Deliverable |
|-----|-----------|-------------|
| Q1–Q2 2026 | Installer & Hoàn Thiện Windows | buttre 0.7.0-beta (installer), buttre 1.0 Windows (ổn định) |
| Q2 2026 | Cài Đặt macOS | buttre 1.0 macOS |
| Q3 2026 | Cài Đặt Linux | buttre 1.0 Linux (IBus) |
| Q4 2026 | Hỗ Trợ Hán Nôm | buttre 1.5 (tất cả nền tảng) |
| 2027    | Tính Năng Nâng Cao | buttre 2.0 (tự động hoàn thành, v.v.) |

**Lưu ý**: Mốc thời gian mang tính **định hướng** và phụ thuộc vào:
- Thời gian của core team (đây là **dự án tình yêu**, không phải thương mại)
- Đóng góp cộng đồng
- Độ phức tạp nền tảng
- Mức độ nghiêm trọng của lỗi

**Tính linh hoạt**: Chúng tôi ưu tiên **chất lượng hơn tốc độ**. Phiên bản có thể bị trễ để đảm bảo ổn định.

---

## Cách Đóng Góp

Muốn đóng góp cho buttre? Đây là cách:

1. **Đóng Góp Code**:
   - Kiểm tra issue mở với nhãn `good first issue`
   - Đọc `docs/02-coding-guide.md` để biết tiêu chuẩn code
   - Submit PR kèm test và tài liệu

2. **Kiểm Thử & Phản Hồi**:
   - Thử bản beta và báo lỗi
   - Kiểm thử trên các nền tảng và ứng dụng khác nhau
   - Cung cấp phản hồi UX

3. **Tài Liệu**:
   - Cải thiện hướng dẫn người dùng
   - Viết hướng dẫn
   - Dịch tài liệu

4. **Ngôn Ngữ Học**:
   - Hỗ trợ từ điển Hán Nôm
   - Xác minh quy tắc chính tả tiếng Việt
   - Hỗ trợ chữ viết ngôn ngữ dân tộc thiểu số

**Tham gia**: [GitHub Discussions](https://github.com/dxsl-org/buttre/discussions)

---

## Liên Hệ & Tài Nguyên

- **GitHub**: [https://github.com/dxsl-org/buttre](https://github.com/dxsl-org/buttre)
- **Issues**: [https://github.com/dxsl-org/buttre/issues](https://github.com/dxsl-org/buttre/issues)
- **Discussions**: [https://github.com/dxsl-org/buttre/discussions](https://github.com/dxsl-org/buttre/discussions)
- **Tài liệu**: Thư mục `docs/` trong repository

---

**Cập nhật lần cuối**: 2026-07-03

_Đây là tài liệu sống và sẽ được cập nhật khi dự án phát triển._
