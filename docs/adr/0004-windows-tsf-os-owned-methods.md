# ADR-0004: Windows TSF giao lựa chọn kiểu gõ cho OS — mỗi kiểu gõ một bàn phím, không tray

- **Trạng thái**: Accepted
- **Ngày**: 2026-11-21
- **Thay thế một phần**: [[ADR-0003]] — dòng "Windows" của bảng quyết định
- **Bối cảnh liên quan**: plan `.agents/261121-1100-tsf-per-method-profiles/`

## Bối cảnh

ADR-0003 xếp Windows vào nhóm "buttre sở hữu kiểu gõ + tray" với lý do Windows có **hai
đường truyền** (TSF + hook) mà chỉ buttre phân xử được, và loại phương án "một profile OS
cho mỗi kiểu gõ" (phương án A) vì: GUID vĩnh viễn + quyền Admin; TOML thêm sau cài không có
profile; nối dây profile hỏng im lặng, nhân N lần.

Từ đó đến nay:

- Hook chạy kèm TSF (`BackendMode::Both`) bị **tắt mặc định** (`c8a67d4`) vì thực địa vẫn
  hụt (browser nhảy con trỏ, Telegram mất chữ). Ở chế độ TSF thực tế chỉ còn một đường.
- macOS chuyển sang mỗi kiểu gõ một input source (`9372b8d`).
- Microsoft tự làm đúng mô hình này cho tiếng Việt (bàn phím "Vietnamese Telex",
  "Vietnamese Number Key-based" riêng).

## Quyết định

> Bản TSF trên Windows thuộc nhóm **OS sở hữu kiểu gõ, không tray**. Bản Hook giữ
> "buttre sở hữu + tray".

| Nền tảng | Đường truyền | Ai sở hữu kiểu gõ | Tray |
|---|---|---|---|
| Windows, đã thêm bàn phím buttre (TSF) | 1 | **OS** (danh sách bàn phím) | không |
| Windows, chưa thêm / `buttre --hook` | 1 (hook) | buttre | có |

Các dòng khác của bảng ADR-0003 giữ nguyên. Quyết định ở `shared::method_owner`
(`SurfacePath::WindowsTsf` / `WindowsHook`).

### Các quyết định kèm theo

1. **Mỗi kiểu gõ là một profile TSF** (`tsf/profiles.rs`): Telex, VNI, Chữ Nôm và từng
   bàn phím TOML cạnh DLL. Telex **giữ GUID profile cũ** `{B7447743-…}` để người dùng cũ
   không mất bàn phím; các kiểu khác lấy GUID tất định từ id (FNV-1a-128 + fmix64, đánh dấu
   UUID v8), nên TOML thêm sau có GUID vĩnh viễn khi đăng ký lại — đây là lời giải cho lý
   do (2) của ADR-0003. Text service lấy kiểu gõ từ profile đang bật, không đọc
   `Settings::input_method`.
2. **Bật/tắt nằm trong buttre, không phải Ctrl+Shift của Windows.** Thực địa: Ctrl+Shift
   chỉ quay vòng sang bàn phím kế tiếp. `Ctrl+Shift+1…9` chọn profile thứ N và bật;
   `` Ctrl+Shift+` `` tắt (gõ thẳng, profile giữ nguyên). `Ctrl+Shift+0` cũng tắt nhưng
   Windows chặn tổ hợp này (tới text service thành chữ "0" trần). Tắt **không** chuyển
   sang bàn phím Windows: khi đó không còn gì của buttre nhận phím để bật lại. Trạng thái
   ở **global compartment** của TSF — chung mọi ứng dụng, không ghi file.
3. **Icon chế độ cạnh "ENG"** (`GUID_LBI_INPUTMODE`, `langbar.rs`): Windows 11 chỉ hiện
   chữ viết tắt ngôn ngữ cho mọi bàn phím (thực địa), nên đây là chỗ duy nhất cho biết kiểu
   gõ và bật/tắt. Bấm trái bật/tắt; bấm phải: "Cấu hình buttre…". Quyết định 5 của
   ADR-0003 ("không langbar") hết hiệu lực trên Windows — lý do duy nhất của nó là tray.
4. **Bỏ hook fallback** (`BackendMode::Both`, `Settings::hook_fallback`). Cần hook thì chạy
   app hook riêng; `transport_claim` được **giữ** để app hook đứng yên trong ứng dụng đang
   dùng bàn phím buttre (không gõ đôi).
5. **Installer chọn bàn phím**: mọi profile đều đăng ký; Telex/VNI/Nôm được thêm sẵn vào
   danh sách của người cài (`buttre --add-keyboard`), các TOML thêm sau trong Settings.
   Thêm **chỉ dưới ngôn ngữ đã có trong danh sách người dùng** — thêm dưới ngôn ngữ khác
   khiến Windows tự thêm cả ngôn ngữ đó (thực địa: en-GB của định dạng vùng). Kèm mục
   chọn sẵn tắt hotkey Ctrl+Shift của Windows để không giành Ctrl+Shift+1…9.

## Phương án đã loại

- **Một profile + menu kiểu gõ trên icon chế độ** (mô hình IBus). Tránh N profile, nhưng
  Win+Space và danh sách bàn phím của Windows không biết kiểu gõ nào đang dùng; hỏng nếu
  icon chế độ không hiện. Chọn N profile vì thực địa phase 01 cho thấy Win+Space hiện và
  đổi đúng từng profile.
- **Ctrl+Shift của Windows để bật/tắt** — chỉ quay vòng, không bật/tắt (thực địa).
- **Ctrl+Shift+0 chuyển về bàn phím Windows** — không quay lại được (quyết định 2).

## Hệ quả

- Hành vi Windows khác nhau giữa bản TSF và bản Hook một cách có chủ ý; tài liệu người
  dùng viết riêng.
- Nối dây N profile vẫn là rủi ro ADR-0003 đã nêu; giảm bằng **một bảng profile duy nhất**
  dùng cho đăng ký, tra ngược, installer, và `check-tsf-status.ps1` liệt kê mọi profile.
- Đăng ký vẫn cần Admin; TOML thêm sau cần chạy lại `--register-tsf` (UAC).
- `lang_check` xét "đã thêm" theo **từng profile**: tắt VNI không che Telex.
- Ứng dụng không hỗ trợ TSF (một số game, app cũ) không có buttre trừ khi chạy app hook
  riêng.
