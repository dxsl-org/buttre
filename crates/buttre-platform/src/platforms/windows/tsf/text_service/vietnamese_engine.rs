// SPDX-License-Identifier: GPL-3.0-only
// Vietnamese Engine Integration for TSF
//
// **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/platform_windows_tsf_tests.rs`.

use super::candidate_ui::CandidateItem;
use super::macro_reload::spawn_reload_watcher;
use buttre_core::state::macros::MacroStore;
use buttre_core::Action;
use buttre_core::InputBuffer;
use buttre_core::Keyboard;
use buttre_core::KeyboardBuilder;
use buttre_core::Settings;
use notify::RecommendedWatcher;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Vietnamese input mode
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VietnameseMode {
    Telex,
    VNI,
    Nom,
    Custom(String), // Custom config with method ID
}

/// Vietnamese Engine for TSF
/// Wraps buttre-keyboard and provides TSF-compatible interface
pub struct VietnameseEngine {
    mode: VietnameseMode,
    keyboard: Option<Keyboard>,
    buffer: InputBuffer,
    /// Shorthand/gõ tắt store (`wire-shorthand-tsf-linux` Phase 3): shared
    /// with every `Keyboard` this engine builds via `load_keyboard`, and
    /// swapped in place by `_macros_watcher` on external `macros.toml` /
    /// `settings.toml` edits — the live `Keyboard`s see the update through
    /// the shared `Arc` without needing re-injection.
    macros: Arc<Mutex<MacroStore>>,
    /// Live-reload watcher, kept alive only to hold the watch open for this
    /// engine's lifetime — dropped (stopping the watch) when the engine
    /// drops, i.e. on TSF `Deactivate`. `None` when the watch could not be
    /// established (see `spawn_reload_watcher`); typing still works, just
    /// without live reload.
    _macros_watcher: Option<RecommendedWatcher>,
}

impl VietnameseEngine {
    /// Create a new Vietnamese engine.
    ///
    /// This is the FIRST config load in the TSF process (no prior code path
    /// read `settings.toml` or `macros.toml` here) — both `Settings::load`
    /// and `MacroStore::load_gated` degrade to safe defaults on any IO/parse
    /// failure rather than erroring, which matters because this DLL runs
    /// in-process inside an arbitrary host app under `panic = abort`.
    pub fn new(mode: VietnameseMode) -> Self {
        let shorthand = Settings::load().shorthand;
        let macros = Arc::new(Mutex::new(MacroStore::load_gated(shorthand)));
        let mut engine = Self::new_with_macros(mode, macros.clone());
        engine._macros_watcher = spawn_reload_watcher(macros);
        engine
    }

    /// Create a new Vietnamese engine with an EXPLICIT shorthand store,
    /// bypassing `Settings::load`/`MacroStore::load_gated` and the
    /// live-reload watcher entirely. Used by integration tests
    /// (`platform_windows_tsf_tests.rs`) that must never touch a real
    /// `%APPDATA%` file — production code calls [`Self::new`] instead.
    pub fn new_with_macros(mode: VietnameseMode, macros: Arc<Mutex<MacroStore>>) -> Self {
        let keyboard = Self::load_keyboard(&mode, &macros);
        Self {
            mode,
            keyboard,
            buffer: InputBuffer::new(),
            macros,
            _macros_watcher: None,
        }
    }

    /// Load keyboard instance for given mode, wiring in the shared shorthand
    /// store (`macros`) for every mode — including when shorthand is off,
    /// in which case `macros` holds an empty `MacroStore` and every lookup
    /// is a no-op, byte-identical to shorthand being unwired entirely.
    fn load_keyboard(mode: &VietnameseMode, macros: &Arc<Mutex<MacroStore>>) -> Option<Keyboard> {
        let mut kb = match mode {
            VietnameseMode::Telex => KeyboardBuilder::telex_with_composition(true).ok(),
            VietnameseMode::VNI => KeyboardBuilder::vni_with_composition(true).ok(),
            VietnameseMode::Nom => {
                // Load Nôm dictionary and create keyboard with TSF composition mode
                let nom_path = buttre_core::vietnamese::get_nom_db_path();
                KeyboardBuilder::nom_with_composition(nom_path, true).ok()
            }
            VietnameseMode::Custom(method_id) => {
                // Load custom config from file (same as Hook)
                tracing::info!("TSF: Loading custom keyboard: {}", method_id);
                let custom_dir = buttre_core::vietnamese::get_custom_dir();
                let config_path = custom_dir.join(format!("{}.toml", method_id));

                if config_path.exists() {
                    match buttre_core::Config::load(config_path.to_str().unwrap()) {
                        Ok(config) => {
                            tracing::info!("TSF: loaded custom keyboard from {:?}", config_path);
                            // Create keyboard with composition mode for TSF
                            KeyboardBuilder::new()
                                .with_config(config)
                                .with_composition(true)
                                .build()
                                .ok()
                        }
                        Err(e) => {
                            tracing::warn!("TSF: Failed to load custom keyboard: {}", e);
                            None
                        }
                    }
                } else {
                    tracing::warn!("TSF: Custom config not found: {:?}", config_path);
                    None
                }
            }
        }?;

        kb.set_macros(macros.clone());
        Some(kb)
    }

    /// Process a key press.
    ///
    /// Returns every action the engine produced for this key, in order —
    /// callers MUST apply all of them. A closed word run followed by a
    /// separator (e.g. `"xin."`) yields `[ConfirmComposition("xin"),
    /// Commit(".")]`; dropping the trailing action silently swallows the
    /// separator (issue #4).
    pub fn process_key(&mut self, ch: char) -> Vec<Action> {
        if let Some(ref mut kb) = self.keyboard {
            match kb.process(ch) {
                Ok(actions) => actions,
                Err(e) => {
                    tracing::warn!("Keyboard process error: {}", e);
                    vec![Action::DoNothing]
                }
            }
        } else {
            vec![Action::DoNothing]
        }
    }

    /// Process backspace
    pub fn process_backspace(&mut self) -> Action {
        if let Some(ref mut kb) = self.keyboard {
            match kb.backspace() {
                Ok(action) => action,
                Err(e) => {
                    tracing::warn!("Keyboard backspace error: {}", e);
                    Action::DoNothing
                }
            }
        } else {
            Action::DoNothing
        }
    }

    /// Reset the engine state
    pub fn reset(&mut self) {
        self.buffer.clear();
        if let Some(ref mut kb) = self.keyboard {
            kb.reset();
        }
    }

    /// Word-boundary final repair probe (event-sourcing-completion Phase 3):
    /// see `buttre_core::keyboard::Keyboard::boundary_repair`.
    ///
    /// Callers (Enter, and TSF's own buffer-reset-key handling in
    /// `text_service_stub.rs`) query this BEFORE ending the composition —
    /// those commit points bypass `process_key`/`ConfirmComposition`
    /// entirely (they call `end_composition` directly), so without this
    /// probe a shape-only inferred word (e.g. VNI `"nhat6"`) would commit
    /// unrepaired.
    pub fn boundary_repair(&self) -> Option<String> {
        self.keyboard.as_ref().and_then(|kb| kb.boundary_repair())
    }

    /// Get current buffer content
    pub fn buffer_content(&self) -> String {
        if let Some(ref kb) = self.keyboard {
            kb.buffer().to_string()
        } else {
            self.buffer.to_string()
        }
    }

    /// Switch input mode
    pub fn set_mode(&mut self, mode: VietnameseMode) {
        if self.mode != mode {
            self.keyboard = Self::load_keyboard(&mode, &self.macros);
            self.mode = mode;
            self.reset();
        }
    }

    /// Generate candidate list (stub for Nom support)
    pub fn generate_candidates(&self, _input: &str) -> Vec<CandidateItem> {
        // TODO: Implement Nom candidate generation when needed
        Vec::new()
    }
}
