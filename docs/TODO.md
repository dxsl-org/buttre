# Lỗi

- [x] VNI gõ "quo7t1" không ra "quớt" — ĐÃ SỬA: quy tắc ghép uo→ươ chiếm nhầm chữ u của phụ âm đầu "qu" (`find_uo_pos`, transform.rs); Telex "quowts" cũng dính và đã sửa cùng chỗ.
- [ ] TELEX chữ test không gõ được, luôn chuyển thành tét dù gõ 1 hay 2 chữ s. Tương tự chữ text.
  - Không tái hiện được ở engine (HEAD, v0.7.7-beta, cả composition mode): "tesst"→"test", "texxt"→"text" đều đúng; "test"→"tét" với 1 chữ s là hành vi Telex chuẩn giống Unikey.
  - Cần xác minh: (1) build đang cài có phải bản cũ hơn repo không — cài lại từ HEAD rồi thử trong Notepad; (2) nếu vẫn lỗi, ghi lại app cụ thể + chuỗi phím chính xác (nghi lớp injection theo app, hoặc thực tế gõ "texts"→"tét"/"tests"→"tets" là hành vi Unikey-chuẩn, lối thoát "texxts"/"tessts").

# Pre-existing (phát hiện 2026-07-12, có từ trước fix quớt — nghi từ 77c8019)

- [ ] buttre-test harness: "chwowng" không ra "chương" (rơi về raw), "wowts" không ra "ướt" (rơi về raw), "fixx" không ra "fix" (giữ nguyên "fixx").
