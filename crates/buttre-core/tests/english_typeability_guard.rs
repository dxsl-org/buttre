//! English typeability guard: every common English word must be producible
//! ON SCREEN — typed plainly or with the Unikey double-key escape (retype
//! the key whenever the screen diverges from the literal prefix) — including
//! the word-boundary commit at the trailing space.
//!
//! Corpus: `tests/data/english-common-words.txt` — the google-10000-english
//! frequency list (Google Trillion Word Corpus, via
//! github.com/first20hours/google-10000-english).
//!
//! This guards the whole "spent undo resurrected" bug class end-to-end:
//! "resset"→"reset" (tone pair), "rowws"→"rows" (transform toggle),
//! "meterss"→"meters" (undo must not eat overridden tone keys). A word
//! showing up here means some raw sequence exists for which NO combination
//! of plain typing + double-key escapes yields the English word — the exact
//! failure users reported for "reset"/"rows"/"towns".

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

/// Type the word plainly, then a space. Returns final screen content.
fn type_plain(word: &str) -> String {
    let mut kb = KeyboardBuilder::telex().unwrap();
    let mut screen = String::new();
    for ch in word.chars() {
        press(&mut kb, &mut screen, ch);
    }
    press(&mut kb, &mut screen, ' ');
    screen
}

/// Greedy user-repair: type each char; whenever the screen no longer equals
/// the literal prefix, retype the same key once (the Unikey escape reflex).
/// Finish with a space; require the final screen == "word ".
fn type_with_escape(word: &str) -> Result<(), String> {
    let mut kb = KeyboardBuilder::telex().unwrap();
    let mut screen = String::new();
    let mut expected = String::new();
    for ch in word.chars() {
        press(&mut kb, &mut screen, ch);
        expected.push(ch);
        if screen != expected {
            press(&mut kb, &mut screen, ch);
            if screen != expected {
                return Err(format!("mid-word: '{screen}'"));
            }
        }
    }
    press(&mut kb, &mut screen, ' ');
    expected.push(' ');
    if screen != expected {
        return Err(format!("at-commit: '{screen}'"));
    }
    Ok(())
}

#[test]
fn every_common_english_word_is_typeable() {
    let path = std::env::var("BUTTRE_WORDLIST")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/data/english-common-words.txt"
            )
            .to_string()
        });
    let words = std::fs::read_to_string(&path).unwrap();
    let mut plain_diverges = 0usize;
    let mut broken = Vec::new();
    let mut total = 0usize;
    for word in words.lines() {
        let word = word.trim();
        // ASCII lowercase letters only; the engine sees other chars as separators.
        if word.len() < 2 || !word.chars().all(|c| c.is_ascii_lowercase()) {
            continue;
        }
        total += 1;
        let plain = type_plain(word);
        if plain == format!("{word} ") {
            continue;
        }
        plain_diverges += 1;
        if let Err(got) = type_with_escape(word) {
            broken.push(format!("{word}: plain='{}' {got}", plain.trim_end()));
        }
    }
    println!(
        "total={total} plain_diverges={plain_diverges} unrecoverable={}",
        broken.len()
    );
    assert!(
        broken.is_empty(),
        "unrecoverable English words (no plain or double-key-escape sequence produces them):\n{}",
        broken.join("\n")
    );
}
