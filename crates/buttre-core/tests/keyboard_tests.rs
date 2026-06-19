use buttre_core::keyboard::{Keyboard, Config, KeyboardBuilder};

#[test]
fn test_keyboard_creation() {
    let toml = r#"
[metadata]
id = "test"
name = "Test"
language = "vietnamese"

[transformations]
"aa" = "â"

[tones]
"s" = "acute"

[rules]
tone_position = "modern"
"#;
    
    // Test with old way converted to new way
    let config = Config::from_str(toml).unwrap();
    let pipeline_config = config.to_pipeline_config();
    let keyboard = Keyboard::new(pipeline_config);
    assert!(keyboard.is_ok());
}

#[test]
fn test_thuowr_via_keyboard() {
    let mut keyboard = KeyboardBuilder::telex().unwrap();
    
    for ch in "thuowr".chars() {
        keyboard.process(ch).unwrap();
    }
    
    assert_eq!(keyboard.buffer(), "thuở", "thuowr should produce thuở");
}

// ── Backspace: grapheme-aware, keeps the word editable, no desync ─────────────

#[test]
fn test_backspace_deletes_grapheme_keeps_tone() {
    use buttre_core::Action;
    let mut kb = KeyboardBuilder::telex().unwrap();

    // "vieetj" → "việt".  Backspace deletes the last grapheme 't' but KEEPS the
    // tone → "việ" (raw order ≠ display order: the tone key 'j' is typed last).
    for ch in "vieetj".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "việt");

    match kb.backspace().unwrap() {
        Action::Replace { backspace_count, text } => {
            assert_eq!(backspace_count, 1, "exactly one displayed char deleted");
            assert_eq!(text, "");
        }
        other => panic!("expected Replace{{1,\"\"}}, got {other:?}"),
    }
    assert_eq!(kb.buffer(), "việ", "tone preserved; only final consonant removed");

    // Composition stays alive: a tone key now re-tones the edited word.
    kb.process('s').unwrap();
    assert_eq!(kb.buffer(), "viế", "re-toning after backspace works");
}

#[test]
fn test_backspace_no_desync_then_fresh_word() {
    use buttre_core::Action;
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in "ngayf".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "ngày");
    // Each backspace deletes exactly one displayed grapheme — no over-deletion
    // reaching into a previous word.
    assert!(matches!(kb.backspace().unwrap(), Action::Replace { backspace_count: 1, .. }));
    assert_eq!(kb.buffer(), "ngà");
    assert!(matches!(kb.backspace().unwrap(), Action::Replace { backspace_count: 1, .. }));
    assert_eq!(kb.buffer(), "ng");
}

// ── Multi-word rolling window: edit/re-tone a previous word (Cách B) ───────────

#[test]
fn test_multiword_retone_previous_word() {
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in "ban cas".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "ban cá");
    // Backspace across the space, deleting the second word entirely.
    kb.backspace().unwrap(); // "ban c"
    kb.backspace().unwrap(); // "ban "
    kb.backspace().unwrap(); // "ban"
    assert_eq!(kb.buffer(), "ban");
    // The previous word is still composable: apply a tone to it.
    kb.process('f').unwrap();
    assert_eq!(kb.buffer(), "bàn", "must re-tone the previous word after backspace");
}

#[test]
fn test_multiword_window_cap_freezes_oldest() {
    // Window keeps the last 3 words; a 4th word scrolls the oldest into the
    // frozen prefix (still shown, no longer recomposed).
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in "mot hai ba bon".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "mot hai ba bon");
}
