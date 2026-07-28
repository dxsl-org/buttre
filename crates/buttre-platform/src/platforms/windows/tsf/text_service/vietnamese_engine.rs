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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// Vietnamese input mode
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VietnameseMode {
    Telex,
    VNI,
    Nom,
    Custom(String), // Custom config with method ID
    /// Pass every key through untouched — the tray's "english" choice.
    English,
}

impl VietnameseMode {
    /// Parse a `Settings::input_method` id ("telex", "vni", "nom",
    /// "english", or a custom method id).
    ///
    /// Unknown ids become [`VietnameseMode::Custom`], which looks for a
    /// matching `<id>.toml`; if that is missing the engine loads no keyboard
    /// and passes keys through, so a stale or misspelt id degrades to plain
    /// typing rather than to the wrong language.
    pub fn from_settings_id(id: &str) -> Self {
        match id {
            "telex" => Self::Telex,
            "vni" => Self::VNI,
            "nom" => Self::Nom,
            "english" => Self::English,
            other => Self::Custom(other.to_string()),
        }
    }
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
    /// `Settings::strict_spelling` mirror, written by `_macros_watcher`'s
    /// callback (notify's own thread) and consumed lazily at the top of
    /// [`Self::process_key`] — a TSF text service has no event loop to
    /// deliver the change on, so the keystroke path itself picks it up.
    strict_spelling: Arc<AtomicBool>,
    /// Last value actually pushed into the live `Keyboard` — lets the
    /// per-keystroke check be a cheap atomic load + compare instead of an
    /// unconditional `set_strict_spelling` write.
    strict_applied: bool,
    /// `Settings::input_method` mirror — how the TRAY's method choice reaches
    /// this process at all.
    ///
    /// The text service runs inside the host application and shares no state
    /// with the tray, so picking VNI in the tray menu changed nothing here:
    /// the service was constructed with a hard-coded Telex and `set_mode` had
    /// no caller anywhere. Written by `_macros_watcher` (notify's thread) on
    /// any `settings.toml` change, consumed lazily on the keystroke path —
    /// same contract as `strict_spelling`, for the same reason: a TSF text
    /// service has no event loop to deliver the change on.
    input_method: Arc<Mutex<String>>,
    /// Method id last applied to the live `Keyboard`, so the per-keystroke
    /// check is a string compare rather than an unconditional rebuild.
    method_applied: String,
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
    /// Starts in the SAVED method (`Settings::input_method`), not a fixed
    /// default — that is how the tray's choice reaches a text service running
    /// in someone else's process.
    ///
    /// No `Default` impl on purpose: this reads `settings.toml` and spawns a
    /// filesystem watcher, which is not what `default()` should mean.
    #[allow(clippy::new_without_default)] // reason: constructing this does real I/O
    pub fn new() -> Self {
        let settings = Settings::load();
        let macros = Arc::new(Mutex::new(MacroStore::load_gated(settings.shorthand)));
        let saved = VietnameseMode::from_settings_id(&settings.input_method);
        let mut engine = Self::new_with_macros(saved, macros.clone());
        engine.method_applied = settings.input_method.clone();
        engine
            .strict_spelling
            .store(settings.strict_spelling, Ordering::Relaxed);
        *engine
            .input_method
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = settings.input_method;
        engine._macros_watcher = spawn_reload_watcher(
            macros,
            engine.strict_spelling.clone(),
            engine.input_method.clone(),
        );
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
            // Starts lenient (`false`) — the engine default. `new()` stores
            // the real `Settings::strict_spelling` right after construction;
            // `process_key`'s lazy sync then pushes it into the keyboard.
            strict_spelling: Arc::new(AtomicBool::new(false)),
            strict_applied: false,
            input_method: Arc::new(Mutex::new(String::new())),
            method_applied: String::new(),
            _macros_watcher: None,
        }
    }

    /// Load keyboard instance for given mode, wiring in the shared shorthand
    /// store (`macros`) for every mode — including when shorthand is off,
    /// in which case `macros` holds an empty `MacroStore` and every lookup
    /// is a no-op, byte-identical to shorthand being unwired entirely.
    fn load_keyboard(mode: &VietnameseMode, macros: &Arc<Mutex<MacroStore>>) -> Option<Keyboard> {
        let mut kb = match mode {
            // No keyboard at all: `process_key` then returns DoNothing and the
            // host application sees the raw keystroke.
            VietnameseMode::English => None,
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
        self.sync_method();
        self.sync_strict_spelling();
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

    /// Flip the open composition between its literal keystrokes and the
    /// composed Vietnamese form (`Ctrl+Shift+Z` — see
    /// `buttre_core::keyboard::Keyboard::toggle_composition` for the full
    /// contract, including the word freeze that carries the choice through to
    /// the commit).
    ///
    /// # Returns
    ///
    /// The composition update to write, or `None` when there is nothing to
    /// toggle (no composition open, or no keyboard loaded) — the caller then
    /// lets the keystroke fall through to the application, so a host app that
    /// uses `Ctrl+Shift+Z` as "redo" keeps working when we have no word to act
    /// on.
    pub fn toggle_composition(&mut self) -> Option<Action> {
        self.keyboard.as_mut()?.toggle_composition()
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
            // A fresh `Keyboard` always starts lenient — force the next
            // `process_key` to re-push the user's strict-spelling choice.
            self.strict_applied = false;
            self.mode = mode;
            self.reset();
        }
    }

    /// Adopt a method the tray switched to since the last keystroke (see the
    /// `input_method` field).
    ///
    /// Cheap when nothing changed: one lock and a string compare. Rebuilding
    /// the keyboard is the expensive branch, and it only runs on an actual
    /// switch.
    fn sync_method(&mut self) {
        let wanted = {
            let guard = self
                .input_method
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            guard.clone()
        };
        if wanted.is_empty() || wanted == self.method_applied {
            return;
        }
        tracing::info!("TSF: switching input method to '{wanted}'");
        self.set_mode(VietnameseMode::from_settings_id(&wanted));
        self.method_applied = wanted;
    }

    /// Push a changed `Settings::strict_spelling` into the live `Keyboard`
    /// (see the `strict_spelling` field doc). Cheap when nothing changed:
    /// one relaxed atomic load + bool compare.
    fn sync_strict_spelling(&mut self) {
        let strict = self.strict_spelling.load(Ordering::Relaxed);
        if strict != self.strict_applied {
            if let Some(kb) = self.keyboard.as_mut() {
                kb.set_strict_spelling(strict);
            }
            self.strict_applied = strict;
        }
    }

    /// Generate candidate list (stub for Nom support)
    pub fn generate_candidates(&self, _input: &str) -> Vec<CandidateItem> {
        // TODO: Implement Nom candidate generation when needed
        Vec::new()
    }
}
