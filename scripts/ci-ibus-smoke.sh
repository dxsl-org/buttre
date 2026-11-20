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

# --- diagnostics (printed on every exit, pass or fail) ---
# The engine logs to $XDG_CACHE_HOME/buttre/engine.log and TRUNCATES it per
# process start, so the spawn wrapper below rotates the previous log aside:
# every engine process the daemon started this run keeps its own file.
ENGINE_LOG_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/buttre"
rm -f "$ENGINE_LOG_DIR"/engine.log* "$ENGINE_LOG_DIR/spawns"
dump_diagnostics() {
    say "--- engine processes spawned: $(grep -c . "$ENGINE_LOG_DIR/spawns" 2>/dev/null || echo 0)"
    cat "$ENGINE_LOG_DIR/spawns" 2>/dev/null
    for f in "$ENGINE_LOG_DIR"/engine.log*; do
        [ -f "$f" ] || continue
        say "--- $f"
        cat "$f"
    done
    say "--- shared state"
    for f in "$HOME/.config/buttre/method" "$HOME/.config/buttre/enabled" \
             "$HOME/.local/share/buttre/settings.toml"; do
        if [ -f "$f" ]; then say "$f:"; cat "$f"; echo; else say "$f: (absent)"; fi
    done
    say "--- global engine: $(ibus engine 2>/dev/null || echo '?')"
}
trap dump_diagnostics EXIT

[ -x "$BIN" ] || die "buttre binary not found at $BIN"

# --- user-level component registration (no sudo) ---
COMP_DIR="$HOME/.local/share/ibus/component"
mkdir -p "$COMP_DIR"
# The component execs a wrapper that rotates the previous engine log and
# records the spawn time, then becomes the real engine (same argv/env).
mkdir -p "$ENGINE_LOG_DIR"
WRAPPER="$COMP_DIR/buttre-ibus-ci-wrapper.sh"
cat > "$WRAPPER" <<EOF
#!/bin/sh
LOG="$ENGINE_LOG_DIR/engine.log"
[ -f "\$LOG" ] && mv "\$LOG" "\$LOG.\$(date +%s.%N)"
echo "\$(date +%T.%N) pid=\$\$" >> "$ENGINE_LOG_DIR/spawns"
exec "$PWD/$BIN" "\$@"
EOF
chmod +x "$WRAPPER"
sed "s|<exec>/usr/bin/buttre --ibus</exec>|<exec>$WRAPPER --ibus</exec>|" \
    installers/linux/buttre.xml > "$COMP_DIR/buttre.xml"
# Registry cache must be invalidated to pick up component XML changes.
rm -f ~/.cache/ibus/bus/registry*
export IBUS_COMPONENT_PATH=/usr/share/ibus/component:$COMP_DIR

# --- session bus + daemon ---
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    eval "$(dbus-launch --sh-syntax)"
fi
pkill -f "buttre --ibus" 2>/dev/null
# DEBUG for buttre's own crates: every ProcessKeyEvent with its engine path,
# outcome and ops lands in the engine log dumped above.
export RUST_LOG="info,buttre=debug,buttre_platform=debug"
# No panel: nothing here tests it, and a starting panel is the likely source
# of the default-engine reset handled in [2].
ibus-daemon -dr --panel=disable 2>/dev/null
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
# On-demand component spawn (ibus-daemon forks `buttre --ibus` on first
# CreateEngine) plus its own startup work can take longer than one second on
# a loaded CI runner, so this polls.
#
# It also waits for the switch to STICK. CI logs (flaky runs) showed the
# daemon side resetting the global engine to xkb:us::eng some seconds after
# startup — FocusOut, Disable, Destroy on the buttre engine with no client
# asking — sometimes before the scenarios start (harmless), sometimes in the
# middle of them (every later key goes to xkb: 0/7). buttre must stay the
# active engine for STABLE_SECS consecutive polls before any key is typed; a
# reset during the wait re-issues the switch and restarts the count.
STABLE_SECS=6
STABLE=0
ACTIVE=""
for _ in $(seq 1 30); do
    ACTIVE=$(ibus engine 2>/dev/null || true)
    if [ "$ACTIVE" = "buttre" ]; then
        STABLE=$((STABLE + 1))
        [ "$STABLE" -ge "$STABLE_SECS" ] && break
    else
        STABLE=0
        ibus engine buttre 2>/dev/null || true   # exit code unreliable (setxkbmap)
    fi
    sleep 1
done
if [ "$STABLE" -ge "$STABLE_SECS" ]; then
    say "PASS: buttre is the active engine (stable ${STABLE_SECS}s)"
else
    die "active engine is '$ACTIVE', expected 'buttre' to stay active ${STABLE_SECS}s"
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
