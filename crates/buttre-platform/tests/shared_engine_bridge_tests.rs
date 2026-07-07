//! Composition-semantics tests for the shared EngineBridge (all platforms).
//!
//! Pure — a real `Keyboard` in composition mode, no D-Bus/Wayland/FFI.
//! These mirror the end-to-end scenarios in `scripts/test-ibus-scenarios.py`
//! so a semantics regression fails in `cargo test` on ANY OS before it ever
//! reaches a bus. The same bridge drives the Linux backends and the macOS
//! FFI, so this suite pins composition behavior for both.

use buttre_platform::shared::engine_bridge::{EngineBridge, ImeOp};

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
            ImeOp::Preedit(_) => None,
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
