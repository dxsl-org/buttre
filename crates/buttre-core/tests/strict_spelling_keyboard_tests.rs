//! `Settings::strict_spelling` at the `Keyboard` layer: the deliberate-đ
//! leniency ("ddt" → "đt") must hold across BOTH word models — the hook's
//! multiword window and the composition-mode (TSF/IBus/Wayland) seam — and
//! `set_strict_spelling` must flip it live. Engine-level coverage lives in
//! `buttre-engine/tests/strict_spelling_dd_tests.rs`.

use buttre_core::state::Settings;
use buttre_core::{Action, Keyboard, KeyboardBuilder};

fn type_str(kb: &mut Keyboard, s: &str) -> Vec<Action> {
    let mut last = Vec::new();
    for ch in s.chars() {
        last = kb.process(ch).expect("process must not error");
    }
    last
}

#[test]
fn hook_multiword_keeps_dd_abbreviation() {
    let mut kb = KeyboardBuilder::telex().unwrap();
    type_str(&mut kb, "ddt ");
    assert_eq!(kb.buffer(), "đt ", "lenient default in the hook window");
}

#[test]
fn hook_multiword_strict_reverts() {
    let mut kb = KeyboardBuilder::telex().unwrap();
    kb.set_strict_spelling(true);
    type_str(&mut kb, "ddt ");
    assert_eq!(kb.buffer(), "ddt ", "strict mode restores the raw revert");
}

#[test]
fn composition_seam_confirms_the_abbreviation() {
    let mut kb = KeyboardBuilder::telex_with_composition(true).unwrap();
    let actions = type_str(&mut kb, "ddt.");
    let confirmed = actions.iter().find_map(|a| match a {
        Action::ConfirmComposition(text) => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(
        confirmed,
        Some("đt"),
        "the separator must confirm the composed abbreviation, got {actions:?}"
    );
}

#[test]
fn toggle_gives_the_literal_escape_hatch() {
    // Lenient mode makes the two projections differ ("đt" vs "ddt"), so
    // Ctrl+Shift+Z (toggle_last_word) now actually has something to flip —
    // the escape hatch for the rare literal-"ddt" intent.
    let mut kb = KeyboardBuilder::telex().unwrap();
    type_str(&mut kb, "ddt");
    assert_eq!(kb.buffer(), "đt");
    kb.toggle_last_word();
    assert_eq!(kb.buffer(), "ddt", "first toggle → literal raw");
    kb.toggle_last_word();
    assert_eq!(kb.buffer(), "đt", "second toggle → back to composed");
}

#[test]
fn settings_default_is_lenient_and_survives_old_files() {
    assert!(!Settings::default().strict_spelling, "default = lenient");
    // Pre-existing settings.toml files lack the field entirely — load must
    // fall back to lenient, never error.
    let toml_str = r#"
        input_method = "telex"
        auto_correct = false
        shorthand = false
        startup = false
    "#;
    let settings: Settings =
        toml::from_str(toml_str).expect("must deserialize without strict_spelling present");
    assert!(!settings.strict_spelling);
}

#[test]
fn settings_round_trips_through_toml() {
    let settings = Settings {
        strict_spelling: true,
        ..Settings::default()
    };
    let serialized = toml::to_string_pretty(&settings).expect("serialize");
    let restored: Settings = toml::from_str(&serialized).expect("deserialize");
    assert!(restored.strict_spelling);
}
