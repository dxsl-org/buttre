#!/usr/bin/env bash
# Build the buttre macOS IMKit host (Buttre.app). Runs on macOS only —
# authored on Windows, built on the macos-latest CI runner and on a Mac.
#
# Produces Buttre.app with the Rust engine dylib embedded. A Developer ID
# Application identity is selected automatically when available; set
# MACOS_SIGNING_IDENTITY=- only for local ad-hoc builds.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

VERSION="${1:-dev}"
# Apple's bundle version fields accept numeric dot-separated components, not
# Cargo prerelease suffixes such as "-beta". Unversioned developer builds use
# a neutral bundle version while retaining "dev" in the archive name.
if [ "$VERSION" = "dev" ]; then
    BUNDLE_VERSION="0.0.0"
else
    BUNDLE_VERSION="${VERSION%%-*}"
fi
if [[ ! "$BUNDLE_VERSION" =~ ^[0-9]+(\.[0-9]+){1,2}$ ]]; then
    echo "Invalid macOS bundle version derived from '$VERSION': '$BUNDLE_VERSION'" >&2
    exit 1
fi
HOST_DIR="hosts/macos"
BUILD_DIR="target/macos-app"
APP="$BUILD_DIR/Buttre.app"
SIGNING_IDENTITY="${MACOS_SIGNING_IDENTITY:-}"
NOTARY_PROFILE="${MACOS_NOTARY_PROFILE:-}"
if [ -z "$SIGNING_IDENTITY" ]; then
    while IFS= read -r identity; do
        if [[ "$identity" =~ \"(Developer\ ID\ Application:.*)\" ]]; then
            SIGNING_IDENTITY="${BASH_REMATCH[1]}"
            break
        fi
    done < <(security find-identity -v -p codesigning)
fi
SIGNING_IDENTITY="${SIGNING_IDENTITY:--}"

echo "==> Building universal engine dylib..."
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
cargo build -p buttre-platform --release --target aarch64-apple-darwin
cargo build -p buttre-platform --release --target x86_64-apple-darwin

echo "==> Assembling bundle skeleton..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$HOST_DIR/Info.plist" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $BUNDLE_VERSION" \
    "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUNDLE_VERSION" \
    "$APP/Contents/Info.plist"
SHORT_VERSION="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" \
    "$APP/Contents/Info.plist")"
BUILD_VERSION="$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" \
    "$APP/Contents/Info.plist")"
INTENDED_LANGUAGE="$(/usr/libexec/PlistBuddy -c "Print :TISIntendedLanguage" \
    "$APP/Contents/Info.plist")"
BUNDLE_IDENTIFIER="$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" \
    "$APP/Contents/Info.plist")"
if [ "$SHORT_VERSION" != "$BUNDLE_VERSION" ] || [ "$BUILD_VERSION" != "$BUNDLE_VERSION" ]; then
    echo "Failed to stamp bundle version $BUNDLE_VERSION" >&2
    exit 1
fi
if [ "$INTENDED_LANGUAGE" != "vi" ]; then
    echo "TISIntendedLanguage must be 'vi' so macOS exposes buttre under Vietnamese" >&2
    exit 1
fi
if [[ "$BUNDLE_IDENTIFIER" != *".inputmethod."* ]]; then
    echo "CFBundleIdentifier must contain '.inputmethod.' for macOS IMKit discovery" >&2
    exit 1
fi

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
cp -R "$HOST_DIR/resources/." "$APP/Contents/Resources/"
cp crates/buttre-platform/icons/vietnamese.png "$APP/Contents/Resources/"
cp keyboards/*.toml "$APP/Contents/Resources/keyboards/" 2>/dev/null || true
for NOM_SRC in "buttre_nom.db" "crates/buttre-core/resources/nom/buttre_nom.db"; do
    if [ -f "$NOM_SRC" ]; then cp "$NOM_SRC" "$APP/Contents/Resources/"; break; fi
done

if [ "$SIGNING_IDENTITY" = "-" ]; then
    echo "==> Ad-hoc signing (local development only)..."
    echo "WARNING: macOS may omit ad-hoc builds from Input Sources." >&2
    echo "Set MACOS_SIGNING_IDENTITY to a Developer ID Application identity." >&2
    codesign --force --sign - \
        "$APP/Contents/Frameworks/libbuttre_platform.dylib"
    codesign --force --sign - --options runtime "$APP"
else
    echo "==> Signing with $SIGNING_IDENTITY..."
    codesign --force --timestamp --sign "$SIGNING_IDENTITY" \
        "$APP/Contents/Frameworks/libbuttre_platform.dylib"
    codesign --force --timestamp --sign "$SIGNING_IDENTITY" \
        --options runtime "$APP"
    if [ -z "$NOTARY_PROFILE" ]; then
        echo "WARNING: Developer ID build is not notarized; macOS may reject it as an Input Source." >&2
        echo "Set MACOS_NOTARY_PROFILE to a notarytool keychain profile." >&2
    fi
fi

echo "==> Verifying..."
lipo -info "$APP/Contents/MacOS/buttre"
codesign --verify --deep --strict --verbose=2 "$APP"

ARCHIVE="$BUILD_DIR/buttre-${VERSION}-macos.zip"
echo "==> Zipping..."
rm -f "$ARCHIVE"
( cd "$BUILD_DIR" && zip -qry "buttre-${VERSION}-macos.zip" "Buttre.app" )

if [ -n "$NOTARY_PROFILE" ]; then
    if [ "$SIGNING_IDENTITY" = "-" ]; then
        echo "MACOS_NOTARY_PROFILE requires Developer ID signing" >&2
        exit 1
    fi
    echo "==> Notarizing..."
    xcrun notarytool submit "$ARCHIVE" \
        --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$APP"
    xcrun stapler validate "$APP"
    spctl --assess --type execute --verbose=2 "$APP"
    rm -f "$ARCHIVE"
    ( cd "$BUILD_DIR" && zip -qry "buttre-${VERSION}-macos.zip" "Buttre.app" )
fi

echo ""
echo "Artifact: $ARCHIVE"
echo "Install:  sudo rm -rf /Library/Input\\ Methods/Buttre.app && sudo ditto '$APP' /Library/Input\\ Methods/Buttre.app"
echo "First install: log out and back in, then add buttre under Vietnamese."
