# buttre Engine — Pipeline Xử Lý 7 Giai Đoạn

**Cập nhật lần cuối**: 2026-06-14

## Tổng Quan

buttre Engine dùng pipeline config-driven 7 giai đoạn để xử lý nhập liệu tiếng Việt.
Mỗi giai đoạn có một trách nhiệm duy nhất và trả về `StageResult` để tiếp tục,
short-circuit sang passthrough, hoặc phát ra output action trực tiếp.

Pipeline thay thế thiết kế dual-engine cũ (incremental Transform/Tone giai đoạn 4-5
+ Permutation/Reconciliation/Retrofix giai đoạn 6) bằng một `ComposeStage`
**recompute-from-raw** duy nhất. Mỗi phím bấm tái tính toán âm tiết từ toàn bộ
raw buffer — không có state tích lũy giữa các giai đoạn bên trong logic composition.

---

## Sơ Đồ Giai Đoạn

```
┌──────────────────────────────────────────────────────┐
│                    KÝ TỰ ĐẦU VÀO                     │
│                         ↓                            │
│  ┌────────────────────────────────────────────────┐  │
│  │  Giai đoạn 1: CHUẨN HÓA                       │  │
│  │  • Chuẩn hóa chữ hoa/thường; đẩy CharInfo     │  │
│  └────────────────────────────────────────────────┘  │
│                         ↓                            │
│  ┌────────────────────────────────────────────────┐  │
│  │  Giai đoạn 2: GATEKEEPER                       │  │
│  │  • Kiểm tra temp_english_mode → PassThrough    │  │
│  │  • Không phải chữ cái → PassThrough (commit)   │  │
│  └────────────────────────────────────────────────┘  │
│                         ↓                            │
│  ┌────────────────────────────────────────────────┐  │
│  │  Giai đoạn 3: COMPOSE  (recompute-from-raw)    │  │
│  │  • segment: base + transform marks + tone keys │  │
│  │  • transform: áp dụng dấu phụ âm, kiểm tra    │  │
│  │    tính hợp lệ của âm tiết tiếng Việt          │  │
│  │  • tone: đặt dấu thanh lên nhân nguyên âm      │  │
│  │  • attestation gate: mark suy luận KHÔNG liền  │  │
│  │    kề chỉ được giữ khi âm tiết ghép ra là âm   │  │
│  │    tiết tiếng Việt có thật (bảng 7.884 mục)    │  │
│  │  • fallback: phát hiện undo / toggle / English  │  │
│  │  Ghi syllable_buffer; đặt temp_english         │  │
│  └────────────────────────────────────────────────┘  │
│                         ↓                            │
│  ┌────────────────────────────────────────────────┐  │
│  │  Giai đoạn 4: CHÍNH TẢ                         │  │
│  │  • Chuẩn hóa vị trí dấu thanh (old/new)        │  │
│  │  • Chuẩn hóa Unicode NFC                        │  │
│  └────────────────────────────────────────────────┘  │
│                         ↓                            │
│  ┌────────────────────────────────────────────────┐  │
│  │  Giai đoạn 5: HỌC (no-op cho đến phase sau)    │  │
│  │  • Theo dõi âm tiết đã hoàn thành để thích nghi│  │
│  └────────────────────────────────────────────────┘  │
│                         ↓                            │
│  ┌────────────────────────────────────────────────┐  │
│  │  Giai đoạn 6: TRA CỨU                          │  │
│  │  • Tra cứu từ điển Hán Nôm tùy chọn            │  │
│  │  • Điền TypingContext::candidates               │  │
│  └────────────────────────────────────────────────┘  │
│                         ↓                            │
│  ┌────────────────────────────────────────────────┐  │
│  │  Giai đoạn 7: ĐẦU RA                           │  │
│  │  • Diff last_output vs syllable_buffer          │  │
│  │  • Phát Replace{backspace_count, text} action   │  │
│  └────────────────────────────────────────────────┘  │
│                         ↓                            │
│                    ACTION ĐẦU RA                     │
└──────────────────────────────────────────────────────┘
```

---

## Nguyên Tắc Bất Biến: Event-Sourcing

Raw keystroke buffer là **event log bất biến**; chữ hiển thị chỉ là một **projection
thuần túy** `compose(raw)`. TUYỆT ĐỐI không thêm quyết định "một chiều": không cờ/latch
nào sau khi đặt lại ngăn việc tái tính từ raw, không tích lũy state giữa các giai đoạn.
Mọi policy phải được **tái đánh giá từ raw đầy đủ mỗi phím** — fold phụ thuộc thứ tự trên
log thì được (không bắt buộc stateless; bắt buộc *derivable từ raw*). Thêm field bền vững
vào `TypingContext` là cờ đỏ khi review. Chi tiết + lý do lịch sử: xem AGENTS.md mục
"Event-sourcing purity". `temp_english_mode` là latch di sản cuối cùng, đang được chuyển
sang tái-suy-diễn theo bằng chứng.

---

## Điều Khiển Luồng

Mỗi giai đoạn trả về `StageResult`:

```rust
enum StageResult {
    Continue,              // Tiếp tục sang giai đoạn tiếp theo
    PassThrough,           // Commit composition đang có; commit ký tự thô; reset
    Output(Vec<Action>),   // Short-circuit; trả về các action này ngay lập tức
}
```

---

## Chi Tiết Từng Giai Đoạn

### Giai Đoạn 1: Chuẩn Hóa

**Mục đích**: Chuẩn hóa ký tự đầu vào và thêm vào char buffer.

- Chuyển ký tự đầu vào sang chữ thường (lưu flag chữ hoa trong `CharInfo`).
- Thêm `CharInfo` vào `TypingContext::char_buffer`.
- Luôn trả về `Continue`.

---

### Giai Đoạn 2: Gatekeeper

**Mục đích**: Định tuyến input không phải tiếng Việt mà không chạm vào logic composition.

- Nếu `temp_english_mode` là true → `PassThrough` (gửi ký tự thô nguyên vẹn).
- Nếu ký tự không phải chữ cái (dấu cách, dấu câu, số) → `PassThrough`
  (commit âm tiết đang chờ, sau đó gửi ký tự).
- Ngược lại → `Continue`.

---

### Giai Đoạn 3: ComposeStage (recompute-from-raw)

**Mục đích**: Tái tạo toàn bộ âm tiết từ `char_buffer` sau mỗi phím bấm.

Đây là trái tim của pipeline. Nó gọi `compose::compose(raw, opts)` và
ghi kết quả vào `context.syllable_buffer`.

#### Các bước nội bộ của `compose()`

| Bước | Module | Chức năng |
|------|--------|-----------|
| 1 | `fallback::check_fallback` | Phát hiện pattern undo / toggle từ số lần nhấn phím. Trả về sớm nếu được xử lý. |
| 2 | `segment::segment` | Tách raw buffer thành (ký tự base, transform mark key, tone key); mỗi mark suy luận được gắn cờ `non_adjacent` dựa trên độ liền kề RAW (không phải vị trí trong chuỗi đã ghép). |
| 3 | `transform::apply_transforms` | Áp dụng dấu phụ âm lên base; kiểm tra bằng Vietnamese syllable validator. |
| 4 | `assemble::apply_tone` | Đặt và áp dụng dấu thanh cuối cùng lên nhân nguyên âm. |
| 5 | Attestation gate (`compose::passes_attestation_gate`, `pipeline::validation::is_attested`/`is_shape_attested`) | Chỉ áp dụng cho mark bị gắn cờ `non_adjacent` ở bước 2. Trigger chữ cái (Telex) đòi hỏi khớp CHÍNH XÁC âm tiết đã lên dấu; trigger không phải chữ cái (số VNI) nới lỏng thành khớp HÌNH DẠNG (bất kỳ dấu thanh nào — tránh nhấp nháy khi dấu thanh đến sau transform digit). Thất bại → giáng cấp (demote): tái ghép một lần với `infer_non_adjacent=false` (segment không trích xuất mark non-adjacent nào), các transform liền kề đã hoàn tất ở nơi khác trong từ được giữ nguyên. Đây là bước sửa lớp lỗi `"data"` → `"dât"` (mark suy luận `a` không liền kề tạo ra âm tiết hợp lệ về cấu trúc nhưng KHÔNG có thật). Đệ quy bị chặn ở độ sâu 1 bởi một cờ duy nhất xuyên suốt mọi lần gọi lại `compose()` (kể cả `try_elongation_fallback` và tái dựng prefix trong `fallback.rs`). Va chạm với âm tiết có thật (`"reset"` → `"rết"`) được CHẤP NHẬN theo thiết kế — lối thoát là hoàn tác không liền kề (gõ lại phím trigger, xem `fallback::check_nonadjacent_transform_toggle`). |

#### Mô hình Superset

| Trục | Tùy chọn |
|------|----------|
| `SegmentMode` | `MarkBased` (Telex/VNI) · `DirectMap` (native script) |
| `Validator` | `Vietnamese` · `Hmong` · `Custom` · `None` |
| `tone_enabled` | `true` khi tone_map không rỗng; `false` bỏ qua bước tone |
| `ToneStyle` | `Old` (đặt óa) · `New` (đặt oá, mặc định) |

`ComposeStage` cũng áp dụng case mask sau khi `compose()` trả về: flag chữ hoa
từ `char_buffer` được ánh xạ lại lên text đầu ra.

---

### Giai Đoạn 4: Chính Tả

**Mục đích**: Đảm bảo âm tiết ở dạng Unicode canonical.

- Áp dụng chuẩn hóa vị trí dấu thanh theo `ToneStyle` khi config yêu cầu.
- Chuyển sang NFC (Unicode Canonical Composition) để hiển thị đúng.
- Luôn trả về `Continue` (sửa đổi `syllable_buffer` tại chỗ).

---

### Giai Đoạn 5: Học

**Mục đích**: Thích nghi pattern người dùng trong tương lai (hiện là no-op).

- Sẽ theo dõi các âm tiết đã xác nhận để xếp hạng tần suất cá nhân hóa.
- Luôn trả về `Continue`.

---

### Giai Đoạn 6: Tra Cứu

**Mục đích**: Tra cứu từ điển Hán Nôm (chữ Nôm) tùy chọn.

- Nếu từ điển Nôm được cấu hình và âm tiết khớp với entries, candidates
  được điền vào `TypingContext::candidates`.
- Luôn trả về `Continue`; candidates được tiêu thụ bởi UI layer.

---

### Giai Đoạn 7: Đầu Ra

**Mục đích**: Tạo `Vec<Action>` cuối cùng mô tả IME cần làm gì.

- Diff `context.last_output` với `context.syllable_buffer`.
- Tìm vị trí ký tự khác nhau đầu tiên.
- Phát `Action::Replace { backspace_count, text }` cho phần suffix đã thay đổi.
- Cập nhật `context.last_output` để khớp với âm tiết mới.

---

## Trạng Thái Gõ Phím

Pipeline duy trì state trong `TypingContext`:

```
Phím bấm → char_buffer          → syllable_buffer  → last_output
─────────────────────────────────────────────────────────────────────
n         → [n]                  → "n"              → "n"
g         → [ng]                 → "ng"             → "ng"
u         → [ngu]                → "ngu"            → "ngu"
w         → [nguw]               → "ngư"            → "ngư"
o         → [nguwo]              → "ngưo"           → "ngưo"
w         → [nguwow]             → "người"          → "người"
i         → [nguwowi]            → "người"          → "người"
f         → [nguwowif]           → "người"          → "người"
```

---

## Ví Dụ: Gõ "người" Với Telex

Chuỗi nhập: `n g u w o w i f`

1. `n` — không có transform/tone → âm tiết: `"n"` → Replace{0, "n"}
2. `g` — không khớp → âm tiết: `"ng"` → Replace{1, "g"}
3. `u` — không khớp → âm tiết: `"ngu"` → Replace{1, "u"}
4. `w` — compose: u+w → ư → âm tiết: `"ngư"` → Replace{3, "ngư"}
5. `o` — không khớp → âm tiết: `"ngưo"` → Replace{1, "o"}
6. `w` — compose: uo+w → ươ → âm tiết: `"người"` → Replace{4, "người"}
7. `i` — không khớp → âm tiết: `"người"` → DoNothing (không có diff)
8. `f` — dấu huyền trên ơ → âm tiết: `"người"` → Replace{6, "người"}

**Kết quả cuối**: `"người"` ✓

---

## Hiệu Năng

- **Mục tiêu**: Dưới 1 ms mỗi phím bấm (được xác minh bởi `compose_bench`).
- Chi phí tái tính toán tỷ lệ với độ dài âm tiết (~7 ký tự tối đa cho tiếng Việt),
  không phải với tổng lịch sử nhập liệu.
- Hàm `compose()` là pure (không có global state, không có I/O) — có thể
  cache theo prefix nếu profiling cho thấy cần thiết.
- Tra cứu dấu thanh O(1) qua static array trong `crate::tone`.

---

## Cấu Hình

Pipeline được config-driven hoàn toàn qua `PipelineConfig`. Preset có sẵn:

```rust
// Telex
let config = presets::telex_config();

// VNI
let config = presets::vni_config();

// VIQR
let config = presets::viqr_config();

// Telex đơn giản hóa (không có một số quy tắc mơ hồ)
let config = presets::simple_telex_config();
```

Config tùy chỉnh có thể chỉ định transform rule, tone map, tone style, validator, và
danh sách các middle stage qua `config.pipeline.enabled`.
