//! Keyboard management for buttre application
//!
//! ## Flow:
//! ```text
//! Platform → KeyboardManager → buttre-keyboard::Keyboard → buttre-engine
//! ```

use anyhow::Result;
use buttre_core::state::learning::{LearningFile, LearningStore};
use buttre_core::state::macros::MacroStore;
use buttre_core::{Keyboard, KeyboardBuilder};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};

/// Personal-learning wiring (event-sourcing-completion Phase 5): the shared
/// store handle plus its off-thread save channel, bundled so
/// `KeyboardManager` can re-inject both into every `Keyboard` it builds (a
/// fresh instance starts with neither — see `Keyboard::set_learning`'s doc).
#[derive(Clone)]
struct LearningWiring {
    store: Arc<Mutex<LearningStore>>,
    save_tx: mpsc::Sender<LearningFile>,
}

/// Keyboard manager that wraps buttre-keyboard with Arc<Mutex<>>
pub struct KeyboardManager {
    keyboard: Arc<RwLock<Option<Keyboard>>>,
    current_method: Arc<Mutex<String>>,
    /// `None` until `set_learning` is called — mirrors `Settings::
    /// learning_enabled` gating at the call site in `main.rs` (event-
    /// sourcing-completion Phase 5).
    learning: Mutex<Option<LearningWiring>>,
    /// `None` until `set_macros` is called — mirrors `Settings::shorthand`
    /// gating at the call site in `main.rs`. Bare `Arc` (no bundling struct
    /// needed): unlike learning, macros carry no save channel — the store is
    /// read-only at the keyboard layer (see `buttre_core::state::macros`).
    macros: Mutex<Option<Arc<Mutex<MacroStore>>>>,
    /// Mirrors `Settings::strict_spelling` — remembered here (like the two
    /// fields above) because a fresh `Keyboard` built in `set_method` always
    /// starts lenient and must have the user's choice re-applied.
    strict_spelling: Mutex<bool>,
}

impl KeyboardManager {
    /// Create a new keyboard manager (starts with no keyboard - English mode)
    pub fn new() -> Result<Self> {
        Ok(Self {
            keyboard: Arc::new(RwLock::new(None)),
            current_method: Arc::new(Mutex::new("english".to_string())),
            learning: Mutex::new(None),
            macros: Mutex::new(None),
            strict_spelling: Mutex::new(false),
        })
    }

    /// Get a clone of the keyboard Arc for sharing across threads
    pub fn get_keyboard(&self) -> Arc<RwLock<Option<Keyboard>>> {
        self.keyboard.clone()
    }

    /// Wire the personal-learning store + off-thread save channel
    /// (event-sourcing-completion Phase 5). The CALLER (`main.rs`) gates
    /// calling this at all on `Settings::learning_enabled` — never calling
    /// it leaves every future `Keyboard` build byte-identical to pre-Phase-5
    /// behavior. Applies immediately to whatever `Keyboard` is CURRENTLY
    /// loaded (if any), and is remembered for every future `set_method`
    /// call, since building a new `Keyboard` there always starts with no
    /// handle (mirrors `BackspaceModeObserver`'s re-apply-on-switch pattern
    /// in `main.rs`).
    pub fn set_learning(
        &self,
        store: Arc<Mutex<LearningStore>>,
        save_tx: mpsc::Sender<LearningFile>,
    ) {
        let wiring = LearningWiring { store, save_tx };
        *self.learning.lock().unwrap() = Some(wiring.clone());
        if let Ok(mut guard) = self.keyboard.write() {
            if let Some(kb) = guard.as_mut() {
                kb.set_learning(wiring.store, wiring.save_tx);
            }
        }
    }

    /// Unwire personal learning at runtime (tray "Học thông minh" off):
    /// forgets the wiring for future `set_method` builds AND detaches it
    /// from the currently loaded `Keyboard` — the exact inverse of
    /// [`Self::set_learning`].
    pub fn clear_learning(&self) {
        *self.learning.lock().unwrap() = None;
        if let Ok(mut guard) = self.keyboard.write() {
            if let Some(kb) = guard.as_mut() {
                kb.clear_learning();
            }
        }
    }

    /// Wire the shorthand/gõ tắt store (mirrors [`Self::set_learning`]'s
    /// gating and re-apply-on-switch pattern; the CALLER gates calling this
    /// at all on `Settings::shorthand`).
    pub fn set_macros(&self, store: Arc<Mutex<MacroStore>>) {
        *self.macros.lock().unwrap() = Some(store.clone());
        if let Ok(mut guard) = self.keyboard.write() {
            if let Some(kb) = guard.as_mut() {
                kb.set_macros(store);
            }
        }
    }

    /// Apply `Settings::strict_spelling` to the current `Keyboard` (if any)
    /// and remember it for every future `set_method` build — same
    /// re-apply-on-switch pattern as learning/macros above.
    pub fn set_strict_spelling(&self, strict: bool) {
        *self.strict_spelling.lock().unwrap() = strict;
        if let Ok(mut guard) = self.keyboard.write() {
            if let Some(kb) = guard.as_mut() {
                kb.set_strict_spelling(strict);
            }
        }
    }

    /// Unwire shorthand at runtime (tray "Gõ tắt" off) — the exact inverse
    /// of [`Self::set_macros`].
    pub fn clear_macros(&self) {
        *self.macros.lock().unwrap() = None;
        if let Ok(mut guard) = self.keyboard.write() {
            if let Some(kb) = guard.as_mut() {
                kb.clear_macros();
            }
        }
    }

    /// Set the input method
    pub fn set_method(&self, method: &str) -> Result<()> {
        tracing::info!("KeyboardManager: Setting method to '{}'", method);

        *self.current_method.lock().unwrap() = method.to_string();

        if method == "english" {
            // English mode - clear keyboard
            *self.keyboard.write().unwrap() = None;
            return Ok(());
        }

        // Load keyboard for this method
        let mut keyboard = match method {
            "telex" => match KeyboardBuilder::telex() {
                Ok(kb) => kb,
                Err(e) => {
                    tracing::error!("Failed to load Telex keyboard: {}", e);
                    *self.keyboard.write().unwrap() = None;
                    return Err(e);
                }
            },
            "vni" => match KeyboardBuilder::vni() {
                Ok(kb) => kb,
                Err(e) => {
                    tracing::error!("Failed to load VNI keyboard: {}", e);
                    *self.keyboard.write().unwrap() = None;
                    return Err(e);
                }
            },
            "nom" => {
                let nom_path = buttre_core::vietnamese::get_nom_db_path();
                if nom_path.is_none() {
                    tracing::warn!("Nôm dictionary not found! Using Telex fallback mode.");
                } else {
                    tracing::info!("Replacing text with Nôm using dictionary: {:?}", nom_path);
                }

                match KeyboardBuilder::nom(nom_path) {
                    Ok(kb) => kb,
                    Err(e) => {
                        tracing::error!("Failed to load Nôm keyboard: {}", e);
                        *self.keyboard.write().unwrap() = None;
                        return Err(e);
                    }
                }
            }
            _ => {
                // Try to load custom keyboard from file
                tracing::info!("Attempting to load custom keyboard: {}", method);
                let custom_dir = buttre_core::vietnamese::get_custom_dir();
                let config_path = custom_dir.join(format!("{}.toml", method));

                if config_path.exists() {
                    match buttre_core::Config::load(config_path.to_str().unwrap()) {
                        Ok(config) => {
                            tracing::info!("✓ Loaded custom keyboard from {:?}", config_path);
                            KeyboardBuilder::new().with_config(config).build()?
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to load custom keyboard: {}, falling back to English",
                                e
                            );
                            *self.keyboard.write().unwrap() = None;
                            return Ok(());
                        }
                    }
                } else {
                    tracing::warn!(
                        "Custom keyboard '{}' not found at {:?}, falling back to English",
                        method,
                        config_path
                    );
                    *self.keyboard.write().unwrap() = None;
                    return Ok(());
                }
            }
        };

        // Re-inject personal-learning wiring (event-sourcing-completion
        // Phase 5): every `Keyboard` above starts with none — a fresh
        // instance's `learning` field always defaults to `None` — so
        // whatever was handed to `set_learning` must be re-applied on every
        // method switch, same as `BackspaceModeObserver` re-applies
        // `backspace_mode` in `main.rs`. No-op when learning was never wired
        // (`self.learning` stays `None`, matching `Settings::learning_enabled
        // == false`).
        let wiring = self.learning.lock().unwrap().clone();
        if let Some(wiring) = wiring {
            keyboard.set_learning(wiring.store, wiring.save_tx);
        }
        // Same re-apply-on-switch story for shorthand — a fresh `Keyboard`
        // always starts with `macros == None`.
        let macros = self.macros.lock().unwrap().clone();
        if let Some(store) = macros {
            keyboard.set_macros(store);
        }
        // And for strict spelling — a fresh `Keyboard` always starts lenient.
        let strict = *self.strict_spelling.lock().unwrap();
        if strict {
            keyboard.set_strict_spelling(true);
        }

        *self.keyboard.write().unwrap() = Some(keyboard);
        tracing::info!("✓ Loaded keyboard for method '{}'", method);
        Ok(())
    }

    /// Get current method
    pub fn current_method(&self) -> String {
        self.current_method.lock().unwrap().clone()
    }
}

impl Default for KeyboardManager {
    fn default() -> Self {
        Self::new().expect("Failed to create keyboard manager")
    }
}
