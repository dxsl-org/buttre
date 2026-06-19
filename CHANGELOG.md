# Nhật Ký Thay Đổi

Tất cả thay đổi đáng chú ý của buttre được ghi lại tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com); phiên bản theo SemVer.

## [Unreleased]
- Engine — Sửa lỗi bỏ dấu (tone toggle) với từ có phụ âm đầu trùng phím thanh Telex (`seess`→`sês`, `fanss`→`fans`, `sinff`→`sinf`): thuật toán cũ tìm lần xuất hiện đầu tiên của phím thanh thay vì đếm run liên tiếp từ cuối chuỗi; nay dùng trailing-run detection đúng theo Unikey/OpenKey
- Sửa chuỗi phiên bản trong hộp thoại trợ giúp (còn để `0.6.3-alpha`, nay cập nhật thành `0.7.1-beta`)

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
