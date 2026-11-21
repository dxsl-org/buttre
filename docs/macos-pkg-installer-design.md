# Thiết kế: bộ cài `.pkg` cho macOS (chọn bàn phím như MSI Windows)

- **Trạng thái**: Đề xuất — chưa cài đặt
- **Ngày**: 2026-11-21
- **Liên quan**: [ADR-0004](adr/0004-windows-tsf-os-owned-methods.md) (mô hình Windows tương ứng),
  `hosts/macos/` (IMKit host), `installers/windows/product.wxs` (bộ cài mẫu)

## Hiện trạng

Release macOS là `buttre-<ver>-macos.zip` gồm `Buttre.app` + `install.sh`
(`hosts/macos/build_app.sh`). Người dùng mở Terminal chạy `./install.sh`:

1. Xoá bản cũ per-user `~/Library/Input Methods/Buttre.app`.
2. `sudo ditto` app vào `/Library/Input Methods/Buttre.app` (hỏi mật khẩu admin).
3. Chạy `buttre --install` dưới quyền người dùng: `TISRegisterInputSource` + bật cứng
   Telex, VNI, Chữ Nôm (`hosts/macos/src/main.m`, có poll vì TIS bật bất đồng bộ).

Mỗi kiểu gõ đã là một input source riêng (`ComponentInputModeDict`; bàn phím TOML được
`build_app.sh` sinh thêm mode `org.dxsl.inputmethod.buttre.<stem>`). Thiếu so với Windows:
không có giao diện cài, không chọn được bàn phím, phải dùng Terminal.

## Mục tiêu

| Windows (MSI) | macOS (`.pkg`) |
|---|---|
| Bấm đúp cài | Bấm đúp `buttre-<ver>-macos.pkg` (Installer.app) |
| Bước chọn bàn phím: Telex/VNI/Nôm chọn sẵn, bàn phím dân tộc không chọn | Nút **Customize** của Installer.app, mỗi bàn phím một ô, cùng mặc định |
| `buttre --add-keyboard <id>` dưới quyền người cài | `buttre --install <mode>` chạy trong phiên người dùng đang đăng nhập |
| Mục tắt Ctrl+Shift của Windows | **Không cần** — macOS không có hotkey xung đột tương tự |
| Shortcut "buttre - Cấu hình" | Đã có: mục "Cấu hình buttre…" trong menu input |

Ngoài phạm vi: Ctrl+Shift+1…9 trên macOS (việc của IMKit host, không phải bộ cài).

## Cấu trúc gói

`productbuild` ghép nhiều component package theo một Distribution XML:

```
buttre-<ver>-macos.pkg            (productbuild --distribution, ký Developer ID Installer)
├── buttre-app.pkg                bắt buộc; payload Buttre.app → /Library/Input Methods
│     preinstall:  pkill bản đang chạy; xoá ~/Library/Input Methods/Buttre.app của console user
│     postinstall: không bật gì (chỉ cài file)
├── kb-telex.pkg                  script-only (pkgbuild --nopayload --scripts)
├── kb-vni.pkg                    script-only
├── kb-nom.pkg                    script-only
└── kb-<stem>.pkg …               một gói cho mỗi keyboards/*.toml
```

Mỗi `kb-<id>.pkg` chỉ có `postinstall`, chạy với quyền root. Script tìm người dùng đang
đăng nhập rồi chạy lệnh bật **trong phiên của họ**, vì `TISEnableInputSource` dưới root
không có tác dụng với người dùng:

```bash
#!/bin/bash
# kb-<id>.pkg postinstall — MODE được build script thay vào.
MODE="@MODE@"
APP="/Library/Input Methods/Buttre.app/Contents/MacOS/buttre"
USER_NAME="$(stat -f %Su /dev/console)"
# Cài lúc chưa ai đăng nhập (MDM, loginwindow): không có phiên để bật — bỏ qua,
# người dùng tự thêm trong System Settings. Không làm hỏng cả bộ cài.
if [ -z "$USER_NAME" ] || [ "$USER_NAME" = "root" ] || [ "$USER_NAME" = "loginwindow" ]; then
    exit 0
fi
USER_ID="$(id -u "$USER_NAME")"
launchctl asuser "$USER_ID" sudo -u "$USER_NAME" "$APP" --install "$MODE" || true
exit 0
```

Lỗi bật một bàn phím **không** làm hỏng cả bộ cài (giống `Return="ignore"` của MSI):
app vẫn được cài, người dùng có thể tự thêm bàn phím trong System Settings → Keyboard →
Text Input → Edit → (+) → Vietnamese.

## Distribution XML

Sinh bởi script build (danh sách bàn phím TOML lấy từ cùng vòng lặp với `build_app.sh`):

```xml
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>buttre</title>
    <options customize="allow" require-scripts="false" hostArchitectures="arm64,x86_64"/>
    <domains enable_localSystem="true" enable_currentUserHome="false" enable_anywhere="false"/>
    <allowed-os-versions><os-version min="11.0"/></allowed-os-versions>

    <choices-outline>
        <line choice="app"/>
        <line choice="keyboards">
            <line choice="kb.telex"/>
            <line choice="kb.vni"/>
            <line choice="kb.nom"/>
            <line choice="kb.thai"/>
            <!-- … một dòng cho mỗi keyboards/*.toml -->
        </line>
    </choices-outline>

    <choice id="app" title="buttre" enabled="false" selected="true">
        <pkg-ref id="org.dxsl.buttre.app"/>
    </choice>
    <choice id="keyboards" title="Bàn phím thêm vào macOS"/>
    <choice id="kb.telex" title="Telex" start_selected="true">
        <pkg-ref id="org.dxsl.buttre.kb.telex"/>
    </choice>
    <choice id="kb.vni" title="VNI" start_selected="true">
        <pkg-ref id="org.dxsl.buttre.kb.vni"/>
    </choice>
    <choice id="kb.nom" title="Chữ Nôm" start_selected="true">
        <pkg-ref id="org.dxsl.buttre.kb.nom"/>
    </choice>
    <choice id="kb.thai" title="Thái (Tây Bắc)" start_selected="false">
        <pkg-ref id="org.dxsl.buttre.kb.thai"/>
    </choice>

    <pkg-ref id="org.dxsl.buttre.app">buttre-app.pkg</pkg-ref>
    <pkg-ref id="org.dxsl.buttre.kb.telex">kb-telex.pkg</pkg-ref>
    <!-- … -->
</installer-gui-script>
```

- Thứ tự cài theo `choices-outline`: `app` luôn trước các `kb.*`, nên app đã nằm trong
  `/Library/Input Methods` khi các script bật bàn phím chạy.
- Tên hiển thị (`title`) lấy từ `metadata.name` của TOML, giống `InfoPlist.strings`.
- Chữ tiếng Việt trong XML UTF-8 hiển thị trực tiếp. Cần kiểm chứng trên máy thật, như
  rủi ro codepage đã gặp ở MSI.

## Thay đổi code

1. **`hosts/macos/src/main.m`**: `--install [mode…]`. Không có đối số thì giữ hành vi
   hiện tại (Telex, VNI, Nôm) để `install.sh` vẫn chạy được; có đối số thì chỉ
   register + bật đúng các mode đó (`<bundle id>.<mode>`). Mode không tồn tại → báo lỗi,
   exit ≠ 0.
2. **`hosts/macos/build_pkg.sh`** (mới, chạy sau `build_app.sh`): `pkgbuild` gói app
   (`--root` chứa `Buttre.app`, `--install-location "/Library/Input Methods"`,
   `--scripts` có `preinstall`); một `pkgbuild --nopayload --scripts` cho mỗi bàn phím;
   sinh Distribution XML; `productbuild --distribution … --package-path … --sign
   "<Developer ID Installer>"`; `xcrun notarytool submit --wait`; `xcrun stapler staple`.
3. **`.github/workflows/release.yml`**: import thêm chứng chỉ Installer, chạy
   `build_pkg.sh`, upload `.pkg` cạnh zip.
4. **Giữ zip + `install.sh`** làm đường dự phòng (cài không cần Installer.app, CI dùng).

## Ký và notarize — điều kiện tiên quyết

`.pkg` phải ký bằng chứng chỉ **Developer ID Installer**. Đây là chứng chỉ khác với
**Developer ID Application** mà `release.yml` đang import từ `MACOS_CERT_P12_BASE64`.

- **Có chứng chỉ Installer:** ký + notarize trên CI, bấm đúp là cài. Thêm secret, ví dụ
  `MACOS_INSTALLER_CERT_P12_BASE64` + `MACOS_INSTALLER_CERT_PASSWORD`. API key notarytool
  hiện có (`APPLE_API_KEY_*`) dùng lại được.
- **Không có:** Gatekeeper chặn `.pkg` chưa ký ("không xác minh được nhà phát triển"),
  người dùng phải chuột phải → Open hoặc System Settings → Privacy & Security → "Open
  Anyway". Khi đó chưa nên phát hành `.pkg`; tiếp tục dùng zip.

Chứng chỉ Installer tạo trong cùng tài khoản Apple Developer (Certificates → Developer ID
Installer), không mất thêm phí.

## Nâng cấp và gỡ cài

- **Cài đè:** preinstall dừng process `buttre` đang chạy để phím kế tiếp nạp bản mới.
  Bỏ chọn một bàn phím khi cài lại **không** tắt bàn phím đó (bộ cài chỉ thêm, giống MSI).
- **Gỡ:** `.pkg` không có uninstaller. Tài liệu hướng dẫn: gỡ các bàn phím buttre trong
  System Settings → Text Input, rồi `sudo rm -rf "/Library/Input Methods/Buttre.app"` và
  `pkgutil --forget org.dxsl.buttre.app` (cùng các `org.dxsl.buttre.kb.*`).

## Kiểm chứng (trên Mac thật)

1. `pkgutil --expand` gói ra, xem Distribution và scripts đúng như thiết kế.
2. `spctl --assess --type install -vv buttre-<ver>-macos.pkg` → `accepted`, `source=Notarized Developer ID`.
3. Cài bằng Installer.app: Customize hiện đủ bàn phím, chữ tiếng Việt đúng, mặc định đúng.
4. Sau khi cài: menu input có đúng các bàn phím đã chọn (chọn thêm Thái để thử), gõ được
   ngay không cần đăng xuất.
5. Cài đè bản mới khi buttre đang chạy: phím kế tiếp dùng bản mới.
6. Máy chưa có ai đăng nhập (nếu thử được qua `sudo installer -pkg … -target /`
   từ SSH): cài xong không lỗi, không bật bàn phím nào.
