# ADR-0001: Học cá nhân hóa chỉ được nới rộng (additive-only) và phải giữ bất biến record-replay

- **Trạng thái**: Accepted
- **Ngày**: 2026-07-13
- **Bối cảnh liên quan**: sự cố "yes không gõ được" (2026-07-12), sự cố "resset/rows" (0.7.7-beta)

## Bối cảnh

Tính năng học cá nhân hóa (`learning.toml`) ghi nhớ hai loại dữ liệu: overlay tự chứng thực
(âm tiết lạ gõ ≥3 lần được coi như có trong từ điển) và pref theo chuỗi raw
(`literal`/`composed`). Phiên bản 0.7.5 còn **tự ghi** `Pref::Literal` cho mọi từ commit
với sự kiện cuối là double-tap undo (raw `yess`, hiển thị user chấp nhận là `yes`).

Khi replay, `Pref::Literal` trả **raw nguyên văn** (`yess`) — khác với hiển thị user đã
chấp nhận lúc ghi (`yes`). Hệ quả: sau một lần dùng lối thoát double-key, chuỗi raw đó bị
chiếm quyền vĩnh viễn; từ `yes` trở thành không gõ được trên đúng máy của user, trong khi
mọi test CI (không nạp learning) vẫn xanh.

## Quyết định

1. **Additive-only**: cơ chế học TỰ ĐỘNG chỉ được *nới rộng* những gì engine chấp nhận
   (overlay chứng thực), không bao giờ được *viết lại* projection của một chuỗi raw.
   Ngoại lệ duy nhất: lệnh tường minh của user (toggle Ctrl+Shift+Z) — và ngay cả khi đó,
   quyết định 2 vẫn phải giữ.

2. **Bất biến record-replay**: mọi tín hiệu học, khi replay ở phiên sau, phải tái tạo
   ĐÚNG hiển thị mà user đã chấp nhận tại thời điểm ghi. Tín hiệu nào không thỏa
   (replay ≠ hiển thị đã chấp nhận) thì không được ghi; nếu đã tồn tại từ trước thì phải
   bị bỏ qua khi tra cứu (lookup guard trong `compose_internal` bước 0) và bị xóa khi có
   tín hiệu mới cho cùng raw.

3. **learning.toml không phải chỗ cho gõ tắt tùy ý**: file chỉ diễn đạt *lựa chọn giữa các
   projection có sẵn* + chứng thực. Ánh xạ raw → văn bản bất kỳ (macro/gõ tắt) nếu làm
   sẽ nằm ở cơ chế riêng (`macros.toml`) với bước expand tại ranh giới từ.

## Cưỡng chế

- **Điểm ghi** (`buttre-core/src/keyboard/engine.rs::collect_and_refresh_learning`):
  nhánh undo-shape không ghi gì; toggle-literal trên raw dạng undo xóa pref cũ thay vì
  ghi pref không thể replay.
- **Điểm đọc** (`buttre-engine/src/compose/mod.rs::compose_internal` bước 0):
  raw dạng undo bỏ qua `Pref::Literal`.
- **CI guard hai-phiên** (`buttre-core/tests/learning_stability_guard.rs`): gõ corpus
  với learning bật ở phiên 1, giữ store, gõ lại phiên 2 — output phải giống hệt.
  Lớp bug "học xong thì gõ khác đi" không thể qua CI.

## Hệ quả

- Từ kiểu `sass`/`raww`/`freee` gõ bằng phím lặp thêm một lần (`sasss`/`rawww`/`freeee`) —
  chuẩn muscle-memory Unikey — thay vì dựa vào pref tự ghi.
- File learning.toml cũ đã nhiễm pref sai tự trung hòa ở lookup guard, không cần migration.
- Mọi tính năng học thêm sau này phải chứng minh giữ được cả hai bất biến trước khi merge
  (reviewer đối chiếu ADR này).
