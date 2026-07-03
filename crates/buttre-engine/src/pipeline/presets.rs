//! Configuration Presets - Ready-to-use configurations for common input methods
//!
//! This module provides preset configurations for popular Vietnamese input methods:
//! - Telex
//! - VNI
//! - VIQR (future)
//! - Simple Telex (future)

use crate::pipeline::config::{PipelineConfig, ToneMark, ToneStyle, UnicodeForm};
use std::sync::Arc;

/// Create a Telex configuration
///
/// ## DEPRECATED
/// This function is deprecated. Use `buttre_core::keyboard::telex::build_config()` instead.
/// 
/// ## Telex Input Method
///
/// Telex is the most popular Vietnamese input method. It uses letter combinations
/// to create Vietnamese characters:
///
/// ### Transformations
/// - `aa` → `â` (a circumflex)
/// - `aw` → `ă` (a breve)
/// - `dd` → `đ` (d stroke)
/// - `ee` → `ê` (e circumflex)
/// - `oo` → `ô` (o circumflex)
/// - `ow` → `ơ` (o horn)
/// - `uw` → `ư` (u horn)
///
/// ### Tones
/// - `s` → Acute (sắc)
/// - `f` → Grave (huyền)
/// - `r` → Hook (hỏi)
/// - `x` → Tilde (ngã)
/// - `j` → Dot (nặng)
pub fn telex_config() -> PipelineConfig {
    let mut config = PipelineConfig::new("telex");
    
    // ========================================
    // DEPRECATED: Use buttre_core::keyboard::telex::build_config() instead
    // ========================================
    // This preset is deprecated. For full Telex functionality with special rules,
    // use buttre_core::keyboard::telex::build_config() which includes:
    // - W-after-ư blocking
    // - OEO pattern blocking
    // - UA tone positioning
    //
    // This preset now only provides basic transforms and tones.
    // ========================================
    
    // Empty context rules (special rules moved to buttre-core)
    config.context_rules = Arc::new(Vec::new());
    
    // ========================================
    // Standard Transformation Rules
    // ========================================
    // Transformation rules
    config.add_transform("aa", "â");
    config.add_transform("aw", "ă");
    config.add_transform("dd", "đ");
    config.add_transform("ee", "ê");
    config.add_transform("oo", "ô");
    config.add_transform("ow", "ơ");
    config.add_transform("uw", "ư");
    // Onset-only w-shorthand ("lwu"→"lưu", "trwong"→"trương"): a 1-char rule
    // makes 'w' fire as an inferred ư-insertion after a pure-consonant onset
    // (see `compose::segment::onset_only_insertion_fires`). Word-initial 'w'
    // stays literal, and unattested results demote — English w-words are safe.
    config.add_transform("w", "ư");

    // Uppercase variants
    config.add_transform("AA", "Â");
    config.add_transform("AW", "Ă");
    config.add_transform("Aw", "Ă");
    config.add_transform("DD", "Đ");
    config.add_transform("Dd", "Đ");
    config.add_transform("EE", "Ê");
    config.add_transform("OO", "Ô");
    config.add_transform("OW", "Ơ");
    config.add_transform("Ow", "Ơ");
    config.add_transform("UW", "Ư");
    config.add_transform("Uw", "Ư");
    
    // Tone marks
    config.add_tone('s', ToneMark::Acute);
    config.add_tone('S', ToneMark::Acute);
    config.add_tone('f', ToneMark::Grave);
    config.add_tone('F', ToneMark::Grave);
    config.add_tone('r', ToneMark::Hook);
    config.add_tone('R', ToneMark::Hook);
    config.add_tone('x', ToneMark::Tilde);
    config.add_tone('X', ToneMark::Tilde);
    config.add_tone('j', ToneMark::Dot);
    config.add_tone('J', ToneMark::Dot);
    
    // Settings
    config.enable_lookup = false; // Disable by default
    config.tone_style = ToneStyle::Old;  // Default: kiểu cũ (óa, úa, úy)
    config.unicode_form = UnicodeForm::NFC;
    
    config
}

/// Create a VNI configuration
///
/// ## VNI Input Method
///
/// VNI is another popular Vietnamese input method. It uses numbers to create
/// Vietnamese characters and apply tones:
///
/// ### Transformations
/// - `a6` or `a8` → `ă` (a breve)
/// - `a6` → `â` (a circumflex)
/// - `d9` → `đ` (d stroke)
/// - `e6` → `ê` (e circumflex)
/// - `o6` → `ô` (o circumflex)
/// - `o7` → `ơ` (o horn)
/// - `u7` → `ư` (u horn)
///
/// ### Tones
/// - `1` → Acute (sắc)
/// - `2` → Grave (huyền)
/// - `3` → Hook (hỏi)
/// - `4` → Tilde (ngã)
/// - `5` → Dot (nặng)
/// - `0` → Remove tone
///
/// ## Example
///
/// ```rust,ignore
/// use buttre_core::engine::pipeline::presets::vni_config;
/// use buttre_core::engine::pipeline::PipelineExecutor;
///
/// let config = vni_config();
/// let mut executor = PipelineExecutor::new(config);
///
/// // Type "Viet5 Nam" → "Việt Nam"
/// executor.process('V');
/// executor.process('i');
/// executor.process('e');
/// executor.process('6'); // e6 → ê
/// executor.process('5'); // add dot tone → ệ
/// executor.process('t');
/// // Result: "Việt"
/// ```
pub fn vni_config() -> PipelineConfig {
    let mut config = PipelineConfig::new("vni");
    
    // Transformation rules
    // Note: VNI has multiple ways to type some characters
    config.add_transform("a6", "â");
    config.add_transform("a8", "ă");
    config.add_transform("d9", "đ");
    config.add_transform("e6", "ê");
    config.add_transform("o6", "ô");
    config.add_transform("o7", "ơ");
    config.add_transform("u7", "ư");
    
    // Uppercase variants
    config.add_transform("A6", "Â");
    config.add_transform("A8", "Ă");
    config.add_transform("D9", "Đ");
    config.add_transform("E6", "Ê");
    config.add_transform("O6", "Ô");
    config.add_transform("O7", "Ơ");
    config.add_transform("U7", "Ư");
    
    // Tone marks (numbers)
    config.add_tone('1', ToneMark::Acute);
    config.add_tone('2', ToneMark::Grave);
    config.add_tone('3', ToneMark::Hook);
    config.add_tone('4', ToneMark::Tilde);
    config.add_tone('5', ToneMark::Dot);
    config.add_tone('0', ToneMark::None); // Remove tone
    
    // Settings
    config.enable_lookup = false; // Disable by default
    config.tone_style = ToneStyle::Old;  // Default: kiểu cũ (óa, úa, úy)
    config.unicode_form = UnicodeForm::NFC;
    
    config
}

/// Create a Simple Telex configuration (no double-key transformations)
///
/// ## Simple Telex
///
/// A simplified version of Telex that doesn't use double-key transformations.
/// Useful for users who want more control or are typing mixed content.
///
/// This is a placeholder for future implementation.
pub fn simple_telex_config() -> PipelineConfig {
    // For now, return standard Telex
    // Future: Implement simplified version
    telex_config()
}

/// Create a VIQR configuration
///
/// ## VIQR Input Method
///
/// VIQR (Vietnamese Quoted-Readable) is an older input method that uses
/// special characters to create Vietnamese characters.
///
/// This is a placeholder for future implementation.
pub fn viqr_config() -> PipelineConfig {
    let config = PipelineConfig::new("viqr");
    
    // Future: Add VIQR transformation rules
    // Example: a^ → â, a( → ă, etc.
    
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telex_config_name() {
        let config = telex_config();
        assert_eq!(config.name, "telex");
    }

    #[test]
    fn test_telex_has_transformations() {
        let config = telex_config();
        
        assert_eq!(config.transform_rules.get("aa"), Some(&"â".to_string()));
        assert_eq!(config.transform_rules.get("aw"), Some(&"ă".to_string()));
        assert_eq!(config.transform_rules.get("dd"), Some(&"đ".to_string()));
        assert_eq!(config.transform_rules.get("ee"), Some(&"ê".to_string()));
        assert_eq!(config.transform_rules.get("oo"), Some(&"ô".to_string()));
        assert_eq!(config.transform_rules.get("ow"), Some(&"ơ".to_string()));
        assert_eq!(config.transform_rules.get("uw"), Some(&"ư".to_string()));
    }

    #[test]
    fn test_telex_has_uppercase_transformations() {
        let config = telex_config();
        
        assert_eq!(config.transform_rules.get("AA"), Some(&"Â".to_string()));
        assert_eq!(config.transform_rules.get("DD"), Some(&"Đ".to_string()));
    }

    #[test]
    fn test_telex_has_tones() {
        let config = telex_config();
        
        assert_eq!(config.tone_map.get(&'s'), Some(&ToneMark::Acute));
        assert_eq!(config.tone_map.get(&'f'), Some(&ToneMark::Grave));
        assert_eq!(config.tone_map.get(&'r'), Some(&ToneMark::Hook));
        assert_eq!(config.tone_map.get(&'x'), Some(&ToneMark::Tilde));
        assert_eq!(config.tone_map.get(&'j'), Some(&ToneMark::Dot));
    }

    #[test]
    fn test_telex_settings() {
        let config = telex_config();
        
        assert_eq!(config.enable_lookup, false);
        assert_eq!(config.tone_style, ToneStyle::Old);  // Uses old style (óa, úa, úy)
        assert_eq!(config.unicode_form, UnicodeForm::NFC);
    }

    #[test]
    fn test_vni_config_name() {
        let config = vni_config();
        assert_eq!(config.name, "vni");
    }

    #[test]
    fn test_vni_has_transformations() {
        let config = vni_config();
        
        assert_eq!(config.transform_rules.get("a6"), Some(&"â".to_string()));
        assert_eq!(config.transform_rules.get("a8"), Some(&"ă".to_string()));
        assert_eq!(config.transform_rules.get("d9"), Some(&"đ".to_string()));
        assert_eq!(config.transform_rules.get("e6"), Some(&"ê".to_string()));
        assert_eq!(config.transform_rules.get("o6"), Some(&"ô".to_string()));
        assert_eq!(config.transform_rules.get("o7"), Some(&"ơ".to_string()));
        assert_eq!(config.transform_rules.get("u7"), Some(&"ư".to_string()));
    }

    #[test]
    fn test_vni_has_tones() {
        let config = vni_config();
        
        assert_eq!(config.tone_map.get(&'1'), Some(&ToneMark::Acute));
        assert_eq!(config.tone_map.get(&'2'), Some(&ToneMark::Grave));
        assert_eq!(config.tone_map.get(&'3'), Some(&ToneMark::Hook));
        assert_eq!(config.tone_map.get(&'4'), Some(&ToneMark::Tilde));
        assert_eq!(config.tone_map.get(&'5'), Some(&ToneMark::Dot));
        assert_eq!(config.tone_map.get(&'0'), Some(&ToneMark::None));
    }

    #[test]
    fn test_vni_settings() {
        let config = vni_config();
        
        assert_eq!(config.enable_lookup, false);
        assert_eq!(config.tone_style, ToneStyle::Old);  // Uses old style (óa, úa, úy)
        assert_eq!(config.unicode_form, UnicodeForm::NFC);
    }

    #[test]
    fn test_simple_telex_returns_config() {
        let config = simple_telex_config();
        assert_eq!(config.name, "telex"); // Currently returns standard Telex
    }

    #[test]
    fn test_viqr_returns_config() {
        let config = viqr_config();
        assert_eq!(config.name, "viqr");
    }
}
