//! buttre's Ctrl+Shift chords, matched inside the IME's keyboard grab.
//!
//! The tray's global hotkeys are X11 grabs (`global-hotkey`). A wlroots or
//! COSMIC compositor sends Xwayland no keys while a Wayland window has focus,
//! so those grabs never fire there; the grab this IME holds while a text
//! field is focused sees every key instead. A key reaches exactly one of the
//! two (grab or Xwayland), so a chord is never handled twice.
//!
//! Effects go through the same stores as the tray's: the `method` file (the
//! tray converges and turns the IME on) and the `enabled` file (the tray
//! mirrors it into `settings.toml`). They also apply here at once, so the
//! next key already uses the new state without waiting for a file watcher.

use super::super::method_sync;
use super::ImeState;
use crate::shared::MethodRegistry;
use buttre_core::hotkey::{ctrl_shift_action, Code, HotkeyAction};
use std::sync::atomic::Ordering;

/// Linux evdev key code → the physical key the chord table names. Physical,
/// not keysym: with Shift held, `1` reads as `!` on a US layout.
fn evdev_code(key: u32) -> Option<Code> {
    Some(match key {
        2 => Code::Digit1,
        3 => Code::Digit2,
        4 => Code::Digit3,
        5 => Code::Digit4,
        6 => Code::Digit5,
        7 => Code::Digit6,
        8 => Code::Digit7,
        9 => Code::Digit8,
        10 => Code::Digit9,
        11 => Code::Digit0,
        57 => Code::Space,
        59 => Code::F1,
        60 => Code::F2,
        61 => Code::F3,
        _ => return None,
    })
}

/// Custom keyboard ids in menu order — the list the tray indexes with
/// Ctrl+Shift+4…0. Scanned per chord press: rare, and always current.
fn custom_method_ids() -> Vec<String> {
    MethodRegistry::new()
        .get_custom()
        .iter()
        .map(|m| m.id.clone())
        .collect()
}

/// The chord `key` forms with exactly Ctrl+Shift held, if it is buttre's.
pub(super) fn chord_for(
    key: u32,
    ctrl: bool,
    shift: bool,
    other_mods: bool,
) -> Option<HotkeyAction> {
    if !(ctrl && shift) || other_mods {
        return None;
    }
    let code = evdev_code(key)?;
    // Only the custom digits need the keyboard count; skip the dir scan
    // for every other key.
    let custom_count = if ctrl_shift_action(code, 0).is_some() {
        0
    } else {
        custom_method_ids().len()
    };
    ctrl_shift_action(code, custom_count)
}

impl ImeState {
    /// Run a chord the way the tray would.
    pub(super) fn run_chord(&mut self, action: HotkeyAction) {
        match action {
            HotkeyAction::Toggle => {
                let want = !self.enabled.load(Ordering::Relaxed);
                self.enabled.store(want, Ordering::Relaxed);
                if let Err(e) = method_sync::write_enabled(want) {
                    tracing::warn!("chord: write_enabled({want}) failed: {e}");
                }
                self.sync_enabled();
            }
            HotkeyAction::Telex => self.select_method("telex"),
            HotkeyAction::Vni => self.select_method("vni"),
            HotkeyAction::Nom => self.select_method("nom"),
            HotkeyAction::Custom(index) => {
                if let Some(id) = custom_method_ids().get(index) {
                    self.select_method(id);
                }
            }
            // Word toggle is a Windows hook feature; never matched here.
            HotkeyAction::ToggleLastWord => {}
        }
    }

    /// Pick `method` and turn the IME on — what every method-selection
    /// surface means (see the tray's `select_method`).
    fn select_method(&mut self, method: &str) {
        if let Err(e) = method_sync::write_method(method) {
            tracing::warn!("chord: write_method({method}) failed: {e}");
        }
        // Adopt it now: the watcher's later read of this write is then a
        // no-op instead of a rebuild that would wipe a word typed meanwhile.
        self.method_state.set(method.to_string());
        self.sync_method();
        if !self.enabled.swap(true, Ordering::Relaxed) {
            if let Err(e) = method_sync::write_enabled(true) {
                tracing::warn!("chord: write_enabled(true) failed: {e}");
            }
        }
        self.sync_enabled();
    }
}
