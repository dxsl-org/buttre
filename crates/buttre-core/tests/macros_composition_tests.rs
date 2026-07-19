//! Composition-mode (TSF/IBus/Wayland) shorthand seam tests: expansion at
//! the `ConfirmComposition` boundary and at `boundary_repair` (Enter/nav
//! commits). The multiword/hook-mode counterpart lives in `macros_tests.rs`;
//! store-level load/hardening tests live in `state/macros.rs`.

use buttre_core::state::macros::{MacroEntry, MacroFile, MacroStore};
use buttre_core::{Action, Keyboard, KeyboardBuilder};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn composition_keyboard() -> Keyboard {
    KeyboardBuilder::telex_with_composition(true).expect("telex composition keyboard")
}

/// `vn` -> "Việt Nam" (enabled), `brb` -> "be right back" (disabled).
fn test_store() -> Arc<Mutex<MacroStore>> {
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
    Arc::new(Mutex::new(MacroStore::from_file(MacroFile { macros })))
}

fn keyboard_with_macros() -> Keyboard {
    let mut kb = composition_keyboard();
    kb.set_macros(test_store());
    kb
}

fn type_str(kb: &mut Keyboard, s: &str) -> Vec<Action> {
    let mut last = Vec::new();
    for ch in s.chars() {
        last = kb.process(ch).expect("process must not error");
    }
    last
}

fn confirm_payload(actions: &[Action]) -> Option<&str> {
    actions.iter().find_map(|a| match a {
        Action::ConfirmComposition(text) => Some(text.as_str()),
        _ => None,
    })
}

#[test]
fn separator_close_expands_trigger() {
    let mut kb = keyboard_with_macros();
    let actions = type_str(&mut kb, "vn ");
    assert_eq!(
        confirm_payload(&actions),
        Some("Việt Nam"),
        "space must confirm the expansion, got {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Commit(sep) if sep == " ")),
        "the separator itself must still commit, got {actions:?}"
    );
}

#[test]
fn punctuation_close_expands_trigger() {
    let mut kb = keyboard_with_macros();
    let actions = type_str(&mut kb, "vn.");
    assert_eq!(confirm_payload(&actions), Some("Việt Nam"));
    assert!(actions
        .iter()
        .any(|a| matches!(a, Action::Commit(sep) if sep == ".")));
}

#[test]
fn trigger_is_case_insensitive() {
    let mut kb = keyboard_with_macros();
    let actions = type_str(&mut kb, "VN ");
    assert_eq!(confirm_payload(&actions), Some("Việt Nam"));
}

#[test]
fn boundary_repair_expands_trigger() {
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "vn");
    assert_eq!(
        kb.boundary_repair().as_deref(),
        Some("Việt Nam"),
        "Enter/nav commit must expand exactly like a separator"
    );
}

#[test]
fn open_word_never_expands() {
    let mut kb = keyboard_with_macros();
    let actions = type_str(&mut kb, "vn");
    assert!(
        actions
            .iter()
            .all(|a| !matches!(a, Action::ConfirmComposition(_))),
        "no confirm may fire while the run is still open, got {actions:?}"
    );
}

#[test]
fn disabled_entry_commits_composed_form() {
    let mut kb = keyboard_with_macros();
    let actions = type_str(&mut kb, "brb ");
    assert_eq!(
        confirm_payload(&actions),
        Some("brb"),
        "a disabled trigger must commit its composed form verbatim"
    );
}

#[test]
fn non_trigger_word_commits_composed_form() {
    let mut kb = keyboard_with_macros();
    let actions = type_str(&mut kb, "vieetj ");
    assert_eq!(
        confirm_payload(&actions),
        Some("việt"),
        "Vietnamese composition must be untouched for non-triggers"
    );
}

#[test]
fn expansion_overrides_composed_form_of_same_raw() {
    let mut macros = HashMap::new();
    macros.insert(
        "vieet".to_string(),
        MacroEntry {
            expand: "EXPANDED".to_string(),
            enabled: true,
        },
    );
    let mut kb = composition_keyboard();
    kb.set_macros(Arc::new(Mutex::new(MacroStore::from_file(MacroFile {
        macros,
    }))));
    let actions = type_str(&mut kb, "vieet ");
    assert_eq!(
        confirm_payload(&actions),
        Some("EXPANDED"),
        "macro precedence: expansion wins over the composed form (would be 'viết')"
    );
}

#[test]
fn no_store_is_byte_identical_to_today() {
    let mut with = composition_keyboard();
    let mut without = composition_keyboard();
    for word in ["vn ", "vieetj ", "xin chaof "] {
        assert_eq!(
            type_str(&mut with, word),
            type_str(&mut without, word),
            "an un-wired keyboard must behave identically for {word:?}"
        );
    }
}

#[test]
fn no_double_fire_after_separator_confirm() {
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "vn ");
    assert_eq!(
        kb.boundary_repair(),
        None,
        "the word was already confirmed by the separator — nothing left to expand"
    );
}

#[test]
fn backspace_reopens_run_and_next_confirm_rechecks() {
    let mut kb = keyboard_with_macros();
    type_str(&mut kb, "vnx");
    kb.backspace().expect("backspace must not error");
    let actions = type_str(&mut kb, " ");
    assert_eq!(
        confirm_payload(&actions),
        Some("Việt Nam"),
        "expansion is a pure projection — editing back to a trigger must expand"
    );
}
