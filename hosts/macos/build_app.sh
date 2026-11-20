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
# Keychain holding the notarytool profile; empty = notarytool's default
# (login keychain). CI stores the profile in a throwaway keychain.
NOTARY_KEYCHAIN="${MACOS_NOTARY_KEYCHAIN:-}"
if [ -z "$SIGNING_IDENTITY" ]; then
    while IFS= read -r identity; do
        if [[ "$identity" =~ \"(Developer\ ID\ Application:.*)\" ]]; then
            SIGNING_IDENTITY="${BASH_REMATCH[1]}"
            break
        fi
    done < <(security find-identity -v -p codesigning)
fi
SIGNING_IDENTITY="${SIGNING_IDENTITY:--}"

echo "==> Building universal engine dylib + settings helper..."
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
# `--bin buttre` is the settings helper (`buttre --config`, the Slint window)
# that the input menu's "Cấu hình buttre…" item spawns.
cargo build -p buttre-platform --lib --bin buttre --release --target aarch64-apple-darwin
cargo build -p buttre-platform --lib --bin buttre --release --target x86_64-apple-darwin

echo "==> Assembling bundle skeleton..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks" \
    "$APP/Contents/Helpers"
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

echo "==> Universal settings helper via lipo..."
# Contents/Helpers, NOT Contents/MacOS: an executable in MacOS/ takes the
# bundle's Info.plist (LSBackgroundOnly) as its own, and a background-only
# process cannot bring the settings window to the front.
lipo -create \
    "target/aarch64-apple-darwin/release/buttre" \
    "target/x86_64-apple-darwin/release/buttre" \
    -output "$APP/Contents/Helpers/buttre-config"

echo "==> Compiling the Objective-C host (universal)..."
clang -ObjC -fobjc-arc -O2 \
    -arch arm64 -arch x86_64 \
    -mmacosx-version-min=11.0 \
    -I include \
    -framework Cocoa -framework InputMethodKit -framework Carbon \
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

echo "==> Rendering input-menu icons..."
# The icon PNGs are 128px tagged at ~23 dpi (≈390pt), so the menu bar scaled
# them up and squashed them. The input menu wants a 16pt image: build a TIFF
# with a 16px (1x) and a 32px @144dpi (2x) rep. `tiffutil -cat` keeps each
# rep's dpi (`-cathidpicheck` resets the 2x rep to 72dpi = 32pt).
ICON_TMP="$(mktemp -d)"
trap 'rm -rf "$ICON_TMP"' EXIT
make_menu_icon() {  # <source png> <output name without extension>
    sips -s format tiff -z 16 16 "$1" --out "$ICON_TMP/1x.tiff" >/dev/null
    sips -s dpiWidth 72 -s dpiHeight 72 "$ICON_TMP/1x.tiff" >/dev/null
    sips -s format tiff -z 32 32 "$1" --out "$ICON_TMP/2x.tiff" >/dev/null
    sips -s dpiWidth 144 -s dpiHeight 144 "$ICON_TMP/2x.tiff" >/dev/null
    tiffutil -cat "$ICON_TMP/1x.tiff" "$ICON_TMP/2x.tiff" \
        -out "$APP/Contents/Resources/$2.tiff" >/dev/null
}
# Same icon per method as the Windows/Linux tray (shared/ui/tray.rs).
for ICON in vietnamese telex vni nom custom; do
    make_menu_icon "crates/buttre-platform/icons/$ICON.png" "$ICON"
done

echo "==> Declaring one input mode per bundled keyboard..."
# Every method is its own input source (see Info.plist): copy the telex mode
# as the template, retarget its ids, and name it from the TOML's
# metadata.name. Stems follow the engine's method-id rules (lowercase, not a
# built-in) — anything else could be shown but never selected.
PLIST="$APP/Contents/Info.plist"
MODES=":ComponentInputModeDict:tsInputModeListKey"
TEMPLATE="$MODES:$BUNDLE_IDENTIFIER.telex"
for TOML in "$APP/Contents/Resources/keyboards/"*.toml; do
    [ -f "$TOML" ] || continue
    STEM="$(basename "$TOML" .toml)"
    case "$STEM" in
        telex|vni|nom|english|config|-) continue ;;
    esac
    if [ "$STEM" != "$(printf '%s' "$STEM" | tr '[:upper:]' '[:lower:]')" ]; then
        echo "  skipping $STEM.toml: keyboard filenames must be lowercase" >&2
        continue
    fi
    NAME="$(sed -n 's/^name[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$TOML" | head -n 1)"
    [ -n "$NAME" ] || NAME="$STEM"
    MODE="$BUNDLE_IDENTIFIER.$STEM"
    # Like the tray: the keyboard's own metadata.icon (a file next to its
    # TOML) when it ships one, else the generic custom-method icon.
    ICON_FILE="$(sed -n 's/^icon[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$TOML" | head -n 1)"
    ICON="custom.tiff"
    if [ -n "$ICON_FILE" ] && [ -f "keyboards/$ICON_FILE" ]; then
        make_menu_icon "keyboards/$ICON_FILE" "keyboard-$STEM"
        ICON="keyboard-$STEM.tiff"
    fi
    /usr/libexec/PlistBuddy -c "Copy $TEMPLATE $MODES:$MODE" "$PLIST"
    /usr/libexec/PlistBuddy -c "Set $MODES:$MODE:TISInputSourceID $MODE" "$PLIST"
    /usr/libexec/PlistBuddy -c "Set $MODES:$MODE:tsInputModeDefaultStateKey false" "$PLIST"
    /usr/libexec/PlistBuddy -c "Set $MODES:$MODE:tsInputModePrimaryInScriptKey false" "$PLIST"
    for KEY in tsInputModeMenuIconFileKey tsInputModeAlternateMenuIconFileKey \
        tsInputModePaletteIconFileKey; do
        /usr/libexec/PlistBuddy -c "Set $MODES:$MODE:$KEY $ICON" "$PLIST"
    done
    /usr/libexec/PlistBuddy -c \
        "Add :ComponentInputModeDict:tsVisibleInputModeOrderedArrayKey: string $MODE" "$PLIST"
    for STRINGS in "$APP/Contents/Resources/"*.lproj/InfoPlist.strings; do
        printf '"%s" = "buttre %s";\n' "$MODE" "${NAME//\"/\\\"}" >> "$STRINGS"
    done
    echo "  $MODE = buttre $NAME"
done
plutil -lint "$PLIST" "$APP/Contents/Resources/"*.lproj/InfoPlist.strings

if [ "$SIGNING_IDENTITY" = "-" ]; then
    echo "==> Ad-hoc signing (local development only)..."
    echo "WARNING: macOS may omit ad-hoc builds from Input Sources." >&2
    echo "Set MACOS_SIGNING_IDENTITY to a Developer ID Application identity." >&2
    codesign --force --sign - \
        "$APP/Contents/Frameworks/libbuttre_platform.dylib"
    codesign --force --sign - --options runtime "$APP/Contents/Helpers/buttre-config"
    codesign --force --sign - --options runtime "$APP"
else
    echo "==> Signing with $SIGNING_IDENTITY..."
    codesign --force --timestamp --sign "$SIGNING_IDENTITY" \
        "$APP/Contents/Frameworks/libbuttre_platform.dylib"
    codesign --force --timestamp --sign "$SIGNING_IDENTITY" \
        --options runtime "$APP/Contents/Helpers/buttre-config"
    codesign --force --timestamp --sign "$SIGNING_IDENTITY" \
        --options runtime "$APP"
    if [ -z "$NOTARY_PROFILE" ]; then
        echo "WARNING: Developer ID build is not notarized; macOS may reject it as an Input Source." >&2
        echo "Set MACOS_NOTARY_PROFILE to a notarytool keychain profile." >&2
    fi
fi

echo "==> Verifying..."
lipo -info "$APP/Contents/MacOS/buttre"
lipo -info "$APP/Contents/Helpers/buttre-config"
codesign --verify --deep --strict --verbose=2 "$APP"

ARCHIVE="$BUILD_DIR/buttre-${VERSION}-macos.zip"
# The zip ships install.sh next to Buttre.app: copy + enable Telex/VNI/Nôm.
cp "$HOST_DIR/install.sh" "$BUILD_DIR/install.sh"
chmod +x "$BUILD_DIR/install.sh"
echo "==> Zipping..."
rm -f "$ARCHIVE"
( cd "$BUILD_DIR" && zip -qry "buttre-${VERSION}-macos.zip" "Buttre.app" "install.sh" )

if [ -n "$NOTARY_PROFILE" ]; then
    if [ "$SIGNING_IDENTITY" = "-" ]; then
        echo "MACOS_NOTARY_PROFILE requires Developer ID signing" >&2
        exit 1
    fi
    echo "==> Notarizing..."
    NOTARY_ARGS=(--keychain-profile "$NOTARY_PROFILE")
    if [ -n "$NOTARY_KEYCHAIN" ]; then
        NOTARY_ARGS+=(--keychain "$NOTARY_KEYCHAIN")
    fi
    xcrun notarytool submit "$ARCHIVE" "${NOTARY_ARGS[@]}" --wait
    xcrun stapler staple "$APP"
    xcrun stapler validate "$APP"
    spctl --assess --type execute --verbose=2 "$APP"
    rm -f "$ARCHIVE"
    ( cd "$BUILD_DIR" && zip -qry "buttre-${VERSION}-macos.zip" "Buttre.app" "install.sh" )
fi

echo ""
echo "Artifact: $ARCHIVE"
echo "Install:  unzip, then run ./install.sh as your user (copies to /Library/Input Methods"
echo "          and enables buttre Telex, VNI, Chữ Nôm in the input menu)."
