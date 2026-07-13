use buttre_engine::pipeline::config::{ToneMark, ValidationSettings};
use buttre_engine::pipeline::{PipelineConfig, PipelineExecutor};
use buttre_engine::types::Action;

fn create_telex_config() -> PipelineConfig {
    let mut config = PipelineConfig::new("telex");

    // Enable permutation for flexible typing
    config.tone.allow_permutation = true;

    // Add transformation rules
    config.add_transform("aa", "â");
    config.add_transform("aw", "ă");
    config.add_transform("dd", "đ");
    config.add_transform("ee", "ê");
    config.add_transform("oo", "ô");
    config.add_transform("ow", "ơ");
    config.add_transform("uw", "ư");

    // Add tone mappings
    config.add_tone('s', ToneMark::Acute);
    config.add_tone('f', ToneMark::Grave);
    config.add_tone('r', ToneMark::Hook);
    config.add_tone('x', ToneMark::Tilde);
    config.add_tone('j', ToneMark::Dot);

    config
}

#[test]
fn test_simple_character() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    let actions = executor.process('a');

    // First 'a' should be committed
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Commit(text) => assert_eq!(text, "a"),
        _ => panic!("Expected Commit action"),
    }
}

#[test]
fn test_transformation_aa_to_a_circumflex() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type first 'a'
    executor.process('a');

    // Type second 'a' - should transform to 'â'
    let actions = executor.process('a');

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Replace {
            backspace_count,
            text,
        } => {
            assert_eq!(*backspace_count, 1);
            assert_eq!(text, "â");
        }
        _ => panic!("Expected Replace action, got {:?}", actions[0]),
    }
}

#[test]
fn test_transformation_aw_to_a_breve() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    let actions = executor.process('w');

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Replace {
            backspace_count,
            text,
        } => {
            assert_eq!(*backspace_count, 1);
            assert_eq!(text, "ă");
        }
        _ => panic!("Expected Replace action"),
    }
}

#[test]
fn test_tone_application() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type 'a'
    executor.process('a');

    // Type 's' (acute tone)
    let actions = executor.process('s');

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Replace {
            backspace_count,
            text,
        } => {
            assert_eq!(*backspace_count, 1);
            assert_eq!(text, "á");
        }
        _ => panic!("Expected Replace action"),
    }
}

#[test]
fn test_complex_word_thuong() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "thuowng" → "thương"
    executor.process('t');
    executor.process('h');
    executor.process('u');
    executor.process('o');
    executor.process('w'); // ow → ơ, retrofix converts uơ→ươ when ng follows
    executor.process('n');
    let _actions = executor.process('g');

    // Should have "thương" in syllable (retrofix applied)
    assert_eq!(executor.syllable(), "thương");
}

#[test]
fn test_reset() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    executor.process('a');

    assert_eq!(executor.syllable(), "â");

    executor.reset();

    assert_eq!(executor.syllable(), "");
    assert_eq!(executor.raw_buffer(), "");
}

#[test]
fn test_passthrough_number() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    let actions = executor.process('1');

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Commit(text) => assert_eq!(text, "1"),
        _ => panic!("Expected Commit action"),
    }
}

#[test]
fn test_passthrough_space() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    let actions = executor.process(' ');

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Commit(text) => assert_eq!(text, " "),
        _ => panic!("Expected Commit action"),
    }
}

#[test]
fn test_syllable_buffer() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // "hallo" contains no tone keys in Telex ('h', 'a', 'l', 'o')
    // 'l' might be used in some variants but standard is okay?
    // Let's use "abcd" where 'a', 'b', 'c', 'd' are safe?
    // Wait, 'd' is transform key! 'a' is transform key! 'b','c' are safe?
    // Using "mnpq" is safest.
    executor.process('m');
    executor.process('n');
    executor.process('p');
    executor.process('q');

    assert_eq!(executor.syllable(), "mnpq");
}

#[test]
fn test_raw_buffer() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    executor.process('a');

    // Raw buffer should contain the actual keystrokes
    assert_eq!(executor.raw_buffer(), "aa");
    // Syllable should be transformed
    assert_eq!(executor.syllable(), "â");
}

#[test]
fn test_multiple_transformations() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // dd → đ
    executor.process('d');
    executor.process('d');
    assert_eq!(executor.syllable(), "đ");

    executor.reset();

    // ee → ê
    executor.process('e');
    executor.process('e');
    assert_eq!(executor.syllable(), "ê");

    executor.reset();

    // oo → ô
    executor.process('o');
    executor.process('o');
    assert_eq!(executor.syllable(), "ô");
}

#[test]
fn test_transformation_and_tone() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "aas" → "ấ" (â + acute tone)
    executor.process('a');
    executor.process('a'); // aa → â
    let actions = executor.process('s'); // add acute tone

    assert_eq!(executor.syllable(), "ấ");
    match &actions[0] {
        Action::Replace {
            backspace_count,
            text,
        } => {
            assert_eq!(*backspace_count, 1);
            assert_eq!(text, "ấ");
        }
        _ => panic!("Expected Replace action"),
    }
}

#[test]
fn test_composition_reset_on_passthrough() {
    let mut config = create_telex_config();
    config.pipeline.use_composition = true;
    let mut executor = PipelineExecutor::new(config);

    // Type "aa" → "â"
    let actions = executor.process('a');
    // First 'a' -> UpdateComposition("a")
    assert!(matches!(actions[0], Action::UpdateComposition { .. }));

    let actions = executor.process('a');
    // Second 'a' -> UpdateComposition("â")
    match &actions[0] {
        Action::UpdateComposition { text, .. } => assert_eq!(text, "â"),
        _ => panic!("Expected UpdateComposition"),
    }

    // Type '.' (passthrough)
    let actions = executor.process('.');

    // Should confirm composition ("â") then commit '.'
    assert_eq!(actions.len(), 2);
    match &actions[0] {
        Action::ConfirmComposition(text) => assert_eq!(text, "â"),
        _ => panic!("Expected ConfirmComposition, got {:?}", actions[0]),
    }
    match &actions[1] {
        Action::Commit(text) => assert_eq!(text, "."),
        _ => panic!("Expected Commit"),
    }

    // And reset buffer
    assert_eq!(executor.syllable(), "");
}

// ==================== Additional Comprehensive Tests ====================

// === Basic Pipeline Flow Tests ===

#[test]
fn test_empty_input_sequence() {
    let config = create_telex_config();
    let executor = PipelineExecutor::new(config);

    // Initially empty
    assert_eq!(executor.syllable(), "");
    assert_eq!(executor.raw_buffer(), "");
    assert!(!executor.is_temp_english_mode());
}

#[test]
fn test_single_character_no_transform() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    let actions = executor.process('b');

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Commit(text) => assert_eq!(text, "b"),
        _ => panic!("Expected Commit"),
    }
}

#[test]
fn test_vietnamese_character_direct() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type Vietnamese character directly (e.g., from on-screen keyboard)
    let actions = executor.process('ă');

    assert_eq!(actions.len(), 1);
    match &actions[0] {
        Action::Commit(text) => assert_eq!(text, "ă"),
        _ => panic!("Expected Commit"),
    }
}

// === Transformation Tests ===

#[test]
fn test_all_telex_transforms() {
    let config = create_telex_config();

    let test_cases = vec![
        ("aa", "â"),
        ("aw", "ă"),
        ("dd", "đ"),
        ("ee", "ê"),
        ("oo", "ô"),
        ("ow", "ơ"),
        ("uw", "ư"),
    ];

    for (input, expected) in test_cases {
        let mut executor = PipelineExecutor::new(config.clone());

        for ch in input.chars() {
            executor.process(ch);
        }

        assert_eq!(executor.syllable(), expected, "Failed for input: {}", input);
    }
}

#[test]
fn test_transform_partial_sequence() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type 'a' alone (no transform yet)
    executor.process('a');
    assert_eq!(executor.syllable(), "a");

    // Reset and try different character
    executor.reset();
    executor.process('d');
    assert_eq!(executor.syllable(), "d");
}

#[test]
fn test_transform_uppercase() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "AA" (uppercase)
    executor.process('A');
    executor.process('A');

    // With uppercase support, AA should produce Â (uppercase)
    let result = executor.syllable();
    assert_eq!(
        result, "Â",
        "Expected uppercase 'Â' from 'AA' input, got: {}",
        result
    );
}

// === Tone Application Tests ===

#[test]
fn test_all_tones() {
    let config = create_telex_config();

    let test_cases = vec![
        ('s', "á"), // Acute
        ('f', "à"), // Grave
        ('r', "ả"), // Hook
        ('x', "ã"), // Tilde
        ('j', "ạ"), // Dot
    ];

    for (tone_key, expected) in test_cases {
        let mut executor = PipelineExecutor::new(config.clone());

        executor.process('a');
        executor.process(tone_key);

        assert_eq!(
            executor.syllable(),
            expected,
            "Failed for tone key: {}",
            tone_key
        );
    }
}

#[test]
fn test_tone_on_transformed_char() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "ees" → "ế" (ê + acute)
    executor.process('e');
    executor.process('e'); // ee → ê
    executor.process('s'); // add acute

    assert_eq!(executor.syllable(), "ế");
}

#[test]
fn test_multiple_tone_changes() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "as" → "á"
    executor.process('a');
    executor.process('s');
    assert_eq!(executor.syllable(), "á");

    // Change tone: "af" → "à"
    executor.process('f');
    assert_eq!(executor.syllable(), "à");

    // Change again: "ar" → "ả"
    executor.process('r');
    assert_eq!(executor.syllable(), "ả");
}

// === Complex Word Tests ===

#[test]
fn test_word_vietnam() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "vieejt" → "việt"
    executor.process('v');
    executor.process('i');
    executor.process('e');
    executor.process('e'); // ee → ê
    executor.process('j'); // add dot below → ệ
    executor.process('t');

    assert_eq!(executor.syllable(), "việt");
}

#[test]
fn test_word_truong() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "truowng" → "trương" (retrofix converts uơ→ươ when ng follows, no tone)
    executor.process('t');
    executor.process('r');
    executor.process('u');
    executor.process('o');
    executor.process('w'); // ow → ơ, but retrofix will convert to ươ when ng follows
    executor.process('n');
    executor.process('g');

    assert_eq!(executor.syllable(), "trương");
}

#[test]
fn test_word_dang() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "ddawng" → "đăng"
    executor.process('d');
    executor.process('d'); // dd → đ
    executor.process('a');
    executor.process('w'); // aw → ă
    executor.process('n');
    executor.process('g');

    assert_eq!(executor.syllable(), "đăng");
}

// === Reset Tests ===

#[test]
fn test_reset_clears_state() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    executor.process('a');
    executor.process('s');

    assert_eq!(executor.syllable(), "ấ");
    assert_eq!(executor.raw_buffer(), "aas");

    executor.reset();

    assert_eq!(executor.syllable(), "");
    assert_eq!(executor.raw_buffer(), "");
}

#[test]
fn test_reset_between_words() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // First word
    executor.process('a');
    executor.process('a');
    assert_eq!(executor.syllable(), "â");

    executor.reset();

    // Second word
    executor.process('e');
    executor.process('e');
    assert_eq!(executor.syllable(), "ê");
}

// === PassThrough Tests ===

#[test]
fn test_passthrough_punctuation() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    let punctuation = vec!['.', ',', '!', '?', ';', ':'];

    for p in punctuation {
        executor.reset();
        let actions = executor.process(p);

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Commit(text) => assert_eq!(text, &p.to_string()),
            _ => panic!("Expected Commit for '{}'", p),
        }
    }
}

#[test]
fn test_passthrough_digits() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    for digit in '0'..='9' {
        executor.reset();
        let actions = executor.process(digit);

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Commit(text) => assert_eq!(text, &digit.to_string()),
            _ => panic!("Expected Commit for '{}'", digit),
        }
    }
}

#[test]
fn test_passthrough_after_syllable() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type a syllable
    executor.process('a');
    executor.process('a');
    assert_eq!(executor.syllable(), "â");

    // Type space (passthrough)
    let actions = executor.process(' ');

    // Should commit space and reset
    assert!(actions
        .iter()
        .any(|a| matches!(a, Action::Commit(text) if text == " ")));
    assert_eq!(executor.syllable(), "");
}

// === Buffer State Tests ===

#[test]
fn test_syllable_buffer_updates() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('t');
    assert_eq!(executor.syllable(), "t");

    executor.process('h');
    assert_eq!(executor.syllable(), "th");

    executor.process('u');
    assert_eq!(executor.syllable(), "thu");
}

#[test]
fn test_raw_buffer_preserves_keystrokes() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    executor.process('a');
    executor.process('s');

    // Raw buffer has actual keys typed
    assert_eq!(executor.raw_buffer(), "aas");
    // Syllable has transformed result
    assert_eq!(executor.syllable(), "ấ");
}

#[test]
fn test_buffer_after_transform() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('d');
    executor.process('d');

    assert_eq!(executor.syllable(), "đ");
    assert_eq!(executor.raw_buffer(), "dd");
}

// === Composition Mode Tests ===

#[test]
fn test_composition_mode_enabled() {
    let mut config = create_telex_config();
    config.pipeline.use_composition = true;
    let mut executor = PipelineExecutor::new(config);

    let actions = executor.process('a');

    // Should use UpdateComposition instead of Commit
    assert!(matches!(actions[0], Action::UpdateComposition { .. }));
}

#[test]
fn test_composition_transform() {
    let mut config = create_telex_config();
    config.pipeline.use_composition = true;
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    let actions = executor.process('a');

    // Transform in composition mode
    match &actions[0] {
        Action::UpdateComposition { text, .. } => assert_eq!(text, "â"),
        _ => panic!("Expected UpdateComposition"),
    }
}

#[test]
fn test_composition_confirm_on_punctuation() {
    let mut config = create_telex_config();
    config.pipeline.use_composition = true;
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    let actions = executor.process('.');

    // Should confirm "a" then commit "."
    assert_eq!(actions.len(), 2);
    assert!(matches!(actions[0], Action::ConfirmComposition(_)));
    assert!(matches!(actions[1], Action::Commit(_)));
}

// === Edge Cases ===

#[test]
fn test_repeated_transform_key() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "aaa" → "aa" (first two transform to "â", third triggers undo)
    executor.process('a');
    executor.process('a'); // aa → â
    executor.process('a'); // undo → aa

    assert_eq!(executor.syllable(), "aa");
    assert!(
        executor.context().temp_english_mode,
        "Should set temp_english_mode after undo"
    );
}

#[test]
fn test_tone_toggling() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "as" → "á" (apply sắc tone)
    executor.process('a');
    executor.process('s');
    assert_eq!(
        executor.syllable(),
        "á",
        "First tone application should produce á"
    );

    // Type "s" again → should undo → "as" (remove tone, keep key - Unikey behavior)
    executor.process('s');
    assert_eq!(
        executor.syllable(),
        "as",
        "Second tone key should undo to 'as'"
    );
    assert!(
        executor.context().temp_english_mode,
        "Should set temp_english_mode after tone undo"
    );
}

#[test]
fn test_uoi_tone_positioning() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Test "cười" (c + ươi + huyền tone)
    // In Vietnamese, "cười" = c + ư + ờ (ơ with huyền) + i
    // The tone goes on the 2nd vowel (ơ) for ươi pattern
    executor.process('c');
    executor.process('u');
    executor.process('w'); // uw → ư
    executor.process('o');
    executor.process('w'); // ưo+w → ươ (via Stage 6)
    executor.process('i');

    // Before tone: should be "cươi" (no tone yet)
    assert_eq!(executor.syllable(), "cươi");

    // Add huyền tone (f) → should produce "cười" (tone on ơ, 2nd vowel)
    executor.process('f');
    assert_eq!(executor.syllable(), "cười", "Tone on ơ for ươi pattern");

    executor.reset();

    // Test "trường" (tr + ườ + ng + huyền)
    // Type "truwowngf" → "trường"
    executor.process('t');
    executor.process('r');
    executor.process('u');
    executor.process('w'); // uw → ư
    executor.process('o');
    executor.process('w'); // ow → ơ
    executor.process('n');
    executor.process('g');

    // Before tone: should be "trương" (transforms done, no tone yet)
    assert_eq!(executor.syllable(), "trương");

    // Add huyền tone (f) → should produce "trường" (tone on ơ)
    executor.process('f');
    assert_eq!(
        executor.syllable(),
        "trường",
        "Tone on ơ for ươ with final consonant"
    );
}

// test_wowts_edge_case removed: relied on the retired Permutation stage which
// tried different mark orderings (w as standalone ư AND as part of ow→ơ).
// Under compose/recompute, canonical `ướt` is typed via `uwowts` or `uwots`.

#[test]
fn test_thuowr_edge_case() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // thuowr → thuở (NOT thưở!)
    // th + uo + w + r
    // The uo+w should produce uơ (since at end of word), NOT ươ
    // Then r adds tone hỏi → thuở
    for ch in "thuowr".chars() {
        executor.process(ch);
    }

    assert_eq!(executor.syllable(), "thuở", "thuowr should produce thuở");
}

#[test]
fn test_dduowjc_edge_case() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // dduowjc → được
    // dd → đ
    // u → đu
    // o → đuo
    // w → đươ (uo+w with following consonant should produce ươ)
    // j → được (nặng tone on ơ)
    // c → được
    for ch in "dduowjc".chars() {
        executor.process(ch);
    }

    assert_eq!(executor.syllable(), "được", "dduowjc should produce được");
}

#[test]
fn test_validation_permissive_mode() {
    let mut config = create_telex_config();
    // Set permissive mode (allow invalid syllables)
    config.validation = Some(ValidationSettings {
        syllable_structure: "vietnamese".to_string(),
        allow_invalid: true,
    });

    let mut executor = PipelineExecutor::new(config);

    // Type invalid syllable "xyz" - should be allowed in permissive mode
    executor.process('x');
    executor.process('y');
    executor.process('z');

    // Should continue processing (not rejected)
    assert!(executor.syllable().contains('x'));
}

#[test]
fn test_validation_strict_mode() {
    let mut config = create_telex_config();
    // Set strict mode (reject invalid syllables)
    config.validation = Some(ValidationSettings {
        syllable_structure: "vietnamese".to_string(),
        allow_invalid: false,
    });

    let mut executor = PipelineExecutor::new(config);

    // Type invalid syllable - should be rejected in strict mode
    // Note: In strict mode, invalid syllables trigger PassThrough
    executor.process('x');
    executor.process('y');
    let actions = executor.process('z');

    // In strict mode with invalid input, should get PassThrough behavior
    // This test verifies the stage was created with strict mode enabled
    assert!(executor.syllable().is_empty() || !actions.is_empty());
}

#[test]
fn test_transform_undo_with_z() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "aaz" → might undo transform (depends on retrofix implementation)
    executor.process('a');
    executor.process('a'); // aa → â
    let _actions = executor.process('z'); // retrofix key

    // Result depends on retrofix stage implementation
    // Just verify it doesn't crash
    assert!(!executor.syllable().is_empty());
}

#[test]
fn test_special_characters() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    let special = vec!['@', '#', '$', '%', '&', '*'];

    for ch in special {
        executor.reset();
        let actions = executor.process(ch);

        // Should pass through
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Commit(text) => assert_eq!(text, &ch.to_string()),
            _ => panic!("Expected Commit for '{}'", ch),
        }
    }
}

// === Integration Tests ===

#[test]
fn test_full_sentence() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "Vieejt Nam"
    // "Vieejt"
    executor.process('V');
    executor.process('i');
    executor.process('e');
    executor.process('e'); // ee → ê
    executor.process('j'); // add dot below
    executor.process('t');
    assert_eq!(executor.syllable(), "Việt");

    // Space
    executor.process(' ');
    assert_eq!(executor.syllable(), "");

    // "Nam"
    executor.process('N');
    executor.process('a');
    executor.process('m');
    assert_eq!(executor.syllable(), "Nam");
}

#[test]
fn test_sequential_transforms() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // dd → đ, then aw → ă
    executor.process('d');
    executor.process('d');
    assert_eq!(executor.syllable(), "đ");

    executor.reset();

    executor.process('a');
    executor.process('w');
    assert_eq!(executor.syllable(), "ă");
}

#[test]
fn test_transform_then_tone_then_more_chars() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "aans" → "ấn"
    executor.process('a');
    executor.process('a'); // aa → â
    executor.process('s'); // add acute → ấ
    executor.process('n');

    assert_eq!(executor.syllable(), "ấn");
}

// === Pipeline Configuration Tests ===

#[test]
fn test_custom_stage_order() {
    let mut config = create_telex_config();
    // Explicitly set stage order
    config.pipeline.enabled = vec![
        "validation".to_string(),
        "transform".to_string(),
        "tone".to_string(),
    ];

    let executor = PipelineExecutor::new(config);

    // Should have: normalization, gatekeeper, validation, transform, tone, output
    // Total: 6 stages (some auto-added)
    assert!(executor.stage_count() >= 3);
}

#[test]
fn test_empty_config_uses_defaults() {
    let config = PipelineConfig::new("telex");
    let executor = PipelineExecutor::new(config);

    // Should still create executor with default stages
    assert!(executor.stage_count() > 0);
}

// === Action Type Tests ===

#[test]
fn test_commit_action() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    let actions = executor.process('x');

    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], Action::Commit(_)));
}

#[test]
fn test_replace_action() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    executor.process('a');
    let actions = executor.process('a');

    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], Action::Replace { .. }));
}

// === Stress Tests ===

#[test]
fn test_long_sequence() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type a long sequence
    let input = "abcdefghij";
    for ch in input.chars() {
        executor.process(ch);
    }

    // Should handle without crashing
    assert!(!executor.syllable().is_empty());
}

#[test]
fn test_many_resets() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    for _ in 0..100 {
        executor.process('a');
        executor.reset();
    }

    assert_eq!(executor.syllable(), "");
}

#[test]
fn test_alternating_chars() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Alternating characters
    for _ in 0..10 {
        executor.process('a');
        executor.process('b');
        executor.reset();
    }

    // Should complete without issues
    assert_eq!(executor.syllable(), "");
}

/// Test for English fallback with "dessign" pattern
///
/// Expected flow:
/// - d → "d"
/// - e → "de"
/// - s → "dé" (tone applied to 'e')
/// - s → "des" (duplicate tone, fallback to English, temp_english_mode=true)
/// - i → "desi" (pass through in English mode)
/// - g → "desig" (pass through)
/// - n → "design" (pass through)
#[test]
fn test_dessign_english_fallback() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "dessign" step by step
    executor.process('d');
    assert_eq!(executor.syllable(), "d", "After 'd'");

    executor.process('e');
    assert_eq!(executor.syllable(), "de", "After 'e'");

    executor.process('s');
    // After first 's', we expect tone to be applied: de → dé
    println!(
        "After first 's': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );

    executor.process('s');
    // After second 's' (duplicate tone), should fallback: dé → des
    // And temp_english_mode should be true
    println!(
        "After second 's': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(
        executor.syllable(),
        "des",
        "After double 's' should be 'des'"
    );
    assert!(
        executor.is_temp_english_mode(),
        "Should be in temp English mode after fallback"
    );

    executor.process('i');
    println!(
        "After 'i': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(executor.syllable(), "desi", "After 'i' should be 'desi'");

    executor.process('g');
    println!(
        "After 'g': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(executor.syllable(), "desig", "After 'g' should be 'desig'");

    executor.process('n');
    println!(
        "After 'n': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(
        executor.syllable(),
        "design",
        "After 'n' should be 'design'"
    );
}

/// Test for English fallback with "tissot" pattern
///
/// This tests that TONE KEYS remain in English mode after fallback.
/// Expected flow:
/// - t → "t"
/// - i → "ti"
/// - s → "tí" (tone applied to 'i')
/// - s → "tis" (duplicate tone, fallback to English, temp_english_mode=true)
/// - o → "tiso" (pass through in English mode - but 'o' is vowel!)
/// - t → "tisot" (pass through)
///
/// Critical: After fallback, 's' and 'o' should NOT trigger tone application!
#[test]
fn test_tissot_english_fallback() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "tissot" step by step
    executor.process('t');
    assert_eq!(executor.syllable(), "t", "After 't'");

    executor.process('i');
    assert_eq!(executor.syllable(), "ti", "After 'i'");

    executor.process('s');
    // After first 's', we expect tone to be applied: ti → tí
    println!(
        "After first 's': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(executor.syllable(), "tí", "After first 's' should be 'tí'");

    executor.process('s');
    // After second 's' (duplicate tone), should fallback: tí → tis
    // And temp_english_mode should be true
    println!(
        "After second 's': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(
        executor.syllable(),
        "tis",
        "After double 's' should be 'tis'"
    );
    assert!(
        executor.is_temp_english_mode(),
        "Should be in temp English mode after fallback"
    );

    executor.process('o');
    // In temp_english_mode, 'o' should just append, NOT trigger any Vietnamese processing
    println!(
        "After 'o': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(executor.syllable(), "tiso", "After 'o' should be 'tiso'");
    assert!(
        executor.is_temp_english_mode(),
        "Should still be in temp English mode"
    );

    executor.process('t');
    println!(
        "After final 't': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(
        executor.syllable(),
        "tisot",
        "After final 't' should be 'tisot'"
    );
}

/// Test for English fallback with multiple consecutive tone keys: "tisssot"
///
/// Critical: After fallback (ss), third and further 's' should pass through as English!
/// Expected flow:
/// - t → "t"
/// - i → "ti"
/// - s → "tí" (tone applied)
/// - s → "tis" (duplicate tone, fallback, temp_english_mode=true)
/// - s → "tiss" (third s, should pass through in English mode!)
/// - o → "tisso"
/// - t → "tissot"
#[test]
fn test_multiple_tone_keys_after_fallback() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "tisssot" step by step
    executor.process('t');
    executor.process('i');
    assert_eq!(executor.syllable(), "ti", "After 'ti'");

    executor.process('s');
    println!(
        "After 1st 's': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(executor.syllable(), "tí", "After 1st 's' should be 'tí'");

    executor.process('s');
    println!(
        "After 2nd 's': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(executor.syllable(), "tis", "After 2nd 's' should be 'tis'");
    assert!(
        executor.is_temp_english_mode(),
        "Should be in temp English mode"
    );

    // CRITICAL: Third 's' should just append, NOT toggle tone!
    executor.process('s');
    println!(
        "After 3rd 's': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(
        executor.syllable(),
        "tiss",
        "After 3rd 's' should be 'tiss' NOT 'tís'!"
    );
    assert!(
        executor.is_temp_english_mode(),
        "Should still be in temp English mode"
    );

    executor.process('o');
    println!(
        "After 'o': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(executor.syllable(), "tisso", "After 'o' should be 'tisso'");

    executor.process('t');
    println!(
        "After 't': syllable='{}', temp_english_mode={}",
        executor.syllable(),
        executor.is_temp_english_mode()
    );
    assert_eq!(
        executor.syllable(),
        "tissot",
        "After 't' should be 'tissot'"
    );
}

// ── Defensive syllable-length cap (bounds O(n²) + desync blast radius) ────────

#[test]
fn test_syllable_length_cap_latches_passthrough() {
    // A run-on buffer with no separator, no undo, no Vietnamese transform:
    // 20 distinct letters. Past the 16-raw cap the engine must stop recomputing
    // from raw and latch literal passthrough.
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);
    for ch in "bcdfghklmnpqrtvzbcdf".chars() {
        executor.process(ch);
    }
    assert!(
        executor.is_temp_english_mode(),
        "run-on buffer past the cap must latch literal passthrough"
    );
}

#[test]
fn test_long_valid_syllable_not_capped() {
    // "nghieengf" (9 raw) → nghiềng. A legitimate long syllable must NOT trip
    // the cap or be treated as run-on English.
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);
    for ch in "nghieengf".chars() {
        executor.process(ch);
    }
    assert!(
        !executor.is_temp_english_mode(),
        "a legitimate long syllable must not be capped: got '{}'",
        executor.syllable()
    );
    assert_eq!(executor.syllable(), "nghiềng");
}

// ── Non-adjacent đ (flexible typing): "datjd" → "đạt" ─────────────────────────

#[test]
fn test_nonadjacent_dd_with_tone() {
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "datjd".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "đạt",
        "trailing 'd' after a toned syllable must turn the onset into đ"
    );
}

#[test]
fn test_nonadjacent_dd_with_coda_no_tone_demotes_to_literal() {
    // Phase 2 (attestation gate): "đat" (ngang tone) is not a real Vietnamese
    // word — Vietnamese checked syllables (coda p/t/c/ch) never take ngang
    // tone, so "đat" can never be attested. The backward-referring đ mark is
    // always flagged non-adjacent (see `segment::mark_non_adjacent`), so it
    // now demotes to a literal 'd' — the exact same bug class as
    // `"data"` → `"dât"`. It still self-heals once a tone key arrives
    // (`test_nonadjacent_dd_with_tone` above: "datjd" → "đạt", attested).
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "datd".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "datd",
        "unattested 'đat' must demote to the literal keystrokes"
    );
}

#[test]
fn test_bare_dad_stays_english() {
    // "dad": second 'd' is the last raw char, no vowel follows it.
    // The open-syllable non-adjacent đ guard must NOT fire — English "dad" preserved.
    // (Fast-typing "dodong"→"đông" fires because vowel 'o' follows the second 'd'.)
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "dad".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "dad");
}

// ── Phase 2: attestation gate self-heal (executor-level) ──────────────────────

#[test]
fn test_viete_then_j_self_heals_to_viet() {
    // "viete" alone: non-adjacent 'e' produces unattested "viêt" → demotes to
    // literal "viete". Recompute-from-raw means the NEXT keystroke ('j') sees
    // the whole raw buffer again ("vietej") and re-derives from scratch —
    // "viet" is one vowel group with a valid coda, so the non-adjacent 'e'
    // fires again, and this time "việt" (with the dot tone) IS attested.
    // Self-heal across keystrokes, mark-typed-last ordering (see phase doc:
    // the reverse order "vietj"+"e" is pre-existing known-broken, unrelated
    // to this phase).
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "viete".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "viete",
        "'viete' alone must demote to literal (unattested 'viêt')"
    );
    ex.process('j');
    assert_eq!(
        ex.context().syllable_buffer,
        "việt",
        "'vietej' must self-heal to attested 'việt' once the tone key arrives"
    );
}

// ── Phase 2: fallback bypass regression (red-team C2) — executor level ───────

#[test]
fn test_dataeee_no_bypass_diacritic() {
    // Differs from the one-shot `compose()` unit test (which sees only the
    // final 7-key raw buffer and gives "dataee" via the gated
    // `check_transform_toggle` path): the INCREMENTAL executor reaches the
    // gate-then-demote-then-literal-fallback path already at 6 keys
    // ("dataee" fails `could_be_vietnamese` even after demoting, and
    // elongation is intentionally not attempted from a demoted pass — see
    // `try_elongation_fallback`), latching `temp_english_mode` with the
    // CLEAN literal "dataee" one key early. The 7th key then appends
    // literally per `ComposeStage`'s temp-English contract, giving the full
    // literal "dataeee" — strictly better than the pre-Phase-2 baseline,
    // which latched at the same point with the spurious diacritic "dâtee"
    // and ended on "dâteee".
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "dataeee".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "dataeee");
    assert!(
        !ex.context().syllable_buffer.contains(['â', 'ê']),
        "no diacritic must leak through the bypass at any keystroke"
    );
}

#[test]
fn test_databaaa_no_bypass_diacritic() {
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "databaaa".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "databaa");
}

#[test]
fn test_vietess_no_bypass_diacritic() {
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "vietess".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "vietes");
}

// ── Phase 4: non-adjacent transform undo (executor level) ────────────────────
// Test Scenario Matrix from phase-04-nonadjacent-undo.md. See the deviation
// note in `compose::tests` (src/compose/tests.rs) for why "can6" (not the
// matrix's "cana7") and "dand" (not "dodongd") are used — both verified
// empirically against this build's real rule tables.

#[test]
fn test_cana_a_undoes_to_literal_latched() {
    // Pre-condition: "cana" alone is the attested collision "cân" this escape
    // hatch targets (see compose::tests::medium_cana_collision_canal_self_heals).
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "cana".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "cân");
    ex.process('a');
    assert_eq!(
        ex.context().syllable_buffer,
        "cana",
        "retyping 'a' right after the collision must undo it to the literal keystrokes"
    );
    assert!(
        ex.context().temp_english_mode,
        "undo must latch English passthrough"
    );
}

#[test]
fn test_cana_a_n_latches_until_separator() {
    // Once undone, ComposeStage's temp_english branch appends subsequent keys
    // LITERALLY onto the already-undone buffer instead of recomputing from
    // raw — "cana"+"a" undoes to "cana", then "n" appends directly: "canan".
    // (The one-shot `compose()` on the full 6-key raw buffer agrees — the
    // interior spent-undo fold recognizes the "canaa" undo event from raw
    // alone; see `compose::tests::high_cana_latch_survives_recompute_no_reentry`.)
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "canaan".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "canan",
        "latched passthrough must append literally, not re-recompute from raw"
    );
}

#[test]
fn test_cana_uppercase_trigger_undoes_case_insensitively() {
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "canaA".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "cana",
        "an uppercase retype of the trigger key must still undo"
    );
    assert!(ex.context().temp_english_mode);
}

#[test]
fn test_vni_digit_undo_parity_with_telex() {
    // Method parity (S8): VNI digit-triggered equivalent of "cana"+"a". This
    // codebase's canonical VNI digit for â is '6' (`presets::vni_config`),
    // not '7' as the matrix's literal example string states — '7' is only
    // ever registered for the o7/u7 horn, never for 'a'.
    let mut ex = PipelineExecutor::new(buttre_engine::pipeline::presets::vni_config());
    for ch in "can6".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "cân",
        "can6 must be the VNI attested collision analogous to Telex cana"
    );
    ex.process('6');
    assert_eq!(
        ex.context().syllable_buffer,
        "can6",
        "retyping the VNI digit trigger must undo exactly like Telex"
    );
    assert!(ex.context().temp_english_mode);
}

#[test]
fn test_dand_d_consonant_class_undo_equivalence_note() {
    // Substitute for the matrix's "dodongd" row: đ analogue of "cana"+"a".
    // "dodongd" does not satisfy the immediacy contract (đ fires on
    // "dodong"'s 3rd raw key, but "dodong" itself ends in 'g', not 'd' — the
    // same non-immediacy shape "vieteje" demonstrates must NOT undo). "dand"
    // fires đ on its OWN final raw key (backward-referring đ with a coda —
    // see `test_nonadjacent_dd_with_coda_no_tone_demotes_to_literal` above
    // for the sibling unattested case), so retyping 'd' right after DOES
    // satisfy immediacy.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "dand".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "đan",
        "dand must be an attested đ collision (đan = to knit/weave)"
    );
    ex.process('d');
    assert_eq!(
        ex.context().syllable_buffer,
        "dand",
        "retyping 'd' right after dand must undo the đ mark"
    );
    assert!(ex.context().temp_english_mode);
}

#[test]
fn test_vieteje_immediacy_violated_no_undo() {
    // "vietej" self-heals to attested "việt" (test_viete_then_j_self_heals_to_viet
    // above). Appending one more 'e' makes the retyped key's predecessor the
    // tone 'j', not the fired 'e' mark's own position — immediacy fails, so
    // no undo fires; the extra key instead runs the ordinary (Phase-4-
    // unrelated) English-fallback path on the full 7-key buffer.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "vietej".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "việt");
    ex.process('e');
    assert_eq!(
        ex.context().syllable_buffer,
        "vieteje",
        "must not undo: immediacy violated (retyped key does not follow the fired mark)"
    );
    assert!(ex.context().temp_english_mode);
}

// ── Phase 5: regression suite — data-class corpus at executor level ─────────
// The compose()-level unit tests (compose::tests::high_data_class_words_stay_literal
// et al.) already cover these at the pure-function layer; these executor-level
// counterparts confirm the SAME words survive the incremental, keystroke-by-
// keystroke path (ComposeStage + Gatekeeper + Output diffing), which is what
// a real host application actually drives.

#[test]
fn test_data_class_words_stay_literal_at_executor_level() {
    for word in [
        "data", "meme", "photo", "papa", "salsa", "radar", "banana", "canal", "media", "dad",
        "dads", "nasa",
    ] {
        let config = create_telex_config();
        let mut ex = PipelineExecutor::new(config);
        for ch in word.chars() {
            ex.process(ch);
        }
        assert_eq!(
            ex.context().syllable_buffer,
            word,
            "'{word}' must stay literal at the incremental executor level, not just at compose()"
        );
    }
}

#[test]
fn test_reset_accepted_collision_at_executor_level() {
    // "reset" composes to the real, attested syllable "rết" (centipede) — an
    // accepted collision by design (see syllable_list.rs's
    // known-attested-collisions comment). The escape hatch is Phase 4's
    // non-adjacent undo (retype the trigger key), not a golden fixup.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "reset".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "rết");
}

// ── Phase 2: evidence-based un-latch ─────────────────────────────────────────
// Test Scenario Matrix from phase-02-evidence-unlatch.md.

#[test]
fn test_vietj_e_unlatches_to_viet() {
    // The flagship fix: "vietj" latches to literal English ('j' fires the dot
    // tone on bare "e", which is not a valid Vietnamese syllable on its own),
    // then the completing non-adjacent 'e' mark (doubling back to "viet"'s
    // own 'e') recomposes the FULL raw to attested "việt" and un-latches.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "vietj".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "vietj",
        "'vietj' alone must still latch literal"
    );
    assert!(
        ex.context().temp_english_mode,
        "'vietj' must be latched before the completing key"
    );

    ex.process('e');
    assert_eq!(
        ex.context().syllable_buffer,
        "việt",
        "'vietje' must un-latch to attested 'việt' — the class this phase fixes"
    );
    assert!(
        !ex.context().temp_english_mode,
        "un-latch must clear temp_english_mode"
    );
}

#[test]
fn test_vietj_e_emits_corrective_replace_action() {
    // The un-latch must surface as a real Replace action to the host, not
    // just a silent internal state change (OutputStage's diff emits the
    // minimal backspace+text needed to turn "vietj" on screen into "việt").
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "vietj".chars() {
        ex.process(ch);
    }
    let actions = ex.process('e');
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Replace { backspace_count, .. } if *backspace_count > 0)),
        "un-latch must emit a corrective Replace with a non-zero backspace, got {actions:?}"
    );
}

#[test]
fn test_cana_a_more_vowels_stays_literal_condition_d() {
    // "cana"+"a" undoes the non-adjacent â mark (latches, buffer "cana").
    // Continuing to type the SAME trigger vowel ('a') must stay literal:
    // the adjacent-toggle tail "aaa" is itself a fresh undo/toggle event per
    // the P6 parity fold, so condition (d) vetoes any resurrection of 'â'.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "canaa".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "cana");
    assert!(ex.context().temp_english_mode);

    ex.process('a');
    assert_eq!(
        ex.context().syllable_buffer,
        "canaa",
        "further vowel taps after the undo must never resurrect 'â'"
    );
    assert!(ex.context().temp_english_mode, "must remain latched");
    assert!(!ex.context().syllable_buffer.contains('â'));
}

#[test]
fn test_unlatch_then_more_keys_relatches_bidirectional() {
    // After "vietje" un-latches to "việt" (temp_english_mode == false), the
    // NEXT key goes through the ordinary (unmodified) normal recompute path
    // — exactly like a fresh word — and can re-latch to English again if the
    // evidence says so. This is OpenKey's bidirectional model: un-latch is
    // not itself a one-way valve either.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "vietje".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.context().syllable_buffer, "việt");
    assert!(!ex.context().temp_english_mode);

    ex.process('x');
    assert_eq!(
        ex.context().syllable_buffer,
        "vietjex",
        "an extra key that makes the word implausible must re-latch to literal"
    );
    assert!(
        ex.context().temp_english_mode,
        "re-latch must fire exactly like a fresh word would"
    );
}

#[test]
fn test_seventeen_char_run_on_never_unlatches() {
    // 17+ raw keys: the run-on cap latches, and per the cap exemption no
    // further probing occurs even when later keys are trigger-eligible —
    // covered at the instrumentation level by
    // `compose_stage::tests::no_probe_past_run_on_cap`; this is the
    // executor-observable-behavior counterpart.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "bcdfghklmnpqrtvzb".chars() {
        ex.process(ch);
    } // 17 chars
    assert!(
        ex.context().temp_english_mode,
        "run-on buffer must latch past the cap"
    );
    ex.process('a'); // trigger-eligible, must still not un-latch
    ex.process('s'); // tone key, must still not un-latch
    assert!(
        ex.context().temp_english_mode,
        "run-on buffer must never un-latch"
    );
    assert_eq!(ex.context().syllable_buffer, "bcdfghklmnpqrtvzbas");
}

#[test]
fn test_uppercase_midword_unlatch_case_correct() {
    // All-uppercase input exercises `apply_case_mask`'s "all case-bearing
    // chars uppercase" fast path on the ADOPTED probe text, proving the
    // un-latch path is case-correct, not just the normal recompute path.
    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config);
    for ch in "VIETJE".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "VIỆT",
        "uppercase un-latch must produce upper-cased attested text, not lowercase 'việt'"
    );
    assert!(!ex.context().temp_english_mode);
}

// ── Phase 5: VNI `nhat61` frame-level assertion (red-team M7) ────────────────
//
// Golden snapshots only pin the FINAL text per case — a regression that
// flickers to an incorrect intermediate frame (then silently self-repairs on
// a later keystroke) would be invisible to the golden suite. This test
// inspects the actual `Action` stream returned at EVERY keystroke — not just
// the final `syllable_buffer` — reconstructing the on-screen text exactly as
// a host application's text buffer would see it (mirrors `gen_golden.rs`'s
// `replay` helper), and asserts the full per-keystroke frame sequence.

#[test]
fn test_vni_nhat61_frame_level_no_literal_flicker() {
    let config = buttre_engine::pipeline::presets::vni_config();
    let mut ex = PipelineExecutor::new(config);
    let mut screen = String::new();
    let mut frames: Vec<String> = Vec::new();
    for ch in "nhat61".chars() {
        for action in ex.process(ch) {
            match action {
                Action::Commit(s) | Action::ConfirmComposition(s) => screen.push_str(&s),
                Action::Replace {
                    backspace_count,
                    text,
                } => {
                    let new_len = screen.chars().count().saturating_sub(backspace_count);
                    screen = screen.chars().take(new_len).collect();
                    screen.push_str(&text);
                }
                Action::UpdateComposition { .. }
                | Action::DoNothing
                | Action::ShowCandidates { .. }
                | Action::HideCandidates => {}
            }
        }
        frames.push(screen.clone());
    }
    // Digit '6' (shape-attested, non-alphabetic trigger) must land directly on
    // "nhât" — never dip through a literal "nhat6" or any other flicker frame
    // — and digit '1' then completes the tone to "nhất".
    assert_eq!(
        frames,
        vec!["n", "nh", "nha", "nhat", "nhât", "nhất"],
        "no literal-flicker frame may appear between shape-attestation ('6') and tone ('1')"
    );
    assert!(
        !ex.is_temp_english_mode(),
        "final state must not be latched English"
    );
}

// ── Phase 2: perf — probe cost stays bounded (red-team M2/M3) ────────────────
//
// `PipelineExecutor` has no backspace of its own — the multiword rolling
// window + `find_window_backspace_raw` candidate search live in buttre-core's
// `Keyboard` (a different crate, outside this phase's file ownership). This
// reproduces the SAME worst-case shape that search drives — an O(window)
// reset-and-replay over every single-key-removal candidate of a LATCHED
// buffer — directly at the buttre-engine level, to prove the pre-filter +
// run-on-cap exemption keep the probe's added cost bounded under it.

#[test]
fn perf_latched_typing_and_backspace_storm_bounded() {
    use std::time::Instant;

    // ── Baseline: a same-length word that never latches at all (no probe
    // ever runs) — the phase's own success criterion is "<2x unlatched cost".
    let start = Instant::now();
    for _ in 0..1000 {
        let config = create_telex_config();
        let mut ex = PipelineExecutor::new(config);
        for ch in "thuongw".chars() {
            ex.process(ch);
        } // 7 chars, never latches
    }
    let unlatched_elapsed = start.elapsed();

    // ── Latched-word typing: exercises the probe path every iteration
    // ("vietj" latches, then "e" is a trigger key that runs a full probe
    // compose before un-latching).
    let start = Instant::now();
    for _ in 0..1000 {
        let config = create_telex_config();
        let mut ex = PipelineExecutor::new(config);
        for ch in "vietje".chars() {
            ex.process(ch);
        }
    }
    let latched_typing_elapsed = start.elapsed();

    println!(
        "[perf] unlatched baseline (1000x 'thuongw', never latches): {unlatched_elapsed:?} total, {:?}/iter",
        unlatched_elapsed / 1000
    );
    let ratio =
        latched_typing_elapsed.as_nanos() as f64 / unlatched_elapsed.as_nanos().max(1) as f64;
    // INFORMATIONAL ONLY, not asserted: both loops run in microseconds, so their
    // ratio is dominated by scheduler jitter when the full workspace test suite runs
    // in parallel (CPU contention) — a hard `ratio < 2.0` here flakes under load while
    // being ~1.05x in isolation. The meaningful, noise-robust guarantee is the absolute
    // per-run ceiling asserted below (<100ms), which is what proves the probe never
    // becomes the O(n^2)/O(n^3) blowup red-team M2 warned about.
    println!(
        "[perf] latched/unlatched ratio: {ratio:.2}x (informational; absolute bound is the gate)"
    );

    // ── Backspace-storm-shaped replay: a 20-char latched buffer (past the
    // 16-char run-on cap, so every candidate ALSO exercises the cap
    // exemption), replayed once per single-key-removal candidate — the same
    // O(window) shape `find_window_backspace_raw` drives per keystroke.
    let window: Vec<char> = "vietjevietjevietjevi".chars().collect(); // 20 chars
    let start = Instant::now();
    for drop_idx in (0..window.len()).rev() {
        let candidate: Vec<char> = window
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != drop_idx)
            .map(|(_, c)| *c)
            .collect();
        let config = create_telex_config();
        let mut ex = PipelineExecutor::new(config);
        for &ch in &candidate {
            ex.process(ch);
        }
    }
    let storm_elapsed = start.elapsed();

    println!(
        "[perf] latched-word typing (1000x 'vietje' incl. probe+unlatch): {latched_typing_elapsed:?} total, {:?}/iter",
        latched_typing_elapsed / 1000
    );
    println!(
        "[perf] backspace-storm-shaped replay ({} single-key-removal candidates over a {}-char latched window): {storm_elapsed:?} total",
        window.len(),
        window.len()
    );

    // Generous bound (plan.md: "<1 ms/keystroke at Keyboard level incl.
    // backspace storms"): 1000 full-word retypes and 20 full-window replays
    // must stay bounded, proving the probe never turns into the unbounded
    // O(n^2)/O(n^3) blowup red-team M2 warned about. 500ms (not 100ms):
    // CI runs `cargo test` in DEBUG mode on slow shared runners, where the
    // healthy path already measured ~111ms — a genuine complexity blowup
    // measures in SECONDS on these buffer sizes, so the looser bound still
    // catches the regression class this test exists for. Even at 500ms the
    // per-keystroke cost (6000 keystrokes) is ~83µs, far inside the plan's
    // 1ms/keystroke budget.
    assert!(
        latched_typing_elapsed.as_millis() < 500,
        "latched typing must stay fast: {latched_typing_elapsed:?}"
    );
    assert!(
        storm_elapsed.as_millis() < 500,
        "backspace-storm-shaped replay must stay bounded: {storm_elapsed:?}"
    );
}

// ── Phase 3: word-boundary final repair — TSF (composition) delivery ────────
//
// The TSF stub's Replace handler ignores `backspace_count` (it only ever
// rewrites the live composition), so a separate Replace-then-Confirm pair is
// unexecutable there — the repair MUST be folded directly into the FIRST
// action's payload (`ConfirmComposition`) rather than delivered as a separate
// Replace. These tests exercise `PipelineExecutor` directly with
// `use_composition = true` (the TSF configuration), asserting on `actions[0]`.

fn vni_composition_config() -> PipelineConfig {
    let mut config = buttre_engine::pipeline::presets::vni_config();
    config.pipeline.use_composition = true;
    config
}

fn telex_composition_config() -> PipelineConfig {
    let mut config = create_telex_config();
    config.pipeline.use_composition = true;
    config
}

fn confirm_text(actions: &[Action]) -> &str {
    match &actions[0] {
        Action::ConfirmComposition(text) => text,
        other => panic!("expected ConfirmComposition as actions[0], got {other:?}"),
    }
}

#[test]
fn boundary_repair_vni_nhat6_space_restores_literal() {
    let mut ex = PipelineExecutor::new(vni_composition_config());
    for ch in "nhat6".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.syllable(),
        "nhât",
        "pre-boundary display is the shape-attested intermediate"
    );
    let actions = ex.process(' ');
    assert_eq!(actions.len(), 2, "ConfirmComposition + Commit(' ')");
    assert_eq!(
        confirm_text(&actions),
        "nhat6",
        "shape-only inferred mark must repair to literal raw at the boundary"
    );
    assert_eq!(actions[1], Action::Commit(" ".to_string()));
}

#[test]
fn boundary_repair_vni_nhat61_space_untouched_exact_attested() {
    let mut ex = PipelineExecutor::new(vni_composition_config());
    for ch in "nhat61".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "nhất");
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "nhất",
        "exact-attested word must be untouched"
    );
}

#[test]
fn boundary_repair_telex_vietej_space_untouched_exact_path() {
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "vietej".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "việt");
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "việt",
        "Telex's exact-attestation path is already correct, untouched by closed"
    );
}

#[test]
fn boundary_repair_data_space_no_double_repair() {
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "data".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "data");
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "data",
        "already-literal word must not be touched again"
    );
}

#[test]
fn boundary_repair_reset_space_accepted_collision_untouched() {
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "reset".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "rết");
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "rết",
        "exact-attested collision must not be repaired"
    );
}

// ── Deliberate-undo latch survives the boundary (escape-hatch integrity) ─────
// A word the user explicitly forced literal through a double-tone-key undo
// must COMMIT exactly as displayed. The stateless closed projection cannot
// see the mid-buffer undo event, so before the `latch_from_undo` guard it
// re-applied the removed tone at the space ("tesst" → displayed "test" but
// committed "tét"), making English words like test/text/reset untypeable by
// ANY key sequence.

#[test]
fn boundary_repair_tesst_space_commits_displayed_test() {
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "tesst".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.syllable(),
        "test",
        "double-s undo then 't' displays 'test'"
    );
    assert!(
        ex.is_temp_english_mode(),
        "undo latches English passthrough"
    );
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "test",
        "deliberate-undo latch must commit as displayed, never re-toned to 'tét'"
    );
}

#[test]
fn boundary_repair_texxt_space_commits_displayed_text() {
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "texxt".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "text");
    let actions = ex.process(' ');
    assert_eq!(confirm_text(&actions), "text");
}

#[test]
fn boundary_repair_reseet_space_commits_displayed_reset() {
    // Non-adjacent-mark undo variant: "rese" fires the inferred 'e' mark
    // ("rế"), retyping 'e' undoes it to literal "rese", then 't' appends —
    // the ONLY key sequence that renders "reset". It must survive commit.
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "reseet".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "reset");
    let actions = ex.process(' ');
    assert_eq!(confirm_text(&actions), "reset");
}

#[test]
fn boundary_repair_chwowng_space_still_rescued_to_chuong() {
    // Counter-case pinning the guard's precision: chwowng's latch is NOT a
    // deliberate undo (the mid-word "chwow" frame trips the structural
    // English fallback), so the closed-projection repair must still fire
    // and rescue the committed text to the attested "chương".
    // Uses the real preset (not this file's local config): the onset
    // w-shorthand needs the 1-char "w"→"ư" rule.
    let mut config = buttre_engine::pipeline::presets::telex_config();
    config.pipeline.use_composition = true;
    let mut ex = PipelineExecutor::new(config);
    for ch in "chwowng".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "chwowng", "mid-word latch shows literal");
    assert!(ex.is_temp_english_mode());
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "chương",
        "non-deliberate latch keeps the boundary repair"
    );
}

#[test]
fn boundary_repair_adjacent_vieet_space_never_repaired() {
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "vieet".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "viêt");
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "viêt",
        "direct/adjacent typing carries no inferred mark, never repaired"
    );
}

#[test]
fn boundary_repair_disabled_flag_keeps_old_behavior() {
    let mut config = vni_composition_config();
    config.boundary_repair = false;
    let mut ex = PipelineExecutor::new(config);
    for ch in "nhat6".chars() {
        ex.process(ch);
    }
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "nhât",
        "boundary_repair=false must reproduce the old shape-attested-only behavior exactly"
    );
}

#[test]
fn boundary_repair_noop_after_p2_unlatch_no_double_replace() {
    // Interaction with Phase 2: a word that un-latched mid-word (`should_unlatch`)
    // is exact-attested BY DEFINITION (condition (b) of the un-latch decision) —
    // boundary repair must be a complete no-op here, and the action list must
    // be exactly [ConfirmComposition, Commit] — no extra Replace anywhere.
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "vietje".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "việt", "P2 un-latch must have already fired");
    assert!(!ex.is_temp_english_mode());
    let actions = ex.process(' ');
    assert_eq!(
        actions.len(),
        2,
        "no double-Replace: exactly ConfirmComposition + Commit"
    );
    assert_eq!(confirm_text(&actions), "việt");
    assert_eq!(actions[1], Action::Commit(" ".to_string()));
}

#[test]
fn boundary_repair_case_masked_diff_vieejt_space() {
    // Red-team M2: the repair diff must be computed against the CASE-MASKED
    // display form, not the lowercase-anchored `compose` output — otherwise a
    // mixed-case word downcases on repair. "Vieejt" (leading-cap Telex) is
    // already exact-attested ("Việt"), so this also doubles as a no-op-repair
    // case-preservation regression guard.
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "Vieejt".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "Việt");
    let actions = ex.process(' ');
    assert_eq!(
        confirm_text(&actions),
        "Việt",
        "case must survive the boundary-repair probe"
    );
}

#[test]
fn boundary_repair_digits_after_word_do_not_commit_telex() {
    // Telex digits continue (Gatekeeper `Continue`s them) rather than commit —
    // pin that no boundary-commit action ever fires while typing them, so the
    // repair hook stays untouched/inert for this path.
    let mut ex = PipelineExecutor::new(telex_composition_config());
    for ch in "vietje".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "việt");
    for ch in "2024".chars() {
        let actions = ex.process(ch);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::ConfirmComposition(_))),
            "digit '{ch}' must not trigger a word-boundary commit"
        );
    }
}

#[test]
fn boundary_repair_enter_no_separator_no_pass_through_hook() {
    // `PipelineExecutor` has no separate Enter handling of its own — Enter
    // reaches the platform layer directly (TSF's own buffer-reset-key branch,
    // Hook's `is_buffer_reset_key`), never `PipelineExecutor::process`. This
    // guards the scope boundary: the executor-level boundary_repair() probe
    // must still report the pending correction on demand (platform layers
    // query it explicitly before their own commit), but nothing here
    // auto-fires it merely by sitting mid-word.
    let mut ex = PipelineExecutor::new(vni_composition_config());
    for ch in "nhat6".chars() {
        ex.process(ch);
    }
    assert_eq!(ex.syllable(), "nhât");
    assert_eq!(
        ex.boundary_repair(),
        Some("nhat6".to_string()),
        "probe available on demand for platform Enter/reset-key handlers"
    );
}

// ── Phase 8: cross-phase interaction tests ───────────────────────────────────
// Test Scenario Matrix item 1 from phase-08-regression-suite.md — proving the
// Combined Contract (plan.md) holds when P2/P3/P5/P6 coexist. The
// un-latch(P2)+boundary(P3) row already has a dedicated test above
// (`boundary_repair_noop_after_p2_unlatch_no_double_replace`); the remaining
// rows follow.

#[test]
fn coda_k_overlay_promotes_via_single_consult_point() {
    // Row: coda-k (P6) + overlay (P5) — a k-coda syllable counts as attested
    // via the SAME single overlay-aware consult point
    // (`pipeline::validation::is_attested_overlay`) any other syllable does,
    // proving P6's coda-k table extension and P5's overlay don't need (and
    // don't have) a separate k-coda-aware code path.
    //
    // "cakw" (c,a,k,w) fires Telex's standalone 'w' modifier non-adjacently
    // on 'a' — separated from its target by the coda consonant 'k', the same
    // shape as the flagship "cana"+'a' non-adjacent-doubling case — producing
    // the structurally-valid (nucleus "ă" takes coda "k" per P6's table) but
    // UNATTESTED k-coda shape "căk" (onset 'c' is not among the 9 P6-embedded
    // k-coda dict entries: búk/lăk/lắk/măk/úk/ăk/đăk/đắk/ắk). Without an
    // overlay this demotes to literal "cakw", exactly like "data"/"dât";
    // with "căk" in the overlay it must survive.
    use buttre_engine::compose::LearningSnapshot;
    use buttre_engine::pipeline::validation::{bit_index, decompose_ids};
    use std::collections::HashSet;
    use std::sync::Arc;

    let config = create_telex_config();
    let mut ex = PipelineExecutor::new(config.clone());
    for ch in "cakw".chars() {
        ex.process(ch);
    }
    assert_eq!(
        ex.context().syllable_buffer,
        "cakw",
        "unattested k-coda shape must demote to literal without an overlay"
    );

    let (o, n, c, t) = decompose_ids("căk").expect("'căk' must be a decomposable k-coda shape (nucleus 'ă' + coda 'k' is a valid P6 combination)");
    let mut overlay = HashSet::new();
    overlay.insert(bit_index(o, n, c, t) as u32);

    let mut ex2 = PipelineExecutor::new(config);
    ex2.set_learning_snapshot(LearningSnapshot {
        user_attested: Some(Arc::new(overlay)),
        raw_prefs: None,
    });
    for ch in "cakw".chars() {
        ex2.process(ch);
    }
    assert_eq!(
        ex2.context().syllable_buffer,
        "căk",
        "overlay-attested k-coda shape must survive the SAME consult point P6's static table uses"
    );
}

#[test]
fn gate_hardening_non_digit_trigger_demoted_before_probe_sees_it() {
    // Row: gate hardening (P6) + un-latch probe (P2) — the strict
    // (exact-attestation) trigger-class classification is honored INSIDE
    // the probe too, not just at the top-level compose() call site.
    //
    // P2's `should_unlatch` probes via the exact same public `compose()`
    // function exercised here directly — there is no separate/parallel gate
    // implementation for the probe path (single source of truth). A
    // non-digit trigger (P6 hardening: only ASCII-digit triggers get
    // shape-relaxation; everything else — including a hypothetical
    // punctuation trigger in a custom config — requires EXACT attestation)
    // whose match is only shape-attested must be demoted BEFORE any probe
    // ever inspects the result: `applied_marks` comes back empty, which is
    // exactly what `should_unlatch`'s condition (c) ("was a mark reported
    // fired at the last raw position") inspects. This is a SECOND,
    // independent line of defense beyond condition (b)'s own exact check —
    // gate hardening's classification is honored before the un-latch
    // decision ever runs its own logic, not merely duplicated by it.
    use buttre_engine::compose::{compose, ComposeOpts};
    use buttre_engine::pipeline::config::PipelineConfig;

    let mut cfg = PipelineConfig::new("custom-punct");
    cfg.add_transform("a'", "â");
    let opts = ComposeOpts::from_config(&cfg);

    let raw: Vec<char> = "nhat'".chars().collect();
    let r = compose(&raw, &opts);
    assert_eq!(
        r.text, "nhat'",
        "a non-digit trigger's shape-only match must demote to literal (P6 hardening)"
    );
    assert!(
        r.applied_marks.is_empty(),
        "the demoted mark must not survive into applied_marks — the un-latch probe's condition (c) would see nothing to pin to the last raw position"
    );
    assert!(!r.temp_english, "a demote with no surviving mark/tone must not itself latch English (matches the 'data' class)");

    // Contrast: VNI's digit trigger DOES get shape-relaxation for the exact
    // same structural shape ("nhat6" -> shape-only "nhât" survives, mark
    // present) — proving the distinction is real, not merely "nothing ever
    // survives a non-adjacent mark on this shape".
    let vni_cfg = buttre_engine::pipeline::presets::vni_config();
    let vni_opts = ComposeOpts::from_config(&vni_cfg);
    let vni_raw: Vec<char> = "nhat6".chars().collect();
    let vni_r = compose(&vni_raw, &vni_opts);
    assert_eq!(
        vni_r.text, "nhât",
        "VNI's digit trigger DOES get shape-relaxation for the same shape"
    );
    assert!(
        !vni_r.applied_marks.is_empty(),
        "the digit mark survives — this is exactly what condition (c) would see for a genuine VNI un-latch probe"
    );
}
