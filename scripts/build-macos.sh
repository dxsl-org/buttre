#!/bin/bash
# build-macos.sh
# Dev build of the buttre macOS dylib (host architecture only).
# For the universal (arm64 + x86_64) release artifact, use:
#   installers/macos/build_dylib.sh <version>

set -e

echo "🍎 Building buttre macOS dylib..."

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

cargo build -p buttre-platform --lib --release

DYLIB="$PROJECT_ROOT/target/release/libbuttre_platform.dylib"
if [ ! -f "$DYLIB" ]; then
    echo "❌ Build did not produce $DYLIB"
    exit 1
fi

echo "✅ Build complete!"
echo "📍 Dylib: $DYLIB"
echo ""
echo "📝 Next steps:"
echo "1. Universal release ZIP: installers/macos/build_dylib.sh <version>"
echo "2. Integration notes: installers/macos/ARTIFACT_README.md"
