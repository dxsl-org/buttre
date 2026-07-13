//! Two-session learning stability guard (ADR-0001 record-replay invariant).
//!
//! The "yes" incident (2026-07-12): after ONE session where the user escaped
//! a tone with the double-key undo ("yess" → "yes"), the learning store
//! recorded a pref whose replay differed from what the user had accepted —
//! the next session, the same keystrokes produced different text and "yes"
//! became untypeable. Every CI test passed, because none of them carried a
//! learning store across sessions.
//!
//! This guard closes that class: type a corpus WITH learning enabled, keep
//! the store, retype the identical keystrokes in later sessions — the screen
//! must be BYTE-IDENTICAL every time. Learning may only ever widen what the
//! engine accepts (additive-only); it must never change what identical
//! keystrokes produce for words the user already typed successfully.
//!
//! Two corpora, two typing styles, chosen to exercise both learning writers:
//! - English (10k common words), typed with the greedy double-key escape —
//!   the undo shapes that triggered the poisonous auto-record.
//! - Vietnamese (the telex.txt harness typing column), typed plainly — the
//!   direct-typed overlay promotion counter path.

use std::sync::{mpsc, Arc, Mutex};

use buttre_core::state::learning::LearningStore;
use buttre_core::{Keyboard, KeyboardBuilder};
use buttre_engine::types::Action;

/// Virtual screen: apply actions the way the platform hook does.
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

fn press(kb: &mut Keyboard, screen: &mut String, ch: char) {
    let actions = kb.process(ch).unwrap();
    apply(screen, &actions);
}

fn learning_keyboard(store: &Arc<Mutex<LearningStore>>) -> Keyboard {
    let (tx, _rx) = mpsc::channel();
    let mut kb = KeyboardBuilder::telex().unwrap();
    kb.set_learning(store.clone(), tx);
    kb
}

/// One session: a single Keyboard (so word scroll-out — the real collection
/// point — actually fires) typing every word with the greedy double-key
/// escape reflex, all sharing `store`. Returns the per-word screen segments.
///
/// A word the escape reflex cannot repair is NOT a failure here (the
/// store-less `english_typeability_guard` owns typeability) — its segment is
/// recorded as-is; THIS guard only asserts the segments never change across
/// sessions.
fn escape_session(store: &Arc<Mutex<LearningStore>>, words: &[&str]) -> Vec<String> {
    let mut kb = learning_keyboard(store);
    let mut screen = String::new();
    let mut segments = Vec::with_capacity(words.len());
    for word in words {
        let base = screen.clone();
        let mut expected: String = base.clone();
        for ch in word.chars() {
            press(&mut kb, &mut screen, ch);
            expected.push(ch);
            if screen != expected {
                press(&mut kb, &mut screen, ch);
            }
            // Re-sync: whatever the screen holds now is what this session
            // produced; cross-session equality is the only assertion.
            expected = screen.clone();
        }
        press(&mut kb, &mut screen, ' ');
        segments.push(screen[base.len()..].to_string());
    }
    segments
}

/// One session typing every word plainly (no escape reflex).
fn plain_session(store: &Arc<Mutex<LearningStore>>, words: &[&str]) -> Vec<String> {
    let mut kb = learning_keyboard(store);
    let mut screen = String::new();
    let mut segments = Vec::with_capacity(words.len());
    for word in words {
        let base = screen.len();
        for ch in word.chars() {
            press(&mut kb, &mut screen, ch);
        }
        press(&mut kb, &mut screen, ' ');
        segments.push(screen[base..].to_string());
    }
    segments
}

fn assert_sessions_identical(
    label: &str,
    words: &[&str],
    session: impl Fn(&Arc<Mutex<LearningStore>>, &[&str]) -> Vec<String>,
) {
    let store = Arc::new(Mutex::new(LearningStore::default()));
    let baseline = session(&store, words);
    // 3 total sessions: session 2 replays whatever session 1 recorded;
    // session 3 additionally sees hit-counters advanced by two prior runs.
    for n in 2..=3 {
        let replay = session(&store, words);
        let diffs: Vec<String> = words
            .iter()
            .zip(baseline.iter().zip(replay.iter()))
            .filter(|(_, (a, b))| a != b)
            .take(20)
            .map(|(w, (a, b))| format!("  '{w}': session1='{a}' session{n}='{b}'"))
            .collect();
        assert!(
            diffs.is_empty(),
            "{label}: learning changed what identical keystrokes produce (session {n} vs 1) — \
             record-replay invariant (ADR-0001) violated for {} word(s):\n{}",
            diffs.len(),
            diffs.join("\n")
        );
    }
}

#[test]
fn english_escape_typing_is_stable_across_learning_sessions() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/english-common-words.txt"
    );
    let raw = std::fs::read_to_string(path).unwrap();
    let words: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|w| w.len() >= 2 && w.chars().all(|c| c.is_ascii_lowercase()))
        .collect();
    assert!(words.len() > 9000, "corpus unexpectedly small");
    assert_sessions_identical("english-escape", &words, escape_session);
}

#[test]
fn vietnamese_plain_typing_is_stable_across_learning_sessions() {
    // The telex.txt harness typing column — every raw the project already
    // pins an expected rendering for, including informal/unattested words
    // (the direct-typed overlay promotion path).
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../buttre-test/data/telex.txt");
    let raw = std::fs::read_to_string(path).unwrap();
    let words: Vec<&str> = raw
        .lines()
        .filter_map(|l| l.split(',').next())
        .map(str::trim)
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric()))
        .collect();
    assert!(words.len() > 2000, "corpus unexpectedly small");
    assert_sessions_identical("vietnamese-plain", &words, plain_session);
}
