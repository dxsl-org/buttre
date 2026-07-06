# buttre macOS Developer Library

**This is NOT an end-user installer.** It is a developer artifact for building macOS host apps that integrate buttre as a Vietnamese IME engine.

## Contents

- `libbuttre_platform.dylib` — universal binary (x86_64 + arm64), exports the buttre FFI surface
- `keyboards/` — TOML keyboard configs for Cham, Hmong, Khmer, Muong/Tay/Nung, Tay Nguyen, Thai scripts
- `README.md` — this file

## Gatekeeper / quarantine

Downloads from the web are flagged with `com.apple.quarantine`, which prevents an unsigned dylib from loading. Remove the flag before linking:

```bash
xattr -d com.apple.quarantine libbuttre_platform.dylib
xattr -dr com.apple.quarantine keyboards/
```

## Linking into a Swift / Objective-C host

1. Drag `libbuttre_platform.dylib` into your Xcode project's `Frameworks` group.
2. Under **Build Phases → Embed Libraries**, ensure the dylib is set to "Embed & Sign".
3. Copy `keyboards/` to your bundle resources.
4. At runtime, point buttre at the keyboards path:
   ```swift
   let keyboards = Bundle.main.resourcePath! + "/keyboards"
   // Pass to buttre FFI init (exact API depends on current buttre_platform.h)
   ```

## FFI surface

The dylib exports C-compatible functions defined in `crates/buttre-platform/src/platforms/macos/ffi.rs`. Key exports:

| Function | Description |
|---|---|
| `buttre_engine_new()` | Create engine instance, returns opaque `u64` handle |
| `buttre_engine_free(id)` | Destroy engine instance |
| `buttre_engine_process_key(id, keycode, shift, capslock)` | Feed a key event, returns action bytes |
| `buttre_engine_process_backspace(id)` | Backspace, returns action bytes |
| `buttre_engine_reset(id)` | Commit preedit and reset state |
| `buttre_engine_set_method(id, method)` | Switch method: 0=telex, 1=vni |
| `buttre_engine_set_enabled(id, enabled)` | Enable/disable IME without destroying state |

## End-user installation

There is no end-user installer for macOS yet. A Swift IMK (Input Method Kit) shell that wraps this dylib is **planned** but not yet built. Until then, this artifact is for developers building their own host app.

## Source

https://github.com/dxsl-org/buttre
