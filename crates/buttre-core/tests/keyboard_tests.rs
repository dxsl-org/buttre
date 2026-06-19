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

// ── Backspace resync (bug: deleting then typing merged words / ate the space) ──

#[test]
fn test_backspace_resets_composition_no_desync() {
    use buttre_core::Action;
    let mut kb = KeyboardBuilder::telex().unwrap();

    // Compose "ngày".
    for ch in "ngayf".chars() {
        kb.process(ch).unwrap();
    }
    assert_eq!(kb.buffer(), "ngày");

    // Backspace must reset the composition and emit exactly one backspace —
    // NOT leave the executor believing "ngày" is still on screen.
    match kb.backspace().unwrap() {
        Action::Replace { backspace_count, text } => {
            assert_eq!(backspace_count, 1);
            assert_eq!(text, "");
        }
        other => panic!("expected Replace{{1,\"\"}}, got {other:?}"),
    }
    assert_eq!(kb.buffer(), "", "composition must reset after backspace");

    // The next keystroke starts a fresh syllable — a clean Commit with no
    // backspaces reaching back into the previously committed word.
    let acts = kb.process('x').unwrap();
    match &acts[0] {
        Action::Commit(t) => assert_eq!(t, "x"),
        Action::Replace { backspace_count, .. } => {
            assert_eq!(*backspace_count, 0, "must not backspace into prior text");
        }
        other => panic!("unexpected first action after backspace: {other:?}"),
    }
}
