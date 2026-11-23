# ADR-0005: Linux IBus — mỗi kiểu gõ một input source, như macOS và Windows TSF

- **Trạng thái**: Accepted
- **Ngày**: 2026-11-23
- **Thay thế một phần**: [[ADR-0003]] — cách OS sở hữu kiểu gõ trên dòng "Linux IBus"
  (vẫn là OS sở hữu, không tray; đổi từ radio trong menu property sang mỗi kiểu gõ một engine)
- **Liên quan**: [[ADR-0004]] (Windows TSF làm đúng mô hình này)

## Bối cảnh

ADR-0003 cho IBus một engine `buttre` duy nhất, kiểu gõ chọn bằng radio trong menu property
(English / Telex / VNI / Chữ Nôm / bàn phím TOML) mà GNOME vẽ dưới input source. macOS
(`9372b8d`) và Windows TSF (ADR-0004) đã chuyển sang mỗi kiểu gõ một input source của OS.
Linux là nền tảng cuối còn kiểu menu riêng, nên cách chọn kiểu gõ khác nhau giữa ba hệ:

- Super+Space chỉ đổi giữa "buttre" và bàn phím khác; đổi Telex ↔ VNI phải mở menu.
- Radio "English" là trạng thái thứ hai của tắt/bật, bên cạnh việc đổi input source. Đó
  chính là gốc lỗi mà ADR-0003 phải gỡ (`Settings::enabled`).
- Đẩy radio lên GNOME Shell cần một luồng `RegisterProperties` + `UpdateProperty` riêng
  (handler một lần của `ibusManager.js`), kèm task làm mới khi file kiểu gõ đổi.

## Quyết định

> Trên IBus, **mỗi kiểu gõ là một engine IBus riêng** (một input source trong GNOME
> Settings / `ibus-setup`). Engine đang hoạt động CHÍNH LÀ kiểu gõ; "tắt" là chọn input
> source khác.

1. **Tên engine** (`platforms/linux/ibus_engines.rs`, bảng duy nhất cho danh sách và tra
   ngược): Telex **giữ tên cũ `buttre`** để người dùng đã thêm `('ibus', 'buttre')` không
   mất input source — giống Telex giữ GUID cũ trên TSF. Các kiểu khác là `buttre-vni`,
   `buttre-nom`, `buttre-<stem TOML>`. Tên đã phát hành không bao giờ đổi.
2. **Danh sách sinh lúc dựng cache**: component XML dùng
   `<engines exec="/usr/bin/buttre --ibus-engines"/>` (cơ chế ibus-table dùng). ibus-daemon
   chạy lệnh mỗi lần dựng lại registry cache, nên TOML thêm sau có engine sau lần dựng
   cache kế tiếp (`ibus write-cache` / đăng nhập lại) mà không sửa file đã cài. Tên hiển
   thị `buttre <label>` như TSF; mọi engine ở ngôn ngữ `vi`.
3. **Kiểu gõ suy ra từ tên engine** (`CreateEngine(name)` → `method_for_engine`), cố định
   suốt đời instance. Engine IBus không còn theo dõi file kiểu gõ (`MethodState`), không
   đọc `Settings::enabled` (như macOS — giá trị `false` cũ từ tray nền tảng khác không được
   làm câm input source người dùng vừa chọn).
4. **Ghi lại lựa chọn** khi engine thành engine toàn cục (`Enable`): file kiểu gõ +
   `settings.toml` (`persist_command`, bật), như macOS — để phiên tray-sở-hữu sau đó
   (Wayland-native, Windows Hook) khởi động đúng kiểu gõ. Chỉ ghi khi khác giá trị hiện có.
5. **Menu property chỉ còn "Cấu hình buttre…"**. Bỏ radio, mục English, luồng
   `UpdateProperty`, task làm mới.

## Phương án đã loại

- **Giữ radio trong menu** (ADR-0003). Đã có và chạy, nhưng khác macOS/Windows và giữ cả
  hai cách tắt.
- **Liệt kê engine tĩnh trong `buttre.xml`**: đơn giản hơn, nhưng TOML thêm sau cài không
  có engine — đúng lý do ADR-0003 đã nêu khi loại "một profile cho mỗi kiểu gõ".
- **Đặt `language` theo `metadata.language` của TOML** (`cja`, `km`…): GNOME nhóm theo
  ngôn ngữ, bàn phím buttre sẽ rải rác ở nhiều nhóm. Để cả ở `vi`, giống TSF đăng ký dưới
  tiếng Việt.

## Hệ quả

- Người dùng nâng cấp vẫn còn input source `buttre` (giờ là "buttre Telex"); VNI/Nôm phải
  thêm một lần trong Settings.
- TOML thêm sau cần dựng lại cache IBus (đăng nhập lại hoặc `ibus restart`) mới hiện; tương
  đương việc TSF cần `--register-tsf` lại.
- Wayland-native (sway/Hyprland/KDE `--ime`) không đổi: không có danh sách input source
  của OS, buttre vẫn sở hữu kiểu gõ qua tray.
- fcitx5 (addon `fcitx5-buttre`) vẫn dùng menu kiểu gõ trên panel: một addon C++ riêng,
  chuyển sang mô hình này là việc riêng.
