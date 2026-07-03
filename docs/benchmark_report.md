# 📊 BÁO CÁO BENCHMARK THỰC TẾ: BUTTRE vs BỘ GÕ KHÁC

**Ngày thực hiện**: `2026-07-03 20:27:09`

Báo cáo này thu thập số liệu **thực tế 100% (Wall-clock real numbers)** bằng cách chạy trực tiếp các core engine đã được biên dịch tối ưu ở chế độ Release (`opt-level = 'z'`, `-O3`) qua hàng nghìn lượt gõ phím thực tế trên bộ dữ liệu kiểm thử chuẩn (`2,429` từ Telex).

## 1. Tốc độ, Độ trễ & Tỷ lệ chính xác (Latency, Throughput & Accuracy)

> **Ghi chú phương thức kiểm thử**: Toàn bộ 5 bộ gõ đều được chạy kiểm chứng trên cùng bộ dữ liệu chuẩn **Telex** gồm 2,429 từ tiếng Việt thực tế.

| Bộ Gõ (Kiến trúc lõi) | Latency TB (ns) | P50 (ns) | P95 (ns) | P99 (ns) | Thông lượng (M Key/s) | Tỷ lệ chính xác (Accuracy) | Ghi chú bổ sung |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :--- |
| **buttre::compose** (Rust Pure Projection / Event-sourcing) | **882.35 ns** | 860 | 1075 | 1340 | **1.13 M/s** | **99.84%** (2425/2429) | Bất biến log sự kiện, loại bỏ hoàn toàn lỗi trạng thái ngầm (sticky state). |
| **buttre::PipelineExecutor** (Rust 7-Stage Pipeline) | **4420.84 ns** | 3300 | 6300 | 7500 | **0.23 M/s** | **99.84%** (2425/2429) | Quản lý luồng theo giai đoạn độc lập, dễ dàng mở rộng kiểm tra ngữ cảnh. |
| **gonhanh::Engine** (Rust Validation-first) | **197.42 ns** | 100 | 500 | 700 | **5.07 M/s** | **98.52%** (2393/2429) | Kiểm tra hợp lệ âm tiết trước khi biến đổi phím con (Pre-gate guards). |
| **openkey::Engine** (C++ STL Dynamic Containers) | **173.40 ns** | 100 | 500 | 600 | **5.77 M/s** | **99.88%** (2426/2429) | Sử dụng `std::vector`/`std::list` lưu trạng thái, hỗ trợ quay lui phím linh hoạt. |
| **unikey::VietKey** (C++ Static Ring Buffer) | **60.13 ns** | 0 | 100 | 100 | **16.63 M/s** | **99.51%** (2417/2429) | Xử lý in-place trên mảng 40 byte, không cấp phát bộ nhớ động trên hot-path. |

### 💡 Phân tích sâu về Tỷ lệ chính xác & Hành vi vi kiến trúc (Micro-architectural Insights):
1. **Bản chất chênh lệch độ chính xác giữa Buttre (99.84%) và OpenKey (99.88%)**:
   - Khoảng cách 0.04% (đúng 1 từ trong 2,429 từ test) xuất phát từ triết lý chuẩn hóa chính tả: `buttre` áp dụng bộ quy tắc ngữ âm tiếng Việt chặt chẽ (Phonology Validation Tables cho Onset/Nucleus/Coda), từ chối biến đổi các tổ hợp nguyên âm sai chuẩn ngữ âm. Trong khi đó, `openkey` nới lỏng kiểm tra hợp lệ để chấp nhận các kiểu gõ tắt tự do (free marking) không theo quy chuẩn.
2. **Nguyên nhân GoNhanh có tỷ lệ chính xác thấp hơn (98.52%)**:
   - `gonhanh` bị từ chối/sai lệch 36 từ do sử dụng cơ chế chặn sớm phím con (`Pre-gate heuristic guards`). Khi người dùng gõ nhanh các cụm phím có trạng thái trung gian chưa hợp lệ, guard của GoNhanh ngắt chuỗi biến đổi. Trong khi đó, kiến trúc Event-sourcing của `buttre` luôn đánh giá lại toàn bộ log phím thô (`compose(raw)`), giúp tự động phục hồi từ đúng khi kết thúc chuỗi gõ mà không bị mắc kẹt ở trạng thái trung gian.
3. **Đánh đổi giữa Hiệu năng bộ nhớ tĩnh (UniKey) và Khả năng quay lui lịch sử (OpenKey & Buttre)**:
   - `unikey::VietKey` đạt tốc độ thô nhanh nhất (~60 ns/key) nhờ hoàn toàn thao tác trên bộ nhớ đệm vòng tĩnh (`buf[40]`), nhưng giới hạn này khiến engine khó duy trì cây lịch sử phức tạp vượt quá phạm vi 1 từ đơn.
   - `openkey` (~172 ns) và `buttre` (~886 ns) chấp nhận chi phí cấp phát và duyệt buffer lịch sử để đổi lấy độ chính xác cao hơn trong các kịch bản gõ macro, sửa lỗi chính tả thông minh và quay lui phím backspace nhiều cấp.
4. **Độ trễ thực tế trong trải nghiệm người dùng (UX Latency Ceiling)**:
   - Ngay cả kiến trúc đầy đủ 7 giai đoạn `buttre::PipelineExecutor` (~4.4 µs/key) vẫn nhanh gấp **3,600 lần** so với ngưỡng cảm nhận tức thì của mắt người (16 ms trên màn hình 60Hz), chứng minh việc tách lớp kiến trúc sạch sẽ không gây tác động tiêu cực đến trải nghiệm gõ phím thực tế.

## 2. Cập nhật 2026-07-04 — sửa methodology + tối ưu hot-path + w-shorthand

Hai thay đổi làm số liệu buttre ở bảng trên hết hiệu lực:

1. **Sửa methodology benchmark**: harness cũ dựng lại `PipelineConfig` + `PipelineExecutor` cho *từng từ* bên trong vùng đo — IME thật dựng engine một lần mỗi phiên và `reset()` giữa từ. Đo steady-state cho thấy số cũ của PipelineExecutor bị thổi phồng ~2.3× (3,725 vs 1,636 ns/key trên cùng commit).
2. **Tối ưu hot-path + w-shorthand**: `opt-level = 3` riêng cho crate engine, bảng tra cứu dựng sẵn trong `ComposeOpts` (loại `format!`/HashMap alloc mỗi phím), và hỗ trợ gõ tắt `w`→`ư` sau phụ âm đầu (onset-only, có cổng attestation — từ tiếng Anh không bị ảnh hưởng).

| Bộ Gõ | Latency TB (ns) | P95 (ns) | Thông lượng (M Key/s) | Tỷ lệ chính xác |
| :--- | :---: | :---: | :---: | :---: |
| **buttre::compose** | **450.28** | 625 | **2.22** | **99.96%** (2428/2429) |
| **buttre::PipelineExecutor** | **1670.33** | 3100 | **0.60** | 99.92% (2427/2429) |
| gonhanh::Engine (đo lại, steady-state) | 180.53 | 500 | 5.54 | 98.52% (2393/2429) |

Từ telex duy nhất còn fail ở `compose` là `wowts` (w **đầu từ** = ư) — bỏ qua có chủ đích để các từ tiếng Anh bắt đầu bằng w (`won`, `with`, `will`…) gõ tự nhiên. `PipelineExecutor` hiển thị thêm `chwowng` ở dạng literal giữa từ (tương tác latch English có sẵn), nhưng `boundary_repair` sửa thành `chương` tại thời điểm commit từ.

Phân bổ chi phí per-stage của executor (~1.64 µs/key): compose stage 1,320 ns (81% — trong đó `compose()` thuần 450 ns, còn lại là wrapper clone opts/lock/alloc mỗi phím), output 225 ns, orthography (NFC) 124 ns, 4 stage còn lại ~86 ns. Overhead tracing + dyn dispatch đo được không đáng kể.

## 3. Tốc độ tra cứu từ điển Hán Nôm (.reference/hannom-dictionaries)

| Chỉ số | Giá trị thực tế |
| :--- | :--- |
| **Tổng số truy vấn test** | `4,000` queries |
| **Thời gian trung bình / truy vấn** | **`0.027 ms`** |
| **Thông lượng tra cứu** | **`37,035.1 queries/giây`** |
