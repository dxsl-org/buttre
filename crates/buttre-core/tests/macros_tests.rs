//! Keyboard-level shorthand/gõ tắt tests: expansion, collision safety, and
//! `Ctrl+Shift+Z` revert. Store-level load/hardening/lookup unit tests live
//! in `crates/buttre-core/src/state/macros.rs`.

use buttre_core::state::macros::{MacroEntry, MacroFile, MacroStore};
use buttre_core::{Keyboard, KeyboardBuilder};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn type_str(kb: &mut Keyboard, s: &str) {
    for ch in s.chars() {
        kb.process(ch).expect("process must not error");
    }
}

/// A store with `vn` -> "Việt Nam" (enabled) and `brb` -> "be right back"
/// (disabled), wired into a fresh Telex keyboard.
fn keyboard_with_macros() -> Keyboard {
    let mut macros = HashMap::new();
    macros.insert(
        "vn".to_string(),
        MacroEntry {
            expand: "Việt Nam".to_string(),
            enabled: true,
        },
    );
    macros.insert(
        "brb".to_string(),
        MacroEntry {
            expand: "be right back".to_string(),
            enabled: false,
        },
    );
    let store = MacroStore::from_file(MacroFile { macros });
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    kb.set_macros(Arc::new(Mutex::new(store)));
    kb
}

#[test]
fn macro_expands_on_closed_word_run() {
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "vn ");
    assert_eq!(kb.buffer(), "Việt Nam ");
}

#[test]
fn macro_case_insensitive_trigger() {
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "VN ");
    assert_eq!(
        kb.buffer(),
        "Việt Nam ",
        "trigger lookup must be case-insensitive"
    );
}

#[test]
fn macro_does_not_fire_on_open_trailing_word() {
    // Collision safety (AutoHotkey-style boundary-on-both-sides): the
    // still-open trailing word never expands mid-type.
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "vn");
    assert_ne!(
        kb.buffer(),
        "Việt Nam",
        "an open (no separator yet) run must never expand"
    );
}

#[test]
fn macro_does_not_fire_as_fragment_of_longer_word() {
    // "advn" must never expand the "vn" fragment inside it — matching is on
    // the WHOLE closed run, not a substring/suffix.
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "advn ");
    assert_ne!(kb.buffer(), "adViệt Nam ");
    assert_ne!(kb.buffer(), "ad Việt Nam ");
}

#[test]
fn disabled_macro_entry_never_fires() {
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "brb ");
    assert_eq!(
        kb.buffer(),
        "brb ",
        "a disabled entry must render as plain typed text"
    );
}

#[test]
fn unknown_trigger_composes_normally() {
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "test ");
    assert_eq!(
        kb.buffer(),
        "tét ",
        "a raw with no macro match must compose exactly as it would with no store at all"
    );
}

#[test]
fn ctrl_shift_z_reverts_expansion_to_literal_then_composed() {
    // Success criteria (plan phase-01): first toggle -> literal raw ("vn"),
    // second toggle -> composed("vn") — NOT back to the macro expansion.
    // `compose_window`'s precedence (toggle > macro > compose) means
    // `toggle_last_word` needs zero macro-specific code for this.
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "vn ");
    assert_eq!(kb.buffer(), "Việt Nam ");

    // "vn" is still the last window word run (the trailing space is just a
    // separator, not a new run) — `toggle_last_word` acts on it directly.
    kb.toggle_last_word().expect("toggle must act");
    assert_eq!(kb.buffer(), "vn ", "first toggle reverts to literal raw");

    kb.toggle_last_word().expect("toggle must act again");
    assert_eq!(
        kb.buffer(),
        "vn ",
        "second toggle composes the raw as Vietnamese (\"vn\" has no tone/transform, stays \"vn\")"
    );
}

#[test]
fn macros_off_by_default_no_store_wired() {
    // Byte-identical to a keyboard that never called set_macros.
    let mut kb = KeyboardBuilder::telex().expect("telex keyboard");
    type_str(&mut kb, "vn ");
    assert_eq!(kb.buffer(), "vn ");
}

#[test]
fn clear_macros_stops_future_expansion_and_clears_pending() {
    let mut kb = keyboard_with_macros();
    kb.clear_macros();
    type_str(&mut kb, "vn ");
    assert_eq!(
        kb.buffer(),
        "vn ",
        "after clear_macros, expansion must stop entirely"
    );
}

#[test]
fn macro_expansion_survives_window_scroll_out() {
    // The expanded word must remain "Việt Nam" once it scrolls past the
    // 3-word rolling window (frozen into `committed`), not revert to raw.
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "vn one two three four");
    assert!(
        kb.buffer().contains("Việt Nam"),
        "expansion must survive scroll-out: {}",
        kb.buffer()
    );
}
