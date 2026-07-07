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
├── build_app.sh               # universal build → Buttre.app → ad-hoc sign → zip
└── src/
    ├── main.m                 # IMKServer bootstrap
    ├── ButtreInputController.h
    └── ButtreInputController.m # NSEvent → engine → setMarkedText/insertText
```

The controller is thin: the Rust engine (FFI v2) does keycode mapping and all
composition; the controller only forwards events and applies the returned
`ButtreKeyResult { handled, commit, preedit }`.

## Build (macOS only)

```bash
# From the repo root, on a Mac (or the macos-latest CI runner):
bash hosts/macos/build_app.sh 0.7.6
# → target/macos-app/Buttre.app  (+ buttre-0.7.6-macos.zip)
```

Requires Xcode command-line tools and the Rust aarch64/x86_64 Apple targets
(the script adds them).

## Install

```bash
cp -R target/macos-app/Buttre.app ~/Library/Input\ Methods/
xattr -dr com.apple.quarantine ~/Library/Input\ Methods/Buttre.app   # if downloaded
# Then log out/in (or: killall the input-method system) and add buttre in
# System Settings → Keyboard → Input Sources → (+) → Vietnamese → buttre.
```

Select buttre, then type `vieejt` → `việt` (marked/underlined preedit while
composing, committed on space). No Accessibility prompt should appear.

## Status

🚧 **In development.** The host compiles (verified by the `macos-imkit` CI
job) but end-to-end typing must be confirmed on a real Mac — IMKit has no
headless harness. Not yet shipped to end users. Signing/notarization with a
Developer ID is a later step; until then the unsigned bundle needs the
`xattr` step above.
