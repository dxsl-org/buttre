//! Application state management with Observer pattern
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/state_tests.rs`.
//!
//! This module provides the centralized `AppState` that serves as the single source
//! of truth for the application's runtime state. It uses the Observer pattern to
//! notify interested components when state changes occur.

use super::{observer::StateObserver, Settings};
use log::info;
use std::sync::Arc;

/// Centralized application state
///
/// This struct holds all runtime state for the buttre application and provides
/// methods to update state while automatically notifying observers.
///
/// # Thread Safety
/// `AppState` is designed to be shared across threads using `Arc<Mutex<AppState>>`.
pub struct AppState {
    /// Whether the input method is on. Mirrors `Settings::enabled`; NOT derived
    /// from `current_method` (see ADR-0003 and `Settings::enabled`'s doc).
    enabled: bool,

    /// Current input method ID: `"telex"`, `"vni"`, `"nom"`, or a custom id.
    /// Never `"english"` — off is [`Self::enabled`].
    current_method: String,

    /// Application settings (persisted to disk)
    settings: Settings,

    /// Registered observers that will be notified of state changes
    observers: Vec<Arc<dyn StateObserver>>,
}

impl AppState {
    /// Create a new `AppState` with loaded settings
    ///
    /// This will load settings from disk and initialize the state accordingly.
    pub fn new() -> Self {
        let state = Self::with_settings(Settings::load());
        info!(
            "Initialized AppState: method={}, enabled={}",
            state.current_method, state.enabled
        );
        state
    }

    /// Create a new `AppState` with custom settings
    ///
    /// Useful for testing or when you want to override default settings.
    pub fn with_settings(settings: Settings) -> Self {
        Self {
            enabled: settings.enabled,
            current_method: settings.input_method.clone(),
            settings,
            observers: Vec::new(),
        }
    }

    /// Register an observer to be notified of state changes
    ///
    /// # Arguments
    /// * `observer` - An implementation of `StateObserver` wrapped in `Arc`
    pub fn add_observer(&mut self, observer: Arc<dyn StateObserver>) {
        self.observers.push(observer);
    }

    /// Check if Vietnamese input is currently enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the current input method ID
    pub fn current_method(&self) -> &str {
        &self.current_method
    }

    /// Get a reference to the current settings
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Get a mutable reference to the settings
    ///
    /// Note: After modifying settings, you should call `save_settings()` to persist changes.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Persist the "Học thông minh" (personal learning) switch.
    ///
    /// Lives HERE — not as a fresh `Settings::load`-modify-save at the call
    /// site — because `AppState` owns the settings object and re-saves it
    /// whole on every method change: an out-of-band write would be silently
    /// reverted by the next `set_method`.
    pub fn set_learning_enabled(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.settings.learning_enabled = enabled;
        self.settings.save()
    }

    /// Persist the "Tự động khởi động" (OS autostart) switch — same
    /// owner-object rationale as [`Self::set_learning_enabled`].
    pub fn set_startup(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.settings.startup = enabled;
        self.settings.save()
    }

    /// Persist the "Gõ tắt" (shorthand/macro expansion) switch — same
    /// owner-object rationale as [`Self::set_learning_enabled`].
    pub fn set_shorthand(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.settings.shorthand = enabled;
        self.settings.save()
    }

    /// Persist the "Kiểm soát gắt gao chính tả tiếng Việt" (strict
    /// spelling) switch — same owner-object rationale as
    /// [`Self::set_learning_enabled`]. Applying it to the LIVE keyboard is
    /// the caller's responsibility (`KeyboardManager::set_strict_spelling`).
    pub fn set_strict_spelling(&mut self, strict: bool) -> anyhow::Result<()> {
        self.settings.strict_spelling = strict;
        self.settings.save()
    }

    /// Persist the backspace-deletion mode (`"grapheme"`/`"raw"`) — same
    /// owner-object rationale as [`Self::set_learning_enabled`]. Applying
    /// the mode to the LIVE keyboard is the caller's responsibility (this
    /// only updates the persisted setting); `buttre-platform`'s config-
    /// window watcher does both together.
    pub fn set_backspace_mode(&mut self, mode: &str) -> anyhow::Result<()> {
        self.settings.backspace_mode = mode.to_string();
        self.settings.save()
    }

    /// Set the input method and notify observers
    ///
    /// This is the primary method for changing the input method. It will:
    /// 1. Update the internal state
    /// 2. Update and save settings
    /// 3. Notify all observers
    ///
    /// # Arguments
    /// * `method` - The new input method ID: `"telex"`, `"vni"`, `"nom"`, or a
    ///   custom id. NEVER `"english"` — use [`Self::set_enabled`].
    ///
    /// # Returns
    /// `Ok(())` if successful, or an error if settings could not be saved
    pub fn set_method(&mut self, method: &str) -> anyhow::Result<()> {
        info!(
            "Setting input method: {} (was: {})",
            method, self.current_method
        );

        // Deliberately does NOT touch `enabled` (ADR-0003 invariant 2). Turning
        // off used to overwrite this field with "english", which is why a
        // `last_vietnamese_method` stash had to exist to undo it. With the two
        // separate, off/on preserves the method for free — nothing to restore.
        self.current_method = method.to_string();
        self.settings.input_method = method.to_string();
        self.settings.save()?;

        self.notify_method_changed();

        Ok(())
    }

    /// Turn the input method on or off, and notify observers.
    ///
    /// Independent of [`Self::set_method`]: toggling off and on again lands back
    /// on the same method. Observers receive the same
    /// [`StateObserver::on_method_changed`] notification as a method switch —
    /// it already carries `(method, enabled)`, so one notification path keeps
    /// observers from ever seeing half of a change.
    ///
    /// # Returns
    /// `Ok(())` if successful, or an error if settings could not be saved
    pub fn set_enabled(&mut self, enabled: bool) -> anyhow::Result<()> {
        if self.enabled == enabled {
            return Ok(());
        }
        info!("Input method {}", if enabled { "on" } else { "off" });

        self.enabled = enabled;
        self.settings.enabled = enabled;
        self.settings.save()?;

        self.notify_method_changed();

        Ok(())
    }

    /// Flip the input method on/off — the tray click and the toggle hotkey.
    ///
    /// # Returns
    /// `Ok(())` if successful, or an error if settings could not be saved
    pub fn toggle(&mut self) -> anyhow::Result<()> {
        self.set_enabled(!self.enabled)
    }

    /// Save current settings to disk
    ///
    /// # Returns
    /// `Ok(())` if successful, or an error if settings could not be saved
    pub fn save_settings(&self) -> anyhow::Result<()> {
        self.settings.save()?;
        self.notify_settings_changed();
        Ok(())
    }

    /// Notify all observers that the input method has changed
    fn notify_method_changed(&self) {
        for observer in &self.observers {
            observer.on_method_changed(&self.current_method, self.enabled);
        }
    }

    /// Notify all observers that settings have changed
    fn notify_settings_changed(&self) {
        for observer in &self.observers {
            observer.on_settings_changed(&self.settings);
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
