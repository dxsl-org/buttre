//! Shared engine-action → IME-operation mapping (all preedit-model hosts).
//!
//! IBus (`linux/ibus.rs`), Wayland-native (`linux/wayland/`), and the macOS
//! FFI (`macos/ffi.rs`, consumed by the IMKit host) all speak the same
//! preedit model; this bridge is the single source of those semantics so
//! backends cannot drift. It is pure — no D-Bus, no Wayland, no FFI — which
//! also makes the full composition behavior unit-testable on any OS
//! (`tests/shared_engine_bridge_tests.rs`).
//!
//! The `Keyboard` runs in composition mode (the TSF mode): the pipeline
//! itself owns word logic, emitting `UpdateComposition` for the growing word
//! and, at separators, `[ConfirmComposition(repaired word), Commit(sep)]`.
//! The bridge folds those into [`ImeOp`]s plus a `handled` verdict — when
//! `handled` is false the backend forwards the ORIGINAL key event to the
//! app (after any queued commit, so the committed word always lands first).

use buttre_core::{Action, Keyboard, KeyboardBuilder};

/// One IME-visible operation, in emission order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeOp {
    /// Update the preedit to this exact text; empty clears the region.
    Preedit(String),
    /// Commit text to the application.
    Commit(String),
}

/// Result of feeding one event to the bridge.
#[derive(Debug, Default)]
pub struct KeyOutcome {
    pub ops: Vec<ImeOp>,
    /// `false` → the backend must let the original key reach the app.
    pub handled: bool,
}

/// Build a keyboard in composition mode.
///
/// Nôm runs WITHOUT its dictionary here: the candidate UI isn't wired on
/// either Linux backend yet, so lookups would produce actions we drop.
fn build_keyboard(method: &str) -> Keyboard {
    match method {
        "vni" => {
            KeyboardBuilder::vni_with_composition(true).expect("Failed to create VNI keyboard")
        }
        "nom" => KeyboardBuilder::nom_with_composition(None, true)
            .expect("Failed to create Nôm keyboard"),
        _ => {
            KeyboardBuilder::telex_with_composition(true).expect("Failed to create Telex keyboard")
        }
    }
}

pub struct EngineBridge {
    keyboard: Keyboard,
    preedit: String,
}

impl EngineBridge {
    pub fn new(method: &str) -> Self {
        Self {
            keyboard: build_keyboard(method),
            preedit: String::new(),
        }
    }

    /// Non-panicking constructor for FFI callers — the release profile is
    /// `panic = "abort"`, so a builder panic would kill the host app.
    pub fn try_new(method: &str) -> Option<Self> {
        let keyboard = match method {
            "vni" => KeyboardBuilder::vni_with_composition(true),
            "nom" => KeyboardBuilder::nom_with_composition(None, true),
            _ => KeyboardBuilder::telex_with_composition(true),
        };
        match keyboard {
            Ok(keyboard) => Some(Self {
                keyboard,
                preedit: String::new(),
            }),
            Err(e) => {
                tracing::warn!("EngineBridge::try_new({method}): {e}");
                None
            }
        }
    }

    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// Switch input method: rebuild the keyboard, discarding any live
    /// composition (a mode switch is a reset by definition).
    pub fn rebuild(&mut self, method: &str) -> KeyOutcome {
        self.keyboard = build_keyboard(method);
        let mut outcome = KeyOutcome {
            ops: Vec::new(),
            handled: true,
        };
        if !self.preedit.is_empty() {
            self.preedit.clear();
            outcome.ops.push(ImeOp::Preedit(String::new()));
        }
        outcome
    }

    /// Feed one character. The engine classifies separators itself.
    pub fn process_char(&mut self, ch: char) -> KeyOutcome {
        let actions = match self.keyboard.process(ch) {
            Ok(actions) => actions,
            Err(e) => {
                tracing::warn!("Keyboard process error: {}", e);
                return KeyOutcome {
                    ops: Vec::new(),
                    handled: false,
                };
            }
        };

        let mut ops = Vec::new();
        let mut emitted = false;
        let mut pass_char = false;
        for action in actions {
            match action {
                Action::UpdateComposition { text, .. } => {
                    self.preedit = text.clone();
                    ops.push(ImeOp::Preedit(text));
                    emitted = true;
                }
                Action::ConfirmComposition(text) => {
                    self.preedit.clear();
                    // Clear the preedit region BEFORE the commit so the word
                    // isn't momentarily doubled in the client.
                    ops.push(ImeOp::Preedit(String::new()));
                    ops.push(ImeOp::Commit(text));
                    emitted = true;
                }
                Action::Commit(text) => {
                    // The engine echoing the input character back is a
                    // pass-through separator — forward the original key.
                    if text.chars().eq(std::iter::once(ch)) {
                        pass_char = true;
                    } else {
                        ops.push(ImeOp::Commit(text));
                        emitted = true;
                    }
                }
                Action::DoNothing => {}
                Action::ShowCandidates { .. } | Action::HideCandidates => {
                    // Nôm candidate UI: future phase on both backends.
                }
                other => {
                    tracing::warn!(
                        "Unexpected hook-model action in composition mode: {:?}",
                        other
                    );
                }
            }
        }

        let handled = if pass_char {
            false
        } else if emitted {
            true
        } else {
            // Pure DoNothing: swallow keys the engine deliberately ignored
            // mid-composition; pass through when nothing is composing.
            !self.preedit.is_empty()
        };
        KeyOutcome { ops, handled }
    }

    /// Backspace shrinks the composition; the engine recomputes the word
    /// from raw keys and the new preedit is its canonical buffer. With no
    /// composition the app handles the key itself.
    pub fn backspace(&mut self) -> KeyOutcome {
        if self.preedit.is_empty() {
            return KeyOutcome {
                ops: Vec::new(),
                handled: false,
            };
        }
        if let Err(e) = self.keyboard.backspace() {
            tracing::warn!("Keyboard backspace error: {}", e);
        }
        self.preedit = self.keyboard.buffer().to_string();
        KeyOutcome {
            ops: vec![ImeOp::Preedit(self.preedit.clone())],
            handled: true,
        }
    }

    /// Commit the pending word out-of-band (shortcuts, navigation keys),
    /// applying the word-boundary final repair — these commit points bypass
    /// the pipeline's own PassThrough repair. No-op when nothing composes.
    pub fn flush_pending(&mut self) -> KeyOutcome {
        if self.preedit.is_empty() {
            return KeyOutcome::default();
        }
        let text = self
            .keyboard
            .boundary_repair()
            .unwrap_or_else(|| self.preedit.clone());
        self.keyboard.reset();
        self.preedit.clear();
        KeyOutcome {
            ops: vec![ImeOp::Preedit(String::new()), ImeOp::Commit(text)],
            handled: true,
        }
    }

    /// Discard the composition without committing (daemon Reset semantics).
    pub fn discard(&mut self) -> KeyOutcome {
        let had = !self.preedit.is_empty();
        self.keyboard.reset();
        self.preedit.clear();
        KeyOutcome {
            ops: if had {
                vec![ImeOp::Preedit(String::new())]
            } else {
                Vec::new()
            },
            handled: true,
        }
    }
}

// ============================================================================
// Keysym classification (shared: IBus keyvals ARE X11 keysyms, which is also
// what xkbcommon produces — one table serves both backends)
// ============================================================================

/// True for modifier-only keysyms (Shift_L/R, Ctrl_L/R, Caps_Lock, …).
pub fn is_modifier_keysym(keysym: u32) -> bool {
    matches!(keysym, 0xFFE1..=0xFFEE | 0xFE01..=0xFE0F)
}

/// True for non-printable keys that end the composition and pass through
/// (navigation, Tab, Escape, Delete, …). Printable separators (space,
/// punctuation) are NOT classified here — the engine pipeline decides those.
pub fn is_break_keysym(keysym: u32) -> bool {
    matches!(
        keysym,
        0xFF09 // Tab
        | 0xFF1B // Escape
        | 0xFF50 // Home
        | 0xFF51
            ..=0xFF54 // Left/Up/Right/Down
        | 0xFF55 // Page_Up
        | 0xFF56 // Page_Down
        | 0xFF57 // End
        | 0xFF63 // Insert
        | 0xFFFF // Delete
    )
}

/// Convert an X11 keysym to a character. XKB resolves Shift/CapsLock BEFORE
/// the keysym reaches us (`Shift+a` arrives as keysym 0x41 = 'A'), so
/// printable ASCII maps by identity.
pub fn keysym_to_char(keysym: u32) -> Option<char> {
    match keysym {
        0x0020..=0x007E => char::from_u32(keysym),
        0xFF0D => Some('\n'),   // Return
        0xFF08 => Some('\x08'), // BackSpace
        _ => None,
    }
}
