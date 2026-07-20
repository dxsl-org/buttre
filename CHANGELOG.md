# Nhật Ký Thay Đổi

Tất cả thay đổi đáng chú ý của buttre được ghi lại tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com); phiên bản theo SemVer.

## [Unreleased]

## [0.7.9-beta] — 2026-07-20

- engine: gõ tắt kiểu "đt"/"đc"/"đkkd" chạy được ngay không cần tự định nghĩa — cụm phụ âm không nguyên âm bắt đầu bằng "đ" (chỉ sinh ra được từ phím biến đổi chủ ý dd/d9) giữ nguyên dạng đã ghép thay vì rơi về phím thô; thêm tùy chọn "Kiểm soát gắt gao chính tả tiếng Việt" trong tab Chung để tắt độ lỏng này (mặc định lỏng kiểu Unikey), áp dụng ngay trên mọi backend (hook, TSF, IBus, Wayland)
- scripts: `build-hook.ps1` build bản release theo mặc định — trước đây build debug (~32MB) khiến bản cài thực tế nặng gấp 5 lần bản release thật (~6.4MB); thêm cờ `-Debug` cho ai cần build nhanh để test, không dùng để cài
- core: kéo dài thời hạn tự xóa từ-đã-học không dùng tới từ 180 ngày lên 365 ngày — người ít dùng buttre không còn mất hết thiết lập sau nửa năm; cập nhật lại hướng dẫn "tắt toàn bộ" trong learning.toml đã lỗi thời
- windows: sửa TSF nuốt ký tự phân giới ở cuối composition (`xin.` ra `xin`, dấu `.` biến mất) — `VietnameseEngine::process_key` trước đây chỉ lấy action đầu tiên trong `Vec<Action>`, bỏ luôn `Commit(dấu phân giới)` đi kèm `ConfirmComposition`; giờ áp dụng đủ toàn bộ vector. Đồng thời vá race ẩn do fix này mở ra: `write_text` gộp các lần ghi liên tiếp qua `pending_edit` để tối ưu, nhưng nếu session ghi từ vừa xác nhận chưa kịp chạy thì lần ghi dấu phân giới kế tiếp sẽ ghi đè lên nó thay vì tạo session riêng — mất luôn cả từ. `end_composition` giờ luôn làm mới `pending_edit` trước khi trả về (đóng #4)
- app: tab Giới thiệu trong cửa sổ Cấu hình có nội dung thật (phiên bản qua CARGO_PKG_VERSION, phím tắt, liên kết mở bằng trình duyệt mặc định) — thay MessageBox cũ đã xóa
- docs: ADR-0002 — tray chỉ sở hữu chọn kiểu gõ, cửa sổ Cấu hình sở hữu mọi thứ còn lại
- tray: gọn lại còn kiểu gõ + "Cấu hình…" + "Thoát" — Học thông minh/Tự động khởi động/Gõ tắt/Từ đã học/Quản lý gõ tắt/Hướng dẫn chuyển hết vào cửa sổ Cấu hình (mở qua "Cấu hình…", process riêng); xóa MessageBox hướng dẫn cũ
- app: cửa sổ Cấu hình thêm tab "Từ đã học" (bảng xem/xóa âm tiết đã học, "Quên tất cả", vẫn giữ nút mở tệp gốc) và tab "Gõ tắt" (bảng thêm/sửa/xóa/bật-tắt macro, cảnh báo không chặn khi gõ tắt trùng một âm tiết tiếng Việt thật — điểm khác biệt so với mọi bộ gõ Việt khác)
- app: thêm cửa sổ Cấu hình native (`buttre --config`, Slint) — tab Chung điều khiển kiểu gõ mặc định, tự động khởi động, chế độ xóa lùi, học thông minh, gõ tắt; lưu là áp dụng ngay cho tray đang chạy (không cần khởi động lại), kể cả khi tray tự đổi cùng lúc; single-instance; process riêng biệt (crash cửa sổ config không ảnh hưởng gõ chữ)
- core: `Settings::save`/`LearningStore::write_atomic`/`MacroStore::write_atomic` giờ dùng tên file tạm duy nhất theo từng lần gọi (không chỉ theo PID) — tránh race giữa nhiều tiến trình (config window + tray) lẫn nhiều luồng chạy song song trong cùng tiến trình (test suite)
- core: gộp logic autostart vào crate `buttre-autostart` dùng chung giữa tray và cửa sổ config
- engine: thêm gõ tắt (macros.toml) — tự định nghĩa chuỗi gõ tắt (vd. vn → Việt Nam), chỉ nổ khi gõ đúng nguyên cả từ rồi sang dấu cách/dấu câu (không nổ giữa chừng, không nổ khi là một phần của từ khác); Ctrl+Shift+Z đảo về nguyên văn; cơ chế tách biệt hoàn toàn khỏi học cá nhân hóa (ADR-0001) — deterministic, tự tay định nghĩa, không suy luận
- tray: thêm "Tùy chọn → Gõ tắt" bật/tắt gõ tắt ngay lập tức + menu "Quản lý gõ tắt" mở macros.toml để xem/tự sửa, tự nạp lại khi sửa tay
- core: bất biến record-replay cho học cá nhân hóa (ADR-0001) — toggle literal trên raw dạng undo xóa pref cũ thay vì ghi pref không thể replay
- test: guard hai-phiên chống lớp bug "học xong thì gõ khác đi" — gõ 10k từ Anh (double-key escape) + toàn bộ corpus telex qua 3 phiên chung learning store, output phải giống hệt, chạy trong CI
- tray: thêm "Tùy chọn → Học thông minh" bật/tắt học cá nhân hóa ngay lập tức, không cần khởi động lại
- tray: "Tự động khởi động" hoạt động thật — đăng ký/hủy autostart theo OS (Windows Run key, Linux XDG autostart), tự re-đăng ký khi exe đổi chỗ; bỏ mục placeholder tắt sẵn "Tự động sửa lỗi chính tả" (trái thiết kế leniency)
- tray: thêm menu "Từ đã học" mở learning.toml để xem/tự sửa; file có hướng dẫn ngay đầu; sửa tay được nạp lại tự động khi buttre đang chạy
- tool: `buttre-test explain <method> <raw> [--learning]` — chẩn đoán từng tầng (pref/overlay/undo/gate) vì sao một chuỗi phím ra kết quả đó, nạp được learning.toml thật của máy
- docs: ADR-0001 học cá nhân hóa additive-only; hướng dẫn cập nhật hotkey Ctrl+Shift+Z và mục Học thông minh
- engine: sửa "yes" không gõ được trên máy đã bật learning — bỏ tự ghi pref literal từ double-tap undo (cái user chấp nhận là kết quả undo "yes", nhưng pref replay raw nguyên văn "yess" nên chiếm vĩnh viễn lối thoát double-key); raw dạng undo giờ không bao giờ replay pref literal, learning.toml cũ tự trung hòa không cần migration
- engine: dấu thanh trên vần khuyết "ye"/"ie" đặt vào e — "yes" ra "yé" như Unikey, không còn "ýe"
- engine: sửa lớp từ tiếng Anh không gõ được (rows, towns, owns, lows) — cặp hủy transform (ww) không còn bị hồi sinh khi có phím dấu theo sau
- engine: undo dấu không còn nuốt phím dấu bị ghi đè trước đó ("meterss" ra "meters", không phải "metes")
- test: guard 10.000 từ tiếng Anh thông dụng — mọi từ phải gõ được bằng gõ thẳng hoặc double-key escape, chạy trong CI chống tái phát
- engine: sửa "quo7t1"/"quowts" không ra "quớt" — quy tắc ghép uo→ươ không còn chiếm chữ u thuộc phụ âm đầu "qu" (trước đây ra "qươt" bất hợp lệ rồi rơi về raw)

## [0.7.7-beta] — 2026-07-07

- engine: sửa không gõ được "reset"/"resset" — phím sau cặp hủy dấu (ss) không còn hồi sinh dấu sắc thành "rết"; undo giữa từ giờ là chung cuộc đến hết từ (chuẩn Unikey)
- linux: gõ tiếng Việt hoạt động thật — engine đăng ký đúng với ibus-daemon (private bus + Factory), trước đây không bao giờ nhận được phím
- linux: sửa semantics preedit — composition dựng dần có gạch chân, commit đúng ở dấu cách/dấu câu
- linux: thêm backend Wayland-native `zwp_input_method_v2` cho sway/Hyprland/KDE, tự fallback IBus cho GNOME/X11
- linux: đổi kiểu gõ Telex/VNI/Nôm từ tray áp dụng ngay vào engine đang chạy, không cần restart
- linux: ô mật khẩu bypass engine; không tap phím toàn cục ở cả hai backend
- macos: FFI v2 (`ButtreKeyResult`) map thẳng vào IMKit; semantics composition dùng chung với Linux; kèm header C
- macos: host IMKit (Objective-C) chạy được, không cần quyền Accessibility; build script đóng gói `Buttre.app` universal
- ci: job integration Linux chạy ibus-daemon thật + headless sway để chống hồi quy (trước đây CI xanh không chứng minh gõ được)
- docs: sửa các tuyên bố sai lệch về mức độ sẵn sàng của macOS/Linux; sửa URL repo cũ

## [0.7.6-beta] — 2026-07-04

- engine: hỗ trợ gõ w thay ư sau phụ âm đầu
- engine: w đầu từ giữ literal cho từ tiếng Anh
- engine: kết quả w không attested tự trả về literal
- engine: sửa mất một chữ s cuối từ tiếng Anh
- engine: sửa từ VNI có dấu bị trả raw khi commit
- engine: thêm từ lóng thông dụng vào từ điển chứng thực
- engine: compose nhanh gấp 5 lần nhờ bỏ allocation
- engine: pipeline executor giảm còn 592 ns mỗi phím
- engine: opt-level 3 riêng cho crate engine
- bench: thêm benchmark so sánh với engine tham chiếu
- bench: đo steady-state, sửa số liệu bị thổi phồng

## [0.7.5-beta] — 2026-07-03

- hook: xử lý lỗi gõ trong Chrome bị lặp ký tự
- hook: UIA lỗi hoặc timeout tự rơi về đường backspace cũ
- hook: passthrough tự nhiên cho ký tự commit không biến đổi
- hook: passthrough giúp app nhận scancode thật, tốt cho game, terminal, RDP
- tsf: thêm chế độ phục hồi khi app tự kết thúc composition giữa chừng
- tsf: ghi đè tại chỗ thay vì chèn ký tự sau phần đã commit
- tsf: đếm ký tự thay vì byte cho previous_length và last_text_len
- tsf: không ghi đè previous_length khi tái dùng edit-session đang chờ
- engine: hoàn thiện kiến trúc event-sourcing, nguyên tắc raw keystroke là event log
- engine: thay temp_english_mode latch một chiều bằng tái suy diễn mỗi phím
- engine: sửa lỗi dấu thanh đến sau transform, ví dụ vietj + e thành việt
- engine: double-tap undo vẫn literal, data-class không nhấp nháy
- engine: thêm sửa lỗi cuối cùng tại ranh giới từ
- engine: mark suy luận không liền kề mà dấu thanh chưa đến, phục hồi về literal
- engine: áp dụng đồng nhất cho Hook multiword và TSF ConfirmComposition
- hook: thêm phím tắt Ctrl+Shift+Z đảo literal và composed cho từ đang gõ
- hook: thêm chord-exemption giữ Ctrl, Shift không reset engine
- hook: thêm focus-guard, alt-tab trước khi bấm hotkey sẽ không làm gì
- core: thêm cài đặt backspace_mode raw, xóa theo phím thô
- core: thêm học cá nhân hóa lưu vào learning.toml
- core: tắt được qua cài đặt learning_enabled
- core: âm tiết gõ đủ ba lần riêng biệt sẽ tự chứng thực cho người dùng
- core: hành động chủ ý như double-tap undo, toggle cũng được ghi nhớ ưu tiên
- engine: mở rộng coda k cho lớp địa danh như đắk, lắk
- engine: làm chặt lớp trigger của cổng chứng thực
- engine: đóng băng số field bool trên TypingContext bằng test purity_audit
- test: thêm test tương tác xuyên phase cho un-latch, boundary-repair, toggle
- test: regen golden snapshot thêm chín âm tiết lớp coda-k
- engine: nhúng bảng 7884 âm tiết thật từ từ điển ibus-bamboo
- engine: mark suy luận không liền kề chỉ giữ khi âm tiết có thật
- engine: áp dụng cổng chứng thực cho cả Telex và VNI
- engine: thêm hoàn tác không liền kề khi gõ lại đúng phím trigger
- engine: chấp nhận va chạm âm tiết thật theo thiết kế, không phân biệt được tiếng Anh
- test: mở rộng golden snapshot Telex/VNI với các từ tiếng Anh mới
- hook: sửa lỗi record_output_hwnd bỏ sót ở nhánh passthrough
- tsf: sửa rò rỉ COM reference trên đường phục hồi khi lỗi
- hook: bỏ qua Shift đồng bộ khi người dùng đang giữ phím Shift

## [0.7.4-beta] — 2026-06-19

- hook: sửa lỗi rớt phím khi gõ nhanh
- hook: try_write bỏ qua phím khi tranh chấp lock gây lệch buffer
- hook: đổi sang write blocking, chịu poison, không bao giờ bỏ phím
- hook: sửa lỗi nhảy ngược lên dòng trên khi nhấn Enter rồi gõ tiếp
- hook: đổi sang reset blocking, ép KEYBOARD_DIRTY để luôn reset đúng ranh giới
- hook: backspace nhận biết grapheme, xóa đúng một ký tự hiển thị
- hook: vẫn cho phép sửa lại từ đang gõ thay vì reset sạch
- hook: thêm cửa sổ nhiều từ kiểu Unikey, backspace xuyên dấu cách
- hook: sửa được một đến hai từ trước đó qua dấu cách
- hook: giữ ba từ gần nhất trong window, từ cũ hơn đóng băng
- hook: hard-reset khi Enter, mũi tên, hoặc chuột để tránh lệch con trỏ
- hook: chỉ áp dụng cho backend hook Telex/VNI, TSF và Nôm giữ đường cũ
- engine: chặn O(n bình phương) bằng giới hạn độ dài âm tiết recompute
- engine: sửa lỗi bỏ dấu khi phụ âm đầu trùng phím thanh Telex
- engine: dùng trailing-run detection đúng theo Unikey và OpenKey
- engine: sửa fallback tiếng Anh cho nguyên âm lặp xuyên ranh giới phụ âm
- engine: chỉ bắn luật non-adjacent khi phần trước là âm tiết Việt hoàn chỉnh
- engine: bỏ luật w thành ư ở đầu từ
- engine: nâng cấp bảng âm vị, port từ Unikey VSeqList và VCPairList
- engine: bổ sung đầy đủ nuclei và ràng buộc nucleus-coda
- engine: sửa lỗi từ chối nhầm iê cộng p hoặc c
- engine: English fallback kiểm tra hợp lệ trước khi trả kết quả
- engine: âm tiết không hợp lệ trả về literal và bật chế độ tiếng Anh
- engine: sửa VNI gõ dấu thanh trước transform, ví dụ huyen26 thành huyền
- engine: thêm dạng bare uye cho nguyen64 và quyen26
- engine: hỗ trợ đ không liền kề khi d cuối tạo thành đ
- engine: chỉ bắn khi âm tiết có coda hoặc dấu thanh, giữ dad là tiếng Anh
- engine: z xóa dấu theo chuẩn Telex
- engine: z và dz đầu từ làm phụ âm cho văn phong informal
- engine: giữ âm tiết hợp lệ khi kéo dài ký tự trong chat, văn chương
- engine: nối đuôi lặp literal thay vì fallback cả từ

## [0.7.1-beta] — 2026-06-14

- engine: tái cấu trúc recompute từ 12 xuống 7 giai đoạn
- engine: gộp bảng dấu thanh và logic vị trí vào tone module
- engine: pipeline config-driven cho Telex, VNI, VIQR và Nôm
- engine: segment mode và validator chọn qua config, không hardcode
- engine: English fallback dùng validation trước khi chấp nhận kết quả
- engine: undo giữ nguyên transform đã áp dụng
- engine: hiệu năng khoảng 250ns đến 8 micro giây mỗi lần gõ
- installer: sửa lỗi cài đặt Windows TSF, macOS FFI, Linux IBus

## [0.6.2-alpha] — 2026-01-13

- engine: sửa lỗi bỏ digit kiểu H2O trong nhập alphanumeric
- engine: cải thiện giữ nguyên literal-mark

## [0.6.1-alpha] — 2026-01-10

- hook: sửa lỗi desync backspace xuyên từ
- hook: mở rộng phát hiện separator

## [0.6.0-alpha] — 2026-01-05

- engine: mốc kiến trúc core, pipeline 12 giai đoạn
- engine: thêm PGO, khoảng 1 micro giây mỗi lần gõ
- engine: thêm gõ linh hoạt bằng permutation
- engine: thêm đồng bộ xuyên từ
- platform: thêm backend hybrid Hook cộng TSF
- engine: thêm retrofix và undo

## [0.2.0-alpha] — 2025-12-27

- engine: tối ưu hiệu năng VNI bằng bảng dấu thanh tính sẵn
- engine: thêm phát hiện range-based
- engine: thêm PGO cho engine core

## [0.1.0-alpha] — 2025-12-19

- engine: hỗ trợ phương thức Telex, VNI, Nôm
- platform: hỗ trợ Windows Hook và TSF, Linux IBus, macOS
- engine: có English fallback, raw mode, tone toggle, undo
