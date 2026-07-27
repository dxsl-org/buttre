# Bối Cảnh Hệ Thống & Quy Tắc Thiết Kế buttre
**Cập nhật lần cuối**: 2026-07-03 (event-sourcing-completion: un-latch, boundary repair, learning, controls)
**Đối tượng**: Developer & AI Agent

---

## Bắt Đầu Nhanh Cho AI Agent

**Trước khi hỗ trợ bất kỳ nhiệm vụ nào, đọc theo thứ tự sau:**

| File | Mô tả |
| --- | --- |
| `01-architecture.md` | Kiến trúc hệ thống |
| `02-coding-guide.md` | Tiêu chuẩn code và các pattern |
| `ROADMAP.md` | Lộ trình dự án và trạng thái hiện tại |

---

## Metadata Dự Án

```yaml
name: buttre
tagline: Modern Vietnamese Input Method Engine
type: cross_platform_input_method
version: 0.7.0-beta
status: beta

platforms:
  - windows (TSF) - ✅ Đã hoàn thành
  - macos (IMKit) - 🚧 Đang lên kế hoạch (Q2 2026)
  - linux (IBus) - 🚧 Đang lên kế hoạch (Q3 2026)

primary_language: rust
rust_version: 1.70+
license: GPL-3.0-only

repository: https://github.com/dxsl-org/buttre
documentation: docs/
```

---

## Tech Stack

### Công Nghệ Cốt Lõi

```yaml
languages:
  rust:
    version: 1.70+
    edition: 2021
    purpose: Core engine, platform backend

databases:
  sqlite:
    purpose: Từ điển Hán Nôm (Phase 4)
    size: ~48.510 entries
    optimization: FTS5 full-text search

frameworks:
  windows-rs:
    version: 0.62.2
    purpose: Windows TSF COM binding

  future_frameworks:
    cocoa: macOS Objective-C binding (đang lên kế hoạch)
    x11: Linux X11 binding (đang lên kế hoạch)
```

### Dependencies

**Workspace Dependencies** (từ `Cargo.toml`):
```yaml
core:
  - unicode-normalization: 0.1 (chuyển đổi NFC/NFD)
  - lazy_static: 1.4 (tra cứu tĩnh)
  - thiserror: 1.0 (lỗi library)
  - anyhow: 1.0 (lỗi application)

serialization:
  - serde: 1.0 (serialization config)
  - serde_json: 1.0 (xử lý JSON)
  - toml: 0.8 (config TOML)

utilities:
  - dirs: 5.0 (thư mục platform)
  - log: 0.4 (logging facade)
  - tracing: 0.1 (structured logging)
  - tracing-subscriber: 0.3 (tracing backend)

platform_specific:
  - windows: 0.62 (Windows API)
  - cocoa: 0.25 (macOS - tương lai)
  - x11: 2.21 (Linux - tương lai)
```

---

## Cấu Trúc Crate

buttre dùng kiến trúc **multi-crate workspace**:

```yaml
buttre-engine:
  purpose: Pipeline xử lý (7 giai đoạn, config-driven, recompute-from-raw)
  location: crates/buttre-engine/
  responsibility: Biến đổi nhập liệu tiếng Việt, dấu thanh, logic undo
  status: ✅ Hoàn thành (600+ test đang pass)
  public_api:
    - PipelineExecutor
    - PipelineConfig
    - telex_config(), vni_config(), viqr_config()

buttre-core:
  purpose: Giao diện bàn phím độc lập nền tảng
  location: crates/buttre-core/
  responsibility: Struct Keyboard, kiểu Action, chọn phương thức nhập
  status: ✅ Hoàn thành
  public_api:
    - Keyboard::new(InputMethod)
    - Keyboard::process(char) -> Vec<Action>
    - Action enum (DoNothing, Commit, Replace, ...)

buttre-platform:
  purpose: Backend đặc thù từng nền tảng
  location: crates/buttre-platform/
  responsibility: Windows TSF, macOS IMKit, Linux IBus
  status: ✅ Windows TSF hoàn thành, các nền tảng khác đang lên kế hoạch
  components:
    - platforms/windows/tsf/ (cài đặt TSF)
    - platforms/macos/ (đang lên kế hoạch)
    - platforms/linux/ (đang lên kế hoạch)

buttre-test:
  purpose: Tiện ích kiểm thử
  location: crates/buttre-test/
  responsibility: Kiểm thử hàng loạt, benchmark, dữ liệu test
  status: ✅ Hoàn thành
```

**Crate Cũ/Legacy** (được đề cập trong tài liệu cũ, nay đã hợp nhất):
- `buttre-app` → Đã gộp vào `buttre-platform`
- `buttre-hotkey` → Đã gộp vào `buttre-platform`
- `buttre-vietnamese` → Nay là `buttre-engine` (pipeline toàn diện)
- `buttre-hannom` → Kế hoạch Phase 4 (Q4 2026)
- `buttre-windows`, `buttre-windows-hook`, `buttre-windows-common`, `buttre-windows-tsf` → Đã hợp nhất vào `buttre-platform/platforms/windows/`

---

## Cấu Trúc Thư Mục

```
buttre/
├── crates/                    # Rust workspace
│   ├── buttre-engine/         # Pipeline xử lý 7 giai đoạn
│   ├── buttre-core/           # Giao diện độc lập nền tảng
│   ├── buttre-platform/       # Backend nền tảng
│   └── buttre-test/           # Tiện ích kiểm thử
│
├── docs/                      # Tài liệu dự án
│   ├── README.md             # Điều hướng tài liệu
│   ├── 00-context.md        # File này
│   ├── 01-architecture.md    # Kiến trúc hệ thống
│   ├── 02-coding-guide.md    # Tiêu chuẩn code
│   ├── ROADMAP.md            # Lộ trình dự án
│   ├── PIPELINE_ARCHITECTURE.md
│   ├── VIETNAMESE_ACCENT.md
│   ├── MANUAL_TESTING_GUIDE.md
│   ├── FFI_SAFETY_GUIDE.md
│   └── journals/             # Nhật ký phát triển
│
├── .agents/                   # Tài liệu AI agent
│   └── (tài liệu lên kế hoạch, báo cáo, phân chia theo phase)
│
├── .reference/                # Cài đặt tham chiếu
│   ├── unikey/               # Unikey (tham chiếu C++)
│   ├── openkey/              # Tham chiếu OpenKey
│   ├── ibus-bamboo/          # IBus Bamboo (tham chiếu Go)
│   └── weasel/               # Tham chiếu Weasel Hán Nôm
│
├── CLAUDE.md                  # Cấu hình AI agent
├── README.md                  # Tổng quan dự án
├── Cargo.toml                 # Cấu hình workspace
├── LICENSE                    # Giấy phép GPL-3.0
└── CODE_OF_CONDUCT.md        # Quy tắc ứng xử
```

**Lưu ý**: Thư mục `.agents/` dùng cho lên kế hoạch và báo cáo; tất cả tài liệu chính thức đều nằm trong `docs/`.

---

## Quy Tắc Chất Lượng Code

### Quy Tắc Bắt Buộc (PHẢI Tuân Theo)

```yaml
error_handling:
  KHÔNG_ĐƯỢC_dùng:
    - unwrap() (trên Option/Result — gây panic!)
    - expect() (trong library code — chỉ dùng trong main/test)
    - panic!() (dùng Result/Option thay thế)
    - todo!() (trong code đã commit)
    - unimplemented!() (trong code đã commit)

  PHẢI_dùng:
    - Result<T, E> cho các thao tác có thể thất bại
    - Option<T> cho các giá trị tùy chọn
    - Toán tử ? để truyền lỗi
    - anyhow::Context cho error chain có thể đọc được
    - thiserror cho lỗi đặc thù domain

unsafe_code:
  rules:
    - buttre-engine: PHẢI là Rust an toàn 100% (không unsafe)
    - buttre-core: PHẢI là Rust an toàn 100% (không unsafe)
    - buttre-platform: Chỉ unsafe cho FFI, giảm thiểu phạm vi
    - PHẢI ghi lại safety invariant bằng comment // SAFETY:

type_safety:
  - Dùng newtype cho khái niệm domain (UserId, không phải u64)
  - Dùng enum thay vì string cho các kiểu
  - Tránh stringly-typed code
  - Ưu tiên type annotation tường minh cho biểu thức phức tạp

code_organization:
  - Tuân theo các pattern hiện có (xem docs/02-coding-guide.md)
  - Tên tự mô tả (không viết tắt)
  - Thiết kế module (single responsibility)
  - Test logic quan trọng (unit + integration test)
```

### Yêu Cầu Kiểm Thử

```yaml
bắt_buộc:
  - unit_tests: Mọi hàm public
  - integration_tests: Các luồng chính (xem buttre-engine/tests/)
  - edge_cases: Rỗng, tối đa, Unicode, ký tự đặc biệt
  - error_paths: Test tất cả điều kiện lỗi

đặt_tên_test: "test_<function>_<scenario>_<expected>"

ví_dụ:
  tốt:
    - test_process_key_valid_input_returns_action
    - test_apply_tone_empty_buffer_returns_none
  xấu:
    - test_1
    - it_works
```

---

## Quy Trình Làm Việc Cho AI Agent

### Chu Kỳ Phát Triển

Khi hỗ trợ phát triển, tuân theo chu kỳ này:

```yaml
phases:
  1. Research:
      - Đọc tài liệu liên quan
      - Hiểu các pattern hiện có
      - Kiểm tra cài đặt tham chiếu (.reference/)

  2. Analyze:
      - Xác định các component bị ảnh hưởng
      - Review code hiện tại
      - Hiểu dependencies

  3. Plan:
      - Thiết kế hướng tiếp cận giải pháp
      - Cân nhắc edge case
      - Lên kế hoạch test

  4. Code:
      - Viết cài đặt theo coding guide
      - Thêm test toàn diện
      - Tài liệu hóa public API

  5. Verify:
      - Chạy cargo check
      - Chạy cargo test
      - Chạy cargo clippy
      - Xác minh tất cả test pass

  6. Document:
      - Cập nhật tài liệu liên quan
      - Thêm comment inline cho logic phức tạp
      - Cập nhật CHANGELOG.md nếu cần

execution: sequential_one_at_a_time
retry_policy: auto_retry_until_exit_conditions
```

### Ràng Buộc

```yaml
KHÔNG_BAO_GIỜ:
  - Bỏ qua bước trong chu kỳ phát triển
  - Viết code viết tắt/không đầy đủ
  - Để công việc làm nửa chừng
  - Dùng unsafe code không có lý do + tài liệu
  - Giả định hành vi mà không test

LUÔN_LUÔN:
  - Hiểu cấu trúc dự án trước (đọc docs/)
  - Xác nhận trước khi thực hiện thay đổi lớn
  - Hoàn thành toàn bộ phạm vi nhiệm vụ được giao
  - Theo dõi tiến độ (dùng TodoWrite tool)
  - Hỏi người dùng để tiếp tục nếu nhiệm vụ lớn
  - Tối ưu token usage (tập trung, súc tích)

QUAN_TRỌNG:
  - Dùng semantic versioning (x.y.z)
  - Cập nhật CHANGELOG.md với thay đổi
  - Commit trước khi thay đổi lớn (backup)
  - Ưu tiên thay đổi nhỏ, tăng dần
  - Tái sử dụng code hiện có, tránh trùng lặp
  - Tài liệu hóa breaking change + cập nhật caller
  - Tóm tắt tiến độ nếu task thất bại giữa chừng
```

---

## Trọng Tâm Hiện Tại

**Phase Đang Hoạt Động**: Ổn Định & Hoàn Thiện Windows (Q1 2026)

```yaml
active_crate: buttre-engine
current_status:
  - Core engine: ✅ Hoàn thành (pipeline 7 giai đoạn, recompute-from-raw)
  - Windows TSF: ✅ Đã cài đặt
  - Tests: ✅ 600+ đang pass (3 lỗi đã biết)
  - Documentation: ✅ Toàn diện (docs/ đã tổ chức lại)

next_steps:
  immediate:
    - Kiểm thử thủ công trong Notepad, Word, trình duyệt
    - Sửa 3 lỗi test có sẵn
    - Build lại TSF DLL để kiểm thử

  short_term:
    - Cải thiện installer (cài im lặng, nâng cấp)
    - Hướng dẫn sử dụng (tiếng Việt)
    - Video hướng dẫn

  medium_term:
    - buttre 1.0 Windows ổn định
    - Bắt đầu cài đặt macOS (Q2 2026)
```

### Vấn Đề Đã Biết

**Lỗi Test Có Sẵn** (3 test):

1. **test_find_best_permutation_thuwowfngf**
   - File: `crates/buttre-engine/src/pipeline/stages/stage6_permutation.rs`
   - Nguyên nhân: Xử lý transform mark trùng lặp thêm 'w' thừa
   - Ưu tiên: Trung bình (ảnh hưởng edge case)
   - Cách sửa: Cải thiện phát hiện trùng lặp trong permutation

2. **test_telex_settings**
   - File: `crates/buttre-engine/src/pipeline/presets.rs`
   - Nguyên nhân: Test expect ToneStyle::New nhưng preset dùng ToneStyle::Old
   - Ưu tiên: Thấp (không khớp giữa test và preset)
   - Cách sửa: Đồng bộ expectation test với default preset

3. **test_vni_settings**
   - File: `crates/buttre-engine/src/pipeline/presets.rs`
   - Nguyên nhân: Giống test_telex_settings
   - Ưu tiên: Thấp
   - Cách sửa: Đồng bộ expectation test với default preset

---

## Quy Tắc Nhập Liệu Tiếng Việt

**Kiến thức domain quan trọng khi làm việc trên engine**:

### Quy Tắc Vị Trí Dấu Thanh

```yaml
thứ_tự_ưu_tiên:
  1. nguyên_âm_đặc_biệt:
      ký_tự: [ă, â, ê, ô, ơ, ư]
      quy_tắc: Luôn nhận dấu
      ví_dụ: "tuấn" (dấu trên â)

  2. ba_nguyên_âm:
      quy_tắc: Dấu trên nguyên âm giữa
      ví_dụ: "uoi → uòi"

  3. hai_nguyên_âm_khép:
      quy_tắc: Dấu trên nguyên âm thứ 2 khi có phụ âm cuối
      ví_dụ: "toán" (dấu trên á, phụ âm cuối n)

  4. hai_nguyên_âm_mở:
      ia_ua_ưa: Dấu trên nguyên âm thứ 1
      oa_oe_uy: Phụ thuộc ToneStyle (Old=1, New=2)
      còn_lại: Dấu trên nguyên âm thứ 1

  5. một_nguyên_âm:
      quy_tắc: Dấu trên nguyên âm đó
      ví_dụ: "á"

tone_styles:
  old: "óa, úy (truyền thống, mặc định)"
  new: "oá, uý (hiện đại)"
```

### Tự Động Sửa

```yaml
uo_to_ươ:
  kích_hoạt: Đặt dấu trên âm tiết chứa 'uo'
  kết_quả: "'uo' → 'ươ' trước khi đặt dấu"
  ví_dụ: "nguoif → người (không phải nguòi)"
  config: auto_correct_uo (mặc định: false)
```

### Chế Độ Tiếng Anh Fallback

```yaml
temp_english_mode:
  kích_hoạt: Sau thao tác undo (nhấn phím đôi)
  hành_vi: Phím chữ cái tiếp theo là raw (không phải tiếng Việt)
  reset_khi: Ký tự không phải chữ cái hoặc ranh giới từ
  ví_dụ: "Aaron" (aa → â → a [undo] → r → o → n [raw])

  un_latch_dua_tren_bang_chung: >-
    (event-sourcing-completion Phase 2) Không còn là latch một chiều — mỗi
    phím thuộc lớp trigger re-probe compose(raw) đầy đủ và tự gỡ latch khi
    bằng chứng nói là tiếng Việt (xem PIPELINE_ARCHITECTURE.md mục
    "Un-latch dựa trên bằng chứng"). Sửa lớp lỗi "vietj"+"e" → "việt".
```

### Điều Khiển Người Dùng & Cá Nhân Hóa (event-sourcing-completion Phase 4/5)

**Toggle từ cuối (bidirectional word toggle)** — chỉ Hook multiword backend (TSF chưa hỗ trợ):

```yaml
hotkey: Ctrl+Shift+Z (mặc định, đăng ký trong buttre-core/src/hotkey/manager.rs)
hanh_vi:
  - Bấm lần 1: đổi từ cuối cùng đang mở sang literal(raw) — vd "rết" → "reset"
  - Bấm lần 2: đổi lại thành compose(raw) — vd "reset" → "rết"
  - Lặp lại vô hạn (khác Unikey Ctrl+Shift+Esc — one-shot, phá hủy dạng đã ghép)
  - Toggle ĐÓNG từ (word-freezing): gõ tiếp sau toggle bắt đầu từ MỚI, không
    nối vào từ đã bị toggle (tránh phím dấu/transform làm hỏng từ đã đóng băng)
an_toan:
  - Chord exemption: giữ Ctrl/Shift không reset engine (nếu không, chord tự
    xóa window trước khi hotkey kịp xử lý)
  - Focus guard: so khớp foreground HWND tại thời điểm dispatch — alt-tab
    sang app khác trước khi bấm hotkey → no-op, không xóa nhầm text app khác
```

**Backspace mode** (`Settings::backspace_mode`, TOML `backspace_mode = "grapheme" | "raw"`):

```yaml
grapheme (mặc định): xóa 1 ký tự HIỂN THỊ, hành vi không đổi từ trước phase này
raw: xóa 1 PHÍM THÔ và tái ghép — nghịch đảo tự nhiên của kiến trúc event-sourcing,
     có thể xóa nhiều/ít hơn 1 glyph hiển thị (vd "việt" raw "vietj" → backspace
     raw xóa 'j' → "viet", không phải "việ")
```

**Personal learning store** (`Settings::learning_enabled`, mặc định `true`):

```yaml
vi_tri_file: "{dirs::data_dir()}/buttre/learning.toml"
  # Windows: %APPDATA%/buttre/learning.toml
noi_dung:
  user_attested: "âm tiết người dùng gõ trực tiếp (không suy luận) ≥ 3 lần
    riêng biệt, dù chưa có trong bảng âm tiết tĩnh — mở khóa gõ trễ/không
    liền kề cho chính âm tiết đó sau này"
  prefs: "chuỗi phím thô chính xác → hình chiếu ưa thích (literal/composed),
    ghi lại từ hành động CHỦ Ý (double-tap undo, hoặc toggle Ctrl+Shift+Z)"
quyen_rieng_tu:
  - "learning.toml chứa MẢNH VỠ của chữ đã gõ (chuỗi phím thô của từ đã
    sửa/toggle) — KHÔNG BAO GIỜ được ghi vào log"
  - "File thuần TOML, người dùng có thể tự đọc/sửa/xóa — đây LÀ cơ chế xóa
    (không có nút 'Clear' trong app ở phase này)"
  - "Tắt hoàn toàn bằng learning_enabled = false trong settings — không thu
    thập, không áp dụng snapshot, hành vi giống hệt lúc chưa có store"
  - "Chỉ tiến trình Hook mới ghi file (atomic temp+rename, ngoài luồng xử
    lý phím — không bao giờ ghi từ LL-hook callback); tiến trình TSF chỉ đọc"
```

---

## Kiến Trúc Pipeline

**Pipeline Xử Lý 7 Giai Đoạn** (config-driven, recompute-from-raw):

```yaml
stage1_normalization:
  purpose: Chuẩn hóa input, đẩy CharInfo vào char_buffer
  input: char
  output: CharInfo (ch chữ thường + flag chữ hoa)

stage2_gatekeeper:
  purpose: Định tuyến input không phải tiếng Việt
  checks: temp_english_mode, non-alphabetic
  decision: Continue | PassThrough

stage3_compose:
  purpose: Tái tính toán âm tiết từ char_buffer thô (pure function)
  internal_steps:
    fallback: Phát hiện undo / toggle / English-fallback
    segment: Phím thô → base + transform marks + tone keys
    transform: Áp dụng dấu phụ âm (có validation)
    assemble: Đặt dấu thanh lên nhân nguyên âm
  output: syllable_buffer; flag temp_english

stage4_orthography:
  purpose: Chuẩn hóa vị trí dấu thanh + Unicode
  apply: ToneStyle (Old/New)
  convert: Sang NFC (canonical composition)

stage5_learning:
  purpose: Theo dõi pattern người dùng (tương lai, hiện là no-op)

stage6_lookup:
  purpose: Tra cứu từ điển Hán Nôm tùy chọn
  output: candidates trong TypingContext

stage7_output:
  purpose: Tạo action cuối cùng
  algorithm: Diff last_output vs syllable_buffer → Replace{backspace_count, text}
```

**Các Kiểu Chính**:
```rust
struct TypingContext {
    raw_buffer: Vec<char>,           // Lịch sử nhập thô
    current_syllable: Syllable,      // Âm tiết hiện tại
    temp_english_mode: bool,         // Chế độ tiếng Anh fallback
    last_transformation: Option<TransformRecord>,
    last_output: String,             // Cho cập nhật tăng dần
    tone_config: ToneConfig,
    candidates: Vec<Candidate>,      // Cho Hán Nôm
}

struct ToneConfig {
    free_marking: bool,              // Cho phép dấu trước transform
    auto_correct_uo: bool,           // uo → ươ trước dấu
    max_modify_length: usize,        // Độ dài backtrack tối đa
}

enum ToneStyle { Old, New }          // óa vs oá
enum ToneMark { None, Acute, Grave, Hook, Tilde, Dot }
```

**Các Method Chính** (trong `buttre-engine`):
```rust
fn find_main_vowel(text: &str) -> Option<usize>
fn auto_correct_uo(syllable: &mut Syllable)
fn reposition_existing_tone(syllable: &mut Syllable)
fn move_tone(text: &mut String, from: usize, to: usize)
```

---

## Lệnh Build

### Development

```bash
# Kiểm tra code
cargo check
cargo check --package buttre-engine

# Chạy test
cargo test
cargo test --package buttre-engine
cargo test --package buttre-engine -- --skip test_find_best_permutation

# Build
cargo build                    # Debug
cargo build --release          # Release (đã tối ưu)

# Chất lượng code
cargo fmt                      # Format code
cargo clippy --all-targets --all-features  # Linting
```

### Triển Khai Windows TSF

```powershell
# Build + cài + đăng ký giống hệt bản MSI sẽ release (yêu cầu Admin)
./scripts/build-tsf.ps1

# Bản debug — cách duy nhất thấy log TSF (release chỉ log mức WARN)
./scripts/build-tsf.ps1 -Debug

# Gỡ sạch
./scripts/build-tsf.ps1 -Uninstall

# Kiểm tra trạng thái đăng ký
./scripts/check-tsf-status.ps1
```

MSI đăng ký bằng **registry thuần**, không có CustomAction nào nên nó không
bao giờ gọi `DllRegisterServer`. Vì thế `build-tsf.ps1` mặc định cũng ghi
registry chứ không chạy `regsvr32` — dùng `regsvr32` là đang test một service
đăng ký đầy đủ hơn bản ship (thêm profile en-US và hai TSF category). Cờ
`-SelfRegister` giữ đường đó lại để khoanh vùng lỗi.

---

## Cài Đặt Tham Chiếu

**Unikey** (tham chiếu C++ cho thuật toán tiếng Việt):
```yaml
location: .reference/unikey/
key_files:
  - vietkey.cpp:
      functions: putToneMark, putBreveMark, doubleChar, tempVietOff
      purpose: Áp dụng dấu thanh, biến đổi ký tự, English fallback

  - ukengine.cpp:
      functions: processTone, getTonePosition, VSeqList, processRoof, processHook
      purpose: Logic vị trí dấu thanh, phát hiện chuỗi nguyên âm
      data: VSeqList (70 chuỗi nguyên âm tiếng Việt được định nghĩa sẵn)

usage:
  - Thuật toán đặt dấu thanh
  - Phát hiện chuỗi nguyên âm
  - Quản lý buffer (tempVietOff = chế độ tiếng Anh tạm thời)
  - Tự động sửa (uo → ươ khi đặt dấu)
  - Hỗ trợ ToneStyle (Old/New)
```

**Tham Chiếu Khác**:
- **OpenKey**: `.reference/openkey/` (IME tiếng Việt thay thế)
- **IBus Bamboo**: `.reference/ibus-bamboo/` (engine IBus dựa trên Go)
- **Weasel**: `.reference/weasel/` (engine Rime cho Hán Nôm)

---

## Quy Tắc Code Rust

### Xử Lý Lỗi (QUAN TRỌNG)

**❌ BỊ CẤM** (sẽ gây panic trong production):
```rust
// KHÔNG BAO GIỜ làm thế này:
let value = result.unwrap();              // PANIC khi Err
let value = option.expect("message");     // PANIC khi None
panic!("error");                           // Luôn crash
todo!();                                   // Chưa cài đặt
unimplemented!();                          // Chưa cài đặt
```

**✅ ĐÚNG**:
```rust
// Dùng Result cho các thao tác có thể thất bại
pub fn parse_config(path: &Path) -> Result<Config, ConfigError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::IoError { path: path.into(), source: e })?;
    toml::from_str(&content)
        .map_err(|e| ConfigError::ParseError { source: e })
}

// Dùng Option cho giá trị tùy chọn
pub fn find_vowel(text: &str) -> Option<char> {
    text.chars().find(|c| is_vowel(*c))
}

// Dùng ? để truyền lỗi
pub fn load_config() -> anyhow::Result<Config> {
    let path = get_config_path()?;
    let config = parse_config(&path)?;
    Ok(config)
}
```

### Type Safety

**✅ Dùng newtype**:
```rust
// TỐT — Type-safe
struct UserId(u64);
struct TonePosition(usize);
enum InputMethod { Telex, Vni, Viqr }

// XẤU — Không có type safety
type UserId = u64;
let method = "telex";  // Stringly typed
```

### Hiệu Năng

**✅ Tránh allocation trong hot path**:
```rust
// TỐT — Không allocation
fn find_vowel(text: &str) -> Option<usize> {
    text.chars().position(|c| is_vowel(c))
}

// XẤU — Allocation không cần thiết
fn find_vowel(text: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();  // Allocation!
    chars.iter().position(|c| is_vowel(*c))
}
```

**✅ Dùng static lookup**:
```rust
use lazy_static::lazy_static;
use std::collections::HashSet;

lazy_static! {
    static ref VOWELS: HashSet<char> = {
        ['a', 'e', 'i', 'o', 'u', 'y',
         'à', 'á', 'ả', 'ã', 'ạ',
         // ... thêm nguyên âm
        ].iter().copied().collect()
    };
}

pub fn is_vowel(c: char) -> bool {
    VOWELS.contains(&c)  // Tra cứu O(1)
}
```

---

## Tiêu Chuẩn Tài Liệu

### Tài Liệu Public API

**Bắt buộc cho tất cả hàm public**:
```rust
/// Xử lý một phím bấm qua pipeline.
///
/// # Arguments
///
/// * `key` — Ký tự cần xử lý
///
/// # Returns
///
/// Vector các action để thực hiện trên text buffer
///
/// # Errors
///
/// Trả về lỗi nếu... (mô tả điều kiện lỗi)
///
/// # Example
///
/// ```
/// use buttre_core::{Keyboard, InputMethod};
///
/// let mut keyboard = Keyboard::new(InputMethod::Telex)?;
/// let actions = keyboard.process('a')?;
/// ```
pub fn process(&mut self, key: char) -> anyhow::Result<Vec<Action>> {
    // ...
}
```

### Tài Liệu Module

```rust
//! Pipeline Module — Pipeline Xử Lý 7 Giai Đoạn
//!
//! Module này cài đặt pipeline config-driven, 7 giai đoạn để xử lý
//! phương thức nhập liệu tiếng Việt (Telex, VNI, v.v.).
//!
//! ## Kiến Trúc
//!
//! Pipeline gồm 7 giai đoạn:
//! 1. Chuẩn hóa
//! 2. Gatekeeper
//! 3. Compose (recompute-from-raw)
//! 4. Chính tả
//! 5. Học (tương lai)
//! 6. Tra cứu
//! 7. Đầu ra
```

---

## Tài Nguyên

- **Rust**: https://doc.rust-lang.org/
- **windows-rs**: https://github.com/microsoft/windows-rs
- **Unicode**: https://unicode.org/reports/tr15/ (chuẩn hóa NFC/NFD)

_File này đảm bảo AI agent có đầy đủ bối cảnh để hỗ trợ phát triển buttre hiệu quả._
