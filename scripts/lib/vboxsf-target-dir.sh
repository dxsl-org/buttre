# shellcheck shell=bash
# Sourced helper — redirect CARGO_TARGET_DIR off a VirtualBox shared folder.
#
# vboxsf does not persist some build-script outputs (e.g. serde_core's
# generated private.rs), so `cargo build` from a vboxsf-mounted checkout fails
# with a missing OUT_DIR file. When the current directory is on vboxsf this
# points CARGO_TARGET_DIR at a local filesystem for the calling process only;
# it never edits the shared cargo config and is a no-op on native filesystems.
#
# Source this AFTER cd-ing to the repo root (it tests the current directory).
# Override the redirect location with BUTTRE_TARGET_DIR.
_buttre_fs_magic="$(stat -f -c '%t' . 2>/dev/null || echo '')"
if [ "$_buttre_fs_magic" = "786f4256" ]; then
    export CARGO_TARGET_DIR="${BUTTRE_TARGET_DIR:-/dev/shm/buttre-target}"
    echo "🔀 vboxsf checkout — using CARGO_TARGET_DIR=$CARGO_TARGET_DIR" >&2
fi
unset _buttre_fs_magic
