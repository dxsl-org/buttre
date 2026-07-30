//! Windows platform backend

pub mod common;
pub mod hook;
pub mod transport_claim;
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
    /// Both transports live at once (phase 03, ADR-0003): TSF types in the
    /// apps that cooperate with it, the hook covers the rest. Never both on
    /// one keystroke: the text service claims the foreground window it owns
    /// through `transport_claim` and the hook stands down there.
    Both(tsf::TsfBackend, hook::HookBackend),
}

/// Windows backend implementation with TSF-first fallback
pub struct WindowsBackend {
    enabled: Arc<Mutex<bool>>,
    current_method: Arc<Mutex<String>>,
    mode: BackendMode,
}

impl WindowsBackend {
    /// Create the Windows backend.
    ///
    /// With TSF available AND `Settings::hook_fallback` on, BOTH transports
    /// start (their union is the coverage goal, see `transport_claim`).
    /// `hook_fallback = false` is the field kill switch back to
    /// one-backend-per-session. No TSF (not registered / not added as an
    /// input method) keeps the plain hook, as before.
    pub fn new() -> Result<Self> {
        let hook_fallback = buttre_core::Settings::load().hook_fallback;
        info!("Creating Windows backend (hook_fallback: {hook_fallback})");

        let mode = match tsf::TsfBackend::new() {
            Ok(tsf) if hook_fallback => {
                let hook = hook::HookBackend::new()?;
                info!("✓ TSF + Hook fallback initialized (transport_claim arbitrates)");
                BackendMode::Both(tsf, hook)
            }
            Ok(tsf) => {
                info!("✓ TSF backend initialized (hook fallback disabled)");
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
            BackendMode::Both(..) => "TSF+Hook",
        };
        info!(
            "Initializing Windows platform backend (mode: {})",
            mode_name
        );

        match &mut self.mode {
            BackendMode::Tsf(tsf) => tsf.init(keyboard),
            BackendMode::Hook(hook) => hook.init(keyboard),
            BackendMode::Both(tsf, hook) => {
                tsf.init(keyboard.clone())?;
                hook.init(keyboard)
            }
        }
    }

    fn process_key(&mut self, _key: char) -> Action {
        // TSF and Hook handle their own key processing asynchronously
        Action::DoNothing
    }

    fn set_enabled(&mut self, enabled: bool) {
        info!("Windows backend toggling enabled state: {enabled}");

        *self.enabled.lock().unwrap() = enabled;

        match &mut self.mode {
            BackendMode::Tsf(tsf) => tsf.set_enabled(enabled),
            BackendMode::Hook(hook) => hook.set_enabled(enabled),
            BackendMode::Both(tsf, hook) => {
                tsf.set_enabled(enabled);
                hook.set_enabled(enabled);
            }
        }
    }

    fn cleanup(&mut self) {
        info!("Cleaning up Windows backend");
        match &mut self.mode {
            BackendMode::Tsf(tsf) => tsf.cleanup(),
            BackendMode::Hook(hook) => hook.cleanup(),
            BackendMode::Both(tsf, hook) => {
                tsf.cleanup();
                hook.cleanup();
            }
        }
    }

    /// TSF handles `Ctrl+Shift+Z` inside the focused app's process (see
    /// `tsf::text_service`'s `VK_WORD_TOGGLE`), so the tray must leave the
    /// chord unregistered. The Hook backend is the opposite: its own callback
    /// deliberately EXEMPTS the chord from the modifier-reset and waits for the
    /// global hotkey to dispatch it (`hook::dispatch_toggle_last_word`).
    /// `Both` also claims the chord: registering it globally would swallow the
    /// keystroke before any focused app's text service saw it. The cost is that
    /// hook-covered apps (no TSF) lose Ctrl+Shift+Z under `Both`. Accepted:
    /// rare-and-degraded beats breaking the chord everywhere TSF works.
    fn owns_word_toggle_chord(&self) -> bool {
        matches!(self.mode, BackendMode::Tsf(_) | BackendMode::Both(..))
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
                // TSF reads state through the settings watcher in the DLL.
                info!("TSF mode: method={}, enabled={}", method, enabled);
            }
            BackendMode::Hook(_) | BackendMode::Both(..) => {
                // The hook side needs the explicit flip (hook was installed
                // but not enabled without it). Under Both, TSF still gets its
                // state via the settings watcher, same as the Tsf arm.
                info!("Hook side: setting Vietnamese enabled = {}", enabled);
                hook::set_vietnamese_mode(enabled);
            }
        }
    }

    fn on_settings_changed(&self, _settings: &Settings) {
        // Settings changed - could update backend configuration here
    }
}
