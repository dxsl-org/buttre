//! Telex Tone Mappings
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/keyboard_telex_tests.rs`.
//!
//! This module defines all Telex tone mark mappings.

use buttre_engine::pipeline::config::ToneMark;
use std::collections::HashMap;

/// Get all Telex tone mappings
///
/// ## Returns
/// HashMap mapping input characters to tone marks
///
/// ## Examples
/// - 's' → Acute (sắc)
/// - 'f' → Grave (huyền)
/// - 'r' → Hook (hỏi)
/// - 'x' → Tilde (ngã)
/// - 'j' → Dot (nặng)
pub fn get_map() -> HashMap<char, ToneMark> {
    let mut map = HashMap::new();
    
    // Lowercase
    map.insert('s', ToneMark::Acute);
    map.insert('f', ToneMark::Grave);
    map.insert('r', ToneMark::Hook);
    map.insert('x', ToneMark::Tilde);
    map.insert('j', ToneMark::Dot);
    // 'z' clears the tone (bỏ dấu) — standard Telex.  Since the last tone key
    // wins and ToneMark::None applies no tone, "asz" → "a" (acute removed).
    // Before a vowel ('z' with no preceding vowel, e.g. "dz") it stays a literal
    // consonant via the segment leading-tone-key guard, so "dzi" still works.
    map.insert('z', ToneMark::None);

    // Uppercase
    map.insert('S', ToneMark::Acute);
    map.insert('F', ToneMark::Grave);
    map.insert('R', ToneMark::Hook);
    map.insert('X', ToneMark::Tilde);
    map.insert('J', ToneMark::Dot);
    map.insert('Z', ToneMark::None);

    map
}

