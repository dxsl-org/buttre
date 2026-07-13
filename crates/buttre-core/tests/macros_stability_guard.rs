//! Shorthand/gõ tắt stability guard: wiring an ACTIVE macro store must never
//! change how a non-trigger word composes, and an EMPTY/unwired store must
//! be byte-identical to shorthand being off entirely.
//!
//! Corpora: the 10k common-English-word list (already used by
//! `english_typeability_guard.rs`) and the full Telex Vietnamese harness
//! corpus (`buttre-test/data/telex.txt`) — between the two, this exercises
//! both plain English collision surface and real Vietnamese typing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use buttre_core::state::macros::{MacroEntry, MacroFile, MacroStore};
use buttre_core::KeyboardBuilder;
use buttre_engine::types::Action;

fn apply(screen: &mut String, actions: &[Action]) {
    for a in actions {
        match a {
            Action::DoNothing | Action::HideCandidates | Action::ShowCandidates { .. } => {}
            Action::Commit(t) | Action::ConfirmComposition(t) => screen.push_str(t),
            Action::Replace {
                backspace_count,
                text,
            } => {
                for _ in 0..*backspace_count {
                    screen.pop();
                }
                screen.push_str(text);
            }
            Action::UpdateComposition { .. } => {}
        }
    }
}

/// Type `word` plainly plus a trailing space on a fresh keyboard, optionally
/// wired to `store`. Returns the final screen content.
fn type_word(word: &str, store: Option<Arc<Mutex<MacroStore>>>) -> String {
    let mut kb = KeyboardBuilder::telex().unwrap();
    if let Some(store) = store {
        kb.set_macros(store);
    }
    let mut screen = String::new();
    for ch in word.chars() {
        let actions = kb.process(ch).unwrap();
        apply(&mut screen, &actions);
    }
    let actions = kb.process(' ').unwrap();
    apply(&mut screen, &actions);
    screen
}

fn english_words() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/english-common-words.txt"
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|w| w.len() >= 2 && w.chars().all(|c| c.is_ascii_lowercase()))
        .map(str::to_string)
        .collect()
}

fn vietnamese_typed_words() -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../buttre-test/data/telex.txt");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|l| l.split(',').next())
        .map(str::trim)
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_string)
        .collect()
}

#[test]
fn empty_macro_store_is_byte_identical_to_unwired() {
    let empty = Arc::new(Mutex::new(MacroStore::from_file(MacroFile {
        macros: HashMap::new(),
    })));
    let mut diffs = 0usize;
    for word in english_words().into_iter().chain(vietnamese_typed_words()) {
        let baseline = type_word(&word, None);
        let wired = type_word(&word, Some(empty.clone()));
        if baseline != wired {
            diffs += 1;
        }
    }
    assert_eq!(
        diffs, 0,
        "an empty/enabled macro store must never change composition versus no store at all"
    );
}

#[test]
fn active_macro_store_does_not_corrupt_non_trigger_words() {
    // A realistic-sized, deliberately common-looking trigger set — the kind
    // a real user might author — checked against BOTH corpora to prove
    // wiring it changes ONLY the exact trigger words, nothing else.
    let mut macros = HashMap::new();
    for (trigger, expand) in [
        ("vn", "Việt Nam"),
        ("brb", "be right back"),
        ("btw", "by the way"),
        ("omg", "oh my god"),
        ("idk", "I don't know"),
        ("tks", "thanks"),
        ("ko", "không"),
        ("dc", "được"),
    ] {
        macros.insert(
            trigger.to_string(),
            MacroEntry {
                expand: expand.to_string(),
                enabled: true,
            },
        );
    }
    let triggers: std::collections::HashSet<String> = macros.keys().cloned().collect();
    let store = Arc::new(Mutex::new(MacroStore::from_file(MacroFile { macros })));

    let mut unexpected_diffs = Vec::new();
    let mut expected_hits = 0usize;
    for word in english_words().into_iter().chain(vietnamese_typed_words()) {
        let baseline = type_word(&word, None);
        let wired = type_word(&word, Some(store.clone()));
        if baseline != wired {
            if triggers.contains(&word.to_lowercase()) {
                expected_hits += 1;
            } else {
                unexpected_diffs.push(format!("'{word}': no-store='{baseline}' wired='{wired}'"));
            }
        }
    }
    assert!(
        unexpected_diffs.is_empty(),
        "an active macro store must never change a NON-trigger word's composition:\n{}",
        unexpected_diffs.join("\n")
    );
    assert!(
        expected_hits > 0,
        "sanity: at least one corpus word must actually be a configured trigger"
    );
}

#[test]
fn shorthand_off_never_consults_store_even_when_set_then_cleared() {
    // clear_macros must fully revert to unwired behavior, not just "empty".
    let mut macros = HashMap::new();
    macros.insert(
        "vn".to_string(),
        MacroEntry {
            expand: "Việt Nam".to_string(),
            enabled: true,
        },
    );
    let store = Arc::new(Mutex::new(MacroStore::from_file(MacroFile { macros })));

    let mut kb = KeyboardBuilder::telex().unwrap();
    kb.set_macros(store);
    kb.clear_macros();

    let mut screen = String::new();
    for ch in "vn ".chars() {
        let actions = kb.process(ch).unwrap();
        apply(&mut screen, &actions);
    }
    assert_eq!(
        screen, "vn ",
        "clear_macros must fully unwire, not leave stale expansion behavior"
    );
}
