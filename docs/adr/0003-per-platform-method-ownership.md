# ADR-0003: Ai sở hữu lựa chọn kiểu gõ — quyết định theo từng nền tảng

- **Trạng thái**: Accepted
- **Ngày**: 2026-07-29
- **Thay thế một phần**: [[ADR-0002]] (tray sở hữu chọn kiểu gõ trên mọi nền tảng)
- **Bối cảnh liên quan**: chuỗi sự cố TSF 28–29/07/2026, lỗi "chọn English là đứt" trên
  IBus, plan `.agents/260729-1107-method-sync-tri-surface/`

## Bối cảnh

`Settings::input_method` giữ **hai khái niệm** trong một field: kiểu gõ
(`telex`/`vni`/`nom`/custom) và bật/tắt (`"english"`). Với hệ điều hành thì "English"
không phải một trạng thái — nó là **sự vắng mặt** của bộ gõ ta. Một bên giữ trạng thái mà
bên kia không diễn tả được, nên mọi vòng đồng bộ tray ↔ hệ thống đều vỡ đúng tại điểm đó:

- IBus: chọn English ở tray thì IBus vẫn tưởng engine buttre đang hoạt động, và chọn
  engine khác ở panel thì tray không biết.
- Windows: cổng chọn backend bị viết lại **ba lần**, sai hai lần đầu, vì cả ba lần đều hỏi
  một câu gián tiếp thay vì hỏi nơi có thẩm quyền.

Ngày 28–29/07 lần ra một chuỗi hệ quả từ cùng gốc này trên Windows TSF: mục bộ gõ ma trỏ
vào chỗ trống, mục đã nối dây mà không có trong vòng Win+Space, mỗi lần cài lại xoá lựa
chọn bàn phím của người dùng, và `Enable=1` do chính installer ghi khiến phép kiểm "người
dùng đã thêm chưa" luôn trả lời "có".

Ba phương án được xét. Chi tiết lập luận trong plan; đây là kết luận.

## Quyết định

> buttre sở hữu lựa chọn kiểu gõ **chỉ ở nơi OS không cho ta một menu, hoặc nơi ta buộc
> phải phân xử nhiều đường truyền.** Còn lại, OS lo.

| Nền tảng | Đường truyền | Menu của OS | Ai sở hữu kiểu gõ | Tray |
|---|---|---|---|---|
| Linux IBus | 1 | có (IBus properties) | **OS** | không |
| Linux fcitx5 | 1 | giả định có¹ | **OS** | không |
| macOS IMKit | 1 | giả định có¹ | **OS** | không |
| Linux Wayland-native | 1 | **không có** | buttre | **có** |
| Windows | **2** (TSF + hook) | có | buttre | **có** |

¹ Chưa xác minh trên môi trường thật. Nếu fcitx5 hoặc IMKit không render được menu kiểu gõ
của chính addon/controller, nền tảng đó chuyển sang nhóm "buttre sở hữu + tray".

Hai nhóm cuối đến cùng một kết luận vì **hai lý do khác nhau**: Wayland-native vì không có
menu nào để hiển thị; Windows vì hook không phải đường truyền mà OS biết, nên chỉ ta phân
xử được giữa nó và TSF.

### Các quyết định kèm theo

1. **Tách bật/tắt khỏi kiểu gõ.** `Settings::enabled: bool` mới; `Settings::input_method`
   luôn là kiểu gõ thật. Giá trị `"english"` bị loại khỏi mọi backend. `AppState::
   last_vietnamese_method` biến mất — nó chỉ tồn tại để hoàn tác việc bật/tắt đè lên kiểu
   gõ.

2. **Lệnh, không phải phản chiếu.** Nhiều nguồn được phép *ghi* vào `enabled`; không nguồn
   nào *phản chiếu* trạng thái của nguồn khác:
   - tray click / hotkey → đảo `enabled`
   - chọn kiểu gõ (menu hoặc hotkey) → đặt kiểu gõ **và** `enabled = true`
   - Windows: `ITfActiveLanguageProfileNotifySink::OnActivated(fActivated)` → lệnh
     bật/tắt, vì người dùng chuyển khỏi profile buttre là một **ý muốn tắt**

   Nhiều chỗ ghi vào một field là chuyện thường (như nút Save và Ctrl+S). Điều bị cấm là
   hai **nguồn sự thật** có thể lệch nhau.

3. **Cửa sổ Cấu hình bỏ lựa chọn kiểu gõ.** Trên nền tảng OS-sở-hữu, một ô chọn kiểu gõ
   trong cửa sổ cấu hình là nơi điều khiển thứ hai. Đây là chỗ thay thế [[ADR-0002]].

4. **`buttre` không tham số trên nền tảng OS-sở-hữu mở cửa sổ Cấu hình**, không dựng tray.
   Người dùng bấm vào app phải được một thứ hữu ích.

5. **Không làm TSF langbar button.** Windows dùng tray, nên nút langbar không cần. Ghi lại
   để không ai coi đây là việc còn thiếu.

## Phương án đã loại

**A — OS sở hữu kiểu gõ ở mọi nơi** (một profile OS cho mỗi kiểu gõ, kiểu Microsoft
Vietnamese IME). Loại vì:

- Một profile phải đăng ký sẵn với **GUID vĩnh viễn** và **quyền admin**. buttre ship 7
  bàn phím TOML (`cham`, `hmong`, `khmer`, `thai`, …) và cho người dùng tự thêm TOML —
  những cái đó không thể có profile OS sau khi cài.
- Wayland-native **không có danh sách bộ gõ nào** để đăng ký vào.
- Dây nối của *một* profile trên Windows (`Enable`/`Preload`/`SortOrder`/`Substitutes`) đã
  tốn một ngày với những kiểu hỏng **im lặng**. Nhân ba cho built-in, nhân N cho custom.

**B — buttre sở hữu kiểu gõ ở mọi nơi, tray ở mọi nơi.** Loại vì trên IBus (và giả định
fcitx5/macOS) OS đã có sẵn menu, nên chạy thêm tray là **nơi điều khiển thứ hai không cần
thiết**. `ibus-unikey` và `ibus-bamboo` không chạy tray, và `ibus_props.rs` của buttre đã
dùng đúng cơ chế đó.

## Một lập luận sai trên đường đi, ghi lại để không lặp

Khi loại A, lý do ban đầu được nêu là *"bàn phím tuỳ chỉnh không thể xuất hiện trong menu
của OS"*. **Sai.** Chúng đang xuất hiện, trên GNOME top-bar, qua `ibus_props.rs` — vì
IBus panel render **properties của engine**, tức là danh sách do *buttre* sở hữu và OS chỉ
*hiển thị*. Lý do đúng để loại A là "A đòi một profile OS cho mỗi kiểu gõ, thứ không cấp
được cho TOML thêm sau khi cài", chứ không phải "menu OS không chứa được custom keyboards".

Phân biệt **ai sở hữu danh sách** với **ai vẽ menu** là điều làm phương án C khả thi.

## Hệ quả

- [[ADR-0002]] còn hiệu lực ở phần "cửa sổ Cấu hình sở hữu cấu hình tần suất thấp" và
  "đồng bộ bằng file-watch, không IPC". Mất hiệu lực ở phần "tray sở hữu chọn kiểu gõ trên
  mọi nền tảng" và ở dòng "Tab Chung: kiểu gõ mặc định".
- Hành vi **khác nhau giữa các nền tảng** một cách có chủ ý. Tài liệu người dùng phải viết
  theo từng OS; `--doctor` / `--tsf-status` nên nói rõ mô hình nào đang hiệu lực.
- Windows: Win+Space **vẫn** là công tắc tắt được, nhờ quyết định 2 (`OnActivated` là
  lệnh). Nếu tín hiệu đó không đáng tin trên thực địa thì phải chọn lại: hoặc bỏ hook
  fallback, hoặc chấp nhận Win+Space không tắt được.
- **Học thông minh (`set_learning`) chỉ được nối trên Windows Hook.** Không có trong
  `platforms/linux/`, không trong `EngineBridge`, không trong `tsf/`. Đây là lỗ hổng có
  sẵn, độc lập với quyết định này, nhưng nó giết câu trả lời tiện "tray là nơi giữ
  learning" — tray chưa bao giờ giữ learning cho các backend khác. Xem plan pha 05.
- Bỏ tray khỏi IBus/fcitx/macOS đòi tiến trình engine phải tự chủ những gì tray đang làm
  (watch settings/macros, autostart, KWin IME lifecycle trên Plasma). Không miễn phí.
