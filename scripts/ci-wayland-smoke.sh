#!/bin/bash
# Wayland-native IME smoke test (CI + local) under headless sway.
#
# Asserts the protocol-level contract of `buttre --ime`:
#   [1] binds zwp_input_method_v2 on a compositor that offers it and waits
#       for text-input activation;
#   [2] a second instance receives Unavailable and falls back to IBus;
#   [3] without WAYLAND_DISPLAY the auto-detect goes straight to IBus.
# Typed input through the grab needs a focused text-input client — that is
# the interactive test / real-hardware territory, not this smoke.
#
# Requirements: sway, and a built buttre binary (BUTTRE_BIN, default
# target/debug/buttre).
set -u
BIN="${BUTTRE_BIN:-target/debug/buttre}"

say() { echo "[wayland-smoke] $*"; }
die() { say "FAIL: $*"; exit 1; }

[ -x "$BIN" ] || die "buttre binary not found at $BIN"
command -v sway >/dev/null || die "sway not installed"

pkill -x sway 2>/dev/null
pkill -f "buttre --ime" 2>/dev/null
pkill -f "buttre --ibus" 2>/dev/null   # a leftover engine owning the IBus name breaks step [2]/[3]
sleep 0.5

# A PRIVATE runtime dir, recreated fresh every run: stale sockets/locks
# from crashed compositors make sway startup flaky, and sharing the session
# dir risks colliding with a real desktop's sockets (WSLg's wayland-0, …).
export XDG_RUNTIME_DIR=/tmp/buttre-wayland-smoke-runtime
rm -rf "$XDG_RUNTIME_DIR"
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"

WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
    sway --config /dev/null > /tmp/sway-smoke.log 2>&1 &
SWAY_PID=$!
SOCK=""
for _ in $(seq 1 10); do
    SOCK=$(ls "$XDG_RUNTIME_DIR" 2>/dev/null | grep -E "^wayland-[0-9]+$" | tail -1)
    [ -n "$SOCK" ] && break
    sleep 1
done
[ -n "$SOCK" ] || { tail -10 /tmp/sway-smoke.log; die "headless sway produced no socket"; }
say "sway up (socket $SOCK)"

# --- [1] binding + activation wait ---
WAYLAND_DISPLAY="$SOCK" RUST_LOG=info "$BIN" --ime > /tmp/buttre-ime-smoke.log 2>&1 &
IME_PID=$!
sleep 2
kill -0 $IME_PID 2>/dev/null || { cat /tmp/buttre-ime-smoke.log; die "buttre --ime exited"; }
grep -q "Wayland input method registered" /tmp/buttre-ime-smoke.log \
    || { cat /tmp/buttre-ime-smoke.log; die "registration log line missing"; }
say "PASS: binds zwp_input_method_v2 and waits for activation"

# --- [2] seat already owned -> Unavailable -> IBus fallback ---
WAYLAND_DISPLAY="$SOCK" RUST_LOG=info timeout 5 "$BIN" --ime > /tmp/buttre-ime-smoke2.log 2>&1
grep -q "falling back to IBus" /tmp/buttre-ime-smoke2.log \
    || { cat /tmp/buttre-ime-smoke2.log; die "Unavailable -> IBus fallback missing"; }
say "PASS: second instance falls back to IBus"

# --- [3] no WAYLAND_DISPLAY -> IBus path directly ---
env -u WAYLAND_DISPLAY RUST_LOG=info timeout 5 "$BIN" --ime > /tmp/buttre-ime-smoke3.log 2>&1
grep -q "No WAYLAND_DISPLAY; using the IBus backend" /tmp/buttre-ime-smoke3.log \
    || { cat /tmp/buttre-ime-smoke3.log; die "no-display IBus path missing"; }
say "PASS: no WAYLAND_DISPLAY routes to IBus"

kill $IME_PID $SWAY_PID 2>/dev/null
say "ALL PASS"
