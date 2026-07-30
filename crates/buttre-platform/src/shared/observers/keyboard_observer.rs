//! Keyboard observer for input method updates

use crate::shared::KeyboardManager;
use buttre_core::state::{Settings, StateObserver};
use log::info;
use std::sync::Arc;

/// Observer that updates the KeyboardManager when input method changes
pub struct KeyboardObserver {
    /// Shared with `main.rs`'s event loop, which also drives the manager
    /// directly for the live learning toggle (tray "Học thông minh").
    keyboard_manager: Arc<KeyboardManager>,
}

impl KeyboardObserver {
    /// Create a new KeyboardObserver
    ///
    /// # Arguments
    /// * `keyboard_manager` - The keyboard manager to update
    pub fn new(keyboard_manager: Arc<KeyboardManager>) -> Self {
        Self { keyboard_manager }
    }
}

impl StateObserver for KeyboardObserver {
    fn on_method_changed(&self, method: &str, enabled: bool) {
        info!("KeyboardObserver: method '{method}', enabled={enabled}");

        // Order matters: the manager only BUILDS a keyboard while enabled, so
        // the flag must land first or an off→on notification would record the
        // method and then build nothing.
        if let Err(e) = self.keyboard_manager.set_enabled(enabled) {
            log::error!("Failed to apply enabled state: {:?}", e);
        }
        if enabled {
            if let Err(e) = self.keyboard_manager.set_method(method) {
                log::error!("Failed to set keyboard method: {:?}", e);
            }
        }
    }

    fn on_settings_changed(&self, _settings: &Settings) {
        // Keyboard doesn't need to react to other settings changes
    }
}
