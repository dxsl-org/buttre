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
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, PoisonError,
};

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
    enabled: bool,
    /// Backing storage for the pointers handed across the FFI — replaced
    /// on every call, hence the "valid until next call" contract.
    commit_c: Option<CString>,
    preedit_c: CString,
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
    state.preedit_c =
        CString::new(state.bridge.preedit()).unwrap_or_else(|_| CString::default());
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

/// Create a new engine instance (telex). Returns a non-zero handle, or 0
/// on failure.
#[no_mangle]
pub extern "C" fn buttre_engine_new() -> u64 {
    let Some(bridge) = EngineBridge::try_new("telex") else {
        return 0;
    };
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let mut engines = ENGINES.lock().unwrap_or_else(PoisonError::into_inner);
    engines.get_or_insert_with(HashMap::new).insert(
        id,
        EngineState {
            bridge,
            enabled: true,
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
#[no_mangle]
pub extern "C" fn buttre_engine_set_method(engine_id: u64, method: u8) -> bool {
    let name = match method {
        0 => "telex",
        1 => "vni",
        2 => "nom",
        _ => return false,
    };
    with_engine(engine_id, |state| {
        let outcome = state.bridge.rebuild(name);
        marshal(state, outcome);
        true
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
