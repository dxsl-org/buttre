#![cfg(platform_linux)]
use buttre_platform::platforms::linux::engine_bridge::*;
use buttre_platform::platforms::linux::ibus::ButtreEngine;

#[test]
fn test_engine_creation() {
    let engine = ButtreEngine::new();
    assert_eq!(engine.preedit_text(), "");
}

#[test]
fn test_keysym_conversion_identity_for_printable_ascii() {
    // XKB resolves Shift/CapsLock before the keysym reaches the engine:
    // Shift+a arrives as keysym 0x41 ('A'), so mapping is identity — no
    // modifier re-application (that would double-flip the case).
    assert_eq!(keysym_to_char(0x0061), Some('a'));
    assert_eq!(keysym_to_char(0x0041), Some('A'));
    assert_eq!(keysym_to_char(0x0020), Some(' '));
    assert_eq!(keysym_to_char(0x0035), Some('5'));
    assert_eq!(keysym_to_char(0x002E), Some('.'));
    assert_eq!(keysym_to_char(0x003F), Some('?'));
}

#[test]
fn test_keysym_conversion_special_keys() {
    assert_eq!(keysym_to_char(0xFF0D), Some('\n')); // Return
    assert_eq!(keysym_to_char(0xFF08), Some('\x08')); // BackSpace
    assert_eq!(keysym_to_char(0xFF1B), None); // Escape — break keysym, not a char
    assert_eq!(keysym_to_char(0xFFE1), None); // Shift_L — modifier
    assert!(is_break_keysym(0xFF1B));
    assert!(is_modifier_keysym(0xFFE1));
}
