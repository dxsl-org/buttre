#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# Redirect the target dir off vboxsf when needed (no-op on a native disk).
# cargo-deb / cargo-generate-rpm resolve the target via `cargo metadata`, so
# they honor this too and read the binary from the redirected location.
# shellcheck source=scripts/lib/vboxsf-target-dir.sh
. "$REPO_ROOT/scripts/lib/vboxsf-target-dir.sh"

echo "==> Building buttre-platform release..."
cargo build -p buttre-platform --release

echo "==> Installing packaging tools (skipped if already present)..."
cargo install cargo-deb --locked --version "^2.7" 2>/dev/null || true
cargo install cargo-generate-rpm --locked --version "^0.14" 2>/dev/null || true

echo "==> Building .deb..."
# --no-build: binary already compiled above; run from workspace root so relative paths in [package.metadata.deb] resolve.
cargo deb --package buttre-platform --no-build --output target/debian/

echo "==> Building .rpm..."
cargo generate-rpm --package crates/buttre-platform

# ── fcitx5-buttre addon .deb (separate package, like every distro fcitx5
# addon) ─ built only when the fcitx5 dev stack is present; otherwise the
# main packages still ship and fcitx5 users fall back to source install
# (addons/fcitx5-buttre/CMakeLists.txt header). DEB only: the addon links
# the builder's libFcitx5Core C++ ABI, so this binary is Debian/Ubuntu-
# family — RPM users compile from source.
if [ -f /usr/lib/x86_64-linux-gnu/cmake/Fcitx5Core/Fcitx5CoreConfig.cmake ] \
   || [ -f /usr/lib/cmake/Fcitx5Core/Fcitx5CoreConfig.cmake ] \
   || [ -f /usr/lib64/cmake/Fcitx5Core/Fcitx5CoreConfig.cmake ]; then
    echo "==> Building fcitx5-buttre addon .deb..."
    # Failure-tolerant on purpose: the addon is an OPTIONAL companion
    # package — a compile break here must never block the main buttre
    # .deb/.rpm from releasing. CI has a dedicated addon job that fails
    # loudly on PRs, which is where breakage gets caught.
    if ! (
        set -e
        cargo build -p buttre-ffi --release
        TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
        VERSION="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; m=json.load(sys.stdin); print(next(p["version"] for p in m["packages"] if p["name"]=="buttre-platform"))')"
        cmake -S addons/fcitx5-buttre -B "$TARGET_DIR/fcitx5-addon" \
            -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_INSTALL_PREFIX=/usr \
            -DBUTTRE_PACKAGED=ON \
            -DBUTTRE_FFI_LIB="$TARGET_DIR/release/libbuttre_ffi.so" \
            -DCPACK_PACKAGE_VERSION="$VERSION"
        cmake --build "$TARGET_DIR/fcitx5-addon" -j"$(nproc)"
        # -B into the repo-relative target/debian, next to cargo-deb's
        # output, so the release workflow picks both .debs up from one dir.
        cd "$TARGET_DIR/fcitx5-addon" && cpack -G DEB -B "$REPO_ROOT/target/debian"
    ); then
        echo "==> WARN: fcitx5-buttre addon failed to compile/package — main packages unaffected"
    fi
else
    echo "==> Skipping fcitx5-buttre addon (libfcitx5core-dev/ECM not installed)"
fi

echo ""
echo "Artifacts:"
ls -lh target/debian/*.deb target/generate-rpm/*.rpm 2>/dev/null || true
