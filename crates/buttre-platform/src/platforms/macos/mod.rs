//! buttre macOS Input Method
//!
//! Implementation using macOS Input Method Kit (IMKit)

#![cfg(target_os = "macos")]

pub mod ffi;

use crate::PlatformBackend;
use anyhow::Result;
use buttre_core::state::{Settings, StateObserver};
use buttre_core::{Action, Keyboard};
use std::sync::{Arc, RwLock};

/// macOS backend implementation
pub struct MacOSBackend {
    enabled: bool,
}

impl PlatformBackend for MacOSBackend {
    fn new() -> Result<Self> {
        Ok(Self { enabled: false })
    }

    fn init(&mut self, _keyboard: Arc<RwLock<Option<Keyboard>>>) -> Result<()> {
        tracing::info!("Initializing macOS (IMKit) backend");
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

impl StateObserver for MacOSBackend {
    fn on_method_changed(&self, _method: &str, enabled: bool) {
        tracing::info!("MacOSBackend: method changed, enabled={}", enabled);
    }

    fn on_settings_changed(&self, _settings: &Settings) {}
}

// Re-export FFI functions for C
pub use ffi::{
    buttre_engine_flush, buttre_engine_free, buttre_engine_new, buttre_engine_process_backspace,
    buttre_engine_process_key, buttre_engine_reset, buttre_engine_set_enabled,
    buttre_engine_set_method, ButtreKeyResult,
};
