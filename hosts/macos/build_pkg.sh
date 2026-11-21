#!/usr/bin/env bash
# Build the buttre macOS installer (buttre-<version>-macos.pkg) from the
# Buttre.app that build_app.sh produced. Runs on macOS only, after
# build_app.sh (which signs, notarizes and staples the app the pkg carries).
# Design: docs/macos-pkg-installer-design.md.
#
# Layout: one product archive (productbuild + Distribution XML) holding
#   buttre-app.pkg  Buttre.app → /Library/Input Methods (preinstall: stop the
#                   running copy, remove old copies; postinstall: restart the
#                   user's imklaunchagent so the new copy types without logout)
#   kb-<id>.pkg     script-only, one per input mode the app declares: its
#                   postinstall runs `buttre --install <id>` in the logged-in
#                   user's session. Installer.app's Customize shows one box per
#                   keyboard — Telex/VNI/Nôm selected, bundled keyboards not.
#
# Signing: MACOS_INSTALLER_IDENTITY (a "Developer ID Installer" identity —
# not the Application one build_app.sh uses), else the first one in the
# keychain, else an unsigned pkg for local testing. MACOS_NOTARY_PROFILE /
# MACOS_NOTARY_KEYCHAIN as in build_app.sh notarize + staple the pkg.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

VERSION="${1:-dev}"
HOST_DIR="hosts/macos"
BUILD_DIR="target/macos-app"
APP="$BUILD_DIR/Buttre.app"
PKG_DIR="$BUILD_DIR/pkg"
PRODUCT="$BUILD_DIR/buttre-${VERSION}-macos.pkg"
# Receipt ids: org.dxsl.buttre.app, org.dxsl.buttre.kb.<id>.
PKG_ID_PREFIX="org.dxsl.buttre"
# Selected by default under Customize — the methods a bare `buttre --install`
# enables (main.m primary_methods) and the MSI preselects.
DEFAULT_KEYBOARDS="telex vni nom"
INSTALLER_IDENTITY="${MACOS_INSTALLER_IDENTITY:-}"
NOTARY_PROFILE="${MACOS_NOTARY_PROFILE:-}"
NOTARY_KEYCHAIN="${MACOS_NOTARY_KEYCHAIN:-}"

if [ ! -d "$APP" ]; then
    echo "$APP not found — run hosts/macos/build_app.sh $VERSION first" >&2
    exit 1
fi
if [ -z "$INSTALLER_IDENTITY" ]; then
    # Installer identities are not code-signing ones: list without -p codesigning.
    while IFS= read -r identity; do
        if [[ "$identity" =~ \"(Developer\ ID\ Installer:.*)\" ]]; then
            INSTALLER_IDENTITY="${BASH_REMATCH[1]}"
            break
        fi
    done < <(security find-identity -v)
fi
if [ -n "$NOTARY_PROFILE" ] && [ -z "$INSTALLER_IDENTITY" ]; then
    echo "MACOS_NOTARY_PROFILE requires a Developer ID Installer identity" >&2
    exit 1
fi

PLIST="$APP/Contents/Info.plist"
plist_get() {  # <plist> <PlistBuddy key path>
    /usr/libexec/PlistBuddy -c "Print $2" "$1"
}
BUNDLE_ID="$(plist_get "$PLIST" :CFBundleIdentifier)"
PKG_VERSION="$(plist_get "$PLIST" :CFBundleShortVersionString)"

xml_escape() {
    printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g'
}

rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/packages"

# Display names come from the built bundle's Vietnamese InfoPlist.strings
# ("<mode id>" = "buttre <name>"), the same strings the input menu shows.
# Converted to XML so PlistBuddy can address the dotted mode-id keys.
NAMES="$PKG_DIR/names.plist"
plutil -convert xml1 -o "$NAMES" "$APP/Contents/Resources/vi.lproj/InfoPlist.strings"

echo "==> Packaging Buttre.app..."
ROOT="$PKG_DIR/root"
mkdir -p "$ROOT" "$PKG_DIR/app-scripts"
ditto "$APP" "$ROOT/Buttre.app"
cp "$HOST_DIR/pkg/app-preinstall" "$PKG_DIR/app-scripts/preinstall"
cp "$HOST_DIR/pkg/app-postinstall" "$PKG_DIR/app-scripts/postinstall"
chmod 755 "$PKG_DIR/app-scripts/preinstall" "$PKG_DIR/app-scripts/postinstall"
# Not relocatable: otherwise Installer "upgrades" whichever Buttre.app with
# this bundle id it finds elsewhere on disk (a build dir, ~/Library) instead
# of installing into /Library/Input Methods.
# Not version-checked: with a newer CFBundleVersion already installed,
# PackageKit decides at planning time to skip the payload — then preinstall
# removes the old bundle and the Mac is left with no Buttre.app at all
# (observed: 0.0.0 dev pkg over 0.8.0). preinstall owns replacing the bundle.
COMPONENTS="$PKG_DIR/app-component.plist"
pkgbuild --analyze --root "$ROOT" "$COMPONENTS" >/dev/null
/usr/libexec/PlistBuddy -c "Set :0:BundleIsRelocatable false" "$COMPONENTS"
/usr/libexec/PlistBuddy -c "Set :0:BundleIsVersionChecked false" "$COMPONENTS"
pkgbuild --root "$ROOT" --component-plist "$COMPONENTS" \
    --identifier "$PKG_ID_PREFIX.app" --version "$PKG_VERSION" \
    --install-location "/Library/Input Methods" \
    --scripts "$PKG_DIR/app-scripts" \
    "$PKG_DIR/packages/buttre-app.pkg"

echo "==> Packaging one keyboard choice per input mode..."
OUTLINE=""
CHOICES=""
REFS=""
MODES=":ComponentInputModeDict:tsVisibleInputModeOrderedArrayKey"
INDEX=0
while MODE_ID="$(plist_get "$PLIST" "$MODES:$INDEX" 2>/dev/null)"; do
    INDEX=$((INDEX + 1))
    ID="${MODE_ID#"$BUNDLE_ID".}"
    # The id lands in a shell script, a pkg identifier and XML ids.
    if [[ ! "$ID" =~ ^[a-z0-9_]+$ ]]; then
        echo "Unexpected input mode id '$MODE_ID'" >&2
        exit 1
    fi
    NAME="$(plist_get "$NAMES" ":$MODE_ID" 2>/dev/null || true)"
    NAME="${NAME#buttre }"
    [ -n "$NAME" ] || NAME="$ID"

    SCRIPTS="$PKG_DIR/kb-$ID-scripts"
    mkdir -p "$SCRIPTS"
    sed "s/@MODE@/$ID/" "$HOST_DIR/pkg/kb-postinstall" > "$SCRIPTS/postinstall"
    chmod 755 "$SCRIPTS/postinstall"
    pkgbuild --nopayload --scripts "$SCRIPTS" \
        --identifier "$PKG_ID_PREFIX.kb.$ID" --version "$PKG_VERSION" \
        "$PKG_DIR/packages/kb-$ID.pkg"

    SELECTED="false"
    case " $DEFAULT_KEYBOARDS " in *" $ID "*) SELECTED="true" ;; esac
    OUTLINE+="            <line choice=\"kb.$ID\"/>"$'\n'
    CHOICES+="    <choice id=\"kb.$ID\" title=\"$(xml_escape "$NAME")\" start_selected=\"$SELECTED\">"$'\n'
    CHOICES+="        <pkg-ref id=\"$PKG_ID_PREFIX.kb.$ID\"/>"$'\n'
    CHOICES+="    </choice>"$'\n'
    REFS+="    <pkg-ref id=\"$PKG_ID_PREFIX.kb.$ID\">kb-$ID.pkg</pkg-ref>"$'\n'
    echo "  kb.$ID = $NAME (selected: $SELECTED)"
done
if [ "$INDEX" -eq 0 ]; then
    echo "No input modes in $PLIST ($MODES)" >&2
    exit 1
fi

echo "==> Writing Distribution XML..."
# Packages install in choices-outline order: the app before any kb.* script.
DISTRIBUTION="$PKG_DIR/Distribution.xml"
cat > "$DISTRIBUTION" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>buttre</title>
    <options customize="always" require-scripts="false" hostArchitectures="arm64,x86_64"/>
    <domains enable_localSystem="true" enable_currentUserHome="false" enable_anywhere="false"/>
    <allowed-os-versions><os-version min="11.0"/></allowed-os-versions>

    <choices-outline>
        <line choice="app"/>
        <line choice="keyboards">
${OUTLINE}        </line>
    </choices-outline>

    <choice id="app" title="buttre" enabled="false" selected="true">
        <pkg-ref id="$PKG_ID_PREFIX.app"/>
    </choice>
    <choice id="keyboards" title="Bàn phím thêm vào macOS"/>
${CHOICES}
    <pkg-ref id="$PKG_ID_PREFIX.app">buttre-app.pkg</pkg-ref>
${REFS}</installer-gui-script>
EOF
xmllint --noout "$DISTRIBUTION"

echo "==> Building product archive..."
rm -f "$PRODUCT"
PRODUCT_ARGS=(--distribution "$DISTRIBUTION" --package-path "$PKG_DIR/packages")
if [ -n "$INSTALLER_IDENTITY" ]; then
    echo "    signing with $INSTALLER_IDENTITY"
    PRODUCT_ARGS+=(--sign "$INSTALLER_IDENTITY" --timestamp)
else
    echo "WARNING: no Developer ID Installer identity — unsigned pkg, Gatekeeper will block it." >&2
    echo "Local testing only; set MACOS_INSTALLER_IDENTITY for a release build." >&2
fi
productbuild "${PRODUCT_ARGS[@]}" "$PRODUCT"

if [ -n "$NOTARY_PROFILE" ]; then
    echo "==> Notarizing..."
    NOTARY_ARGS=(--keychain-profile "$NOTARY_PROFILE")
    if [ -n "$NOTARY_KEYCHAIN" ]; then
        NOTARY_ARGS+=(--keychain "$NOTARY_KEYCHAIN")
    fi
    xcrun notarytool submit "$PRODUCT" "${NOTARY_ARGS[@]}" --wait
    xcrun stapler staple "$PRODUCT"
    xcrun stapler validate "$PRODUCT"
    spctl --assess --type install --verbose=2 "$PRODUCT"
elif [ -n "$INSTALLER_IDENTITY" ]; then
    echo "WARNING: signed but not notarized; Gatekeeper may still warn." >&2
fi

if [ -n "$INSTALLER_IDENTITY" ]; then
    pkgutil --check-signature "$PRODUCT"
fi

echo ""
echo "Artifact: $PRODUCT"
echo "Install:  double-click it; Customize picks the keyboards to add."
