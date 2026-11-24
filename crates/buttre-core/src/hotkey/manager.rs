//! buttre Hotkey Manager - Global hotkey management
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/hotkey_tests.rs`.

use crate::hotkey::chords::{CUSTOM_CHORD_KEYS, FIXED_CHORDS};
use crate::hotkey::error::{HotkeyError, Result};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use log::{debug, info, warn};
use std::collections::HashMap;

/// Hotkey event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Toggle between current method and English
    Toggle,
    /// Switch to Telex
    Telex,
    /// Switch to VNI
    Vni,
    Nom, // Unified Nôm method
    /// Switch to custom method (index)
    Custom(usize),
    /// Toggle the last (current) multi-word window word between
    /// `literal(raw)` and `compose(raw)` (event-sourcing-completion Phase 4).
    ///
    /// Delivered as a GLOBAL hotkey only for backends that receive their
    /// keystrokes from a hook — see `buttre_platform`'s
    /// `platforms::windows::hook::dispatch_toggle_last_word`. TSF handles the
    /// same chord in-process instead and the registration below is SKIPPED for
    /// it (`PlatformBackend::owns_word_toggle_chord`), because `RegisterHotKey`
    /// would withhold the keystroke from the focused app.
    ///
    /// The chord (Ctrl+Shift+Z) is exempted from `hook.rs`'s modifier-reset
    /// (`is_toggle_chord_exempt`) and matched by TSF's `VK_WORD_TOGGLE` — keep
    /// all three in sync if it ever changes.
    ToggleLastWord,
}

/// Manages global hotkeys for buttre
pub struct ButtreHotkeyManager {
    manager: GlobalHotKeyManager,
    hotkeys: HashMap<u32, HotkeyAction>,
}

impl ButtreHotkeyManager {
    /// Create new hotkey manager with default hotkeys, including the
    /// `ToggleLastWord` chord.
    pub fn new() -> Result<Self> {
        Self::new_with_word_toggle(true)
    }

    /// Create the manager, choosing whether to claim the `ToggleLastWord`
    /// chord (`Ctrl+Shift+Z`) globally.
    ///
    /// # Arguments
    ///
    /// * `register_word_toggle` — pass `false` when the active input backend
    ///   handles the chord itself from inside the focused application (see
    ///   `PlatformBackend::owns_word_toggle_chord`). `RegisterHotKey` withholds
    ///   the keystroke from the focused window, so claiming it here would make
    ///   the chord permanently invisible to an in-process text service. Every
    ///   OTHER hotkey is registered identically either way.
    pub fn new_with_word_toggle(register_word_toggle: bool) -> Result<Self> {
        info!("Creating hotkey manager");

        // No X11 display → no manager: `GlobalHotKeyManager::new()` does not
        // fail gracefully — its X11 backend (the only one on Linux/BSD)
        // dereferences the null display and SIGSEGVs the whole process.
        // WAYLAND_DISPLAY alone is not enough: niri without
        // xwayland-satellite (or any compositor run without Xwayland) has no
        // DISPLAY, and the tray crashed there at startup. Every caller
        // already tolerates this error; on Wayland-native sessions
        // `buttre --ime` matches the chords itself.
        #[cfg(all(unix, not(target_os = "macos")))]
        if std::env::var_os("DISPLAY").is_none() {
            return Err(HotkeyError::ManagerCreationFailed(
                "no X11 display (headless, or Wayland without Xwayland) — \
                 global hotkeys are unavailable"
                    .to_string(),
            ));
        }

        let manager = GlobalHotKeyManager::new()
            .map_err(|e| HotkeyError::ManagerCreationFailed(e.to_string()))?;

        let mut hotkeys = HashMap::new();

        let mods = Modifiers::CONTROL | Modifiers::SHIFT;
        for (code, action) in FIXED_CHORDS {
            let hotkey = HotKey::new(Some(mods), code);

            manager
                .register(hotkey)
                .map_err(|e| HotkeyError::RegistrationFailed(format!("{:?}: {}", action, e)))?;

            info!(
                "Registered hotkey {:?} + {:?} -> {:?} (ID: {})",
                mods,
                code,
                action,
                hotkey.id()
            );
            hotkeys.insert(hotkey.id(), action);
        }

        // ToggleLastWord (event-sourcing-completion Phase 4): registered
        // LENIENTLY (warn + continue, not `?`) — unlike the core hotkeys
        // above, Ctrl+Shift+Z is a common app-level "redo" accelerator
        // elsewhere, so a registration collision is plausible. A collision
        // here must not take down method-switching/EN-VI toggle. Default
        // chord per the checkpointed decision (user-confirmed 2026-07-02);
        // NOT Ctrl+Shift+Esc — that's owned by Windows Task Manager and
        // cannot be registered at all (verified). Must stay in sync with the
        // chord exemption in buttre-platform's `hook.rs`
        // (`is_toggle_chord_exempt`).
        if register_word_toggle {
            let toggle_hotkey =
                HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyZ);
            match manager.register(toggle_hotkey) {
                Ok(()) => {
                    info!(
                        "Registered hotkey Ctrl+Shift+Z -> ToggleLastWord (ID: {})",
                        toggle_hotkey.id()
                    );
                    hotkeys.insert(toggle_hotkey.id(), HotkeyAction::ToggleLastWord);
                }
                Err(e) => {
                    warn!(
                        "Failed to register ToggleLastWord hotkey (Ctrl+Shift+Z): {} — \
                         word-toggle feature disabled this session",
                        e
                    );
                }
            }
        } else {
            info!(
                "Ctrl+Shift+Z left unregistered — the input backend claims it in-process \
                 (RegisterHotKey would withhold the key from the focused app)"
            );
        }

        info!("Hotkey manager initialized with {} hotkeys", hotkeys.len());

        Ok(Self { manager, hotkeys })
    }

    /// Register the custom keyboards' chords ([`CUSTOM_CHORD_KEYS`]).
    pub fn register_custom_methods(&mut self, count: usize) -> Result<()> {
        for (i, code) in CUSTOM_CHORD_KEYS.iter().take(count).enumerate() {
            let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), *code);
            let action = HotkeyAction::Custom(i);

            // Check if hotkey is already registered (to avoid duplicates or errors)
            if self.hotkeys.contains_key(&hotkey.id()) {
                info!("Hotkey for Custom {} already registered, skipping", i);
                continue;
            }

            match self.manager.register(hotkey) {
                Ok(_) => {
                    info!(
                        "Registered custom hotkey {:?} -> Custom {} (ID: {})",
                        code,
                        i,
                        hotkey.id()
                    );
                    self.hotkeys.insert(hotkey.id(), action);
                }
                Err(e) => {
                    // Log warning but continue registering others
                    // Some hotkeys might be taken by system
                    warn!("Failed to register hotkey for Custom {}: {}", i, e);
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Check for hotkey events
    /// Returns the action if a hotkey was pressed
    /// Note: Only processes Pressed events, ignores Released events
    pub fn check_hotkey(&self) -> Option<HotkeyAction> {
        use global_hotkey::HotKeyState;

        let mut last_action = None;
        let mut event_count = 0;
        let mut pressed_count = 0;
        let mut released_count = 0;

        // Drain all pending events
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            event_count += 1;

            // IMPORTANT: Only process Pressed events, ignore Released
            // This prevents double-triggering and spurious events
            match event.state {
                HotKeyState::Pressed => {
                    pressed_count += 1;
                    debug!(
                        "Received hotkey PRESSED event #{}: ID {}",
                        pressed_count, event.id
                    );

                    if let Some(action) = self.hotkeys.get(&event.id) {
                        info!("Hotkey pressed: {:?}", action);
                        last_action = Some(*action);
                    } else {
                        debug!("Unknown hotkey ID: {}", event.id);
                    }
                }
                HotKeyState::Released => {
                    released_count += 1;
                    debug!("Ignoring hotkey RELEASED event (ID {})", event.id);
                }
            }
        }

        if event_count > 0 {
            debug!(
                "Total events: {} (Pressed: {}, Released: {})",
                event_count, pressed_count, released_count
            );
        }

        last_action
    }
}

impl Drop for ButtreHotkeyManager {
    fn drop(&mut self) {
        for (id, action) in &self.hotkeys {
            // Note: We can't easily unregister by ID, but GlobalHotKeyManager
            // will clean up on drop
            debug!("Cleaning up hotkey {:?} (ID: {})", action, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── HotkeyAction::ToggleLastWord (event-sourcing-completion Phase 4) ────

    #[test]
    fn toggle_last_word_action_parses_and_compares() {
        assert_eq!(HotkeyAction::ToggleLastWord, HotkeyAction::ToggleLastWord);
        assert_ne!(HotkeyAction::ToggleLastWord, HotkeyAction::Toggle);
        assert_ne!(HotkeyAction::ToggleLastWord, HotkeyAction::Custom(0));
    }

    #[test]
    fn manager_creation_never_panics_on_toggle_registration() {
        // CI/headless environments can fail ALL global hotkey registration
        // (no desktop session) — `ButtreHotkeyManager::new()` itself may
        // legitimately return `Err` (see the pre-existing
        // `hotkey_tests::test_create_manager` tolerance). This test only
        // asserts the LENIENT registration path for ToggleLastWord never
        // panics or aborts manager creation when the core hotkeys DO
        // succeed — a Ctrl+Shift+Z collision must degrade gracefully, not
        // take down method-switching.
        if let Ok(mgr) = ButtreHotkeyManager::new() {
            let registered = mgr
                .hotkeys
                .values()
                .any(|a| matches!(a, HotkeyAction::ToggleLastWord));
            // Either outcome is acceptable (registration can fail on a
            // collision) — the point is we got here without a panic/abort.
            eprintln!("ToggleLastWord hotkey registered: {registered}");
        } else {
            eprintln!("Note: hotkey manager creation failed (expected in CI/headless)");
        }
    }
}
