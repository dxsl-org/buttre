//! The Ctrl+Shift chord table — one source for every surface that matches it:
//! the global registration ([`super::ButtreHotkeyManager`]) and input methods
//! that see the keys themselves (the Wayland IME, whose compositor gives a
//! global X11 grab nothing while a Wayland window has focus).

use super::manager::HotkeyAction;
use global_hotkey::hotkey::Code;

/// Ctrl+Shift+<key> → fixed action.
pub const FIXED_CHORDS: [(Code, HotkeyAction); 7] = [
    // Toggle: closest to Unikey's bare Ctrl+Shift (a chord needs a key).
    (Code::Space, HotkeyAction::Toggle),
    (Code::F1, HotkeyAction::Telex),
    (Code::Digit1, HotkeyAction::Telex),
    (Code::F2, HotkeyAction::Vni),
    (Code::Digit2, HotkeyAction::Vni),
    (Code::F3, HotkeyAction::Nom),
    (Code::Digit3, HotkeyAction::Nom),
];

/// Ctrl+Shift+<key> for the N-th custom keyboard (menu order): 4…9, then 0.
pub const CUSTOM_CHORD_KEYS: [Code; 7] = [
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
    Code::Digit9,
    Code::Digit0,
];

/// The action of Ctrl+Shift+`code`, or `None` when the chord is not buttre's.
/// `custom_count`: how many custom keyboards exist — a custom key past the
/// end is not a chord (the key reaches the app instead).
pub fn ctrl_shift_action(code: Code, custom_count: usize) -> Option<HotkeyAction> {
    if let Some((_, action)) = FIXED_CHORDS.iter().find(|(c, _)| *c == code) {
        return Some(*action);
    }
    CUSTOM_CHORD_KEYS
        .iter()
        .take(custom_count)
        .position(|c| *c == code)
        .map(HotkeyAction::Custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_chords_map_to_their_actions() {
        assert_eq!(
            ctrl_shift_action(Code::Space, 0),
            Some(HotkeyAction::Toggle)
        );
        assert_eq!(ctrl_shift_action(Code::Digit2, 0), Some(HotkeyAction::Vni));
        assert_eq!(ctrl_shift_action(Code::F3, 0), Some(HotkeyAction::Nom));
    }

    #[test]
    fn custom_chords_follow_menu_order_and_stop_at_the_last_keyboard() {
        assert_eq!(
            ctrl_shift_action(Code::Digit4, 7),
            Some(HotkeyAction::Custom(0))
        );
        assert_eq!(
            ctrl_shift_action(Code::Digit0, 7),
            Some(HotkeyAction::Custom(6))
        );
        assert_eq!(ctrl_shift_action(Code::Digit6, 2), None);
    }

    #[test]
    fn other_keys_are_not_chords() {
        assert_eq!(ctrl_shift_action(Code::KeyC, 7), None);
    }
}
