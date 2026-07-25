#!/usr/bin/env python3
"""IBus end-to-end scenario harness for the buttre engine (Linux).

Plays the role a GTK application does: creates a persistent IBus
InputContext, selects the buttre engine, drives real key events through
ibus-daemon, and asserts on the preedit/commit signals coming back.

Prerequisites: ibus-daemon running with the buttre component registered
(see LINUX_README.md), python3-gi with the IBus typelib (ships with the
`ibus` package on Debian/Ubuntu).

Usage:
    python3 scripts/test-ibus-scenarios.py

Exit code 0 = all scenarios pass. Used manually today; wired into the CI
ibus-smoke job in the ci workflow (plan phase 5).
"""
import os
import sys
import time
import gi
gi.require_version("IBus", "1.0")
from gi.repository import IBus, GLib

T0 = time.monotonic()

IBUS_RELEASE_MASK = 1 << 30
CONTROL_MASK = 1 << 2

IBus.init()
bus = IBus.Bus()
if not bus.is_connected():
    print("FAIL: cannot connect to ibus-daemon")
    sys.exit(1)

ic = bus.create_input_context("buttre-scenarios")
ic.set_capabilities(IBus.Capabilite.PREEDIT_TEXT | IBus.Capabilite.FOCUS)

events = []

def on_preedit(_ic, text, cursor, visible):
    events.append(("preedit", text.get_text(), visible))
    print(f"[+{time.monotonic()-T0:6.3f}s] preedit {text.get_text()!r} visible={visible}", file=sys.stderr)

def on_commit(_ic, text):
    events.append(("commit", text.get_text()))
    print(f"[+{time.monotonic()-T0:6.3f}s] commit {text.get_text()!r}", file=sys.stderr)

ic.connect("update-preedit-text", on_preedit)
ic.connect("commit-text", on_commit)

ic.focus_in()
ic.set_engine("buttre")

loop = GLib.MainLoop()

def pump(ms=150):
    GLib.timeout_add(ms, loop.quit)
    loop.run()

def key(keyval, state=0):
    print(f"[+{time.monotonic()-T0:6.3f}s] key {keyval:#x} state={state:#x}", file=sys.stderr)
    handled = ic.process_key_event(keyval, 0, state)
    # 120ms is plenty on a bare Linux box (verified: 6/6 clean runs locally,
    # including under real Xvfb matching CI's setup exactly, even pinned to
    # a single CPU core). GitHub Actions' shared runners have reproduced a
    # failure that 300ms didn't fix — root cause not yet confirmed, so this
    # run also logs a per-event timeline (stderr) to diagnose from the CI
    # log directly rather than guessing at another margin blindly.
    pump(1000 if os.environ.get("CI") else 120)
    return handled

def type_str(s):
    return [key(ord(c)) for c in s]

def commits():
    return [e[1] for e in events if e[0] == "commit"]

def visible_preedits():
    return [e[1] for e in events if e[0] == "preedit" and e[2]]

pump(800)  # let SetEngine settle (daemon spawns/attaches the engine)
results = []

# --- Scenario A: telex word + space commits the composed word ---
events.clear()
type_str("vieejt")
h_space = key(0x20)
ok = commits() == ["việt"] and visible_preedits()[-1] == "việt" and h_space is False
results.append(("A telex+space -> việt, space passes", ok, commits(), visible_preedits()[-1:]))

# --- Scenario B: punctuation commits the word and passes through ---
events.clear()
type_str("xin")
h_dot = key(ord("."))
ok = commits() == ["xin"] and h_dot is False
results.append(("B punctuation '.' commits word + passes", ok, commits(), h_dot))

# --- Scenario C: backspace shrinks the preedit (engine-canonical) ---
events.clear()
type_str("hoaf")
before = visible_preedits()[-1] if visible_preedits() else None
h_bs = key(0xFF08)
preedits_after_bs = [e for e in events if e[0] == "preedit"]
# Guarded (not a bare [-1] index): a prior CI-only failure hit zero preedit
# events here with no local repro (6/6 clean runs, incl. real Xvfb and a
# single-pinned CPU core) — surface full diagnostics instead of crashing so
# every later scenario (D-G) still runs and reports in the same CI attempt.
after = preedits_after_bs[-1][1] if preedits_after_bs else None
ok = (
    before == "hòa"
    and h_bs is True
    and after is not None
    and before is not None
    and len(after) < len(before)
    and commits() == []
)
results.append(
    ("C backspace shrinks preedit, no commit", ok, before, after, events if after is None else None)
)
key(0xFF1B)  # escape -> commit pending, clean state
events.clear()

# --- Scenario D: key RELEASE events are ignored ---
events.clear()
h_press = key(ord("a"))
h_release = key(ord("a"), IBUS_RELEASE_MASK)
pre = visible_preedits()
ok = h_press is True and h_release is False and pre[-1] == "a" and len(pre) == 1
results.append(("D release filtered (no double-processing)", ok, h_press, h_release, pre))
key(0xFF1B); events.clear()

# --- Scenario E: Ctrl combo commits pending word, combo passes through ---
events.clear()
type_str("em")
h_ctrl = key(ord("c"), CONTROL_MASK)
ok = commits() == ["em"] and h_ctrl is False
results.append(("E Ctrl+C commits pending + passes", ok, commits(), h_ctrl))

# --- Scenario F: Enter commits word then passes ---
events.clear()
type_str("chaof")
h_enter = key(0xFF0D)
ok = commits() == ["chào"] and h_enter is False
results.append(("F Enter commits 'chào' + passes", ok, commits(), h_enter))

# --- Scenario G (B5): tray-side method switch applies to the live engine ---
import pathlib
method_file = pathlib.Path(os.path.expanduser("~/.config/buttre/method"))
method_file.parent.mkdir(parents=True, exist_ok=True)

def set_method(name):
    tmp = method_file.parent / ".method.tmp"
    tmp.write_text(name)
    tmp.rename(method_file)  # atomic, same as the tray writer
    pump(1200)  # give the watcher time to fire

original = method_file.read_text().strip() if method_file.exists() else "telex"
set_method("vni")
events.clear()
type_str("viet65")
key(0x20)
vni_commits = commits()
set_method(original if original in ("telex", "vni", "nom") else "telex")
ok = vni_commits == ["việt"]
results.append(("G live method switch telex->vni (viet65 -> việt)", ok, vni_commits))

print("--- scenarios ---")
fails = 0
for r in results:
    status = "PASS" if r[1] else "FAIL"
    if not r[1]:
        fails += 1
    print(f"{status}: {r[0]}  detail={r[2:]}")

print(f"RESULT: {len(results)-fails}/{len(results)} scenarios passed")
sys.exit(1 if fails else 0)
