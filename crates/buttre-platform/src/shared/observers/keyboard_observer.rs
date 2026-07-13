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
    fn on_method_changed(&self, method: &str, _enabled: bool) {
        info!("KeyboardObserver: Updating keyboard to method '{}'", method);

        if let Err(e) = self.keyboard_manager.set_method(method) {
            log::error!("Failed to set keyboard method: {:?}", e);
        }
    }

    fn on_settings_changed(&self, _settings: &Settings) {
        // Keyboard doesn't need to react to other settings changes
    }
}
