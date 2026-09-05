# buttre cho macOS

buttre có input source native dùng Input Method Kit (IMKit). macOS chỉ nhận
bundle khi nó nằm trong một trong hai thư mục `Input Methods`; mở
`Buttre.app` trực tiếp hoặc chép vào `/Applications` không phải là cài đặt.

## Build

Yêu cầu: Xcode Command Line Tools và Rust toolchain.

```bash
xcode-select --install
rustup target add aarch64-apple-darwin x86_64-apple-darwin
bash hosts/macos/build_app.sh 0.7.11
```

Kết quả:

- `target/macos-app/Buttre.app`
- `target/macos-app/buttre-0.7.11-macos.zip`

Bundle chứa executable universal (`arm64` + `x86_64`), Rust dylib, keyboard
tables và cơ sở dữ liệu Nôm. Xem
[`hosts/macos/README.md`](hosts/macos/README.md) để biết cách ký Developer ID
và notarize bản phát hành.

Artifact developer chỉ chứa `libbuttre_platform.dylib` vẫn được build riêng
bằng `installers/macos/build_dylib.sh`; nó không phải bộ gõ có thể cài đặt.

## Cài đặt

Cài cho mọi tài khoản:

```bash
sudo rm -rf "/Library/Input Methods/Buttre.app"
sudo ditto target/macos-app/Buttre.app "/Library/Input Methods/Buttre.app"
```

Hoặc chỉ cài cho tài khoản hiện tại:

```bash
mkdir -p "$HOME/Library/Input Methods"
rm -rf "$HOME/Library/Input Methods/Buttre.app"
ditto target/macos-app/Buttre.app "$HOME/Library/Input Methods/Buttre.app"
```

Đăng xuất rồi đăng nhập lại. Sau đó mở:

`System Settings → Keyboard → Text Input → Edit → (+) → Vietnamese → buttre`

Không cần cấp quyền Accessibility. Chọn buttre ở menu Input Sources rồi thử
`vieejt` → `việt`.

## Không thấy buttre trong Input Sources

1. Xác nhận bundle nằm đúng đường dẫn:

   ```bash
   test -d "/Library/Input Methods/Buttre.app" \
     || test -d "$HOME/Library/Input Methods/Buttre.app"
   ```

2. Không giữ đồng thời hai bản có cùng bundle ID trong cả thư mục hệ thống và
   thư mục người dùng. Xóa bản cũ, cài lại một bản duy nhất.
3. Kiểm tra artifact phát hành:

   ```bash
   codesign --verify --deep --strict --verbose=2 \
     "/Library/Input Methods/Buttre.app"
   spctl --assess --type execute --verbose=2 \
     "/Library/Input Methods/Buttre.app"
   ```

4. Đăng xuất/đăng nhập sau mỗi lần thay đổi metadata của bundle. macOS cache
   danh sách input source theo phiên đăng nhập.

Nếu ZIP có tên `macos-universal` và chỉ chứa
`libbuttre_platform.dylib`, đó là thư viện cho developer, không phải
`Buttre.app`.

## Kiến trúc

IMKit định tuyến phím tới `ButtreInputController`; controller chuyển sự kiện
qua C FFI tới `buttre-engine`, rồi áp dụng preedit/commit bằng
`setMarkedText` và `insertText`. Không dùng `CGEventTap`, vì vậy buttre không
theo dõi phím toàn cục và không cần Accessibility.

Mã liên quan:

```text
hosts/macos/
├── Info.plist
├── build_app.sh
└── src/
    ├── main.m
    ├── ButtreInputController.h
    └── ButtreInputController.m

crates/buttre-platform/src/platforms/macos/ffi.rs
```
