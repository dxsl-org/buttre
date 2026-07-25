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

echo ""
echo "Artifacts:"
ls -lh target/debian/*.deb target/generate-rpm/*.rpm
