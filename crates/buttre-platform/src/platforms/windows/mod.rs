//! Windows platform backend

pub mod common;
pub mod hook;
pub mod tsf;

use crate::PlatformBackend;
use anyhow::Result;
use buttre_core::state::{Settings, StateObserver};
use buttre_core::Action;
use log::{info, warn};
use std::sync::{Arc, Mutex, RwLock};

/// Windows backend mode
pub enum BackendMode {
    Tsf(tsf::TsfBackend),
    Hook(hook::HookBackend),
}

/// Windows backend implementation with TSF-first fallback
pub struct WindowsBackend {
    enabled: Arc<Mutex<bool>>,
    current_method: Arc<Mutex<String>>,
    mode: BackendMode,
}

impl WindowsBackend {
    /// Create Windows backend with TSF-first fallback
    pub fn new() -> Result<Self> {
        info!("Creating Windows backend with TSF-first fallback");

        let mode = match tsf::TsfBackend::new() {
            Ok(tsf) => {
                info!("✓ TSF backend initialized");
                BackendMode::Tsf(tsf)
            }
            Err(e) => {
                warn!("✗ TSF initialization failed: {}. Falling back to Hook.", e);
                let hook = hook::HookBackend::new()?;
                info!("✓ Hook backend initialized");
                BackendMode::Hook(hook)
            }
        };

        Ok(Self {
            // Off + no method yet; the first `on_method_changed` notification
            // fills both. "english" is not a method value anymore (ADR-0003).
            enabled: Arc::new(Mutex::new(false)),
            current_method: Arc::new(Mutex::new(String::new())),
            mode,
        })
    }
}

impl PlatformBackend for WindowsBackend {
    fn new() -> Result<Self> {
        Self::new()
    }

    fn init(&mut self, keyboard: Arc<RwLock<Option<buttre_core::Keyboard>>>) -> Result<()> {
        let mode_name = match &self.mode {
            BackendMode::Tsf(_) => "TSF",
            BackendMode::Hook(_) => "Hook",
        };
        info!(
            "Initializing Windows platform backend (mode: {})",
            mode_name
        );

        match &mut self.mode {
            BackendMode::Tsf(tsf) => tsf.init(keyboard),
            BackendMode::Hook(hook) => hook.init(keyboard),
        }
    }

    fn process_key(&mut self, _key: char) -> Action {
        // TSF and Hook handle their own key processing asynchronously
        Action::DoNothing
    }

    fn set_enabled(&mut self, enabled: bool) {
        let mode_name = match &self.mode {
            BackendMode::Tsf(_) => "TSF",
            BackendMode::Hook(_) => "Hook",
        };
        info!(
            "Windows backend (mode: {}) toggling enabled state: {}",
            mode_name, enabled
        );

        *self.enabled.lock().unwrap() = enabled;

        match &mut self.mode {
            BackendMode::Tsf(tsf) => tsf.set_enabled(enabled),
            BackendMode::Hook(hook) => hook.set_enabled(enabled),
        }
    }

    fn cleanup(&mut self) {
        info!("Cleaning up Windows backend");
        match &mut self.mode {
            BackendMode::Tsf(tsf) => tsf.cleanup(),
            BackendMode::Hook(hook) => hook.cleanup(),
        }
    }

    /// TSF handles `Ctrl+Shift+Z` inside the focused app's process (see
    /// `tsf::text_service`'s `VK_WORD_TOGGLE`), so the tray must leave the
    /// chord unregistered. The Hook backend is the opposite: its own callback
    /// deliberately EXEMPTS the chord from the modifier-reset and waits for the
    /// global hotkey to dispatch it (`hook::dispatch_toggle_last_word`).
    fn owns_word_toggle_chord(&self) -> bool {
        matches!(self.mode, BackendMode::Tsf(_))
    }
}

impl StateObserver for WindowsBackend {
    fn on_method_changed(&self, method: &str, enabled: bool) {
        info!(
            "WindowsBackend (Observer): Method changed to {} (enabled: {})",
            method, enabled
        );

        *self.current_method.lock().unwrap() = method.to_string();
        *self.enabled.lock().unwrap() = enabled;

        // Update backend state based on mode
        // Since &self is immutable, we use lock-free functions for Hook
        match &self.mode {
            BackendMode::Tsf(_) => {
                // TSF handles its own state or listens to settings
                // TODO: Implement TSF state update if needed
                info!("TSF mode: method={}, enabled={}", method, enabled);
            }
            BackendMode::Hook(_) => {
                // CRITICAL FIX: Actually enable/disable the hook when method changes
                // This was the missing piece - hook was installed but not enabled!
                info!("Hook mode: setting Vietnamese enabled = {}", enabled);
                hook::set_vietnamese_mode(enabled);
            }
        }
    }

    fn on_settings_changed(&self, _settings: &Settings) {
        // Settings changed - could update backend configuration here
    }
}
