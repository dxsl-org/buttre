#!/bin/bash
# IBus integration smoke test (CI + local).
#
# Proves the WHOLE chain a green unit-test run cannot: component
# registration with a real ibus-daemon, daemon-spawned engine, CreateEngine
# routing, and typed end-to-end scenarios through a real InputContext
# (scripts/test-ibus-scenarios.py, 7 scenarios incl. live method switch).
#
# Requirements: ibus, dbus-x11, python3-gi, gir1.2-ibus-1.0, and a built
# buttre binary (BUTTRE_BIN, default target/debug/buttre). Run under
# `xvfb-run -a` on displayless machines — ibus-daemon's UI helpers want X.
set -u
BIN="${BUTTRE_BIN:-target/debug/buttre}"
FAIL=0

say() { echo "[ibus-smoke] $*"; }
die() { say "FAIL: $*"; exit 1; }

[ -x "$BIN" ] || die "buttre binary not found at $BIN"

# --- user-level component registration (no sudo) ---
COMP_DIR="$HOME/.local/share/ibus/component"
mkdir -p "$COMP_DIR"
sed "s|<exec>/usr/bin/buttre --ibus</exec>|<exec>$PWD/$BIN --ibus</exec>|" \
    installers/linux/buttre.xml > "$COMP_DIR/buttre.xml"
# Registry cache must be invalidated to pick up component XML changes.
rm -f ~/.cache/ibus/bus/registry*
export IBUS_COMPONENT_PATH=/usr/share/ibus/component:$COMP_DIR

# --- session bus + daemon ---
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    eval "$(dbus-launch --sh-syntax)"
fi
pkill -f "buttre --ibus" 2>/dev/null
export RUST_LOG=info
ibus-daemon -dr 2>/dev/null
for _ in $(seq 1 15); do
    ibus list-engine >/dev/null 2>&1 && break
    sleep 1
done

# --- [1] engine visible in the registry ---
if ibus list-engine 2>/dev/null | grep -q "buttre"; then
    say "PASS: engine listed in registry"
else
    say "registry dump:"; ibus list-engine 2>&1 | head -20
    die "engine missing from ibus registry"
fi

# --- [2] activation spawns the component and routes to it ---
ibus engine buttre 2>/dev/null || true   # exit code unreliable (setxkbmap)
sleep 1
ACTIVE=$(ibus engine 2>/dev/null || true)
if [ "$ACTIVE" = "buttre" ]; then
    say "PASS: buttre is the active engine"
else
    die "active engine is '$ACTIVE', expected 'buttre'"
fi
if pgrep -f "buttre --ibus" >/dev/null; then
    say "PASS: daemon spawned the component process"
else
    die "component process not running"
fi

# --- [3] typed end-to-end scenarios ---
if python3 scripts/test-ibus-scenarios.py; then
    say "PASS: typed scenarios"
else
    FAIL=1
    say "scenario failures above"
fi

pkill -f "buttre --ibus" 2>/dev/null
if [ "$FAIL" -ne 0 ]; then
    die "typed scenarios failed"
fi
say "ALL PASS"
