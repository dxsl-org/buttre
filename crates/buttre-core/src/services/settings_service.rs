//! Settings Service - Settings persistence with event bus integration
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/service_settings_tests.rs`.
//!
//! This service wraps the Settings struct and integrates it with the event bus,
//! automatically publishing events when settings change.

use crate::events::{AppEvent, SharedEventBus};
use crate::state::Settings;
use anyhow::Result;

/// Settings Service - Manages application settings with event bus integration
///
/// This service provides a clean interface for settings management and
/// automatically publishes events when settings change.
///
/// # Example
///
/// ```rust,ignore
/// use buttre_core::services::SettingsService;
/// use buttre_core::events::create_event_bus;
///
/// let bus = create_event_bus();
/// let mut service = SettingsService::new(bus.clone());
///
/// // Update settings
/// service.update(|settings| {
///     settings.auto_correct = true;
/// })?;
///
/// // Save to disk
/// service.save()?;
/// ```
pub struct SettingsService {
    /// Current settings
    settings: Settings,

    /// Event bus for publishing events
    event_bus: SharedEventBus,
}

impl SettingsService {
    /// Create a new SettingsService
    ///
    /// This will load settings from disk if they exist, or use defaults.
    ///
    /// # Arguments
    ///
    /// * `event_bus` - Shared event bus for publishing events
    pub fn new(event_bus: SharedEventBus) -> Self {
        let settings = Settings::load();

        Self {
            settings,
            event_bus,
        }
    }

    /// Create a SettingsService with custom settings
    ///
    /// # Arguments
    ///
    /// * `settings` - Initial settings
    /// * `event_bus` - Shared event bus for publishing events
    pub fn with_settings(settings: Settings, event_bus: SharedEventBus) -> Self {
        Self {
            settings,
            event_bus,
        }
    }

    /// Get a reference to the current settings
    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// Update settings using a closure
    ///
    /// This will apply the changes and publish a SettingsChanged event.
    /// Note: This does NOT save to disk automatically. Call `save()` to persist.
    ///
    /// # Arguments
    ///
    /// * `f` - Closure that modifies the settings
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// service.update(|settings| {
    ///     settings.auto_correct = true;
    ///     settings.shorthand = false;
    /// })?;
    /// ```
    pub fn update<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Settings),
    {
        // Apply changes
        f(&mut self.settings);

        // Publish event
        self.event_bus
            .publish(AppEvent::SettingsChanged(self.settings.clone()));

        Ok(())
    }

    /// Save settings to disk
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the file could not be written
    pub fn save(&self) -> Result<()> {
        self.settings.save()?;
        Ok(())
    }

    /// Load settings from disk
    ///
    /// This will reload settings from disk and publish a SettingsChanged event.
    pub fn load(&mut self) -> Result<()> {
        self.settings = Settings::load();
        self.event_bus
            .publish(AppEvent::SettingsChanged(self.settings.clone()));
        Ok(())
    }

    /// Set the input method
    ///
    /// This is a convenience method that updates the input_method field
    /// and publishes both SettingsChanged and MethodChanged events.
    ///
    /// # Arguments
    ///
    /// * `method` - New input method ID (never `"english"` — off is
    ///   [`Settings::enabled`], see [`Self::set_enabled`])
    pub fn set_input_method(&mut self, method: impl Into<String>) -> Result<()> {
        let method = method.into();
        self.settings.input_method = method.clone();

        // Publish both events. `enabled` is carried from settings, not derived
        // from the method (ADR-0003).
        self.event_bus
            .publish(AppEvent::SettingsChanged(self.settings.clone()));
        self.event_bus
            .publish(AppEvent::method_changed(method, self.settings.enabled));

        Ok(())
    }

    /// Turn the input method on or off — the counterpart of
    /// [`Self::set_input_method`] for the other half of the split state.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.enabled = enabled;
        self.event_bus
            .publish(AppEvent::SettingsChanged(self.settings.clone()));
        self.event_bus.publish(AppEvent::method_changed(
            self.settings.input_method.clone(),
            enabled,
        ));
        Ok(())
    }

    /// Get the current input method
    pub fn input_method(&self) -> &str {
        &self.settings.input_method
    }

    /// Borrow the full settings (read-only).
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Check if auto-correct is enabled
    pub fn auto_correct(&self) -> bool {
        self.settings.auto_correct
    }

    /// Check if shorthand is enabled
    pub fn shorthand(&self) -> bool {
        self.settings.shorthand
    }

    /// Check if startup is enabled
    pub fn startup(&self) -> bool {
        self.settings.startup
    }
}
