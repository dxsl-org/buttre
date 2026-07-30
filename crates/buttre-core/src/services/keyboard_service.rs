//! Keyboard Service - Manages keyboard instances
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/service_keyboard_tests.rs`.
//!
//! This service handles the lifecycle of keyboard instances, including:
//! - Creating keyboards from configs
//! - Switching between keyboards
//! - Processing input through the current keyboard
//! - Publishing keyboard events to the event bus

use crate::events::{AppEvent, SharedEventBus};
use crate::Action;
use crate::{Keyboard, KeyboardBuilder, KeyboardConfig};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Keyboard Service - Manages multiple keyboard instances
///
/// This service maintains a collection of loaded keyboards and tracks
/// which one is currently active. It integrates with the event bus to
/// publish keyboard-related events.
///
/// # Example
///
/// ```rust,ignore
/// use buttre_core::services::KeyboardService;
/// use buttre_core::events::create_event_bus;
///
/// let bus = create_event_bus();
/// let mut service = KeyboardService::new(bus.clone());
///
/// // Create Telex keyboard
/// service.create_preset(Preset::Telex)?;
///
/// // Switch to it
/// service.switch("telex")?;
///
/// // Process input
/// let action = service.process('a')?;
/// ```
pub struct KeyboardService {
    /// Map of keyboard ID to keyboard instance
    keyboards: HashMap<String, Keyboard>,

    /// Currently active keyboard ID
    current_id: Option<String>,

    /// Event bus for publishing events
    event_bus: SharedEventBus,
}

/// Preset keyboard types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Telex input method
    Telex,
    /// VNI input method
    Vni,
    /// Nôm input method (requires dictionary)
    Nom,
}

impl Preset {
    /// Get the ID for this preset
    pub fn id(&self) -> &'static str {
        match self {
            Preset::Telex => "telex",
            Preset::Vni => "vni",
            Preset::Nom => "nom",
        }
    }

    /// Get the display name for this preset
    pub fn name(&self) -> &'static str {
        match self {
            Preset::Telex => "Telex",
            Preset::Vni => "VNI",
            Preset::Nom => "Chữ Nôm",
        }
    }
}

impl KeyboardService {
    /// Create a new KeyboardService
    ///
    /// # Arguments
    ///
    /// * `event_bus` - Shared event bus for publishing events
    pub fn new(event_bus: SharedEventBus) -> Self {
        Self {
            keyboards: HashMap::new(),
            current_id: None,
            event_bus,
        }
    }

    /// Create a keyboard from a preset
    ///
    /// This will create and register a keyboard using one of the built-in presets.
    ///
    /// # Arguments
    ///
    /// * `preset` - The preset type to create
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if keyboard creation fails
    pub fn create_preset(&mut self, preset: Preset) -> Result<()> {
        let id = preset.id().to_string();

        let keyboard = match preset {
            Preset::Telex => KeyboardBuilder::telex()?,
            Preset::Vni => KeyboardBuilder::vni()?,
            Preset::Nom => KeyboardBuilder::nom(None)?, // TODO: Add dictionary path support
        };

        self.keyboards.insert(id.clone(), keyboard);

        // Publish config loaded event
        self.event_bus.publish(AppEvent::ConfigLoaded { id });

        Ok(())
    }

    /// Create a keyboard from a custom config
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for this keyboard
    /// * `config` - Keyboard configuration
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if keyboard creation fails
    pub fn create(&mut self, id: impl Into<String>, config: KeyboardConfig) -> Result<()> {
        let id = id.into();

        let keyboard = KeyboardBuilder::new()
            .with_config(config)
            .build()
            .context("Failed to create keyboard from config")?;

        self.keyboards.insert(id.clone(), keyboard);

        // Publish config loaded event
        self.event_bus.publish(AppEvent::ConfigLoaded { id });

        Ok(())
    }

    /// Switch to a different keyboard
    ///
    /// This will make the specified keyboard the active one for processing input.
    ///
    /// # Arguments
    ///
    /// * `id` - ID of the keyboard to switch to
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the keyboard doesn't exist
    pub fn switch(&mut self, id: &str) -> Result<()> {
        if !self.keyboards.contains_key(id) {
            return Err(anyhow::anyhow!("Keyboard '{}' not found", id));
        }

        self.current_id = Some(id.to_string());

        // A registered keyboard is by definition a real method, so switching to
        // one always reports enabled. Off is not a keyboard (ADR-0003) — it is
        // `current_id = None`, and no event claims otherwise.
        self.event_bus.publish(AppEvent::method_changed(id, true));

        Ok(())
    }

    /// Process a keystroke through the current keyboard
    ///
    /// # Arguments
    ///
    /// * `key` - The character to process
    ///
    /// # Returns
    ///
    /// The action to perform (DoNothing, Commit, or Replace)
    /// Note: Returns first action only for backward compatibility
    pub fn process(&mut self, key: char) -> Result<Action> {
        // Publish input event
        self.event_bus.publish(AppEvent::KeyboardInput(key));

        let action = if let Some(id) = &self.current_id {
            if let Some(keyboard) = self.keyboards.get_mut(id) {
                // Process returns Vec<Action>, take first one
                let actions = keyboard.process(key)?;
                actions.into_iter().next().unwrap_or(Action::DoNothing)
            } else {
                Action::DoNothing
            }
        } else {
            Action::DoNothing
        };

        // Publish output event
        self.event_bus
            .publish(AppEvent::KeyboardOutput(action.clone()));

        Ok(action)
    }

    /// Process backspace
    ///
    /// # Returns
    ///
    /// The action to perform
    pub fn backspace(&mut self) -> Result<Action> {
        if let Some(id) = &self.current_id {
            if let Some(keyboard) = self.keyboards.get_mut(id) {
                return keyboard.backspace();
            }
        }
        Ok(Action::DoNothing)
    }

    /// Reset the current keyboard's state
    pub fn reset(&mut self) {
        if let Some(id) = &self.current_id {
            if let Some(keyboard) = self.keyboards.get_mut(id) {
                keyboard.reset();
                self.event_bus.publish(AppEvent::KeyboardReset);
            }
        }
    }

    /// Get the current keyboard ID
    pub fn current(&self) -> Option<&str> {
        self.current_id.as_deref()
    }

    /// List all loaded keyboard IDs
    pub fn list(&self) -> Vec<&str> {
        self.keyboards.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a keyboard exists
    pub fn has(&self, id: &str) -> bool {
        self.keyboards.contains_key(id)
    }

    /// Remove a keyboard
    ///
    /// # Arguments
    ///
    /// * `id` - ID of the keyboard to remove
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if trying to remove the current keyboard
    pub fn remove(&mut self, id: &str) -> Result<()> {
        if self.current_id.as_deref() == Some(id) {
            return Err(anyhow::anyhow!("Cannot remove current keyboard"));
        }

        self.keyboards.remove(id);
        Ok(())
    }
}
