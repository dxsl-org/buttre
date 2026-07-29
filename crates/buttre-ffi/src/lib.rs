//! buttre Linux — C ABI around [`EngineBridge`] for out-of-tree hosts
//! (fcitx-backend-auto-priority plan, Phase 2; first consumer: the fcitx5
//! C++ addon of Phase 3 — fcitx5 has no out-of-process engine protocol, so
//! its engine must live in-process and reach Rust through this ABI).
//!
//! **Header**: `include/buttre_ffi.h` — hand-maintained (same convention as
//! `include/buttre_platform.h` for the macOS IMKit host), keep in sync.
//! **Conventions**: mirrors `platforms/macos/ffi.rs` deliberately —
//! handle-based opaque `u64` ids, per-engine string storage ("pointers are
//! valid until the NEXT call on the SAME engine"), panic-free everywhere
//! (release profile is `panic = "abort"`: fallible construction returns
//! handle `0`, lock poisoning is absorbed via `PoisonError::into_inner`).
//!
//! Key input is X11 KEYSYMS (`process_keysym`) — fcitx5's `KeyEvent::sym()`
//! is keysym-compatible, and the classification helpers are the same ones
//! the IBus engine uses, so composition semantics cannot drift between the
//! ibus and fcitx paths.
//!
//! Not exposed (Phase 3 items): the no-preedit/commit-as-you-go model
//! (`ImeOp::DeleteSurrounding` can therefore never be emitted here) and
//! tri-surface method-file sync (the addon will reuse the shared
//! `method_sync` watcher from its host process instead).

#![warn(clippy::all)]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::module_name_repetitions, clippy::must_use_candidate)]
#![allow(unsafe_code)] // reason: this crate IS the C ABI boundary

use buttre_platform::shared::engine_bridge::{
    is_break_keysym, is_modifier_keysym, keysym_to_char, EngineBridge, ImeOp, KeyOutcome,
};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, PoisonError,
};

// ============================================================================
// Result type (mirror of include/buttre_ffi.h)
// ============================================================================

/// Result of one key event. Pointers are UTF-8, owned by the engine, and
/// valid until the NEXT call on the SAME engine (per-engine storage — two
/// engines never clobber each other's strings).
#[repr(C)]
pub struct BtKeyResult {
    /// `false` → the host must let the original key event through (after
    /// inserting `commit`, if any — the committed word lands first).
    pub handled: bool,
    /// Text to insert into the client, or null when nothing commits.
    pub commit: *const c_char,
    /// The full current composition (preedit). Empty string = clear the
    /// preedit region. Never null on a live engine.
    pub preedit: *const c_char,
}

impl BtKeyResult {
    /// For dead/invalid handles and ignored keys: nothing happened, the
    /// host handles the key itself.
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
    /// Current candidate list (Nôm), refreshed by every marshal; display
    /// and value share an index. Cleared on `HideCandidates`.
    candidates: Vec<(CString, CString)>,
    /// Global cursor into `candidates` — the bridge owns candidate
    /// navigation (hosts render the highlight, never move it themselves).
    cursor: u32,
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

/// Marshal a bridge outcome into the engine's C storage. `force_pass`
/// downgrades `handled` — used when the original key must still reach the
/// client (break keys: commit the word, then let Tab/arrow/etc. through).
fn marshal(state: &mut EngineState, outcome: KeyOutcome, force_pass: bool) -> BtKeyResult {
    let mut commit_text: Option<String> = None;
    for op in outcome.ops {
        match op {
            ImeOp::Commit(text) => match &mut commit_text {
                Some(existing) => existing.push_str(&text),
                None => commit_text = Some(text),
            },
            ImeOp::Candidates { items, cursor } => {
                state.candidates = items
                    .into_iter()
                    .filter_map(|c| {
                        Some((CString::new(c.display).ok()?, CString::new(c.value).ok()?))
                    })
                    .collect();
                state.cursor = cursor as u32;
            }
            ImeOp::HideCandidates => {
                state.candidates.clear();
                state.cursor = 0;
            }
            // Preedit is read back from the bridge below; DeleteSurrounding
            // is unreachable (no-preedit mode is never enabled through this
            // ABI — module docs).
            ImeOp::Preedit(_) | ImeOp::DeleteSurrounding(_) => {}
        }
    }
    state.commit_c = commit_text.and_then(|t| CString::new(t).ok());
    state.preedit_c = CString::new(state.bridge.preedit()).unwrap_or_default();
    BtKeyResult {
        handled: outcome.handled && !force_pass,
        commit: state
            .commit_c
            .as_ref()
            .map_or(std::ptr::null(), |c| c.as_ptr()),
        preedit: state.preedit_c.as_ptr(),
    }
}

/// Read a caller-supplied method id; null → telex (the engine default).
///
/// # Safety
/// `method` must be null or a valid NUL-terminated C string (see callers).
unsafe fn method_from_ptr<'a>(method: *const c_char) -> Option<&'a str> {
    if method.is_null() {
        return Some("telex");
    }
    // SAFETY: non-null per the check above; NUL-terminated and live for the
    // duration of the call per this function's (and every caller's) # Safety
    // contract — the standard C string-argument convention.
    unsafe { CStr::from_ptr(method) }.to_str().ok()
}

/// Built-in method ids — mirrors
/// `platforms::linux::method_sync::KNOWN_METHODS`, which isn't reachable
/// from here off Linux (gated behind `cfg(platform_linux)`, not the plain
/// `cfg(target_os = "linux")` this crate can see everywhere). Only used by
/// the non-Linux arm of `method_is_known` below — on Linux itself
/// `is_engine_method` covers built-ins already, so this would be dead code.
#[cfg(not(target_os = "linux"))]
const BUILTIN_METHODS: [&str; 4] = ["telex", "vni", "nom", "english"];

/// The boundary validation for method ids: `build_keyboard` is lenient
/// (unknown → telex fallback), so bogus ids must be rejected HERE for
/// `bt_engine_set_method` to honestly report failure — `build_keyboard`
/// joins the id straight into a path (`engine_bridge.rs`'s
/// `get_custom_dir().join(format!("{id}.toml"))`) with no guard of its own,
/// so this is the only checkpoint before a `..`/separator id ever reaches
/// that join.
///
/// On Linux this delegates to the sync channel's canonical rule. Off Linux
/// (no fcitx host — the addon never runs there, but this crate still builds
/// and its tests still run in CI) the same rule is reconstructed from
/// `buttre-core` directly: a built-in, or a syntactically-safe id
/// (non-empty, no path separator, no `..`, lowercase) whose `{id}.toml`
/// actually exists in the custom keyboards dir.
fn method_is_known(id: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        buttre_platform::platforms::linux::method_sync::is_engine_method(id)
    }
    #[cfg(not(target_os = "linux"))]
    {
        BUILTIN_METHODS.contains(&id)
            || (!id.is_empty()
                && !id.contains(['/', '\\'])
                && !id.contains("..")
                && id == id.to_lowercase()
                && buttre_core::vietnamese::get_custom_dir()
                    .join(format!("{id}.toml"))
                    .is_file())
    }
}

// ============================================================================
// Public FFI surface
// ============================================================================

/// Create an engine for `method` ("telex"/"vni"/"nom"/custom keyboard id;
/// null → telex). Returns a non-zero handle, or 0 on failure (unknown
/// method, invalid UTF-8).
///
/// # Safety
/// `method` must be null or a valid NUL-terminated C string that outlives
/// this call.
#[no_mangle]
pub unsafe extern "C" fn bt_engine_new(method: *const c_char) -> u64 {
    // SAFETY: forwarded contract — see this function's # Safety.
    let Some(method) = (unsafe { method_from_ptr(method) }) else {
        return 0;
    };
    if !method_is_known(method) {
        return 0;
    }
    let Some(bridge) = EngineBridge::try_new(method) else {
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
            candidates: Vec::new(),
            cursor: 0,
        },
    );
    id
}

/// Free an engine instance. Passing 0 or an unknown id is a safe no-op.
#[no_mangle]
pub extern "C" fn bt_engine_free(engine_id: u64) {
    if engine_id == 0 {
        return;
    }
    let mut engines = ENGINES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(map) = engines.as_mut() {
        map.remove(&engine_id);
    }
}

/// Feed one key press as an X11 keysym (fcitx5: `KeyEvent::rawKey().sym()`).
/// The routing matches the IBus engine: modifiers pass untouched, BackSpace
/// edits the composition, break keys (Tab/arrows/Escape/…) commit the
/// pending word and STILL pass (`handled == false` with `commit` set),
/// printable ASCII composes. Keys this function passes on have not changed
/// engine state unless `commit` is non-null.
#[no_mangle]
pub extern "C" fn bt_engine_process_keysym(engine_id: u64, keysym: u32) -> BtKeyResult {
    with_engine(engine_id, |state| {
        if !state.enabled || is_modifier_keysym(keysym) {
            return BtKeyResult::pass();
        }
        match keysym_to_char(keysym) {
            Some('\x08') => {
                let outcome = state.bridge.backspace();
                marshal(state, outcome, false)
            }
            Some(ch) => {
                let outcome = state.bridge.process_char(ch);
                marshal(state, outcome, false)
            }
            None if is_break_keysym(keysym) => {
                let outcome = state.bridge.flush_pending();
                marshal(state, outcome, true)
            }
            None => BtKeyResult::pass(),
        }
    })
    .unwrap_or(BtKeyResult::pass())
}

/// Commit the pending word out-of-band, with word-boundary repair — call on
/// focus loss or before shortcuts, then act on `commit`/`preedit` as usual.
/// No-op result when nothing is composing.
#[no_mangle]
pub extern "C" fn bt_engine_flush(engine_id: u64) -> BtKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.flush_pending();
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

/// Discard the composition WITHOUT committing (Escape/focus-out semantics
/// when the host clears the preedit itself).
#[no_mangle]
pub extern "C" fn bt_engine_reset(engine_id: u64) {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.discard();
        marshal(state, outcome, false);
    });
}

/// Switch the input method by id ("telex"/"vni"/"nom"/custom). Discards any
/// live composition (a mode switch is a reset). Returns true on success;
/// false leaves the previous method active.
///
/// # Safety
/// `method` must be null or a valid NUL-terminated C string that outlives
/// this call.
#[no_mangle]
pub unsafe extern "C" fn bt_engine_set_method(engine_id: u64, method: *const c_char) -> bool {
    // SAFETY: forwarded contract — see this function's # Safety.
    let Some(method) = (unsafe { method_from_ptr(method) }) else {
        return false;
    };
    if !method_is_known(method) {
        return false;
    }
    let method = method.to_string();
    with_engine(engine_id, |state| match state.bridge.rebuild(&method) {
        Some(outcome) => {
            marshal(state, outcome, false);
            true
        }
        None => false, // builder failed — keyboard unchanged, report failure
    })
    .unwrap_or(false)
}

/// Enable/disable. Disabling discards the composition — flush first if the
/// pending word should be committed. Disabled engines pass everything.
#[no_mangle]
pub extern "C" fn bt_engine_set_enabled(engine_id: u64, enabled: bool) {
    with_engine(engine_id, |state| {
        if state.enabled && !enabled {
            let outcome = state.bridge.discard();
            marshal(state, outcome, false);
        }
        state.enabled = enabled;
    });
}

/// Number of candidates in the current (Nôm) list; 0 when hidden/absent.
#[no_mangle]
pub extern "C" fn bt_engine_candidate_count(engine_id: u64) -> u32 {
    with_engine(engine_id, |state| state.candidates.len() as u32).unwrap_or(0)
}

/// Display text of candidate `index` (e.g. `"𡗶 (trời)"`), or null when out
/// of range. Valid until the next call on this engine.
#[no_mangle]
pub extern "C" fn bt_engine_candidate_display(engine_id: u64, index: u32) -> *const c_char {
    with_engine(engine_id, |state| {
        state
            .candidates
            .get(index as usize)
            .map_or(std::ptr::null(), |(display, _)| display.as_ptr())
    })
    .unwrap_or(std::ptr::null())
}

/// Committed value of candidate `index` (the bare character), or null when
/// out of range. Valid until the next call on this engine.
#[no_mangle]
pub extern "C" fn bt_engine_candidate_value(engine_id: u64, index: u32) -> *const c_char {
    with_engine(engine_id, |state| {
        state
            .candidates
            .get(index as usize)
            .map_or(std::ptr::null(), |(_, value)| value.as_ptr())
    })
    .unwrap_or(std::ptr::null())
}

/// Global cursor (highlight) into the current candidate list. The bridge
/// owns candidate navigation — hosts render this, never move it themselves.
#[no_mangle]
pub extern "C" fn bt_engine_candidate_cursor(engine_id: u64) -> u32 {
    with_engine(engine_id, |state| state.cursor).unwrap_or(0)
}

/// Candidate navigation, mirroring the IBus engine's key routing (call only
/// while `bt_engine_candidate_count() > 0`; no-ops otherwise). Each returns
/// the refreshed panel state, with the moved cursor readable via
/// [`bt_engine_candidate_cursor`].
#[no_mangle]
pub extern "C" fn bt_engine_cursor_next(engine_id: u64) -> BtKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.cursor_next();
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

/// See [`bt_engine_cursor_next`].
#[no_mangle]
pub extern "C" fn bt_engine_cursor_prev(engine_id: u64) -> BtKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.cursor_prev();
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

/// See [`bt_engine_cursor_next`]. `page` is the host's page size.
#[no_mangle]
pub extern "C" fn bt_engine_cursor_page_down(engine_id: u64, page: u32) -> BtKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.cursor_page_down(page as usize);
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

/// See [`bt_engine_cursor_next`]. `page` is the host's page size.
#[no_mangle]
pub extern "C" fn bt_engine_cursor_page_up(engine_id: u64, page: u32) -> BtKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.cursor_page_up(page as usize);
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

/// Commit the candidate under the cursor (Return/Space while the list is
/// showing).
#[no_mangle]
pub extern "C" fn bt_engine_select_current(engine_id: u64) -> BtKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.select_current();
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

/// Commit candidate number `index` (0-based) ON THE PAGE the cursor is in —
/// the digit-key contract (`page` = host page size).
#[no_mangle]
pub extern "C" fn bt_engine_select_at_page(engine_id: u64, index: u32, page: u32) -> BtKeyResult {
    with_engine(engine_id, |state| {
        let outcome = state.bridge.select_at_page(index as usize, page as usize);
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

/// Commit candidate `index` from the current list. Out-of-range indexes
/// return a pass result and change nothing.
#[no_mangle]
pub extern "C" fn bt_engine_select_candidate(engine_id: u64, index: u32) -> BtKeyResult {
    with_engine(engine_id, |state| {
        if (index as usize) >= state.candidates.len() {
            return BtKeyResult::pass();
        }
        let outcome = state.bridge.select_candidate(index as usize);
        marshal(state, outcome, false)
    })
    .unwrap_or(BtKeyResult::pass())
}

// ============================================================================
// Tests — drive the ABI exactly as the C++ addon will
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn cstr(ptr: *const c_char) -> String {
        assert!(!ptr.is_null(), "expected non-null string from ABI");
        // SAFETY: ptr comes from this crate's per-engine CString storage,
        // non-null (asserted) and NUL-terminated by construction; no call
        // is made on the engine between obtaining and reading it.
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    fn type_str(id: u64, text: &str) -> BtKeyResult {
        let mut last = BtKeyResult::pass();
        for ch in text.chars() {
            last = bt_engine_process_keysym(id, ch as u32);
        }
        last
    }

    #[test]
    fn telex_roundtrip_through_the_abi() {
        // SAFETY: pointer from a live CString local, NUL-terminated.
        let id = unsafe { bt_engine_new(CString::new("telex").unwrap().as_ptr()) };
        assert_ne!(id, 0);
        let result = type_str(id, "vieejt");
        assert!(result.handled);
        assert_eq!(cstr(result.preedit), "việt");
        let flushed = bt_engine_flush(id);
        assert_eq!(cstr(flushed.commit), "việt");
        assert_eq!(cstr(flushed.preedit), "");
        bt_engine_free(id);
    }

    #[test]
    fn break_keysym_commits_then_passes() {
        // SAFETY: null is allowed (defaults to telex) per bt_engine_new's contract.
        let id = unsafe { bt_engine_new(std::ptr::null()) };
        assert_ne!(id, 0);
        type_str(id, "xin");
        let tab = bt_engine_process_keysym(id, 0xFF09);
        assert!(!tab.handled, "break key must reach the client");
        assert_eq!(cstr(tab.commit), "xin");
        bt_engine_free(id);
    }

    #[test]
    fn set_method_switches() {
        // SAFETY: null is allowed (defaults to telex).
        let id = unsafe { bt_engine_new(std::ptr::null()) };
        let vni = CString::new("vni").unwrap();
        // SAFETY: pointer from a live CString local, NUL-terminated.
        assert!(unsafe { bt_engine_set_method(id, vni.as_ptr()) });
        let result = type_str(id, "viet65");
        assert_eq!(cstr(result.preedit), "việt");
        bt_engine_free(id);
    }

    #[test]
    fn set_method_rejects_unknown() {
        // SAFETY: null is allowed (defaults to telex).
        let id = unsafe { bt_engine_new(std::ptr::null()) };
        let bogus = CString::new("not-a-method").unwrap();
        // SAFETY: pointer from a live CString local, NUL-terminated.
        assert!(!unsafe { bt_engine_set_method(id, bogus.as_ptr()) });
        // Syntactically-invalid ids (path traversal / separators) must be
        // rejected on every platform — this is the only checkpoint before
        // build_keyboard's unguarded path join.
        let traversal = CString::new("../../../etc/passwd").unwrap();
        // SAFETY: pointer from a live CString local, NUL-terminated.
        assert!(!unsafe { bt_engine_set_method(id, traversal.as_ptr()) });
        bt_engine_free(id);
    }

    #[test]
    fn dead_handles_are_safe() {
        assert!(!bt_engine_process_keysym(0, 'a' as u32).handled);
        assert_eq!(bt_engine_candidate_count(9999), 0);
        assert!(bt_engine_candidate_display(9999, 0).is_null());
        bt_engine_free(0);
        bt_engine_free(9999);
    }

    #[test]
    fn disabled_engine_passes_everything() {
        // SAFETY: null is allowed (defaults to telex).
        let id = unsafe { bt_engine_new(std::ptr::null()) };
        bt_engine_set_enabled(id, false);
        let result = bt_engine_process_keysym(id, 'a' as u32);
        assert!(!result.handled);
        assert!(result.commit.is_null());
        bt_engine_free(id);
    }
}
