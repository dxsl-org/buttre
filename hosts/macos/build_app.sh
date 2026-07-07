#!/usr/bin/env bash
# Build the buttre macOS IMKit host (Buttre.app). Runs on macOS only —
# authored on Windows, built on the macos-latest CI runner and on a Mac.
#
# Produces Buttre.app with the Rust engine dylib embedded, ad-hoc signed
# with a stable bundle id (so the OS's input-source registration and TCC
# state survive rebuilds). Install by copying to ~/Library/Input Methods.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

VERSION="${1:-dev}"
HOST_DIR="hosts/macos"
BUILD_DIR="target/macos-app"
APP="$BUILD_DIR/Buttre.app"
BUNDLE_ID="org.dxsl.buttre.inputmethod"

echo "==> Building universal engine dylib..."
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
cargo build -p buttre-platform --release --target aarch64-apple-darwin
cargo build -p buttre-platform --release --target x86_64-apple-darwin

echo "==> Assembling bundle skeleton..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$HOST_DIR/Info.plist" "$APP/Contents/Info.plist"

echo "==> Universal dylib via lipo..."
lipo -create \
    "target/aarch64-apple-darwin/release/libbuttre_platform.dylib" \
    "target/x86_64-apple-darwin/release/libbuttre_platform.dylib" \
    -output "$APP/Contents/Frameworks/libbuttre_platform.dylib"
# The app finds the dylib via @rpath -> Frameworks (set on the executable
# below); stamp the dylib's own install name to match.
install_name_tool -id "@rpath/libbuttre_platform.dylib" \
    "$APP/Contents/Frameworks/libbuttre_platform.dylib"

echo "==> Compiling the Objective-C host (universal)..."
clang -ObjC -fobjc-arc -O2 \
    -arch arm64 -arch x86_64 \
    -mmacosx-version-min=11.0 \
    -I include \
    -framework Cocoa -framework InputMethodKit \
    -rpath @executable_path/../Frameworks \
    -L "$APP/Contents/Frameworks" -lbuttre_platform \
    "$HOST_DIR/src/main.m" "$HOST_DIR/src/ButtreInputController.m" \
    -o "$APP/Contents/MacOS/buttre"

echo "==> Bundling keyboards + Nôm DB..."
mkdir -p "$APP/Contents/Resources/keyboards"
cp keyboards/*.toml "$APP/Contents/Resources/keyboards/" 2>/dev/null || true
for NOM_SRC in "buttre_nom.db" "crates/buttre-core/resources/nom/buttre_nom.db"; do
    if [ -f "$NOM_SRC" ]; then cp "$NOM_SRC" "$APP/Contents/Resources/"; break; fi
done

echo "==> Ad-hoc signing (stable identifier so TCC/registration persists)..."
codesign --force --sign - --identifier "$BUNDLE_ID" \
    "$APP/Contents/Frameworks/libbuttre_platform.dylib"
codesign --force --sign - --identifier "$BUNDLE_ID" \
    --options runtime "$APP"

echo "==> Verifying..."
lipo -info "$APP/Contents/MacOS/buttre"
codesign -dv "$APP" 2>&1 | head -3

echo "==> Zipping..."
( cd "$BUILD_DIR" && zip -qry "buttre-${VERSION}-macos.zip" "Buttre.app" )
echo ""
echo "Artifact: $BUILD_DIR/buttre-${VERSION}-macos.zip"
echo "Install:  cp -R '$APP' ~/Library/Input\\ Methods/ && (logout/login or reselect in Input Sources)"
