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
    /// Whether Vietnamese input is currently enabled
    enabled: bool,

    /// Current input method ID (e.g., "telex", "vni", "nom", "english")
    current_method: String,

    /// Last Vietnamese method used (for toggle functionality)
    last_vietnamese_method: String,

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
        let settings = Settings::load();
        let enabled = settings.input_method != "english";
        let current_method = settings.input_method.clone();
        let last_vietnamese_method = if enabled {
            current_method.clone()
        } else {
            "telex".to_string() // Default Vietnamese method
        };

        info!(
            "Initialized AppState: method={}, enabled={}",
            current_method, enabled
        );

        Self {
            enabled,
            current_method,
            last_vietnamese_method,
            settings,
            observers: Vec::new(),
        }
    }

    /// Create a new `AppState` with custom settings
    ///
    /// Useful for testing or when you want to override default settings.
    pub fn with_settings(settings: Settings) -> Self {
        let enabled = settings.input_method != "english";
        let current_method = settings.input_method.clone();
        let last_vietnamese_method = if enabled {
            current_method.clone()
        } else {
            "telex".to_string()
        };

        Self {
            enabled,
            current_method,
            last_vietnamese_method,
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

    /// Set the input method and notify observers
    ///
    /// This is the primary method for changing the input method. It will:
    /// 1. Update the internal state
    /// 2. Update and save settings
    /// 3. Notify all observers
    ///
    /// # Arguments
    /// * `method` - The new input method ID (e.g., "telex", "vni", "nom", "english")
    ///
    /// # Returns
    /// `Ok(())` if successful, or an error if settings could not be saved
    pub fn set_method(&mut self, method: &str) -> anyhow::Result<()> {
        info!(
            "Setting input method: {} (was: {})",
            method, self.current_method
        );

        // Remember last Vietnamese method if switching away from one
        if self.enabled && method == "english" {
            self.last_vietnamese_method = self.current_method.clone();
        }

        // Update state
        self.current_method = method.to_string();
        self.enabled = method != "english";

        // Update last_vietnamese_method if switching to a Vietnamese method
        if self.enabled {
            self.last_vietnamese_method = method.to_string();
        }

        // Update and save settings
        self.settings.input_method = method.to_string();
        self.settings.save()?;

        // Notify observers
        self.notify_method_changed();

        Ok(())
    }

    /// Toggle between Vietnamese and English input
    ///
    /// If currently in English mode, switches to the last used Vietnamese method (or Telex as default).
    /// If currently in Vietnamese mode, switches to English.
    ///
    /// # Returns
    /// `Ok(())` if successful, or an error if settings could not be saved
    pub fn toggle(&mut self) -> anyhow::Result<()> {
        let new_method = if self.enabled {
            // Currently enabled -> switch to English
            "english".to_string()
        } else {
            // Currently disabled -> switch to last Vietnamese method
            self.last_vietnamese_method.clone()
        };

        info!("Toggling: {} -> {}", self.current_method, new_method);
        self.set_method(&new_method)
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
