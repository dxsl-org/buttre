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

use buttre_core::state::learning::{LearningFile, LearningStore};
use buttre_core::state::macros::MacroStore;
use buttre_core::{Action, Keyboard, KeyboardBuilder};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use crate::shared::candidates::CandidateState;

/// Re-exported so existing backends keep importing it from here. The type
/// itself lives in [`crate::shared::candidates`] because the Windows TSF text
/// service needs the same candidate model without pulling in this bridge.
pub use crate::shared::candidates::CandidateView;

/// One IME-visible operation, in emission order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeOp {
    /// Update the preedit to this exact text; empty clears the region.
    Preedit(String),
    /// Commit text to the application.
    Commit(String),
    /// Show/replace the candidate list (Nôm) with `cursor` highlighted. Empty
    /// is never emitted — use [`ImeOp::HideCandidates`] to clear. `cursor` is a
    /// GLOBAL index into `items`; the backend's page size decides which page is
    /// shown. Backends without a candidate UI (Wayland, macOS today) ignore it.
    Candidates {
        items: Vec<CandidateView>,
        cursor: usize,
    },
    /// Hide the candidate list.
    HideCandidates,
    /// Delete `n` characters immediately before the cursor from ALREADY
    /// COMMITTED text (no-preedit / commit-as-you-go mode). Emitted only when
    /// composition is off; a backend without an in-place delete primitive must
    /// never receive it — the IBus engine only turns composition off after
    /// confirming the client advertises surrounding-text support.
    DeleteSurrounding(usize),
}

/// Result of feeding one event to the bridge.
#[derive(Debug, Default)]
pub struct KeyOutcome {
    pub ops: Vec<ImeOp>,
    /// `false` → the backend must let the original key reach the app.
    pub handled: bool,
}

/// Build a keyboard in composition mode. Returns `None` on builder failure
/// (logged) — NEVER panics: the release profile is `panic = "abort"`, so a
/// panic here would kill the host process outright. Callers decide whether
/// to keep the current keyboard, fall back, or report failure.
///
/// Nôm loads its dictionary from the discovered `buttre_nom.db` (the same path
/// resolver the Windows TSF host uses) so lookups produce candidates for the
/// IBus lookup table. A missing db degrades to no-candidate Nôm rather than
/// failing the build — `get_nom_db_path` returns `None` and
/// `nom_with_composition` runs dictionary-less. Telex/VNI ignore the path.
///
/// `use_composition` selects the preedit (underline) model vs the no-preedit
/// diff model (`Replace`/`Commit`). Nôm ALWAYS composes regardless — its
/// candidate popup needs a preedit anchor — so the flag only affects Telex/VNI.
///
/// A non-built-in `method` is a CUSTOM keyboard id: its config is loaded from
/// `keyboards/{method}.toml` (the id was admitted into the sync channel by
/// `method_sync::is_engine_method_in`, which stat'ed that exact file). A TOML
/// deleted or broken between admission and this build degrades to Telex — the
/// same fallback the sync channel's read path uses — rather than failing.
fn build_keyboard(method: &str, use_composition: bool) -> Option<Keyboard> {
    let composition = method == "nom" || use_composition;
    let result = match method {
        "telex" => KeyboardBuilder::telex_with_composition(composition),
        "vni" => KeyboardBuilder::vni_with_composition(composition),
        "nom" => KeyboardBuilder::nom_with_composition(
            buttre_core::vietnamese::get_nom_db_path(),
            composition,
        ),
        custom => match load_custom_config(custom) {
            Some(config) => KeyboardBuilder::new()
                .with_config(config)
                .with_composition(composition)
                .build(),
            None => KeyboardBuilder::telex_with_composition(composition),
        },
    };
    result
        .map_err(|e| tracing::warn!("build_keyboard({method}): {e}"))
        .ok()
}

/// Load a custom keyboard's `Config` from `keyboards/{id}.toml`, `None` (with
/// a warning) when the file is missing or unparseable. Split from
/// [`build_keyboard`] so the fallback decision stays readable there.
fn load_custom_config(id: &str) -> Option<buttre_core::Config> {
    let path = buttre_core::vietnamese::get_custom_dir().join(format!("{id}.toml"));
    let Some(path_str) = path.to_str() else {
        tracing::warn!("custom keyboard path {path:?} is not UTF-8, falling back to telex");
        return None;
    };
    match buttre_core::Config::load(path_str) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!("custom keyboard {id:?} failed to load ({e}), falling back to telex");
            None
        }
    }
}

pub struct EngineBridge {
    keyboard: Keyboard,
    preedit: String,
    /// Shorthand/gõ tắt store, re-applied to every fresh `Keyboard` built by
    /// [`Self::rebuild`] (a rebuild always starts from a store-less
    /// `Keyboard`). `None` means no host injected one — byte-identical to
    /// today's behavior. The bridge stays pure: it only holds and forwards
    /// the `Arc`, never loads or watches `macros.toml` itself (that is the
    /// Linux host's `platforms::linux::macro_sync` job).
    macros: Option<Arc<Mutex<MacroStore>>>,
    /// Personal-learning wiring, re-applied to every fresh `Keyboard` by
    /// [`Self::install_keyboard`] — same lifecycle as `macros`. `None` =
    /// learning off (the `Keyboard::new` default): no collection, no
    /// consultation, byte-identical to pre-learning behavior. The bridge
    /// stays pure — the host loads the store, owns the save channel's
    /// receiving end, and does every disk write.
    learning: Option<(Arc<Mutex<LearningStore>>, Sender<LearningFile>)>,
    /// `Settings::strict_spelling` mirror injected by the host via
    /// [`Self::set_strict_flag`] (same purity rule as `macros`: the bridge
    /// never reads `settings.toml` itself). Written by the host's
    /// `macro_sync` watcher thread; consumed lazily at the top of
    /// [`Self::process_char`] — the engine processes have no other event
    /// delivery path. `None` = permanently lenient (the engine default).
    strict_spelling: Option<Arc<AtomicBool>>,
    /// Last value pushed into the live `Keyboard` (see
    /// `VietnameseEngine::strict_applied` — same cheap-compare pattern).
    strict_applied: bool,
    /// Candidates for the current composition (Nôm) plus the highlight.
    /// Mirrors what the engine last emitted via `Action::ShowCandidates`, so a
    /// backend can select one by index ([`Self::select_candidate`]) without
    /// re-querying. Empty when no list is showing; always empty for Telex/VNI.
    candidates: CandidateState,
    /// The current method id (`"telex"`/`"vni"`/`"nom"`), needed to rebuild the
    /// keyboard when [`Self::set_use_composition`] toggles the preedit model and
    /// to keep Nôm on composition regardless of that toggle.
    method: String,
    /// Whether the active keyboard uses the preedit (underline) model. Always
    /// `true` at construction (macOS/Wayland/Windows and the IBus default);
    /// only the IBus engine flips it off via [`Self::set_use_composition`] once
    /// the client is confirmed to support in-place deletion. Nôm ignores it.
    use_composition: bool,
    /// Passthrough mode — the IME is OFF: the bridge stops composing entirely,
    /// every key returns unhandled with no ops, so the raw keystroke reaches
    /// the app. The engine stays the active IBus engine, it just goes silent
    /// (Unikey model). The `keyboard` field keeps its last real build (never
    /// consulted while passthrough) so no call path has to handle a missing
    /// keyboard.
    ///
    /// Owned by `Settings::enabled` via [`Self::set_enabled`] (ADR-0003).
    /// `rebuild("english")` still flips it as a LEGACY-WIRE shim: no buttre
    /// surface writes `"english"` into the `method_sync` file anymore, but a
    /// stale file from an older build — and the fcitx5 addon's own English
    /// item (`bt_engine_set_method("english")`) — still speak it, and those
    /// callers cannot be broken from here.
    passthrough: bool,
}

impl EngineBridge {
    /// Infallible constructor for the Linux engine processes (tests too).
    /// A failed non-telex method degrades to telex rather than crashing the
    /// engine process; only a telex-build failure — the hardcoded default,
    /// meaning the whole app is unusable — is treated as unrecoverable.
    pub fn new(method: &str) -> Self {
        let keyboard = build_keyboard(method, true)
            .or_else(|| build_keyboard("telex", true))
            .expect("the built-in telex keyboard must always build");
        Self {
            keyboard,
            preedit: String::new(),
            macros: None,
            learning: None,
            strict_spelling: None,
            strict_applied: false,
            candidates: CandidateState::default(),
            method: method.to_string(),
            use_composition: true,
            passthrough: method == "english",
        }
    }

    /// Same as [`Self::new`] but wires a shorthand store into the keyboard
    /// at construction, and remembers it so [`Self::rebuild`] can re-apply
    /// it to every subsequent `Keyboard`.
    pub fn new_with_macros(method: &str, macros: Arc<Mutex<MacroStore>>) -> Self {
        let mut keyboard = build_keyboard(method, true)
            .or_else(|| build_keyboard("telex", true))
            .expect("the built-in telex keyboard must always build");
        keyboard.set_macros(macros.clone());
        Self {
            keyboard,
            preedit: String::new(),
            macros: Some(macros),
            learning: None,
            strict_spelling: None,
            strict_applied: false,
            candidates: CandidateState::default(),
            method: method.to_string(),
            use_composition: true,
            passthrough: method == "english",
        }
    }

    /// Constructor for FFI callers that reports failure instead of degrading
    /// — the macOS host decides what to do when `buttre_engine_new` fails.
    pub fn try_new(method: &str) -> Option<Self> {
        Some(Self {
            keyboard: build_keyboard(method, true)?,
            preedit: String::new(),
            macros: None,
            learning: None,
            strict_spelling: None,
            strict_applied: false,
            candidates: CandidateState::default(),
            method: method.to_string(),
            use_composition: true,
            passthrough: method == "english",
        })
    }

    /// Fallible counterpart of [`Self::new_with_macros`] for FFI callers
    /// (macOS host ctor plumbing) — not yet wired into any caller, kept here
    /// so the store-holding path compiles and is ready for that host to
    /// adopt without another `EngineBridge` change.
    pub fn try_new_with_macros(method: &str, macros: Arc<Mutex<MacroStore>>) -> Option<Self> {
        let mut keyboard = build_keyboard(method, true)?;
        keyboard.set_macros(macros.clone());
        Some(Self {
            keyboard,
            preedit: String::new(),
            macros: Some(macros),
            learning: None,
            strict_spelling: None,
            strict_applied: false,
            candidates: CandidateState::default(),
            method: method.to_string(),
            use_composition: true,
            passthrough: method == "english",
        })
    }

    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// Turn the IME on or off — the model-level control [`Self::passthrough`]
    /// belongs to. Turning off flushes nothing by itself; hosts that need the
    /// pending word committed first call [`Self::flush_pending`] before this.
    /// Returns the ops that clear any live preedit/candidates so the switch
    /// never strands half a word on screen.
    pub fn set_enabled(&mut self, enabled: bool) -> KeyOutcome {
        if self.passthrough == enabled {
            self.passthrough = !enabled;
            if !enabled {
                self.keyboard.reset();
                return self.reset_outcome();
            }
        }
        KeyOutcome {
            ops: Vec::new(),
            handled: true,
        }
    }

    /// Is the IME on?
    pub fn is_enabled(&self) -> bool {
        !self.passthrough
    }

    /// Inject the host's `Settings::strict_spelling` mirror (see the
    /// `strict_spelling` field doc) and apply its current value immediately.
    /// Hosts call this once, right after construction — later changes flow
    /// through the shared flag and are picked up by [`Self::process_char`].
    pub fn set_strict_flag(&mut self, flag: Arc<AtomicBool>) {
        self.strict_spelling = Some(flag);
        self.sync_strict_spelling();
    }

    /// Wire personal learning into the live keyboard and remember it for
    /// every later rebuild — the composition-backend counterpart of
    /// `KeyboardManager::set_learning`. Hosts gate calling this on
    /// `Settings::learning_enabled`. An external `learning.toml` reload is
    /// a content swap of the SAME shared store; the live snapshot picks it
    /// up at the next word commit (`collect_and_refresh_learning`
    /// re-snapshots every time), no re-call needed.
    pub fn set_learning(
        &mut self,
        store: Arc<Mutex<LearningStore>>,
        save_tx: Sender<LearningFile>,
    ) {
        self.keyboard.set_learning(store.clone(), save_tx.clone());
        self.learning = Some((store, save_tx));
    }

    /// Detach learning from the live keyboard AND from future rebuilds —
    /// the runtime "Học thông minh off" toggle. After this the bridge is
    /// byte-identical to one that never had learning wired.
    pub fn clear_learning(&mut self) {
        self.keyboard.clear_learning();
        self.learning = None;
    }

    /// Is learning currently wired? The hosts' per-keystroke `sync_learning`
    /// compares this against the `Settings::learning_enabled` mirror.
    pub fn has_learning(&self) -> bool {
        self.learning.is_some()
    }

    /// Push a changed strict-spelling value into the live `Keyboard`. Cheap
    /// when nothing changed: one relaxed atomic load + bool compare.
    fn sync_strict_spelling(&mut self) {
        let Some(flag) = &self.strict_spelling else {
            return;
        };
        let strict = flag.load(Ordering::Relaxed);
        if strict != self.strict_applied {
            self.keyboard.set_strict_spelling(strict);
            self.strict_applied = strict;
        }
    }

    /// Drop any showing candidate list, yielding a `HideCandidates` op only
    /// when one was actually showing — callers append it to their outcome so a
    /// reset/commit/method-switch never leaves a stale popup on screen.
    fn clear_candidates_op(&mut self) -> Option<ImeOp> {
        self.candidates.clear().then_some(ImeOp::HideCandidates)
    }

    /// The `Candidates` op for the current list + cursor. Callers only build it
    /// when `candidates` is non-empty (the variant is never emitted empty).
    fn candidates_op(&self) -> ImeOp {
        ImeOp::Candidates {
            items: self.candidates.items().to_vec(),
            cursor: self.candidates.cursor(),
        }
    }

    /// Install a freshly built keyboard, re-applying the shorthand store and
    /// strict-spelling choice — a new `Keyboard` always starts store-less and
    /// lenient, so without this a method or preedit-model switch would silently
    /// drop both until the next `macro_sync` reload happened to fire. Shared by
    /// [`Self::rebuild`] and [`Self::set_use_composition`].
    fn install_keyboard(&mut self, mut keyboard: Keyboard) {
        if let Some(store) = &self.macros {
            keyboard.set_macros(store.clone());
        }
        if let Some((store, save_tx)) = &self.learning {
            keyboard.set_learning(store.clone(), save_tx.clone());
        }
        self.keyboard = keyboard;
        self.strict_applied = false;
        self.sync_strict_spelling();
    }

    /// Build the outcome that clears any live preedit + candidate popup after a
    /// keyboard swap (method or preedit-model change resets composition).
    fn reset_outcome(&mut self) -> KeyOutcome {
        let mut outcome = KeyOutcome {
            ops: Vec::new(),
            handled: true,
        };
        if !self.preedit.is_empty() {
            self.preedit.clear();
            outcome.ops.push(ImeOp::Preedit(String::new()));
        }
        if let Some(op) = self.clear_candidates_op() {
            outcome.ops.push(op);
        }
        outcome
    }

    /// Switch input method, discarding any live composition (a mode switch
    /// is a reset by definition). Returns `None` — keyboard unchanged — when
    /// the requested method fails to build, so `set_method` can report the
    /// failure rather than silently switching to something else or crashing.
    /// The new keyboard keeps the current preedit model (`use_composition`),
    /// except Nôm which always composes.
    ///
    /// LEGACY-WIRE shim (kept deliberately): `"english"` still arrives here
    /// from stale `method_sync` files written by older builds and from the
    /// fcitx5 addon's English item — neither caller can be broken from here.
    /// It maps to `set_enabled(false)` — note it does NOT overwrite
    /// [`Self::method`], so the real method survives and switching back on
    /// lands where the user left off.
    pub fn rebuild(&mut self, method: &str) -> Option<KeyOutcome> {
        if method == "english" {
            return Some(self.set_enabled(false));
        }
        let keyboard = build_keyboard(method, self.use_composition)?;
        self.method = method.to_string();
        self.passthrough = false;
        self.install_keyboard(keyboard);
        Some(self.reset_outcome())
    }

    /// Turn the preedit (underline) model on/off for Telex/VNI, rebuilding the
    /// current keyboard. No-op when unchanged or for Nôm (always composes). Any
    /// pending word is COMMITTED first (not discarded) so flipping mid-word
    /// never loses text; the returned ops carry that commit plus a preedit
    /// clear. Callers gate this on the client actually supporting the
    /// no-preedit model (surrounding-text) before turning composition off.
    pub fn set_use_composition(&mut self, use_composition: bool) -> KeyOutcome {
        // Passthrough guard mirrors the Nôm guard: there is nothing to rebuild
        // while english is active; the model is re-negotiated per keystroke by
        // the host (`sync_use_preedit`) once a real method is back.
        if self.passthrough || self.method == "nom" || use_composition == self.use_composition {
            return KeyOutcome::default();
        }
        let mut outcome = self.flush_pending();
        self.use_composition = use_composition;
        // The current method already built once; rebuilding it with a different
        // composition flag cannot newly fail. Keep the existing keyboard if it
        // somehow does rather than leave the engine keyboard-less.
        if let Some(keyboard) = build_keyboard(&self.method, use_composition) {
            self.install_keyboard(keyboard);
        }
        outcome.handled = true;
        outcome
    }

    /// Feed one character. Dispatches to the preedit (composition) mapping or
    /// the no-preedit (commit-as-you-go) mapping based on the active model. Nôm
    /// always uses composition (its `use_composition` never flips).
    pub fn process_char(&mut self, ch: char) -> KeyOutcome {
        // English/passthrough: never touch the keyboard — the raw key must
        // reach the app unmodified (handled=false, no ops).
        if self.passthrough {
            return KeyOutcome::default();
        }
        self.sync_strict_spelling();
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
        if self.use_composition {
            self.map_composition_actions(ch, actions)
        } else {
            self.map_direct_actions(ch, actions)
        }
    }

    /// Preedit-model mapping: fold the engine's composition actions into
    /// `ImeOp`s (preedit updates, confirmed commits, Nôm candidates). Extracted
    /// verbatim so the no-preedit path is a sibling, not a branch inside a
    /// giant function.
    fn map_composition_actions(&mut self, ch: char, actions: Vec<Action>) -> KeyOutcome {
        let mut ops = Vec::new();
        let mut emitted = false;
        let mut pass_char = false;
        // A committed word ends the composition. Track it (and whether the
        // engine explicitly refreshed/hid candidates this round) so a stale
        // popup is dropped after the loop: the punctuation PassThrough path
        // emits `ConfirmComposition` + `Commit(sep)` with NO candidate action,
        // which would otherwise leave the old word's candidate list live and
        // let the next Space/digit commit a character from it.
        let mut committed = false;
        let mut candidates_refreshed = false;
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
                    committed = true;
                }
                Action::Commit(text) => {
                    // The engine echoing the input character back is a
                    // pass-through separator — forward the original key.
                    if text.chars().eq(std::iter::once(ch)) {
                        pass_char = true;
                    } else {
                        ops.push(ImeOp::Commit(text));
                        emitted = true;
                        committed = true;
                    }
                }
                Action::DoNothing => {}
                Action::ShowCandidates { candidates, .. } => {
                    // Nôm lookup hit: remember the list for index selection and
                    // hand the backend a view to render. A candidate list means
                    // the key was consumed into the composition.
                    // `set` also puts the highlight back on the top candidate.
                    self.candidates.set(CandidateView::from_engine(&candidates));
                    ops.push(self.candidates_op());
                    emitted = true;
                    candidates_refreshed = true;
                }
                Action::HideCandidates => {
                    if let Some(op) = self.clear_candidates_op() {
                        ops.push(op);
                    }
                    candidates_refreshed = true;
                }
                other => {
                    tracing::warn!(
                        "Unexpected hook-model action in composition mode: {:?}",
                        other
                    );
                }
            }
        }

        // A commit with no explicit candidate action this round left the popup
        // stale (punctuation PassThrough) — drop it so a later selection key
        // can't commit the previous word's candidate.
        if committed && !candidates_refreshed {
            if let Some(op) = self.clear_candidates_op() {
                ops.push(op);
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

    /// No-preedit (commit-as-you-go) mapping for Telex/VNI: the engine emits a
    /// left-aligned diff per keystroke — `Replace{backspace_count, text}`
    /// (delete then insert), `Commit(text)`, or `DoNothing`. Nothing here
    /// touches `preedit` (there is none) or candidates (Telex/VNI produce
    /// none). Mirrors the Windows Hook consumer.
    fn map_direct_actions(&mut self, ch: char, actions: Vec<Action>) -> KeyOutcome {
        let mut ops = Vec::new();
        let mut emitted = false;
        let mut pass_char = false;
        for action in actions {
            match action {
                Action::Replace {
                    backspace_count,
                    text,
                } => {
                    if backspace_count > 0 {
                        ops.push(ImeOp::DeleteSurrounding(backspace_count));
                    }
                    if !text.is_empty() {
                        ops.push(ImeOp::Commit(text));
                    }
                    emitted = true;
                }
                Action::Commit(text) => {
                    // The engine echoing the just-typed char back is a natural
                    // pass-through (no transform) — let the app insert it, same
                    // rule as the composition path.
                    if text.chars().eq(std::iter::once(ch)) {
                        pass_char = true;
                    } else {
                        ops.push(ImeOp::Commit(text));
                        emitted = true;
                    }
                }
                Action::DoNothing => {}
                other => {
                    // Composition/candidate actions cannot occur with
                    // composition off for Telex/VNI; log if the engine ever
                    // emits one so the divergence is visible.
                    tracing::warn!("unexpected composition action in direct mode: {:?}", other);
                }
            }
        }
        // No preedit buffer here, so anything not emitted/passed just falls
        // through to the app (handled=false).
        KeyOutcome {
            ops,
            handled: emitted && !pass_char,
        }
    }

    /// Backspace. In preedit mode it shrinks the composition (the engine
    /// recomputes the word and the new preedit is its canonical buffer). In
    /// no-preedit mode it consumes the engine's returned diff action and
    /// corrects the already-committed text in place.
    pub fn backspace(&mut self) -> KeyOutcome {
        // Same passthrough contract as process_char: the app handles its own
        // backspace while english is active.
        if self.passthrough {
            return KeyOutcome::default();
        }
        if self.use_composition {
            self.composition_backspace()
        } else {
            self.direct_backspace()
        }
    }

    fn composition_backspace(&mut self) -> KeyOutcome {
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
        let mut ops = vec![ImeOp::Preedit(self.preedit.clone())];
        // MVP: backspace hides the candidate list rather than re-querying the
        // shrunken buffer; the next character re-populates it (Phase 2 can wire
        // `Keyboard::backspace_with_candidates` for live refresh).
        if let Some(op) = self.clear_candidates_op() {
            ops.push(op);
        }
        KeyOutcome { ops, handled: true }
    }

    /// No-preedit backspace: the engine pops its buffer and returns how to
    /// repair the on-screen (already-committed) text. A `DoNothing` means the
    /// engine has nothing to correct, so we let the app delete one char itself
    /// (handled=false) — keeping engine and screen in sync, exactly like the
    /// Windows Hook "let system handle" branch.
    fn direct_backspace(&mut self) -> KeyOutcome {
        let action = match self.keyboard.backspace() {
            Ok(action) => action,
            Err(e) => {
                tracing::warn!("Keyboard backspace error: {}", e);
                return KeyOutcome {
                    ops: Vec::new(),
                    handled: false,
                };
            }
        };
        match action {
            Action::Replace {
                backspace_count,
                text,
            } => {
                let mut ops = Vec::new();
                if backspace_count > 0 {
                    ops.push(ImeOp::DeleteSurrounding(backspace_count));
                }
                if !text.is_empty() {
                    ops.push(ImeOp::Commit(text));
                }
                let handled = !ops.is_empty();
                KeyOutcome { ops, handled }
            }
            Action::Commit(text) => KeyOutcome {
                ops: vec![ImeOp::Commit(text)],
                handled: true,
            },
            _ => KeyOutcome {
                ops: Vec::new(),
                handled: false,
            },
        }
    }

    /// Commit the pending word out-of-band (shortcuts, navigation keys),
    /// applying the word-boundary final repair — these commit points bypass
    /// the pipeline's own PassThrough repair. No-op when nothing composes.
    pub fn flush_pending(&mut self) -> KeyOutcome {
        if self.preedit.is_empty() {
            // No composition, but a stray candidate list must not linger.
            return match self.clear_candidates_op() {
                Some(op) => KeyOutcome {
                    ops: vec![op],
                    handled: true,
                },
                None => KeyOutcome::default(),
            };
        }
        let text = self
            .keyboard
            .boundary_repair()
            .unwrap_or_else(|| self.preedit.clone());
        // Out-of-band commit = an accepted word too: collect learning
        // BEFORE the reset discards the raw (no-op unless a store is wired).
        self.keyboard.collect_pending_learning();
        self.keyboard.reset();
        self.preedit.clear();
        let mut ops = vec![ImeOp::Preedit(String::new()), ImeOp::Commit(text)];
        if let Some(op) = self.clear_candidates_op() {
            ops.push(op);
        }
        KeyOutcome { ops, handled: true }
    }

    /// Discard the composition without committing (daemon Reset semantics).
    pub fn discard(&mut self) -> KeyOutcome {
        let had = !self.preedit.is_empty();
        self.keyboard.reset();
        self.preedit.clear();
        let mut ops = if had {
            vec![ImeOp::Preedit(String::new())]
        } else {
            Vec::new()
        };
        if let Some(op) = self.clear_candidates_op() {
            ops.push(op);
        }
        KeyOutcome { ops, handled: true }
    }

    /// Number of candidates currently offered (0 when none / not Nôm). The
    /// backend consults this to decide whether to route navigation/selection
    /// keys to the popup instead of composition.
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Move the highlight to the next candidate, wrapping at the end. Re-emits
    /// the list so the panel repaints (and re-pages if the cursor crossed a
    /// page boundary). No-op when nothing is showing.
    pub fn cursor_next(&mut self) -> KeyOutcome {
        self.candidates.move_next();
        self.cursor_outcome()
    }

    /// Move the highlight to the previous candidate, wrapping at the start.
    pub fn cursor_prev(&mut self) -> KeyOutcome {
        self.candidates.move_prev();
        self.cursor_outcome()
    }

    /// Advance the highlight by one `page` (clamped to the last candidate).
    pub fn cursor_page_down(&mut self, page: usize) -> KeyOutcome {
        self.candidates.page_down(page);
        self.cursor_outcome()
    }

    /// Retreat the highlight by one `page` (clamped to the first candidate).
    pub fn cursor_page_up(&mut self, page: usize) -> KeyOutcome {
        self.candidates.page_up(page);
        self.cursor_outcome()
    }

    /// Wrap a cursor move as an outcome. Re-emits the list whenever one is
    /// showing, even if the cursor did not actually move (a clamped page jump
    /// at the end): the user pressed a navigation key, so the key IS handled
    /// and must not fall through to the application.
    fn cursor_outcome(&self) -> KeyOutcome {
        if self.candidates.is_empty() {
            return KeyOutcome::default();
        }
        KeyOutcome {
            ops: vec![self.candidates_op()],
            handled: true,
        }
    }

    /// Commit the highlighted candidate (Space/Enter, panel double-click).
    pub fn select_current(&mut self) -> KeyOutcome {
        let taken = self.candidates.take_current();
        self.selection_outcome(taken)
    }

    /// Commit the candidate at position `page_index` (0-based) WITHIN the page
    /// currently holding the cursor — the mapping for number keys 1..=9 and the
    /// panel's `CandidateClicked` (both are page-relative). Out-of-range no-ops.
    pub fn select_at_page(&mut self, page_index: usize, page: usize) -> KeyOutcome {
        let taken = self.candidates.take_at_page(page_index, page);
        self.selection_outcome(taken)
    }

    /// Commit the candidate at global `index` and reset the composition.
    /// Out-of-range is a no-op (empty, unhandled outcome).
    pub fn select_candidate(&mut self, index: usize) -> KeyOutcome {
        let taken = self.candidates.take_at(index);
        self.selection_outcome(taken)
    }

    /// Finish a selection: reset the engine, clear the preedit BEFORE
    /// committing so the word is never momentarily doubled, then hide the
    /// now-stale list. `None` means nothing was selected — leave everything
    /// alone and report unhandled.
    fn selection_outcome(&mut self, value: Option<String>) -> KeyOutcome {
        let Some(value) = value else {
            return KeyOutcome::default();
        };
        self.keyboard.reset();
        self.preedit.clear();
        KeyOutcome {
            ops: vec![
                ImeOp::Preedit(String::new()),
                ImeOp::Commit(value),
                ImeOp::HideCandidates,
            ],
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
