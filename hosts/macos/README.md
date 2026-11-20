# buttre macOS IMKit host

A native **Input Method Kit** input source (Objective-C) that wraps the
buttre Rust engine via the C FFI in [`include/buttre_platform.h`](../../include/buttre_platform.h).

## Why IMKit (not CGEventTap)

The OS routes keystrokes here only while buttre is the selected input source
— there is **no global key tap and no Accessibility permission**. That is the
whole point: an Accessibility-based tap is what macOS and users flag as a
keylogger. IMKit is the legitimate, standards-compliant path.

Trade-off: password / secure-input fields deliver no events to any input
method (Apple TN2150) — buttre simply won't compose there, by design.

## Layout

```
hosts/macos/
├── Info.plist                 # IMKit bundle keys (connection name, controller class, input mode)
├── build_app.sh               # universal build → sign/notarize → Buttre.app → zip
└── src/
    ├── main.m                 # IMKServer bootstrap
    ├── ButtreInputController.h
    └── ButtreInputController.m # NSEvent → engine → setMarkedText/insertText; input-menu items
```

`build_app.sh` also bundles the Rust `buttre` binary as
`Contents/Helpers/buttre-config` — the settings window (`--config`).

The controller is thin: the Rust engine (FFI v3) does all composition; the
controller feeds it the layout-translated character (`event.characters`, so
Dvorak/AZERTY/Colemak work), applies the returned
`ButtreKeyResult { handled, commit, preedit, candidates_changed }`, drives the
shared `IMKCandidates` panel for Nôm, and reports the selected input mode.

## Build (macOS only)

```bash
# From the repo root, on a Mac (or the macos-latest CI runner):
bash hosts/macos/build_app.sh 0.7.6
# → target/macos-app/Buttre.app  (+ buttre-0.7.6-macos.zip)
```

The script automatically selects a Developer ID Application identity. For a
distributable Input Source, configure a `notarytool` keychain profile and set
`MACOS_NOTARY_PROFILE`; ad-hoc and unnotarized bundles may be omitted by macOS:

```bash
xcrun notarytool store-credentials buttre-notary
MACOS_NOTARY_PROFILE=buttre-notary bash hosts/macos/build_app.sh 0.7.6
```

Requires Xcode command-line tools and the Rust aarch64/x86_64 Apple targets
(the script adds them).

## Install

`Buttre.app` is an input-method bundle, not a normal application. Install it
under an `Input Methods` directory; launching it in Finder or copying it to
`/Applications` does not register an input source.

```bash
sudo rm -rf "/Library/Input Methods/Buttre.app"
sudo ditto target/macos-app/Buttre.app "/Library/Input Methods/Buttre.app"
```

Log out and back in, then open **System Settings → Keyboard → Text Input →
Edit → (+) → Vietnamese** and add the buttre modes you want (buttre Telex,
buttre VNI, buttre Chữ Nôm, one per bundled keyboard).

Select buttre, then type `vieejt` → `việt` (marked/underlined preedit while
composing, committed on space). No Accessibility prompt should appear.

## What the Rust side already wires (no host code needed)

The first `buttre_engine_new()` brings up the SAME tri-surface sync the
Linux engine processes run (`shared/method_sync`, `macro_sync`,
`learning_sync` — see `platforms/macos/ffi.rs::host_sync`). Per keystroke
the engine lazily applies:

- **method** = the selected input mode: the controller reports it on every
  mode switch and activation (`buttre_engine_select_method`), which persists
  it to `~/Library/Application Support/buttre/`'s shared method file so
  every engine in the process follows
- **`Settings::enabled`** is ignored on macOS: "off" is picking another input
  source (ABC), so a stale `false` from another platform cannot silence it
- **shorthand/gõ tắt** (`macros.toml`) + **strict spelling** + **học thông
  minh** (learning collects at word commits, saves via the merged-write
  thread — many-writer safe against the tray/other sessions)
- **resources**: `Contents/Resources` (Nôm dictionary, keyboard TOMLs) is
  derived from the executable path — the default exe-dir search never sees it
- **logs**: `~/Library/Logs/buttre/imk.log` (WARN; create
  `~/Library/Logs/buttre/debug` for DEBUG, which includes typed text)

## Input modes

Every method is its own input source (`ComponentInputModeDict` in
`Info.plist`): Telex, VNI and Chữ Nôm are declared statically; `build_app.sh`
appends one mode per bundled `keyboards/*.toml` (copy of the telex entry,
named from `metadata.name`). Mode id `org.dxsl.inputmethod.buttre.<method>`
maps to engine method `<method>`. Adding or removing a mode changes bundle
metadata — log out and back in after installing.

## Status

🚧 **Awaiting real-Mac verification.** Compiles + bundles on CI
(`macos-imkit` job); the Rust wiring above is exercised by the shared
bridge/sync tests on Linux/Windows. IMKit runtime behavior has no headless
harness — verify by hand below.

## Verification checklist (real Mac)

Cầm checklist này khi sang máy Mac — mọi thứ compile sẵn, việc còn lại là
kiểm chứng runtime:

1. **Cài & đăng ký**: build + install như trên; buttre xuất hiện trong
   Input Sources sau logout/login; KHÔNG có prompt Accessibility.
2. **Gõ cơ bản**: `vieejt` → `việt` (preedit gạch chân, space chốt);
   `hoaf` → `hoà`; backspace giữa từ giữ composition; Enter/điều hướng
   chốt từ (flush).
3. **Input modes**: menu bàn phím ở thanh menu liệt kê buttre Telex / VNI /
   Chữ Nôm / từng keyboard dân tộc ngang hàng ABC; đổi bằng click hoặc
   Ctrl+Space, gõ đúng kiểu vừa chọn.
   **Nôm**: gõ `trowif` → bảng ứng viên hiện dưới con trỏ; ↑/↓ di chuyển,
   Space/Enter/1–9/click chọn, Esc huỷ.
4. **Tri-surface sync**: sửa method file / `settings.toml` (strict,
   shorthand, learning, enabled) từ ngoài trong lúc gõ → áp dụng ở phím kế
   tiếp, không cần restart host.
5. **Học thông minh**: gõ một âm tiết lạ (vd `daat`)×3 → `learning.toml`
   có `"dât" = 3`; tắt "Học thông minh" → ngừng học ngay.
6. **Password bypass**: ô mật khẩu không nhận composition (đúng thiết kế,
   TN2150).
7. **Smoke từng app**: TextEdit, Notes, Safari (cả address bar), Terminal,
   Chrome. Ghi lại quirk (tương đương vụ Chromium omnibox trên Windows).
8. **Cấu hình**: menu input source ở thanh menu (khi đang chọn buttre) có
   mục **Cấu hình buttre…** → mở cửa sổ Cấu hình lên trước (bấm lần hai
   không mở cửa sổ thứ hai); checkbox autostart phải ẨN.

Developer ID signing and notarization are supported by `build_app.sh`; release
automation still needs the notarization credentials provisioned in CI.

> Lưu ý hạ tầng: plan chi tiết nằm trong `.agents/` (gitignored — KHÔNG đi
> theo clone) nên checklist này là nguồn chính khi làm việc trên máy khác.
