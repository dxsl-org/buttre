//! Telex Transformation Rules
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/keyboard_telex_tests.rs`.
//!
//! This module defines all Telex character transformations.

use std::collections::HashMap;

/// Get all Telex transformation rules
///
/// ## Returns
/// HashMap mapping input sequences to output characters
///
/// ## Examples
/// - "aa" → "â"
/// - "aw" → "ă"
/// - "dd" → "đ"
pub fn get_rules() -> HashMap<String, String> {
    let mut rules = HashMap::new();
    
    // Basic transformations
    rules.insert("aa".to_string(), "â".to_string());
    rules.insert("aw".to_string(), "ă".to_string());
    rules.insert("dd".to_string(), "đ".to_string());
    rules.insert("ee".to_string(), "ê".to_string());
    rules.insert("oo".to_string(), "ô".to_string());
    rules.insert("ow".to_string(), "ơ".to_string());
    rules.insert("uw".to_string(), "ư".to_string());

    // NOTE: standalone 'w' → 'ư' is intentionally NOT registered.
    // A leading bare 'w' would turn every English w-word ("won", "with",
    // "will", "want", …) into "ư…".  In this Telex layout 'w' is only the
    // modifier in aw/ow/uw, and 'ư' at the start of a word is typed as "uw"
    // (uwng → ưng, uwu → ưu).  See segment.rs: a standalone alphabetic modifier
    // is treated as a literal base char unless a compatible vowel precedes it.

    // Uppercase variants
    rules.insert("AA".to_string(), "Â".to_string());
    rules.insert("AW".to_string(), "Ă".to_string());
    rules.insert("Aw".to_string(), "Ă".to_string());
    rules.insert("DD".to_string(), "Đ".to_string());
    rules.insert("Dd".to_string(), "Đ".to_string());
    rules.insert("EE".to_string(), "Ê".to_string());
    rules.insert("OO".to_string(), "Ô".to_string());
    rules.insert("OW".to_string(), "Ơ".to_string());
    rules.insert("Ow".to_string(), "Ơ".to_string());
    rules.insert("UW".to_string(), "Ư".to_string());
    rules.insert("Uw".to_string(), "Ư".to_string());
    
    // NOTE: "uow" → "ươ" rules are intentionally REMOVED
    // Stage 6 handles uo+w contextually:
    // - thuowr → thuở (uơ, only hook o when at end of word)
    // - tuowng → tương (ươ, hook both when followed by consonant)
    // Keeping this HashMap rule would override the Stage 4 skip logic.
    
    rules
}

