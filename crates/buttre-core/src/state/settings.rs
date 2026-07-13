//! Settings management for buttre application
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/state_tests.rs`.
//!
//! This module handles loading and saving application settings to disk.
//! Settings are stored in a platform-specific location using TOML format.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application settings
///
/// These settings are persisted to disk and loaded on application startup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Current input method ID (e.g., "english", "telex", "vni", "nom", or custom method ID)
    pub input_method: String,

    /// Enable auto-correction features
    pub auto_correct: bool,

    /// Enable shorthand/macro expansion
    pub shorthand: bool,

    /// Launch buttre on system startup
    pub startup: bool,

    /// Backspace deletion granularity (event-sourcing-completion Phase 4):
    /// `"grapheme"` (default) deletes the last DISPLAYED character —
    /// unchanged pre-phase behavior. `"raw"` deletes the last RAW keystroke
    /// and recomposes — the event-sourced engine's trivially-correct
    /// inverse, at the cost of sometimes removing more or less than one
    /// visible glyph. Parsed via `buttre_core::keyboard::BackspaceMode::
    /// from_settings_str`, which falls back to `"grapheme"` for any unknown
    /// value (never fails to load).
    #[serde(default = "default_backspace_mode")]
    pub backspace_mode: String,

    /// Enable personal learning (event-sourcing-completion Phase 5): the
    /// user-attested syllable overlay and raw-sequence preference memory
    /// persisted to `learning.toml`. When `false`, no signals are collected
    /// and no snapshot is applied (behavior is byte-identical to no store).
    /// PRIVACY: `learning.toml` holds fragments of typed words (raw key
    /// sequences the user corrected); it is local-only, never logged, and is
    /// removed/reset by deleting the file. Default on — the feature silently
    /// improves typing over time; flip to `false` to disable and stop
    /// collection.
    #[serde(default = "default_learning_enabled")]
    pub learning_enabled: bool,
}

/// `serde(default)` value for `Settings::backspace_mode` — also the fallback
/// `Settings::default()` uses, so both paths agree on one literal.
fn default_backspace_mode() -> String {
    "grapheme".to_string()
}

/// `serde(default)` value for `Settings::learning_enabled`.
fn default_learning_enabled() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_method: "english".to_string(),
            auto_correct: false,
            shorthand: false,
            startup: false,
            backspace_mode: default_backspace_mode(),
            learning_enabled: default_learning_enabled(),
        }
    }
}

impl Settings {
    /// Get the settings file path
    ///
    /// Platform-specific paths:
    /// - Windows: %APPDATA%\buttre\settings.toml
    /// - macOS: ~/Library/Application Support/buttre/settings.toml
    /// - Linux: ~/.config/buttre/settings.toml
    pub fn get_path() -> Result<PathBuf> {
        let data_dir =
            dirs::data_dir().ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?;
        let buttre_dir = data_dir.join("buttre");
        fs::create_dir_all(&buttre_dir)?;
        Ok(buttre_dir.join("settings.toml"))
    }

    /// Load settings from file, or return default if file doesn't exist
    ///
    /// This method will never fail - if the settings file cannot be loaded,
    /// it will return default settings instead.
    pub fn load() -> Self {
        match Self::get_path() {
            Ok(path) => {
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(settings) = toml::from_str(&content) {
                            return settings;
                        }
                    }
                }
            }
            Err(e) => eprintln!("Failed to get settings path: {:?}", e),
        }
        Self::default()
    }

    /// Save settings to file — atomically (temp file + rename).
    ///
    /// Not just belt-and-suspenders: the config window (a separate process)
    /// and the tray both now touch this file — the tray watches it for
    /// live-reload (`buttre-platform/src/main.rs`'s settings watcher) — so a
    /// plain in-place write could race a concurrent read and hand back a
    /// half-written, unparseable file. Mirrors `LearningStore::write_atomic`
    /// / `MacroStore::write_atomic`'s existing pattern.
    ///
    /// The temp filename is unique per call (see
    /// `super::atomic_write::unique_temp_path`'s doc): with two independent
    /// PROCESSES (the config window and the tray) able to save this same
    /// file — plus, in tests, many THREADS in one process — a shared temp
    /// name would let one writer's `fs::write` truncate mid-write of
    /// another's, or one writer's rename consume the temp file out from
    /// under a concurrent one.
    ///
    /// # Errors
    /// Returns an error if the settings file cannot be written.
    pub fn save(&self) -> Result<()> {
        let path = Self::get_path()?;
        let content = toml::to_string_pretty(self)?;
        let tmp_path = super::atomic_write::unique_temp_path(&path, "toml");
        fs::write(&tmp_path, content)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_backspace_mode_is_grapheme() {
        assert_eq!(Settings::default().backspace_mode, "grapheme");
    }

    #[test]
    fn backspace_mode_defaults_when_absent_from_toml() {
        // Old settings.toml files predate this field entirely — `load()`
        // promises to never fail, and a missing field must fall back to
        // "grapheme" (byte-identical pre-phase behavior), not an error.
        let toml_str = r#"
            input_method = "telex"
            auto_correct = false
            shorthand = false
            startup = false
        "#;
        let settings: Settings =
            toml::from_str(toml_str).expect("must deserialize without backspace_mode present");
        assert_eq!(settings.backspace_mode, "grapheme");
    }

    #[test]
    fn backspace_mode_round_trips_through_toml() {
        let settings = Settings {
            backspace_mode: "raw".to_string(),
            ..Settings::default()
        };
        let serialized = toml::to_string_pretty(&settings).expect("serialize");
        let restored: Settings = toml::from_str(&serialized).expect("deserialize");
        assert_eq!(restored.backspace_mode, "raw");
    }
}
