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
3. Copy `keyboards/` and `buttre_nom.db` to your bundle's `Contents/Resources`.
   The first `buttre_engine_new()` resolves `Contents/Resources` from the
   executable path (`Contents/MacOS/<exe>`), so no init call is needed.

## FFI surface

The dylib exports the C functions declared in `buttre_platform.h` (FFI v3,
implemented in `crates/buttre-platform/src/platforms/macos/ffi.rs`):

| Function | Description |
|---|---|
| `buttre_engine_new()` / `buttre_engine_free(id)` | Create / destroy an engine (opaque `u64` handle) |
| `buttre_engine_process_char(id, codepoint)` | Feed the layout-translated character of a key event |
| `buttre_engine_process_backspace(id)` | Backspace |
| `buttre_engine_flush(id)` / `buttre_engine_reset(id)` | Commit / discard the pending word |
| `buttre_engine_candidate_count/_at/select_candidate` | Nôm candidate list |
| `buttre_engine_select_method(id, method)` | Report the selected input mode (`telex`/`vni`/`nom`/keyboard stem), persisted |

## End-user installation

This ZIP is still a developer library, not an installable input source. Build
or download the separate `buttre-<version>-macos.zip` artifact containing
`Buttre.app`, then follow [`../../MACOS_README.md`](../../MACOS_README.md).

## Source

https://github.com/dxsl-org/buttre
