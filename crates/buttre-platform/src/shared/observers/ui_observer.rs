//! UI observer for tray icon and menu updates
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/shared_ui_observer_tests.rs`.
//!
//! This observer listens to state changes and updates the tray icon and menu
//! to reflect the current input method and enabled state.
//!
//! ## Design
//!
//! Uses callback pattern to avoid `Send + Sync` issues with UI types.
//! The callback is implemented in main.rs where UI references live.

use buttre_core::{Settings, StateObserver};
use log::info;
use std::sync::Arc;

/// Callback trait for UI updates
///
/// This trait is implemented in main.rs where UI references (tray icon, menu items)
/// are available. The observer calls these methods when state changes.
pub trait UICallback: Send + Sync {
    /// Update menu checkmarks for the given state. `enabled` rides along
    /// because an unchecked-everything menu is the correct rendering of OFF —
    /// method checkmarks must never claim a method is active while the IME is
    /// not (ADR-0003).
    fn update_menu_checkmarks(&self, method: &str, enabled: bool);

    /// Update tray icon and tooltip
    ///
    /// # Arguments
    /// * `method` - The new input method ID
    /// * `enabled` - Whether Vietnamese input is enabled
    fn update_tray_icon(&self, method: &str, enabled: bool);
}

/// Observer that updates UI (tray icon and menu) when input method changes
///
/// ## Algorithm
///
/// 1. Receives state change notification
/// 2. Calls UICallback methods
/// 3. Callback implementation (in main.rs) updates actual UI
///
/// ## Example
///
/// ```rust,ignore
/// // In main.rs
/// struct MyUICallback {
///     // UI references (not Send + Sync, only used in main thread)
/// }
///
/// impl UICallback for MyUICallback {
///     fn update_menu_checkmarks(&self, method: &str, enabled: bool) {
///         // Update menu items
///     }
///     
///     fn update_tray_icon(&self, method: &str, enabled: bool) {
///         // Update tray icon
///     }
/// }
///
/// // Create observer with callback
/// let ui_callback = Arc::new(MyUICallback { /* ... */ });
/// let ui_observer = Arc::new(UIObserver::new(ui_callback));
/// app_state.add_observer(ui_observer);
/// ```
pub struct UIObserver {
    /// Callback for UI updates
    callback: Arc<dyn UICallback>,
}

impl UIObserver {
    /// Create a new UIObserver
    ///
    /// # Arguments
    /// * `callback` - Implementation of UICallback that will handle actual UI updates
    pub fn new(callback: Arc<dyn UICallback>) -> Self {
        info!("Creating UIObserver");
        Self { callback }
    }
}

impl StateObserver for UIObserver {
    fn on_method_changed(&self, method: &str, enabled: bool) {
        info!(
            "UIObserver: Method changed to '{}' (enabled: {})",
            method, enabled
        );

        // Update menu checkmarks via callback
        self.callback.update_menu_checkmarks(method, enabled);

        // Update tray icon via callback
        self.callback.update_tray_icon(method, enabled);
    }

    fn on_settings_changed(&self, _settings: &Settings) {
        // UI doesn't need to react to other settings changes
        // (auto_correct, shorthand, startup don't affect UI)
    }
}
