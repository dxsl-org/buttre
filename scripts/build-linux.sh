#!/bin/bash
# build-linux.sh
# Dev build of the buttre Linux artifacts (tray app + config window).
#
# Why this wrapper exists: VirtualBox shared folders (vboxsf) do not persist
# some build-script outputs — notably serde_core's generated `private.rs` —
# so `cargo build` from a vboxsf-mounted checkout fails with a missing OUT_DIR
# file. When the repo lives on vboxsf this script redirects CARGO_TARGET_DIR to
# a local filesystem for THIS invocation only; it never edits the shared cargo
# config, so a normal on-disk checkout is unaffected (the redirect is skipped).
#
# Usage:
#   scripts/build-linux.sh                  # debug build of the Linux crates
#   scripts/build-linux.sh test             # forwards to `cargo test`
#   scripts/build-linux.sh clippy --workspace
#   BUTTRE_TARGET_DIR=/mnt/ssd/bt scripts/build-linux.sh --release
#
# Override the redirect target with BUTTRE_TARGET_DIR (e.g. a persistent local
# SSD path). The default is a RAM-backed tmpfs — fast, but cleared on reboot,
# so the first build of each session recompiles from scratch.

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# vboxsf reports statfs magic 0x786f4256 ("VBox"); a native filesystem (ext4,
# btrfs, …) reports anything else. Only redirect when we must, so this stays a
# transparent `cargo` wrapper everywhere else.
FS_MAGIC="$(stat -f -c '%t' . 2>/dev/null || echo '')"
if [ "$FS_MAGIC" = "786f4256" ]; then
    export CARGO_TARGET_DIR="${BUTTRE_TARGET_DIR:-/dev/shm/buttre-target}"
    echo "🔀 Repo is on a VirtualBox shared folder — building to CARGO_TARGET_DIR=$CARGO_TARGET_DIR" >&2
fi

# No args → debug-build the shippable Linux crates. Any args are forwarded
# verbatim to cargo (test, clippy, run, --release, -p …).
if [ "$#" -eq 0 ]; then
    set -- build -p buttre-platform -p buttre-config
fi

echo "🐧 cargo $*"
exec cargo "$@"
