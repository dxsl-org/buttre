//! buttre Linux Input Method
//!
//! Supports IBus via D-Bus (zbus 3).

#![cfg(target_os = "linux")]

pub mod ibus;
pub mod ibus_bus;

use crate::PlatformBackend;
use anyhow::Result;
use buttre_core::state::{Settings, StateObserver};
use buttre_core::{Action, Keyboard};
use std::sync::{Arc, RwLock};

/// Linux backend — tray-side only.
///
/// The IBus engine is NOT hosted here: ibus-daemon spawns `buttre --ibus`
/// as its own process (see `ibus_bus::run_engine`) per the component XML
/// and owns that process's lifecycle. Hosting the engine inside the tray
/// app was part of the original "typing dead" bug — the daemon-spawned
/// copy died on the single-instance lock while the tray copy sat invisible
/// on the session bus.
pub struct LinuxBackend {
    enabled: bool,
}

impl PlatformBackend for LinuxBackend {
    fn new() -> Result<Self> {
        Ok(Self { enabled: false })
    }

    fn init(&mut self, _keyboard: Arc<RwLock<Option<Keyboard>>>) -> Result<()> {
        tracing::info!(
            "Linux backend: tray mode only — the IBus engine runs as a \
             separate ibus-daemon-spawned process (`buttre --ibus`)"
        );
        Ok(())
    }

    fn process_key(&mut self, _key: char) -> Action {
        Action::DoNothing
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn cleanup(&mut self) {}
}

impl StateObserver for LinuxBackend {
    fn on_method_changed(&self, _method: &str, enabled: bool) {
        tracing::info!("LinuxBackend: method changed, enabled={}", enabled);
    }

    fn on_settings_changed(&self, _settings: &Settings) {}
}
