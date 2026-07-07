# Tài Liệu Kiến Trúc buttre

> Tổng quan kiến trúc đầy đủ của buttre Vietnamese Input Method Engine

**Cập nhật lần cuối**: 2026-07-03 (event-sourcing-completion: un-latch, boundary repair, learning, controls)
**Phiên bản**: 0.7.0-beta
**Trạng thái**: Core sẵn sàng production, Tích hợp platform đang thực hiện

---

## Mục Lục

1. [Tổng Quan Hệ Thống](#tổng-quan-hệ-thống)
2. [Kiến Trúc Crate](#kiến-trúc-crate)
3. [Kiến Trúc Pipeline](#kiến-trúc-pipeline)
4. [Quản Lý State](#quản-lý-state)
5. [Luồng Dữ Liệu](#luồng-dữ-liệu)
6. [Tích Hợp Platform](#tích-hợp-platform)
7. [Nguyên Tắc Thiết Kế](#nguyên-tắc-thiết-kế)

---

## Tổng Quan Hệ Thống

buttre là engine bộ gõ tiếng Việt đa nền tảng được viết bằng Rust, được thiết kế cho:
- **Hiệu năng**: Xử lý phím bấm dưới mili-giây
- **Độ chính xác**: Tuân thủ 100% quy tắc chính tả tiếng Việt
- **Linh hoạt**: Hỗ trợ Telex, VNI, VIQR và phương thức nhập Hán Nôm
- **Đa nền tảng**: Windows (TSF), macOS (IMKit), Linux (IBus/Fcitx5)

### Kiến Trúc Cấp Cao

```
┌─────────────────────────────────────────────────────────────────┐
│                     Tầng Platform                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │ Windows TSF  │  │  macOS IMKit │  │ Linux IBus   │          │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘          │
└─────────┼──────────────────┼──────────────────┼─────────────────┘
          │                  │                  │
┌─────────┼──────────────────┼──────────────────┼─────────────────┐
│         │      buttre-core (Độc Lập Nền Tảng) │                  │
│         └──────────────────┴──────────────────┘                 │
│              Giao Diện Keyboard + Kiểu Action                   │
└─────────────────────────┬───────────────────────────────────────┘
                          │
┌─────────────────────────┴───────────────────────────────────────┐
│                   buttre-engine (Pipeline)                        │
│  ┌────────────────────────────────────────────────────────┐    │
│  │   Pipeline Xử Lý 7 Giai Đoạn (Config-Driven)           │    │
│  │   Telex | VNI | VIQR | Hán Nôm                         │    │
│  └────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘
```

---

## Kiến Trúc Crate

buttre dùng kiến trúc multi-crate workspace để tách biệt các mối quan tâm:

### Các Crate Cốt Lõi

#### 1. `buttre-engine` — Pipeline Xử Lý

**Mục đích**: Pipeline xử lý nhập liệu universal, config-driven

**Vị trí**: `crates/buttre-engine/`

**Trách nhiệm**:
- Cài đặt pipeline xử lý 7 giai đoạn
- Xử lý biến đổi nhập liệu tiếng Việt (aa→â, aw→ă, v.v.)
- Áp dụng và vị trí dấu thanh
- Logic undo/redo
- Tra cứu từ điển (Hán Nôm)
- Tạo action xử lý

**Các Module Chính**:
```
buttre-engine/
├── src/
│   ├── pipeline/
│   │   ├── config.rs          # Cấu hình pipeline
│   │   ├── context.rs         # State typing context
│   │   ├── executor.rs        # Pipeline executor (7 giai đoạn)
│   │   ├── presets.rs         # Preset Telex/VNI/VIQR
│   │   └── stages/
│   │       ├── stage1_normalization.rs
│   │       ├── stage2_gatekeeper.rs
│   │       ├── compose_stage.rs    # Giai đoạn 3: recompute-from-raw
│   │       ├── stage9_orthography.rs
│   │       ├── stage10_learning.rs
│   │       ├── stage11_lookup.rs
│   │       └── stage12_output.rs
│   ├── compose/               # Engine tái tính toán thuần túy
│   │   ├── mod.rs             # Điểm vào compose() + ComposeOpts
│   │   ├── segment.rs         # Phím thô → base + marks + tones
│   │   ├── transform.rs       # Áp dụng dấu phụ âm (có validation)
│   │   ├── assemble.rs        # Đặt dấu thanh lên nhân nguyên âm
│   │   └── fallback.rs        # Phát hiện undo / toggle / English-fallback
│   ├── types.rs               # Kiểu Action
│   └── lib.rs
└── tests/                     # Integration test
```

**Public API**:
```rust
// Tạo pipeline executor
let config = telex_config();
let mut executor = PipelineExecutor::new(config);

// Xử lý phím bấm
let actions = executor.process('a');  // Trả về Vec<Action>
```

---

#### 2. `buttre-core` — Giao Diện Độc Lập Nền Tảng

**Mục đích**: Giao diện keyboard và kiểu action độc lập nền tảng

**Vị trí**: `crates/buttre-core/`

**Trách nhiệm**:
- Định nghĩa giao diện `Keyboard`
- Định nghĩa kiểu action (DoNothing, Commit, Replace, v.v.)
- Bọc `buttre-engine` với API sạch
- Chọn phương thức nhập (Telex/VNI/VIQR/Nôm)

**Các Module Chính**:
```
buttre-core/
├── src/
│   ├── keyboard/
│   │   ├── keyboard.rs        # Struct Keyboard chính
│   │   ├── telex/             # Logic đặc thù Telex
│   │   ├── vni/               # Logic đặc thù VNI
│   │   └── nom/               # Logic Hán Nôm
│   ├── action.rs              # Enum Action
│   └── lib.rs
└── tests/
```

**Public API**:
```rust
use buttre_core::{Keyboard, InputMethod, Action};

// Tạo keyboard
let mut keyboard = Keyboard::new(InputMethod::Telex)?;

// Xử lý phím bấm
let actions = keyboard.process('a')?;

// Xử lý action
for action in actions {
    match action {
        Action::DoNothing => { /* buffer */ }
        Action::Commit(text) => { /* gửi text */ }
        Action::Replace { backspace_count, text } => { /* thay thế */ }
        _ => {}
    }
}
```

---

#### 3. `buttre-platform` — Backend Nền Tảng

**Mục đích**: Cài đặt đặc thù nền tảng (Windows TSF, macOS, Linux)

**Vị trí**: `crates/buttre-platform/`

**Trách nhiệm**:
- Windows: TSF (Text Services Framework) — hoạt động
- Linux: IBus (GNOME/X11) + Wayland-native `zwp_input_method_v2` (sway/Hyprland/KDE) — hoạt động; semantics composition dùng chung qua `shared/engine_bridge.rs`
- macOS: FFI (`ButtreKeyResult`) sẵn sàng; IMKit host đang phát triển
- UI system tray
- Quản lý cài đặt

**Các Module Chính**:
```
buttre-platform/
├── src/
│   ├── platforms/
│   │   └── windows/
│   │       └── tsf/
│   │           ├── com.rs                      # Tiện ích COM (DllMain, ref count)
│   │           ├── factory.rs                  # COM class factory
│   │           ├── registration.rs             # Đăng ký TSF
│   │           ├── text_ops.rs                 # Thao tác text
│   │           ├── ipc.rs                      # Giao tiếp liên tiến trình
│   │           ├── logging.rs                  # Debug logging
│   │           ├── text_service/
│   │           │   ├── text_service_stub.rs    # ITfTextInputProcessorEx + ITfKeyEventSink
│   │           │   ├── composition.rs          # State composition
│   │           │   ├── edit_session.rs         # Xử lý edit session
│   │           │   ├── display_attribute.rs    # Thuộc tính hiển thị
│   │           │   ├── candidate_ui.rs         # Cửa sổ candidate
│   │           │   ├── vietnamese_engine.rs    # Xử lý tiếng Việt
│   │           │   └── mod.rs
│   │           └── mod.rs
│   └── lib.rs
└── Cargo.toml
```

**Tích Hợp Platform**:
- **Windows**: Biên dịch thành DLL, đăng ký qua `regsvr32`
- **macOS**: dylib + FFI cho IMKit host (host đang phát triển)
- **Linux**: binary chạy 2 chế độ — `--ibus` (ibus-daemon spawn) và `--ime` (Wayland-native tự dò, fallback IBus)

---

#### 4. `buttre-test` — Tiện Ích Kiểm Thử

**Mục đích**: Hạ tầng kiểm thử đa nền tảng

**Vị trí**: `crates/buttre-test/`

**Trách nhiệm**:
- Kiểm thử hàng loạt từ file text
- Benchmark hiệu năng
- Quản lý dữ liệu test

---

## Kiến Trúc Pipeline

### Pipeline Xử Lý 7 Giai Đoạn

Engine dùng **pipeline 7 giai đoạn** để xử lý nhập liệu tiếng Việt.
Đổi mới cốt lõi là **Giai Đoạn 3: Compose** — engine tái tính toán thuần túy từ raw
thay thế các giai đoạn Transform/Tone/Permutation/Reconciliation/Retrofix incremental cũ
bằng một lời gọi hàm deterministic duy nhất.

```
Phím Đầu Vào
    ↓
┌────────────────────────────────────────────┐
│ Giai Đoạn 1: CHUẨN HÓA                    │
│ • Chuẩn hóa chữ hoa/thường; đẩy CharInfo  │
└────────────────┬───────────────────────────┘
                 ↓
┌────────────────────────────────────────────┐
│ Giai Đoạn 2: GATEKEEPER                   │
│ • temp_english_mode → PassThrough          │
│ • Không phải chữ cái → PassThrough         │
│ • Ngược lại → Continue                     │
└────────────────┬───────────────────────────┘
                 ↓
┌────────────────────────────────────────────┐
│ Giai Đoạn 3: COMPOSE  (recompute-from-raw) │
│ Các bước nội bộ (compose/mod.rs):          │
│  1. fallback — phát hiện undo/toggle       │
│  2. segment — base + transforms + tones   │
│  3. transform — áp dụng dấu phụ (gated)  │
│  4. assemble — đặt tone lên nucleus       │
│  5. attestation gate — non-adjacent marks  │
│     phải match attested syllables          │
│  6. English fallback — nếu không phải VN  │
│ Ghi syllable_buffer; đặt temp_english     │
└────────────────┬───────────────────────────┘
                 ↓
┌────────────────────────────────────────────┐
│ Giai Đoạn 4: CHÍNH TẢ                      │
│ • Chuẩn hóa vị trí dấu thanh              │
│ • Áp dụng ToneStyle (Old: óa, New: oá)    │
│ • Chuyển sang NFC                          │
└────────────────┬───────────────────────────┘
                 ↓
┌────────────────────────────────────────────┐
│ Giai Đoạn 5: HỌC  (no-op, tương lai)       │
│ • Theo dõi pattern người dùng              │
└────────────────┬───────────────────────────┘
                 ↓
┌────────────────────────────────────────────┐
│ Giai Đoạn 6: TRA CỨU                       │
│ • Candidates từ điển Hán Nôm               │
└────────────────┬───────────────────────────┘
                 ↓
┌────────────────────────────────────────────┐
│ Giai Đoạn 7: ĐẦU RA                        │
│ • Diff last_output vs syllable_buffer      │
│ • Phát Replace{backspace_count, text}      │
└────────────────┬───────────────────────────┘
                 ↓
                Action Đầu Ra
```

### Giai Đoạn 3: Compose — Recompute-From-Raw Engine

**Mô đun**: `crates/buttre-engine/src/compose/`

Giai đoạn này là **lõi xử lý của buttre**. Khác với các pipeline tinh chỉnh từng bước, compose **tái tính toán toàn bộ âm tiết từ raw key buffer** trên mỗi phím bấm:

- **Quy trình**: Segment → Transform (validation-gated) → Assemble → **Attestation Gate** → English Fallback
- **Attestation Gate**: Non-adjacent marks (delayed diacritics like Telex 'viete' → 'việt') chỉ được chấp nhận nếu âm tiết cuối cùng là một **từ tiếng Việt có thực** trong bảng attested-syllables. Điều này sửa lỗi `"data"` → `"dât"` (falsepositive) mà không ảnh hưởng đến gõ adjacent/thông thường.

**Bảng Attested Syllables**:
- **Nguồn**: ibus-bamboo vietnamese.cm.dict (GPLv3); 7,642 âm tiết sau khi lọc vowel-less/k-coda
- **Format**: Bitset (~13 KB) tối ưu `(onset_id, nucleus_id, coda_id, tone_id)`
- **File**: `crates/buttre-engine/src/pipeline/attested_data.rs` (generated)
- **Tạo lại**: `cargo run -p buttre-engine --example gen_attested_syllables`
- **Accessors**: `validation::is_attested(text)` (exact tone) + `validation::is_shape_attested(text)` (any tone)

**Trade-off**: Delayed-mark Telex (không tone) không hiển thị dấu live — thay vào đó dấu xuất hiện sau khi gõ tone key. Ví dụ: `viete` → `viete` (literal) + `j` (tone sắc) → `việt`. Đây là cách duy nhất để chặn `data→dât` mà không mở lại lỗi trong VNI/VIQR.

#### Un-Latch Dựa Trên Bằng Chứng (event-sourcing-completion Phase 2)

`temp_english_mode` không còn là latch một chiều — mỗi phím bấm thuộc lớp trigger (tone key hoặc transform trigger) sẽ re-probe `compose(&full_raw)`. Un-latch tự động khi CẢ BỐN điều kiện đúng: probe không tự phân loại là tiếng Anh; text khớp âm tiết đã xác nhận trong bảng; trigger là ký tự cuối cùng trong raw buffer; từ không ở trạng thái vừa hoàn tác. Sửa lỗi như `"vietj"+"e"` → `"việt"` thay vì kẹt `"vietje"`.

#### Word-Boundary Repair (event-sourcing-completion Phase 3)

`compose_closed()` ép buộc khớp CHÍNH XÁC cho mọi lớp trigger tại ranh giới từ (separator/Enter/reset-key), không nới lỏng theo hình dạng. VNI `"nhat6"` hiển thị `"nhât"` khi đang gõ (open) nhưng phục hồi về literal `"nhat6"` tại khóa từ (closed). Áp dụng đồng nhất cho Hook (multiword) và TSF (ConfirmComposition).

#### Coda-k & Nâng Cấp Bảng Âm Vị

Mở rộng coda để bao gồm `"k"` cho các lớp địa danh như Đắk Lắk (`đắk`, `lắk`, `búk`). Làm chặt lớp trigger của attestation gate — chỉ số VNI nới lỏng theo hình dạng; mọi trigger khác đòi hỏi khớp chính xác.

### Kết Quả Giai Đoạn

Mỗi giai đoạn trả về `StageResult`:

```rust
pub enum StageResult {
    Continue,              // Tiếp tục sang giai đoạn tiếp theo
    PassThrough,           // Dừng, gửi input nguyên vẹn
    Output(Vec<Action>),   // Dừng, trả về các action này
}
```

### Cấu Hình Pipeline

Pipeline được **config-driven**, dễ dàng thêm phương thức nhập mới:

```rust
pub struct PipelineConfig {
    pub input_method_type: InputMethodType,
    pub tone_config: ToneConfig,
    pub transform_rules: Vec<TransformRule>,
    pub tone_rules: Vec<ToneRule>,
    pub special_handlers: Vec<SpecialHandler>,
}
```

**Preset Có Sẵn**:
- `telex_config()` — Phương thức nhập Telex
- `vni_config()` — Phương thức nhập VNI
- `viqr_config()` — Phương thức nhập VIQR
- `nom_config()` — Phương thức nhập Hán Nôm

---

## Quản Lý State

### Typing Context

Pipeline duy trì state có thể thay đổi trong `TypingContext`:

```rust
pub struct TypingContext {
    /// Lịch sử nhập thô (cho undo/redo)
    pub raw_buffer: Vec<char>,

    /// Âm tiết đang được xây dựng
    pub current_syllable: Syllable,

    /// Chế độ tiếng Anh fallback (DERIVED từ evidence-based un-latch, không phải latch một chiều)
    pub temp_english_mode: bool,

    /// Biến đổi cuối cùng được áp dụng
    pub last_transformation: Option<TransformRecord>,

    /// Đầu ra cuối cùng (cho cập nhật tăng dần)
    pub last_output: String,

    /// Cấu hình dấu thanh
    pub tone_config: ToneConfig,

    /// Candidates (cho Hán Nôm)
    pub candidates: Vec<Candidate>,

    /// Học tập được bật (từ Settings::learning_enabled)
    pub learning_enabled: bool,

    /// Đang hiển thị candidates từ tra cứu từ điển
    pub showing_candidates: bool,
}
```

### Ví Dụ Tiến Triển State

Gõ "người" (nguwowif):

```
Phím → Raw Buffer   → Âm tiết  → Đầu ra
──────────────────────────────────────────
n   → [n]          → n         → "n"
g   → [ng]         → ng        → "ng"
u   → [ngu]        → ngu       → "ngu"
w   → [nguw]       → ngư       → "ngư"       (u→ư)
o   → [nguwo]      → ngưo      → "ngưo"
w   → [nguwow]     → người     → "người"     (uo→ươ)
i   → [nguwowi]    → người     → "người"
f   → [nguwowif]   → người     → "người"     (dấu thanh)
```

---

## Luồng Dữ Liệu

### Luồng Xử Lý Phím Bấm

```
┌──────────────────────────────────────────────────────────┐
│ 1. Tầng Platform (Windows TSF/macOS/Linux)              │
│    Bắt phím thô từ OS                                    │
└───────────────────────┬──────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────────────┐
│ 2. buttre-core::Keyboard                                  │
│    keyboard.process(key) → Vec<Action>                   │
└───────────────────────┬──────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────────────┐
│ 3. buttre-engine::PipelineExecutor                        │
│    executor.process(key) → Vec<Action>                   │
│    ┌──────────────────────────────────────────────────┐ │
│    │ Giai đoạn 1 → 2 → ... → 7                       │ │
│    │ (Mỗi giai đoạn trả về Continue/PassThrough/Output)│ │
│    └──────────────────────────────────────────────────┘ │
└───────────────────────┬──────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────────────┐
│ 4. Xử Lý Action (về buttre-core)                         │
│    DoNothing | Commit | Replace | UpdateComposition     │
└───────────────────────┬──────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────────────┐
│ 5. Tầng Platform                                         │
│    - Replace: Gửi backspace + text                       │
│    - Commit: Gửi text trực tiếp                          │
│    - UpdateComposition: Cập nhật composition string (TSF)│
└──────────────────────────────────────────────────────────┘
```

### Các Kiểu Action

```rust
pub enum Action {
    DoNothing,                              // Không có đầu ra
    Commit(String),                         // Thêm text
    Replace { backspace_count: usize, text: String },  // Thay thế
    UpdateComposition { text: String, cursor: usize }, // Composition TSF
    ConfirmComposition(String),             // Xác nhận composition
    ShowCandidates { candidates: Vec<Candidate>, input: String }, // Hán Nôm
    HideCandidates,                         // Ẩn candidates
}
```

---

## Tích Hợp Platform

### Windows TSF (Text Services Framework)

**Trạng thái**: ✅ Đã cài đặt và hoạt động

**Kiến trúc**:
- COM DLL được đăng ký là Text Input Processor (TIP)
- Cài đặt giao diện `ITfTextInputProcessorEx`
- Cài đặt `ITfKeyEventSink` để bắt phím
- Dùng composition string cho cập nhật tăng dần

**Đăng ký**:
```powershell
# Build DLL
cargo build --release --package buttre-platform

# Đăng ký (yêu cầu Admin)
regsvr32 target/release/buttre_platform.dll
```

**File Chính**:
- `crates/buttre-platform/src/platforms/windows/tsf/text_service/text_service_stub.rs` — Cài đặt ITfTextInputProcessorEx và ITfKeyEventSink
- `crates/buttre-platform/src/platforms/windows/tsf/com.rs` — Điểm vào DllMain và tiện ích COM

---

### macOS IMKit

**Trạng thái**: Đang lên kế hoạch

**Kiến trúc**:
- Bundle framework với Objective-C bridge
- Dùng framework `InputMethodKit`
- Core Rust được bọc bởi wrapper Objective-C

---

### Linux IBus/Fcitx5

**Trạng thái**: Đang lên kế hoạch

**Kiến trúc**:
- Shared object (.so) được IBus/Fcitx5 tải
- Giao tiếp D-Bus
- Core Rust expose qua C FFI

---

## Nguyên Tắc Thiết Kế

### 1. **Kiến Trúc Config-Driven**

Tất cả phương thức nhập được định nghĩa qua cấu hình, không hardcode logic:

```rust
// Thêm phương thức nhập mới chỉ là cấu hình
let custom_config = PipelineConfig {
    input_method_type: InputMethodType::Custom,
    transform_rules: vec![
        TransformRule { pattern: "aa", result: "â", ... },
        // ... thêm quy tắc
    ],
    // ...
};
```

### 2. **Zero Unsafe Trong Core**

- `buttre-engine`: 100% Rust an toàn
- `buttre-core`: 100% Rust an toàn
- Unsafe code chỉ trong `buttre-platform` cho FFI (giảm thiểu)

### 3. **Khả Năng Kiểm Thử**

- **Unit test**: Mỗi module có unit test toàn diện
- **Integration test**: 600+ test trong `buttre-engine/tests/`
- **Property-based test**: Fuzzing cho edge case
- **Độ phủ test**: >85%

### 4. **Hiệu Năng**

- **Zero-allocation hot path**: Buffer kích thước cố định
- **Tra cứu O(1)**: Tone map tĩnh, bảng băm
- **Cập nhật tăng dần**: Chỉ gửi phần thay đổi
- **Lazy evaluation**: Trì hoãn các thao tác tốn kém

### 5. **Phát Triển Tăng Dần**

Mỗi giai đoạn có thể được kiểm thử độc lập:

```rust
#[test]
fn test_stage4_transform() {
    let mut ctx = TypingContext::new();
    ctx.raw_buffer = vec!['a', 'a'];

    let result = Stage4Transform.process('a', &mut ctx);

    assert_eq!(ctx.current_syllable.text, "â");
}
```

---

## Tóm Tắt

**Kiến trúc buttre** cung cấp:

✅ **Tách biệt mối quan tâm**: Engine ← Core ← Platform
✅ **Config-Driven**: Dễ thêm phương thức nhập mới
✅ **Khả năng kiểm thử**: Mỗi component được kiểm thử độc lập
✅ **Hiệu năng**: Xử lý dưới ms, zero-allocation hot path
✅ **Đa nền tảng**: Cùng core cho Windows/macOS/Linux
✅ **Bảo trì**: Kiến trúc sạch, ranh giới rõ ràng

**File Chính**:
- `crates/buttre-engine/src/pipeline/executor.rs` — Thực thi pipeline
- `crates/buttre-core/src/keyboard/keyboard.rs` — Giao diện keyboard
- `crates/buttre-platform/src/platforms/windows/tsf/` — Windows TSF
- `docs/PIPELINE_ARCHITECTURE.md` — Tài liệu pipeline chi tiết
- `docs/VIETNAMESE_ACCENT.md` — Quy tắc chính tả tiếng Việt
