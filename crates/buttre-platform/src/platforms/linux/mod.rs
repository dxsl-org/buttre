//! buttre Linux Input Method
//!
//! Supports IBus via D-Bus (zbus 3).

#![cfg(target_os = "linux")]

pub mod ibus;
pub mod ibus_bus;
pub mod macro_sync;
pub mod method_sync;
pub mod wayland;

/// Composition semantics shared with the macOS FFI — see `shared/engine_bridge.rs`.
pub use crate::shared::engine_bridge;

/// Engine-mode entry with backend auto-detection (`buttre --ime`):
/// Wayland-native `zwp_input_method_v2` when the compositor supports it and
/// no other IME owns the seat; otherwise IBus (GNOME/Mutter, X11). The
/// `--ibus` flag still forces the IBus component — that is what ibus-daemon
/// spawns per the component XML.
pub fn run_engine_auto() -> anyhow::Result<()> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        match wayland::run_engine() {
            Err(e) if e.downcast_ref::<wayland::Unavailable>().is_some() => {
                tracing::info!("{e}; falling back to IBus");
            }
            other => return other,
        }
    } else {
        tracing::info!("No WAYLAND_DISPLAY; using the IBus backend");
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(ibus_bus::run_engine())
}

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
    /// Tray-side half of the method sync (B5): persist the chosen method so
    /// the daemon-spawned engine's watcher picks it up. "english" and custom
    /// TOML ids are skipped by `write_method` — IBus enable/disable is the
    /// OS input-source switcher's job.
    fn on_method_changed(&self, method: &str, enabled: bool) {
        tracing::info!("LinuxBackend: method changed to {method} (enabled={enabled})");
        if let Err(e) = method_sync::write_method(method) {
            tracing::warn!("LinuxBackend: method sync write failed: {e}");
        }
    }

    fn on_settings_changed(&self, _settings: &Settings) {}
}
