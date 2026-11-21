# macOS installers

The end-user installer is built from the IMKit host, not from this folder:

- `hosts/macos/build_app.sh <version>` → `Buttre.app` + `buttre-<version>-macos.zip` (with `install.sh`)
- `hosts/macos/build_pkg.sh <version>` → `buttre-<version>-macos.pkg` (Installer.app, keyboard picker under Customize)

See [`hosts/macos/README.md`](../../hosts/macos/README.md) and
[`docs/macos-pkg-installer-design.md`](../../docs/macos-pkg-installer-design.md).

This folder only holds the developer artifact: `build_dylib.sh` packages the
universal `libbuttre_platform.dylib` + keyboards
(`buttre-<version>-macos-universal.zip`, see `ARTIFACT_README.md`). It is not
an installable input method.
