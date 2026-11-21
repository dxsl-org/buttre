//! Windows platform backend

pub mod common;
pub mod hook;
pub mod transport_claim;
pub mod tsf;

use crate::PlatformBackend;
use anyhow::Result;
use buttre_core::state::{Settings, StateObserver};
use buttre_core::Action;
use log::info;
use std::sync::{Arc, Mutex, RwLock};

/// The backend the tray drives: the global keyboard hook.
///
/// The tray only runs where buttre itself owns method choice — no buttre
/// keyboard added in Windows, or `buttre --hook` (ADR-0004). With a buttre
/// keyboard added, the TSF text service lives inside each application and is
/// switched through the OS keyboard list; it needs no tray process at all, so
/// this backend never starts it. The hook still stands down in any process
/// whose text service has claimed it (`transport_claim`), so running the
/// hook app alongside a buttre keyboard never types a key twice.
pub struct WindowsBackend {
    enabled: Arc<Mutex<bool>>,
    current_method: Arc<Mutex<String>>,
    hook: hook::HookBackend,
}

impl WindowsBackend {
    pub fn new() -> Result<Self> {
        let hook = hook::HookBackend::new()?;
        info!("✓ Hook backend initialized");
        Ok(Self {
            // Off + no method yet; the first `on_method_changed` notification
            // fills both. "english" is not a method value anymore (ADR-0003).
            enabled: Arc::new(Mutex::new(false)),
            current_method: Arc::new(Mutex::new(String::new())),
            hook,
        })
    }
}

impl PlatformBackend for WindowsBackend {
    fn new() -> Result<Self> {
        Self::new()
    }

    fn init(&mut self, keyboard: Arc<RwLock<Option<buttre_core::Keyboard>>>) -> Result<()> {
        info!("Initializing Windows platform backend (Hook)");
        self.hook.init(keyboard)
    }

    fn process_key(&mut self, _key: char) -> Action {
        // The hook handles its own key processing asynchronously
        Action::DoNothing
    }

    fn set_enabled(&mut self, enabled: bool) {
        info!("Windows backend toggling enabled state: {enabled}");
        *self.enabled.lock().unwrap_or_else(|e| e.into_inner()) = enabled;
        self.hook.set_enabled(enabled);
    }

    fn cleanup(&mut self) {
        info!("Cleaning up Windows backend");
        self.hook.cleanup();
    }

    /// The hook callback deliberately EXEMPTS `Ctrl+Shift+Z` from the
    /// modifier-reset and waits for the tray's global hotkey to dispatch it
    /// (`hook::dispatch_toggle_last_word`), so the tray must register it.
    fn owns_word_toggle_chord(&self) -> bool {
        false
    }
}

impl StateObserver for WindowsBackend {
    fn on_method_changed(&self, method: &str, enabled: bool) {
        info!(
            "WindowsBackend (Observer): Method changed to {} (enabled: {})",
            method, enabled
        );
        *self
            .current_method
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = method.to_string();
        *self.enabled.lock().unwrap_or_else(|e| e.into_inner()) = enabled;
        // The hook was installed but stays inactive without the explicit flip.
        hook::set_vietnamese_mode(enabled);
    }

    fn on_settings_changed(&self, _settings: &Settings) {
        // Settings changed - could update backend configuration here
    }
}
