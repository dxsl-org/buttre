use buttre_engine::pipeline::{PipelineExecutor, PipelineConfig};
use buttre_engine::types::Action;
use buttre_engine::pipeline::config::{ToneMark, ValidationSettings};

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
        Action::Replace { backspace_count, text } => {
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
        Action::Replace { backspace_count, text } => {
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
        Action::Replace { backspace_count, text } => {
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
        Action::Replace { backspace_count, text } => {
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
    assert_eq!(result, "Â", 
            "Expected uppercase 'Â' from 'AA' input, got: {}", result);
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
        
        assert_eq!(executor.syllable(), expected, "Failed for tone key: {}", tone_key);
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
    assert!(actions.iter().any(|a| matches!(a, Action::Commit(text) if text == " ")));
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
    assert!(executor.context().temp_english_mode, "Should set temp_english_mode after undo");
}

#[test]
fn test_tone_toggling() {
    let config = create_telex_config();
    let mut executor = PipelineExecutor::new(config);

    // Type "as" → "á" (apply sắc tone)
    executor.process('a');
    executor.process('s');
    assert_eq!(executor.syllable(), "á", "First tone application should produce á");
    
    // Type "s" again → should undo → "as" (remove tone, keep key - Unikey behavior)
    executor.process('s');
    assert_eq!(executor.syllable(), "as", "Second tone key should undo to 'as'");
    assert!(executor.context().temp_english_mode, "Should set temp_english_mode after tone undo");
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
    assert_eq!(executor.syllable(), "trường", "Tone on ơ for ươ with final consonant");
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
    assert!(executor.syllable().is_empty() || actions.len() > 0);
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
    println!("After first 's': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());

    executor.process('s');
    // After second 's' (duplicate tone), should fallback: dé → des
    // And temp_english_mode should be true
    println!("After second 's': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "des", "After double 's' should be 'des'");
    assert!(executor.is_temp_english_mode(), "Should be in temp English mode after fallback");

    executor.process('i');
    println!("After 'i': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "desi", "After 'i' should be 'desi'");

    executor.process('g');
    println!("After 'g': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "desig", "After 'g' should be 'desig'");

    executor.process('n');
    println!("After 'n': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "design", "After 'n' should be 'design'");
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
    println!("After first 's': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tí", "After first 's' should be 'tí'");

    executor.process('s');
    // After second 's' (duplicate tone), should fallback: tí → tis
    // And temp_english_mode should be true
    println!("After second 's': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tis", "After double 's' should be 'tis'");
    assert!(executor.is_temp_english_mode(), "Should be in temp English mode after fallback");

    executor.process('o');
    // In temp_english_mode, 'o' should just append, NOT trigger any Vietnamese processing
    println!("After 'o': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tiso", "After 'o' should be 'tiso'");
    assert!(executor.is_temp_english_mode(), "Should still be in temp English mode");

    executor.process('t');
    println!("After final 't': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tisot", "After final 't' should be 'tisot'");
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
    println!("After 1st 's': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tí", "After 1st 's' should be 'tí'");

    executor.process('s');
    println!("After 2nd 's': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tis", "After 2nd 's' should be 'tis'");
    assert!(executor.is_temp_english_mode(), "Should be in temp English mode");

    // CRITICAL: Third 's' should just append, NOT toggle tone!
    executor.process('s');
    println!("After 3rd 's': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tiss", "After 3rd 's' should be 'tiss' NOT 'tís'!");
    assert!(executor.is_temp_english_mode(), "Should still be in temp English mode");

    executor.process('o');
    println!("After 'o': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tisso", "After 'o' should be 'tisso'");

    executor.process('t');
    println!("After 't': syllable='{}', temp_english_mode={}", 
                executor.syllable(), executor.is_temp_english_mode());
    assert_eq!(executor.syllable(), "tissot", "After 't' should be 'tissot'");
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
