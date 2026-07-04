//! Windows-only integration tests — the whole `platforms::windows` module
//! tree does not exist on other targets.
#![cfg(windows)]

use buttre_platform::platforms::windows::common::key_utils::*;
use buttre_platform::platforms::windows::common::vk_codes::*;

#[test]
fn test_movement_keys() {
    assert!(is_movement_key(VK_LEFT));
    assert!(is_movement_key(VK_RIGHT));
    assert!(is_movement_key(VK_HOME));
    assert!(is_movement_key(VK_END));
    assert!(!is_movement_key(VK_SPACE));
    assert!(!is_movement_key(VK_RETURN));
}

#[test]
fn test_modifier_keys() {
    assert!(is_modifier_key(VK_SHIFT));
    assert!(is_modifier_key(VK_CONTROL));
    assert!(is_modifier_key(VK_LWIN));
    assert!(!is_modifier_key(VK_SPACE));
}

#[test]
fn test_special_keys() {
    assert!(is_special_key(VK_F1));
    assert!(is_special_key(VK_F12));
    assert!(is_special_key(VK_ESCAPE));
    assert!(!is_special_key(VK_SPACE));
    assert!(!is_special_key(VK_BACK)); // Backspace is NOT special (handled separately)
}

#[test]
fn test_buffer_reset_keys() {
    // Movement keys
    assert!(is_buffer_reset_key(VK_LEFT));
    assert!(is_buffer_reset_key(VK_RIGHT));
    assert!(is_buffer_reset_key(VK_UP));
    assert!(is_buffer_reset_key(VK_DOWN));
    assert!(is_buffer_reset_key(VK_HOME));
    assert!(is_buffer_reset_key(VK_END));
    assert!(is_buffer_reset_key(VK_PRIOR)); // Page Up
    assert!(is_buffer_reset_key(VK_NEXT)); // Page Down

    // Line/field terminators
    assert!(is_buffer_reset_key(VK_RETURN));
    assert!(is_buffer_reset_key(VK_TAB));
    assert!(is_buffer_reset_key(VK_ESCAPE));

    // Editing keys
    assert!(is_buffer_reset_key(VK_INSERT));
    assert!(is_buffer_reset_key(VK_DELETE));

    // Function keys
    assert!(is_buffer_reset_key(VK_F1));
    assert!(is_buffer_reset_key(VK_F12));
    assert!(is_buffer_reset_key(VK_F24));

    // NOT buffer reset keys
    assert!(!is_buffer_reset_key(VK_SPACE)); // Space is soft separator
    assert!(!is_buffer_reset_key(VK_BACK)); // Backspace handled separately
    assert!(!is_buffer_reset_key(VK_SHIFT)); // Modifier only
    assert!(!is_buffer_reset_key(0x41)); // 'A' - regular character
}
