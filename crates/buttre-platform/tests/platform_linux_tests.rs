#![cfg(platform_linux)]
use buttre_platform::platforms::linux::ibus::*;

#[test]
fn test_engine_creation() {
    let engine = ButtreEngine::new();
    assert_eq!(*engine.preedit.lock().unwrap(), "");
}

#[test]
fn test_keyval_conversion_identity_for_printable_ascii() {
    // XKB resolves Shift/CapsLock before the keysym reaches the engine:
    // Shift+a arrives as keyval 0x41 ('A'), so mapping is identity — no
    // modifier re-application (that would double-flip the case).
    assert_eq!(keyval_to_char(0x0061), Some('a'));
    assert_eq!(keyval_to_char(0x0041), Some('A'));
    assert_eq!(keyval_to_char(0x0020), Some(' '));
    assert_eq!(keyval_to_char(0x0035), Some('5'));
    assert_eq!(keyval_to_char(0x002E), Some('.'));
    assert_eq!(keyval_to_char(0x003F), Some('?'));
}

#[test]
fn test_keyval_conversion_special_keys() {
    assert_eq!(keyval_to_char(0xFF0D), Some('\n')); // Return
    assert_eq!(keyval_to_char(0xFF08), Some('\x08')); // BackSpace
    assert_eq!(keyval_to_char(0xFF1B), None); // Escape — break keyval, not a char
    assert_eq!(keyval_to_char(0xFFE1), None); // Shift_L — modifier
}
