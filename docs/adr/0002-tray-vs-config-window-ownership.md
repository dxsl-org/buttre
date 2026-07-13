# ADR-0002: Tray sở hữu chọn kiểu gõ; cửa sổ Cấu hình sở hữu mọi thứ khác

- **Trạng thái**: Accepted
- **Ngày**: 2026-07-13
- **Bối cảnh liên quan**: [[ADR-0001]] (học cá nhân hóa additive-only), plan
  `.agents/260713-1308-config-window-and-shorthand/`

## Bối cảnh

Tray ban đầu (menu chuột phải) tích lũy dần: chọn kiểu gõ, submenu Tùy chọn (Học thông
minh, Tự động khởi động), item "Từ đã học" (mở learning.toml), "Quản lý gõ tắt" (mở
macros.toml), "Hướng dẫn" (MessageBox), "Thoát". Mỗi tính năng mới thêm một item — đúng
như user quan sát: "menu ngày càng phức tạp".

Máy tính người dùng cuối chạy đồng thời hai vai trò rất khác nhau:
1. **Thao tác hằng ngày, tần suất cao**: đổi kiểu gõ (nhiều lần/ngày, cần nhanh, có hotkey).
2. **Cấu hình, tần suất thấp**: bật/tắt tính năng, xem/sửa dữ liệu đã học, đọc hướng dẫn.

Nhét cả hai vào một menu chuột phải phẳng làm (1) chậm đi (menu dài hơn để tìm mục hay
dùng) mà không làm (2) tốt hơn (checkbox trong context menu không phải chỗ để xem bảng
dữ liệu hay đọc văn bản dài).

## Quyết định

**Tray chỉ sở hữu (1)**: English/Telex/VNI/Nôm/custom methods + "Cấu hình…" + "Thoát".
Không còn checkbox, không còn bảng, không còn MessageBox trong tray.

**Cửa sổ Cấu hình (Slint, process riêng, `buttre --config`) sở hữu toàn bộ (2)**:
- Tab Chung: kiểu gõ mặc định, tự động khởi động, chế độ xóa lùi, học thông minh, gõ tắt.
- Tab Từ đã học: bảng xem/xóa `user_attested`.
- Tab Gõ tắt: bảng CRUD `macros.toml`.
- Tab Giới thiệu: phiên bản (qua `CARGO_PKG_VERSION`, không hardcode), phím tắt, liên kết.

Đồng bộ tray↔cửa sổ là **file-watch, không IPC** (xem ADR liên quan trong
`phase-02-slint-config-scaffold.md`): cửa sổ ghi `settings.toml`/`learning.toml`/
`macros.toml` atomic; tray watch 3 file, so sánh với `AppState` in-memory, áp dụng khác
biệt qua đúng code path mà chính tray đã dùng cho các thao tác của nó.

## Hệ quả

- Tray nhẹ, nhất quán về mặt UX với các bộ gõ khác (Unikey, EVKey) — chỉ có chọn kiểu gõ
  + lối vào cấu hình.
- Mọi tính năng cấu hình mới (tương lai) mặc định vào cửa sổ, KHÔNG vào tray — trừ khi nó
  là thao tác tần suất cao cần hotkey riêng (như đổi kiểu gõ hiện tại).
- macOS: model tray khác biệt (IMKit host, không có menu chuột phải kiểu Windows/Linux) —
  cần `ButtreInputController.menu()` override + launch helper `.app` (pattern mozc, xem
  `research-03` trong plan). CHƯA triển khai — IMKit host vẫn ở giai đoạn pre-ship.
- learning.toml/macros.toml/settings.toml giờ có writer từ HAI process (tray + cửa sổ) —
  atomic write với tên file tạm unique-per-call (không chỉ theo PID) là bắt buộc, không
  phải tùy chọn — xem commit lịch sử P2 (`state::atomic_write`).
