//! Composition-semantics tests for the shared EngineBridge (all platforms).
//!
//! Pure — a real `Keyboard` in composition mode, no D-Bus/Wayland/FFI.
//! These mirror the end-to-end scenarios in `scripts/test-ibus-scenarios.py`
//! so a semantics regression fails in `cargo test` on ANY OS before it ever
//! reaches a bus. The same bridge drives the Linux backends and the macOS
//! FFI, so this suite pins composition behavior for both.

use buttre_core::state::macros::{MacroEntry, MacroFile, MacroStore};
use buttre_platform::shared::engine_bridge::{EngineBridge, ImeOp};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn type_chars(bridge: &mut EngineBridge, s: &str) -> Vec<ImeOp> {
    let mut ops = Vec::new();
    for ch in s.chars() {
        let outcome = bridge.process_char(ch);
        assert!(outcome.handled, "letter {ch:?} must be handled");
        ops.extend(outcome.ops);
    }
    ops
}

fn commits(ops: &[ImeOp]) -> Vec<String> {
    ops.iter()
        .filter_map(|op| match op {
            ImeOp::Commit(t) => Some(t.clone()),
            ImeOp::Preedit(_)
            | ImeOp::Candidates { .. }
            | ImeOp::HideCandidates
            | ImeOp::DeleteSurrounding(_) => None,
        })
        .collect()
}

#[test]
fn telex_word_builds_preedit_and_space_commits() {
    let mut bridge = EngineBridge::new("telex");
    let ops = type_chars(&mut bridge, "vieejt");
    assert_eq!(ops.last(), Some(&ImeOp::Preedit("việt".into())));

    let space = bridge.process_char(' ');
    assert!(!space.handled, "separator must pass through to the app");
    assert_eq!(commits(&space.ops), vec!["việt"]);
    // Preedit cleared BEFORE the commit so the word is never doubled.
    assert_eq!(space.ops.first(), Some(&ImeOp::Preedit(String::new())));
    assert_eq!(bridge.preedit(), "");
}

#[test]
fn punctuation_is_a_separator_too() {
    let mut bridge = EngineBridge::new("telex");
    type_chars(&mut bridge, "xin");
    let dot = bridge.process_char('.');
    assert!(!dot.handled);
    assert_eq!(commits(&dot.ops), vec!["xin"]);
}

#[test]
fn backspace_shrinks_preedit_without_commit() {
    let mut bridge = EngineBridge::new("telex");
    // Modern orthography: hoaf -> "hòa" (not "hoà").
    type_chars(&mut bridge, "hoaf");
    assert_eq!(bridge.preedit(), "hòa");

    let bs = bridge.backspace();
    assert!(bs.handled);
    assert!(commits(&bs.ops).is_empty());
    assert!(bridge.preedit().chars().count() < 3);
}

#[test]
fn backspace_with_no_composition_passes_through() {
    let mut bridge = EngineBridge::new("telex");
    let bs = bridge.backspace();
    assert!(!bs.handled);
    assert!(bs.ops.is_empty());
}

#[test]
fn digits_join_the_composition_in_telex() {
    // Engine-canonical: telex buffers digits like any raw char (same as the
    // Windows hook path); they commit unchanged at the next separator.
    let mut bridge = EngineBridge::new("telex");
    let five = bridge.process_char('5');
    assert!(five.handled);
    assert!(commits(&five.ops).is_empty());
    assert_eq!(bridge.preedit(), "5");

    let space = bridge.process_char(' ');
    assert!(!space.handled);
    assert_eq!(commits(&space.ops), vec!["5"]);
}

#[test]
fn vni_uses_digits_as_tone_keys() {
    let mut bridge = EngineBridge::new("vni");
    let ops = type_chars(&mut bridge, "viet65");
    assert_eq!(ops.last(), Some(&ImeOp::Preedit("việt".into())));
}

#[test]
fn flush_pending_commits_with_boundary_repair() {
    let mut bridge = EngineBridge::new("telex");
    type_chars(&mut bridge, "em");
    let flush = bridge.flush_pending();
    assert_eq!(commits(&flush.ops), vec!["em"]);
    assert_eq!(bridge.preedit(), "");
    // Second flush is a no-op.
    assert!(bridge.flush_pending().ops.is_empty());
}

#[test]
fn discard_clears_without_committing() {
    let mut bridge = EngineBridge::new("telex");
    type_chars(&mut bridge, "chaof");
    let discard = bridge.discard();
    assert!(commits(&discard.ops).is_empty());
    assert_eq!(discard.ops, vec![ImeOp::Preedit(String::new())]);
    assert_eq!(bridge.preedit(), "");
}

#[test]
fn rebuild_switches_method_and_clears_composition() {
    let mut bridge = EngineBridge::new("telex");
    type_chars(&mut bridge, "vie");
    let rebuilt = bridge.rebuild("vni").expect("vni must build");
    assert_eq!(rebuilt.ops, vec![ImeOp::Preedit(String::new())]);

    let ops = type_chars(&mut bridge, "viet65");
    assert_eq!(ops.last(), Some(&ImeOp::Preedit("việt".into())));
}

#[test]
fn enter_commits_word_and_passes_through() {
    let mut bridge = EngineBridge::new("telex");
    type_chars(&mut bridge, "chaof");
    let enter = bridge.process_char('\n');
    assert!(!enter.handled);
    assert_eq!(commits(&enter.ops), vec!["chào"]);
}

// ============================================================================
// Shorthand/gõ tắt wiring (phase-02: EngineBridge holds an injected store)
// ============================================================================

/// A store with a single enabled `vn` -> "Việt Nam" trigger.
fn vn_store() -> Arc<Mutex<MacroStore>> {
    let mut macros = HashMap::new();
    macros.insert(
        "vn".to_string(),
        MacroEntry {
            expand: "Việt Nam".to_string(),
            enabled: true,
        },
    );
    Arc::new(Mutex::new(MacroStore::from_file(MacroFile { macros })))
}

#[test]
fn injected_store_expands_on_separator_commit() {
    let mut bridge = EngineBridge::new_with_macros("telex", vn_store());
    type_chars(&mut bridge, "vn");
    let space = bridge.process_char(' ');
    assert!(!space.handled, "separator must pass through to the app");
    assert_eq!(commits(&space.ops), vec!["Việt Nam"]);
}

#[test]
fn injected_store_expands_on_flush_pending() {
    let mut bridge = EngineBridge::new_with_macros("telex", vn_store());
    type_chars(&mut bridge, "vn");
    let flush = bridge.flush_pending();
    assert_eq!(commits(&flush.ops), vec!["Việt Nam"]);
}

#[test]
fn injected_store_expands_on_enter() {
    let mut bridge = EngineBridge::new_with_macros("telex", vn_store());
    type_chars(&mut bridge, "vn");
    let enter = bridge.process_char('\n');
    assert!(!enter.handled);
    assert_eq!(commits(&enter.ops), vec!["Việt Nam"]);
}

#[test]
fn rebuild_reapplies_the_injected_store() {
    let mut bridge = EngineBridge::new_with_macros("telex", vn_store());
    bridge.rebuild("vni").expect("vni must build");
    type_chars(&mut bridge, "vn");
    let space = bridge.process_char(' ');
    assert_eq!(
        commits(&space.ops),
        vec!["Việt Nam"],
        "rebuild must re-attach the store to the fresh keyboard"
    );
}

#[test]
fn content_swap_to_empty_store_disables_expansion() {
    let store = vn_store();
    let mut bridge = EngineBridge::new_with_macros("telex", store.clone());
    // Content swap (the live reload model): same Arc, contents replaced.
    *store.lock().unwrap() = MacroStore::default();
    type_chars(&mut bridge, "vn");
    let space = bridge.process_char(' ');
    assert_eq!(
        commits(&space.ops),
        vec!["vn"],
        "an emptied store must fall through to plain composition"
    );
}

#[test]
fn no_store_injected_behaves_exactly_like_new() {
    // `new`/`try_new` with no store must stay byte-identical to today.
    let mut bridge = EngineBridge::new("telex");
    type_chars(&mut bridge, "vn");
    let space = bridge.process_char(' ');
    assert_eq!(commits(&space.ops), vec!["vn"]);
    assert!(EngineBridge::try_new("telex").is_some());
}

// ---------------------------------------------------------------------------
// No-preedit (commit-as-you-go) mode — Telex/VNI with composition turned off.
// ---------------------------------------------------------------------------

#[test]
fn default_bridge_uses_the_preedit_model() {
    // Constructors must stay composition=true so macOS/Wayland/Windows and the
    // IBus default are unchanged — the first key emits a Preedit op.
    let mut bridge = EngineBridge::new("telex");
    let out = bridge.process_char('v');
    assert!(
        out.ops.iter().any(|o| matches!(o, ImeOp::Preedit(_))),
        "default bridge must compose (emit preedit), got {:?}",
        out.ops
    );
}

#[test]
fn direct_mode_passes_plain_letter_then_replaces_on_tone() {
    let mut bridge = EngineBridge::new("telex");
    // Fresh bridge has nothing pending, so flipping emits nothing.
    assert!(bridge.set_use_composition(false).ops.is_empty());

    // A plain letter with no transform is a natural passthrough: the app
    // inserts it, the engine just tracks it (no ops, not handled).
    let a = bridge.process_char('a');
    assert!(!a.handled, "plain letter passes through in direct mode");
    assert!(a.ops.is_empty());

    // The Telex tone key rewrites the committed letter in place: delete 1, then
    // commit the toned form — NO preedit, so no underline ever appears.
    let s = bridge.process_char('s');
    assert!(s.handled);
    assert_eq!(
        s.ops,
        vec![ImeOp::DeleteSurrounding(1), ImeOp::Commit("á".to_string())]
    );
}

#[test]
fn direct_mode_separator_passes_through() {
    let mut bridge = EngineBridge::new("telex");
    bridge.set_use_composition(false);
    bridge.process_char('a');
    // Space is a separator; the word is already committed as-you-go, so nothing
    // to correct — it passes through to the app.
    let space = bridge.process_char(' ');
    assert!(!space.handled, "separator passes through in direct mode");
}

#[test]
fn mid_word_flip_to_direct_commits_pending_word() {
    // Flipping the model mid-composition must COMMIT the pending word (not drop
    // it) before switching, so no text is lost.
    let mut bridge = EngineBridge::new("telex");
    bridge.process_char('v');
    bridge.process_char('i'); // preedit "vi"
    let flip = bridge.set_use_composition(false);
    assert_eq!(
        commits(&flip.ops),
        vec!["vi"],
        "mid-word flip must commit the pending word, got {:?}",
        flip.ops
    );
}

#[test]
fn set_use_composition_is_a_noop_for_nom() {
    // Nôm always composes (its candidate popup needs the preedit); the toggle
    // must not flip it.
    let mut bridge = EngineBridge::new("nom");
    assert!(
        bridge.set_use_composition(false).ops.is_empty(),
        "Nôm must ignore the no-preedit toggle"
    );
    // Still composing: a keystroke yields a preedit, not a direct commit.
    let out = bridge.process_char('a');
    assert!(
        out.ops.iter().any(|o| matches!(o, ImeOp::Preedit(_)))
            || out.ops.iter().any(|o| matches!(o, ImeOp::Candidates { .. })),
        "Nôm stays in composition after the toggle, got {:?}",
        out.ops
    );
}

// ============================================================================
// English / passthrough (tắt bộ gõ — Unikey model, OS input source untouched)
// ============================================================================

#[test]
fn english_passthrough_lets_every_key_through_untouched() {
    let mut bridge = EngineBridge::new("english");
    for ch in ['v', 'i', 'e', 'j', 't', ' ', 'w', '\x08'] {
        let outcome = if ch == '\x08' {
            bridge.backspace()
        } else {
            bridge.process_char(ch)
        };
        assert!(!outcome.handled, "{ch:?} must pass through unhandled");
        assert!(outcome.ops.is_empty(), "{ch:?} must emit no IME ops");
    }
    assert_eq!(bridge.preedit(), "", "passthrough never builds a preedit");
}

#[test]
fn rebuild_to_english_discards_composition_and_goes_silent() {
    let mut bridge = EngineBridge::new("telex");
    type_chars(&mut bridge, "vieej");
    let outcome = bridge
        .rebuild("english")
        .expect("english rebuild can never fail — no keyboard build involved");
    // The live preedit is cleared on the way out (mode switch = reset).
    assert_eq!(outcome.ops, vec![ImeOp::Preedit(String::new())]);
    let after = bridge.process_char('w');
    assert!(!after.handled && after.ops.is_empty(), "silent after switch");
}

#[test]
fn rebuild_from_english_back_to_telex_composes_again() {
    let mut bridge = EngineBridge::new("english");
    assert!(!bridge.process_char('a').handled);
    bridge
        .rebuild("telex")
        .expect("telex must always build");
    let ops = type_chars(&mut bridge, "vieejt");
    assert_eq!(ops.last(), Some(&ImeOp::Preedit("việt".into())));
}

/// A minimal custom keyboard with NO transformations/tones: under it "aa"
/// stays "aa", while the telex fallback would compose "â" — so the assertion
/// distinguishes "custom TOML actually loaded" from "silently fell back".
const E2E_CUSTOM_TOML: &str = "\
[metadata]
id = \"bridge-e2e-custom\"
name = \"Bridge E2E Custom\"
language = \"xx\"

[transformations]

[tones]

[rules]
";

/// `keyboards/bridge-e2e-custom.toml` next to the TEST EXECUTABLE —
/// `get_custom_dir()`'s highest-priority location, so the ambient lookup in
/// `build_keyboard` resolves here deterministically, whatever the machine's
/// XDG data dir contains.
fn e2e_keyboard_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("test exe path");
    exe.parent()
        .expect("test exe has a parent dir")
        .join("keyboards/bridge-e2e-custom.toml")
}

fn install_e2e_keyboard() {
    let path = e2e_keyboard_path();
    std::fs::create_dir_all(path.parent().unwrap()).expect("create exe-side keyboards dir");
    std::fs::write(path, E2E_CUSTOM_TOML).expect("write e2e custom keyboard");
}

/// End-to-end guard for the custom-method path: switching the bridge to a
/// custom id must LOAD that TOML, not silently fall back to telex (the bug
/// that once let every custom radio type telex while showing checked).
#[test]
fn rebuild_to_custom_keyboard_loads_its_toml() {
    install_e2e_keyboard();
    let mut bridge = EngineBridge::new("telex");
    bridge
        .rebuild("bridge-e2e-custom")
        .expect("present custom TOML must build");
    let space = {
        type_chars(&mut bridge, "aa");
        bridge.process_char(' ')
    };
    // Remove the fixture before asserting so a failure doesn't leave it
    // behind to shadow later ambient-dir lookups in other test binaries.
    let _ = std::fs::remove_file(e2e_keyboard_path());
    assert_eq!(
        commits(&space.ops),
        vec!["aa"],
        "custom keyboard must be in effect (telex fallback would commit \"â\")"
    );
}

/// A custom id whose TOML is missing degrades to telex — same fallback the
/// sync channel's read path uses — instead of failing or going silent.
#[test]
fn rebuild_to_missing_custom_falls_back_to_telex() {
    let mut bridge = EngineBridge::new("no-such-custom-keyboard");
    let space = {
        type_chars(&mut bridge, "aa");
        bridge.process_char(' ')
    };
    assert_eq!(commits(&space.ops), vec!["â"], "missing custom must behave as telex");
}
