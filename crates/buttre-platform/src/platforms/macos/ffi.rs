//! buttre macOS — C ABI for the IMKit host (FFI v2).
//!
//! **Tests**: `crates/buttre-platform/tests/platform_macos_tests.rs` (ABI
//! surface) and `tests/shared_engine_bridge_tests.rs` (composition
//! semantics — the same [`EngineBridge`] drives the Linux backends).
//! **Header**: `include/buttre_platform.h` — hand-maintained, keep in sync.
//!
//! Handle-based (opaque `u64` ids) so memory management needs zero unsafe
//! on the Rust side. The host maps [`ButtreKeyResult`] onto IMKit directly:
//!
//! - `commit` non-null → `insertText(commit)` (before updating the preedit)
//! - `preedit`         → `setMarkedText(preedit)`; empty string → `unmarkText`
//! - `handled == false`→ return `false` from `handle(event)` so the system
//!   delivers the ORIGINAL key to the client (after the commit above —
//!   that ordering is how separators work: word first, then the separator)
//!
//! v1 → v2 (breaking, no consumers existed — ARTIFACT_README confirmed):
//! the old API returned a bare string for both "replace" and "commit"
//! (indistinguishable) and dropped everything past the first engine action,
//! which broke composition the same way it did on Linux (debug report B0).
//! `backspace_count` is gone — IMKit replaces the whole marked range, so a
//! delete count has no meaning in the preedit model.
//!
//! Panic policy: the release profile is `panic = "abort"` — a panic here
//! kills the host app, and `catch_unwind` cannot help (nothing unwinds).
//! Every reachable path is written panic-free instead: fallible
//! construction returns handle `0`, lock poisoning is absorbed via
//! `PoisonError::into_inner` (poisoning requires an unwind, which the
//! release profile rules out anyway).

use crate::shared::engine_bridge::{EngineBridge, ImeOp, KeyOutcome};
use crate::shared::learning_sync::{self, LearningWiring};
use crate::shared::method_sync::{self, MethodState};
use crate::shared::{macro_sync, method_owner};
use buttre_core::state::macros::MacroStore;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock, PoisonError,
};

// ============================================================================
// Per-process shared state (tri-surface sync — the same watchers the Linux
// engine processes run)
// ============================================================================

/// Everything the IMKit host process shares across its engine instances:
/// the method file state, the settings mirrors, the shorthand store, and
/// the learning wiring — kept current by the `method_sync`/`macro_sync`/
/// `learning_sync` watchers, consumed lazily per keystroke exactly like the
/// IBus engine does. Initialized on the FIRST `buttre_engine_new` (never at
/// dylib load: watchers own threads, and constructors must stay cheap and
/// panic-free inside someone else's process).
struct HostSync {
    method_state: Arc<MethodState>,
    macros: Arc<Mutex<MacroStore>>,
    strict: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    learning: LearningWiring,
}

static HOST_SYNC: OnceLock<HostSync> = OnceLock::new();

fn host_sync() -> &'static HostSync {
    HOST_SYNC.get_or_init(|| {
        let method_state = MethodState::load();
        method_sync::spawn_watcher(method_state.clone());

        let macros = macro_sync::load_initial();
        let strict = macro_sync::load_initial_strict();
        // Loaded for the shared watcher signature only — IMKit is a preedit
        // (marked-text) host, the no-preedit model does not apply.
        let use_preedit = macro_sync::load_initial_use_preedit();
        let enabled = macro_sync::load_initial_enabled();
        macro_sync::spawn_watcher(macros.clone(), strict.clone(), use_preedit, enabled.clone());

        let (learning, learning_rx) = learning_sync::load_initial();
        learning_sync::spawn_writer(learning_rx);
        learning_sync::spawn_watcher(&learning);

        tracing::info!(
            "macOS host sync initialized (method={}, owner={:?})",
            method_state.method(),
            method_owner::decide(),
        );
        HostSync {
            method_state,
            macros,
            strict,
            enabled,
            learning,
        }
    })
}

// ============================================================================
// Result type (mirror of include/buttre_platform.h)
// ============================================================================

/// Result of one key event. Pointers are UTF-8, owned by the engine, and
/// valid until the NEXT call on the SAME engine (per-engine storage — two
/// engines never clobber each other's strings).
#[repr(C)]
pub struct ButtreKeyResult {
    /// `false` → the host must let the original key event through.
    pub handled: bool,
    /// Text to insert into the client, or null when nothing commits.
    pub commit: *const c_char,
    /// The full current composition (marked text). Empty string = clear
    /// the marked range. Never null on a live engine.
    pub preedit: *const c_char,
}

impl ButtreKeyResult {
    /// For dead/invalid handles: nothing happened, host handles the key.
    const fn pass() -> Self {
        Self {
            handled: false,
            commit: std::ptr::null(),
            preedit: std::ptr::null(),
        }
    }
}

// ============================================================================
// Global handle table
// ============================================================================

struct EngineState {
    bridge: EngineBridge,
    /// Host-driven switch ([`buttre_engine_set_enabled`]) — the IMKit
    /// controller's own gate (deactivate, secure input). Independent of the
    /// `Settings::enabled` mirror, which drives the bridge's passthrough via
    /// [`sync_shared`]: either one off means keys pass untouched.
    enabled: bool,
    /// Last [`MethodState::generation`] applied — one atomic compare per
    /// keystroke, same lazy rebuild the IBus engine uses.
    seen_generation: u64,
    /// Backing storage for the pointers handed across the FFI — replaced
    /// on every call, hence the "valid until next call" contract.
    commit_c: Option<CString>,
    preedit_c: CString,
}

/// Apply pending tri-surface changes before a key is processed — method
/// switch, `Settings::enabled`, and the "Học thông minh" toggle, each a
/// cheap compare against the shared mirrors. The ORDER mirrors the IBus
/// engine and is load-bearing: a successful rebuild re-enables the bridge,
/// and the settings store must win that disagreement.
fn sync_shared(state: &mut EngineState) {
    let sync = host_sync();

    let generation = sync.method_state.generation();
    if generation != state.seen_generation {
        state.seen_generation = generation;
        let method = sync.method_state.method();
        // Outcome ops deliberately dropped: the next marshal re-reads the
        // bridge's (now empty) preedit, and IMKit replaces the whole marked
        // range on every update — there is no stale-region hazard.
        if state.bridge.rebuild(&method).is_none() {
            tracing::warn!("macOS engine: method switch to {method} failed; keeping current");
        }
    }

    let want = sync.enabled.load(Ordering::Relaxed);
    if state.bridge.is_enabled() != want {
        state.bridge.set_enabled(want);
    }

    let want_learning = sync.learning.enabled.load(Ordering::Relaxed);
    if state.bridge.has_learning() != want_learning {
        if want_learning {
            state
                .bridge
                .set_learning(sync.learning.store.clone(), sync.learning.save_tx.clone());
        } else {
            state.bridge.clear_learning();
        }
    }
}

static ENGINES: Mutex<Option<HashMap<u64, EngineState>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn with_engine<R>(engine_id: u64, f: impl FnOnce(&mut EngineState) -> R) -> Option<R> {
    if engine_id == 0 {
        return None;
    }
    let mut engines = ENGINES.lock().unwrap_or_else(PoisonError::into_inner);
    engines.as_mut()?.get_mut(&engine_id).map(f)
}

/// Marshal a bridge outcome into the engine's C storage.
fn marshal(state: &mut EngineState, outcome: KeyOutcome) -> ButtreKeyResult {
    let mut commit_text: Option<String> = None;
    for op in outcome.ops {
        if let ImeOp::Commit(text) = op {
            // The bridge emits at most one commit per key; be safe anyway.
            match &mut commit_text {
                Some(existing) => existing.push_str(&text),
                None => commit_text = Some(text),
            }
        }
    }
    state.commit_c = commit_text.and_then(|t| CString::new(t).ok());
    state.preedit_c = CString::new(state.bridge.preedit()).unwrap_or_else(|_| CString::default());
    ButtreKeyResult {
        handled: outcome.handled,
        commit: state
            .commit_c
            .as_ref()
            .map_or(std::ptr::null(), |c| c.as_ptr()),
        preedit: state.preedit_c.as_ptr(),
    }
}

// ============================================================================
// Public FFI surface
// ============================================================================

/// Create a new engine instance. Returns a non-zero handle, or 0 on failure.
///
/// The FIRST call also brings up the per-process tri-surface sync (method
/// file + settings + macros + learning watchers) — from then on the engine
/// starts in the SAVED method with shorthand/strict/learning wired, and
/// external changes (config window, hand-edits, another session's writes)
/// apply on the next keystroke. The host never has to know any of this.
#[no_mangle]
pub extern "C" fn buttre_engine_new() -> u64 {
    let sync = host_sync();
    let Some(mut bridge) =
        EngineBridge::try_new_with_macros(&sync.method_state.method(), sync.macros.clone())
    else {
        return 0;
    };
    bridge.set_strict_flag(sync.strict.clone());
    // Start on the persisted states — a keystroke must not have to arrive
    // before a saved "off" (or an already-earned learning store) applies.
    if !sync.enabled.load(Ordering::Relaxed) {
        bridge.set_enabled(false);
    }
    if sync.learning.enabled.load(Ordering::Relaxed) {
        bridge.set_learning(sync.learning.store.clone(), sync.learning.save_tx.clone());
    }
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let mut engines = ENGINES.lock().unwrap_or_else(PoisonError::into_inner);
    engines.get_or_insert_with(HashMap::new).insert(
        id,
        EngineState {
            bridge,
            enabled: true,
            seen_generation: sync.method_state.generation(),
            commit_c: None,
            preedit_c: CString::default(),
        },
    );
    id
}

/// Free an engine instance. Passing 0 or an unknown id is a safe no-op.
#[no_mangle]
pub extern "C" fn buttre_engine_free(engine_id: u64) {
    if engine_id == 0 {
        return;
    }
    let mut engines = ENGINES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(map) = engines.as_mut() {
        map.remove(&engine_id);
    }
}

/// Process a key press.
///
/// - `keycode`: macOS virtual keycode (US ANSI table below). Space and
///   Return ARE mapped — the engine classifies separators itself and the
///   result carries the committed word with `handled == false`, so the
///   original key still reaches the client after the commit.
/// - `shift` / `capslock`: letter case = `capslock XOR shift`.
///
/// Unmapped keycodes (arrows, Tab, Escape, …) return `handled == false`
/// with no state change — call [`buttre_engine_flush`] first for keys that
/// should end the composition.
#[no_mangle]
pub extern "C" fn buttre_engine_process_key(
    engine_id: u64,
    keycode: u16,
    shift: bool,
    capslock: bool,
) -> ButtreKeyResult {
    with_engine(engine_id, |state| {
        sync_shared(state);
        if !state.enabled {
            return ButtreKeyResult::pass();
        }
        let Some(ch) = keycode_to_char(keycode, shift, capslock) else {
            return ButtreKeyResult::pass();
        };
        let outcome = state.bridge.process_char(ch);
        marshal(state, outcome)
    })
    .unwrap_or(ButtreKeyResult::pass())
}

/// Process backspace. `handled == false` when nothing is composing (the
/// host lets the key delete normally).
#[no_mangle]
pub extern "C" fn buttre_engine_process_backspace(engine_id: u64) -> ButtreKeyResult {
    with_engine(engine_id, |state| {
        sync_shared(state);
        if !state.enabled {
            return ButtreKeyResult::pass();
        }
        let outcome = state.bridge.backspace();
        marshal(state, outcome)
    })
    .unwrap_or(ButtreKeyResult::pass())
}

/// Commit the pending word out-of-band, with word-boundary repair — call on
/// focus loss (`deactivateServer`), navigation keys, or shortcuts, then act
/// on `commit`/`preedit` as usual. No-op result when nothing is composing.
#[no_mangle]
pub extern "C" fn buttre_engine_flush(engine_id: u64) -> ButtreKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.flush_pending();
        marshal(state, outcome)
    })
    .unwrap_or(ButtreKeyResult::pass())
}

/// Discard the composition WITHOUT committing (Escape semantics).
#[no_mangle]
pub extern "C" fn buttre_engine_reset(engine_id: u64) {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.discard();
        marshal(state, outcome);
    });
}

/// Switch the input method: 0 = telex, 1 = vni, 2 = nom. Discards any live
/// composition (a mode switch is a reset). Returns true on success.
///
/// Also PERSISTS the choice to the shared method file (best-effort), so it
/// survives a restart and every other engine instance/process converges —
/// picking a method is a command, the same rule every other surface follows.
/// The primary method channel is that file (config window / another
/// session's write applies with no host involvement); this export remains
/// for a host-side menu.
#[no_mangle]
pub extern "C" fn buttre_engine_set_method(engine_id: u64, method: u8) -> bool {
    let name = match method {
        0 => "telex",
        1 => "vni",
        2 => "nom",
        _ => return false,
    };
    with_engine(engine_id, |state| match state.bridge.rebuild(name) {
        Some(outcome) => {
            marshal(state, outcome);
            if let Err(e) = method_sync::write_method(name) {
                tracing::warn!("buttre_engine_set_method: persist failed: {e}");
            }
            // The file write above re-fires the watcher and bumps the
            // generation; adopt it now so sync_shared doesn't rebuild the
            // same method again on the next key.
            state.seen_generation = host_sync().method_state.generation();
            true
        }
        None => false, // builder failed — keyboard unchanged, report failure
    })
    .unwrap_or(false)
}

/// Enable/disable. Disabling discards the composition — flush first if the
/// pending word should be committed. Disabled engines pass everything.
#[no_mangle]
pub extern "C" fn buttre_engine_set_enabled(engine_id: u64, enabled: bool) {
    with_engine(engine_id, |state| {
        if state.enabled && !enabled {
            let outcome = state.bridge.discard();
            marshal(state, outcome);
        }
        state.enabled = enabled;
    });
}

// ============================================================================
// macOS virtual keycode → char (US ANSI layout)
// ============================================================================

/// Letter case uses `capslock XOR shift`, matching system behavior.
fn keycode_to_char(keycode: u16, shift: bool, capslock: bool) -> Option<char> {
    let letter = match keycode {
        0 => 'a',
        1 => 's',
        2 => 'd',
        3 => 'f',
        4 => 'h',
        5 => 'g',
        6 => 'z',
        7 => 'x',
        8 => 'c',
        9 => 'v',
        11 => 'b',
        12 => 'q',
        13 => 'w',
        14 => 'e',
        15 => 'r',
        16 => 'y',
        17 => 't',
        31 => 'o',
        32 => 'u',
        34 => 'i',
        35 => 'p',
        37 => 'l',
        38 => 'j',
        40 => 'k',
        45 => 'n',
        46 => 'm',
        // Separators the engine must see (it classifies them itself).
        49 => return Some(' '),
        36 => return Some('\n'),
        _ => return keycode_to_char_non_letter(keycode, shift),
    };
    let uppercase = capslock != shift;
    Some(if uppercase {
        letter.to_ascii_uppercase()
    } else {
        letter
    })
}

fn keycode_to_char_non_letter(keycode: u16, shift: bool) -> Option<char> {
    Some(if shift {
        match keycode {
            // Shifted digits
            18 => '!',
            19 => '@',
            20 => '#',
            21 => '$',
            23 => '%',
            22 => '^',
            26 => '&',
            28 => '*',
            25 => '(',
            29 => ')',
            // Shifted punctuation
            27 => '_',
            24 => '+',
            33 => '{',
            30 => '}',
            42 => '|',
            41 => ':',
            39 => '"',
            43 => '<',
            47 => '>',
            44 => '?',
            50 => '~',
            _ => return None,
        }
    } else {
        match keycode {
            // Unshifted digits
            18 => '1',
            19 => '2',
            20 => '3',
            21 => '4',
            23 => '5',
            22 => '6',
            26 => '7',
            28 => '8',
            25 => '9',
            29 => '0',
            // Unshifted punctuation
            27 => '-',
            24 => '=',
            33 => '[',
            30 => ']',
            42 => '\\',
            41 => ';',
            39 => '\'',
            43 => ',',
            47 => '.',
            44 => '/',
            50 => '`',
            _ => return None,
        }
    })
}
