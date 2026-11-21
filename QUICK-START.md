# buttre — Hướng Dẫn Nhanh (Windows)

## Cài đặt

### Bộ cài MSI (khuyến nghị)

1. Chạy `buttre-<phiên bản>-x86_64.msi`.
2. Ở bước **chọn bàn phím**:
   - **Telex, VNI, Chữ Nôm** chọn sẵn — chúng được thêm thẳng vào danh sách bàn phím của bạn.
   - Các bàn phím dân tộc (Chăm, Hmong, Khmer, Mường/Tày/Nùng, Tây Nguyên, Thái) không chọn
     sẵn. Bàn phím không chọn vẫn được cài; thêm sau ở **Settings → Time & language →
     Language & region → (ngôn ngữ) → Keyboards → Add a keyboard**.
   - **Tắt Ctrl+Shift đổi bàn phím của Windows** chọn sẵn — để Ctrl+Shift+1…9 của buttre
     không bị Windows giành. Win+Space vẫn đổi bàn phím như cũ.
3. Cài xong là gõ được — không có chương trình nào phải chạy thêm, không có icon tray.

### Từ mã nguồn (developer)

```powershell
# PowerShell chạy bằng quyền Administrator
cd <thư mục buttre>
cargo build --release -p buttre-platform
.\scripts\build-tsf.ps1 -Install -NoBuild
```

Sau đó thêm bàn phím buttre trong Settings như trên (hoặc `buttre --add-keyboard telex`).

## Sử dụng

Mỗi kiểu gõ là một bàn phím Windows riêng: **buttre Telex**, **buttre VNI**,
**buttre Chữ Nôm**, …

| Thao tác | Phím / chuột |
|---|---|
| Chọn kiểu gõ | `Ctrl+Shift+1` Telex, `Ctrl+Shift+2` VNI, `Ctrl+Shift+3` Chữ Nôm, `Ctrl+Shift+4…9` bàn phím dân tộc (theo thứ tự cài) — hoặc `Win+Space` |
| Tắt (gõ thẳng) | `` Ctrl+Shift+` `` (phím ngay trái số 1) |
| Bật lại | `Ctrl+Shift+1…9` |
| Xem đang gõ gì | icon buttre cạnh chữ **ENG** trên thanh tác vụ: icon kiểu gõ khi bật, **E** khi tắt |
| Bật/tắt bằng chuột | bấm trái icon buttre |
| Mở Cấu hình | bấm phải icon buttre → **Cấu hình buttre…**, hoặc Start → **buttre - Cấu hình** |
| Đảo từ vừa gõ (nguyên văn ⇄ có dấu) | `Ctrl+Shift+Z` |

Bật/tắt áp dụng cho mọi ứng dụng cùng lúc.

Gõ thử: `vieejt` → `việt`, `hoaf` → `hoà`; VNI `vie65t` → `việt`; Chữ Nôm `troi` rồi
`1`…`9` để chọn chữ.

## Bản Hook (ứng dụng không hỗ trợ TSF)

Một số ứng dụng (game, phần mềm cũ) không nhận bộ gõ TSF. Khi đó chạy `buttre --hook`:
buttre chạy bằng keyboard hook với icon tray riêng. Trong ứng dụng đang dùng bàn phím
buttre, hook tự đứng yên nên không gõ đôi. Máy chưa thêm bàn phím buttre nào thì `buttre`
tự chạy bản Hook.

## Kiểm tra / sự cố

```powershell
.\scripts\check-tsf-status.ps1
```

Báo bàn phím buttre đã đăng ký và đã thêm chưa, buttre đang chạy theo kiểu nào (TSF hay
Hook), và hotkey Ctrl+Shift của Windows có còn bật không.
