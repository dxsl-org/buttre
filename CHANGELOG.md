# Nhật Ký Thay Đổi

Tất cả thay đổi đáng chú ý của buttre được ghi lại tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com); phiên bản theo SemVer.

## [0.7.3-beta] — 2026-06-19

### Gõ nhanh & đồng bộ (Windows hook)
- Sửa lỗi **rớt phím khi gõ nhanh**: đường xử lý phím trong hook dùng `try_write()` và bỏ qua phím khi tranh chấp lock, khiến phím thô lọt lên màn hình còn buffer engine tụt lại → lệch `last_output`. Nay dùng `write()` blocking, chịu poison — không bao giờ bỏ phím.
- Sửa lỗi **xóa rồi gõ tiếp bị dính ngược, mất khoảng trắng**: `backspace()` trước đây chỉ pop chuỗi mirror, không cập nhật executor. Nay reset composition khi xóa, không thể desync xuyên ranh giới từ.
- Chặn `O(n²)`: giới hạn độ dài âm tiết cho recompute (input run-on quá dài → passthrough literal).

### Bộ gõ & chính tả (engine)
- Sửa lỗi bỏ dấu (tone toggle) với từ có phụ âm đầu trùng phím thanh Telex (`seess`→`sês`, `fanss`→`fans`, `sinff`→`sinf`): dùng trailing-run detection đúng theo Unikey/OpenKey.
- Sửa fallback tiếng Anh với từ có nguyên âm lặp xuyên ranh giới phụ âm (`fallback`, `implement`, VNI `color`/`expect`): luật non-adjacent chỉ bắn khi phần trước là một âm tiết tiếng Việt hoàn chỉnh (một nucleus + coda hợp lệ).
- **Bỏ luật `w`→`ư` đầu từ**: từ tiếng Anh bắt đầu bằng `w` (`won`, `with`, `will`, `water`...) gõ tự nhiên; `ư` đầu từ gõ bằng `uw` (`uwng`→`ưng`). `w` chỉ còn là modifier trong `aw`/`ow`/`uw`.
- **Nâng cấp bảng âm vị** (port từ Unikey `VSeqList`/`VCPairList`): bổ sung đầy đủ nuclei (uê, yê, oo loanword...) và ràng buộc nucleus–coda; sửa lỗi cũ từ chối nhầm `iê`+`p/c` (tiếp/biếc).
- **English fallback validation-first**: âm tiết không hợp lệ tiếng Việt sau khi áp dấu/transform → trả literal + chế độ tiếng Anh (`water`→`water`, `wonder`→`wonder`, `result`→`result`).
- **`z` xóa dấu** (bỏ dấu) theo chuẩn Telex; `z`/`dz` đầu từ làm phụ âm cho văn phong informal (`dzí dzụ`, `zô`).
- **Kéo dài ký tự** trong văn chương/chat: giữ âm tiết hợp lệ + nối đuôi lặp literal (`khôngggg`, `trờiii`, `ơiii`, `vèoooo`) thay vì fallback cả từ — ưu tiên linh hoạt như Unikey.
- Sửa phiên bản hiển thị trong hộp thoại trợ giúp.

### Tài liệu
- Thêm quy tắc đổi rule gõ tiếng Việt vào `AGENTS.md` (đi qua 7-stage pipeline, thuật toán tổng quát, không hardcode).
- Cập nhật golden snapshot Telex/VNI/Nôm cho các từ bị ảnh hưởng.

## [0.7.1-beta] — 2026-06-14
- Engine — Tái cấu trúc recompute (12 → 7 giai đoạn)
- Thống nhất tất cả bảng dấu thanh và logic vị trí vào `crates/buttre-engine/src/tone/`
- Một pipeline config-driven phục vụ Telex, VNI, VIQR, và Nôm; segment mode (`MarkBased`/`DirectMap`) và validator (`Vietnamese`/`Hmong`/`Custom`/`None`) được chọn qua config, không hardcode.
- Hành vi: VNI `u7o7` các hợp âm compose đúng theo bất kỳ thứ tự nào; English fallback validation-first, undo giữ nguyên transform
- Hiệu năng: ~250 ns–8 µs/lần gõ phím (dưới 1 ms)
- Sửa lỗi bộ cài đặt Windows TSF, macOS FFI và Linux IBus
- Viết lại toàn bộ tài liệu docs/ và README sang tiếng Việt

## [0.6.2-alpha] — 2026-01-13
- Sửa lỗi bỏ digit kiểu "H2O" trong nhập alphanumeric; cải thiện giữ nguyên literal-mark

## [0.6.1-alpha] — 2026-01-10
- Thêm workflow bảo trì tự động bằng agent
- Sửa lỗi desync backspace xuyên từ; mở rộng phát hiện separator

## [0.6.0-alpha] — 2026-01-05
- Mốc kiến trúc core: pipeline 12 giai đoạn, PGO (~1 µs/lần gõ), gõ linh hoạt (permutation), đồng bộ xuyên từ, backend hybrid Hook+TSF, retrofix/undo

## [0.2.0-alpha] — 2025-12-27
- Hiệu năng VNI: bảng dấu thanh được tính sẵn + phát hiện range-based; PGO engine core

## [0.1.0-alpha] — 2025-12-19
- Phát hành đầu tiên. Phương thức: Telex, VNI, Nôm. Nền tảng: Windows (Hook+TSF), Linux (IBus), macOS. Tính năng: English fallback, raw mode, tone toggle, undo
