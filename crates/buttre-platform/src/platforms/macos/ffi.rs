//! buttre macOS — C ABI for the IMKit host (FFI v3).
//!
//! **Tests**: `crates/buttre-platform/tests/platform_macos_tests.rs` (ABI
//! surface) and `tests/shared_engine_bridge_tests.rs` (composition
//! semantics — the same [`EngineBridge`] drives the Linux backends).
//! **Header**: `include/buttre_platform.h` — hand-maintained, keep in sync.
//!
//! Handle-based (opaque `u64` ids); the only unsafe is reading the C string
//! [`buttre_engine_select_method`] receives. The host maps [`ButtreKeyResult`]
//! onto IMKit directly:
//!
//! - `commit` non-null  → `insertText(commit)` (before updating the preedit)
//! - `preedit` non-null → `setMarkedText(preedit)`; empty string clears it.
//!   NULL means "marked text unchanged — do not touch the client": with no
//!   marked text, `setMarkedText("")` REPLACES the client's selection, so an
//!   unconditional empty update deleted selected text on Cmd+C / Shift+arrow.
//! - `candidates_changed` → re-read the list ([`buttre_engine_candidate_count`]
//!   / [`buttre_engine_candidate_at`]) and show or hide the candidate panel
//! - `handled == false` → return `false` from `handle(event)` so the system
//!   delivers the ORIGINAL key to the client (after the commit above —
//!   that ordering is how separators work: word first, then the separator)
//!
//! v2 → v3 (breaking, the in-repo IMKit host is the only consumer): keys are
//! fed as the layout-translated character (`event.characters`), not a US-ANSI
//! keycode, so Dvorak/AZERTY/Colemak users type what their keyboard says;
//! `preedit` became nullable (above); Nôm candidates are exposed;
//! `set_method`/`set_enabled` are replaced by
//! [`buttre_engine_select_method`].
//!
//! Method = input mode: every method (Telex, VNI, Chữ Nôm, each bundled
//! keyboard) is its own macOS input source (`ComponentInputModeDict`), so
//! the OS menu IS the method switcher and "off" is simply picking another
//! input source (ABC). The host reports the selected mode through
//! [`buttre_engine_select_method`]; `Settings::enabled` has no macOS surface
//! and is ignored here — honoring it would let a stale `false` written by
//! another platform's tray silence an input source the user just picked.
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
use crate::shared::{macro_sync, method_menu, method_owner};
use buttre_core::state::macros::MacroStore;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
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
    learning: LearningWiring,
}

static HOST_SYNC: OnceLock<HostSync> = OnceLock::new();

/// `Buttre.app/Contents/Resources` for an executable at
/// `Buttre.app/Contents/MacOS/<exe>` — where `build_app.sh` bundles the Nôm
/// dictionary and keyboard TOMLs. `None` outside a bundle (tests, dev runs),
/// which leaves the default exe-dir/cwd/data-dir search in charge.
fn bundle_resource_dir(exe: &Path) -> Option<PathBuf> {
    let resources = exe.parent()?.parent()?.join("Resources");
    resources.is_dir().then_some(resources)
}

fn host_sync() -> &'static HostSync {
    HOST_SYNC.get_or_init(|| {
        super::logging::init();
        // Must precede every keyboard build: the default resolver searches next
        // to the executable (`Contents/MacOS`), never `Contents/Resources`, so
        // without this Nôm ran dictionary-less and bundled keyboards vanished.
        match std::env::current_exe()
            .ok()
            .and_then(|exe| bundle_resource_dir(&exe))
        {
            Some(dir) => buttre_core::vietnamese::set_resource_dir(dir),
            None => tracing::warn!("macOS host: not running from an app bundle"),
        }

        let method_state = MethodState::load();
        method_sync::spawn_watcher(method_state.clone());

        let macros = macro_sync::load_initial();
        let strict = macro_sync::load_initial_strict();
        // Loaded for the shared watcher signature only: IMKit is a preedit
        // (marked-text) host, and on/off is the OS input-source choice.
        let use_preedit = macro_sync::load_initial_use_preedit();
        let enabled = macro_sync::load_initial_enabled();
        macro_sync::spawn_watcher(macros.clone(), strict.clone(), use_preedit, enabled);

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
            learning,
        }
    })
}

// ============================================================================
// Result type (mirror of include/buttre_platform.h)
// ============================================================================

/// Result of one engine call. Pointers are UTF-8, owned by the engine, and
/// valid until the NEXT call on the SAME engine (per-engine storage — two
/// engines never clobber each other's strings).
#[repr(C)]
pub struct ButtreKeyResult {
    /// `false` → the host must let the original key event through.
    pub handled: bool,
    /// Text to insert into the client, or null when nothing commits.
    pub commit: *const c_char,
    /// The full current composition (marked text) when it changed; empty
    /// string = clear the marked range; null = leave the client untouched.
    pub preedit: *const c_char,
    /// The candidate list changed (shown, replaced or hidden) — re-read it.
    pub candidates_changed: bool,
}

impl ButtreKeyResult {
    /// Nothing happened: the host handles the key, the client is untouched.
    const fn pass() -> Self {
        Self {
            handled: false,
            commit: std::ptr::null(),
            preedit: std::ptr::null(),
            candidates_changed: false,
        }
    }
}

// ============================================================================
// Global handle table
// ============================================================================

struct EngineState {
    bridge: EngineBridge,
    /// Last [`MethodState::generation`] applied — one atomic compare per
    /// keystroke, same lazy rebuild the IBus engine uses.
    seen_generation: u64,
    /// Backing storage for the pointers handed across the FFI — replaced
    /// on every call, hence the "valid until next call" contract.
    commit_c: Option<CString>,
    preedit_c: Option<CString>,
    /// Display strings of the live candidate list (empty = none showing).
    candidates_c: Vec<CString>,
}

/// Apply pending tri-surface changes before a key is processed — method
/// switch and the "Học thông minh" toggle, each a cheap compare against the
/// shared mirrors (same lazy model as the IBus engine).
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

/// Run `f` on a live engine and marshal its outcome; dead handles pass.
fn engine_call(engine_id: u64, f: impl FnOnce(&mut EngineState) -> KeyOutcome) -> ButtreKeyResult {
    with_engine(engine_id, |state| {
        let outcome = f(state);
        marshal(state, outcome)
    })
    .unwrap_or(ButtreKeyResult::pass())
}

/// Marshal a bridge outcome into the engine's C storage.
fn marshal(state: &mut EngineState, outcome: KeyOutcome) -> ButtreKeyResult {
    let mut commit_text: Option<String> = None;
    let mut preedit_changed = false;
    let mut candidates_changed = false;
    for op in outcome.ops {
        match op {
            ImeOp::Commit(text) => match &mut commit_text {
                // The bridge emits at most one commit per key; be safe anyway.
                Some(existing) => existing.push_str(&text),
                None => commit_text = Some(text),
            },
            ImeOp::Preedit(_) => preedit_changed = true,
            ImeOp::Candidates { items, .. } => {
                state.candidates_c = items
                    .into_iter()
                    .map(|c| CString::new(c.display).unwrap_or_default())
                    .collect();
                candidates_changed = true;
            }
            ImeOp::HideCandidates => {
                state.candidates_c.clear();
                candidates_changed = true;
            }
            // Only emitted with composition off, which this host never selects.
            ImeOp::DeleteSurrounding(_) => {}
        }
    }
    // `insertText` replaces the client's marked range, so after any commit the
    // composition is re-sent: a live word survives, and an empty one is
    // harmless there (the insert already collapsed the selection).
    let preedit_changed = preedit_changed || commit_text.is_some();
    state.commit_c = commit_text.and_then(|t| CString::new(t).ok());
    state.preedit_c =
        preedit_changed.then(|| CString::new(state.bridge.preedit()).unwrap_or_default());
    ButtreKeyResult {
        handled: outcome.handled,
        commit: state
            .commit_c
            .as_ref()
            .map_or(std::ptr::null(), |c| c.as_ptr()),
        preedit: state
            .preedit_c
            .as_ref()
            .map_or(std::ptr::null(), |c| c.as_ptr()),
        candidates_changed,
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
    // Start on the persisted learning state — a keystroke must not have to
    // arrive before an already-earned learning store applies.
    if sync.learning.enabled.load(Ordering::Relaxed) {
        bridge.set_learning(sync.learning.store.clone(), sync.learning.save_tx.clone());
    }
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let mut engines = ENGINES.lock().unwrap_or_else(PoisonError::into_inner);
    engines.get_or_insert_with(HashMap::new).insert(
        id,
        EngineState {
            bridge,
            seen_generation: sync.method_state.generation(),
            commit_c: None,
            preedit_c: None,
            candidates_c: Vec::new(),
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

/// Map a host character to what the engine composes, `None` for keys that
/// compose nothing (arrows, F-keys, Escape, Tab, forward delete…).
///
/// AppKit reports Return as `\r` and keypad Enter as U+0003; both are the
/// engine's line separator. Other C0/C1 controls and AppKit's function-key
/// range (`NSUpArrowFunctionKey` U+F700 … U+F8FF) are not text.
fn host_char(codepoint: u32) -> Option<char> {
    match char::from_u32(codepoint)? {
        '\r' | '\u{3}' => Some('\n'),
        '\u{F700}'..='\u{F8FF}' => None,
        ch if ch.is_control() => None,
        ch => Some(ch),
    }
}

/// Process a key press.
///
/// - `codepoint`: the layout-translated character AppKit reports for the
///   event (`event.characters`, one Unicode scalar; 0 when it is not exactly
///   one). Space and Return ARE composed — the engine classifies separators
///   itself and the result carries the committed word with `handled ==
///   false`, so the original key still reaches the client after the commit.
///
/// Keys that compose nothing (navigation, function keys, Escape, Tab, dead
/// keys, multi-character events) END the word: the pending composition is
/// committed and the result is `handled == false` so the app still gets the
/// key. Passing such a key through with the marked text still live would let
/// the app's own insert replace it — the word would vanish from the screen
/// while the engine kept composing it.
#[no_mangle]
pub extern "C" fn buttre_engine_process_char(engine_id: u64, codepoint: u32) -> ButtreKeyResult {
    engine_call(engine_id, |state| {
        sync_shared(state);
        match host_char(codepoint) {
            Some(ch) => state.bridge.process_char(ch),
            None => {
                let mut outcome = state.bridge.flush_pending();
                outcome.handled = false;
                outcome
            }
        }
    })
}

/// Process backspace. `handled == false` when nothing is composing (the
/// host lets the key delete normally).
#[no_mangle]
pub extern "C" fn buttre_engine_process_backspace(engine_id: u64) -> ButtreKeyResult {
    engine_call(engine_id, |state| {
        sync_shared(state);
        state.bridge.backspace()
    })
}

/// Commit the pending word out-of-band, with word-boundary repair — call on
/// focus loss (`deactivateServer`), shortcuts, or `commitComposition:`, then
/// act on `commit`/`preedit` as usual. No-op result when nothing is composing.
#[no_mangle]
pub extern "C" fn buttre_engine_flush(engine_id: u64) -> ButtreKeyResult {
    engine_call(engine_id, |state| state.bridge.flush_pending())
}

/// Discard the composition WITHOUT committing (Escape while candidates show).
#[no_mangle]
pub extern "C" fn buttre_engine_reset(engine_id: u64) -> ButtreKeyResult {
    engine_call(engine_id, |state| state.bridge.discard())
}

/// Number of Nôm candidates currently offered (0 = hide the panel).
#[no_mangle]
pub extern "C" fn buttre_engine_candidate_count(engine_id: u64) -> u32 {
    with_engine(engine_id, |state| {
        u32::try_from(state.candidates_c.len()).unwrap_or(u32::MAX)
    })
    .unwrap_or(0)
}

/// Display string of candidate `index` (character plus gloss), or null when
/// out of range. Valid until the next call on the same engine.
#[no_mangle]
pub extern "C" fn buttre_engine_candidate_at(engine_id: u64, index: u32) -> *const c_char {
    with_engine(engine_id, |state| {
        usize::try_from(index)
            .ok()
            .and_then(|i| state.candidates_c.get(i))
            .map_or(std::ptr::null(), |c| c.as_ptr())
    })
    .unwrap_or(std::ptr::null())
}

/// Commit candidate `index` (global, 0-based) and end the composition.
/// Out of range → `handled == false`, nothing changes.
#[no_mangle]
pub extern "C" fn buttre_engine_select_candidate(engine_id: u64, index: u32) -> ButtreKeyResult {
    engine_call(engine_id, |state| match usize::try_from(index) {
        Ok(i) => state.bridge.select_candidate(i),
        Err(_) => KeyOutcome::default(),
    })
}

/// The OS selected input mode `method` (`"telex"`, `"vni"`, `"nom"` or a
/// bundled keyboard stem — the host strips the bundle-id prefix from the
/// mode id). Called on every mode switch AND on activation, so the engine
/// always types what the menu bar shows.
///
/// A change commits the pending word (carried in the result), rebuilds this
/// engine, and persists like every other surface (method file +
/// `settings.toml`, which also records "on": picking an input source is
/// intent to type), so the other engines in this process and later sessions
/// follow. The current method is a no-op. Unknown ids, the legacy
/// `"english"` id and null are rejected (`handled == false`, nothing
/// changes): the method file must only ever name a buildable method.
///
/// `method` must be NUL-terminated UTF-8, valid for the duration of the call.
#[no_mangle]
pub extern "C" fn buttre_engine_select_method(
    engine_id: u64,
    method: *const c_char,
) -> ButtreKeyResult {
    if method.is_null() {
        return ButtreKeyResult::pass();
    }
    // SAFETY: non-null (checked above), and the header contract requires a
    // NUL-terminated string that stays valid for this call; it is copied
    // into an owned String before anything else runs.
    let method = unsafe { CStr::from_ptr(method) }
        .to_string_lossy()
        .into_owned();
    engine_call(engine_id, |state| {
        if method == "english" || !method_sync::is_engine_method(&method) {
            tracing::warn!("select_method: {method:?} is not an engine method; ignored");
            return KeyOutcome::default();
        }
        let sync = host_sync();
        if sync.method_state.method() == method {
            // Already current process-wide; just make sure THIS engine has
            // caught up (e.g. created before another engine's switch).
            sync_shared(state);
            return KeyOutcome::default();
        }
        let flushed = state.bridge.flush_pending();
        let written = match method_sync::write_method(&method) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("select_method: write_method({method}) failed: {e}");
                false
            }
        };
        method_menu::persist_command(true, written.then_some(method.as_str()));
        if state.bridge.rebuild(&method).is_none() {
            tracing::warn!("select_method: building {method} failed; keeping current");
        }
        // Adopt our own write now: other engines rebuild on the generation
        // bump, this one already has, and the watcher's later read of the same
        // file is a no-op instead of a rebuild that would wipe a fresh word.
        sync.method_state.set(method);
        state.seen_generation = sync.method_state.generation();
        flushed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_char_maps_appkit_characters() {
        assert_eq!(host_char('a' as u32), Some('a'));
        assert_eq!(host_char('Đ' as u32), Some('Đ'));
        assert_eq!(host_char(' ' as u32), Some(' '));
        // Return and keypad Enter are the engine's line separator.
        assert_eq!(host_char('\r' as u32), Some('\n'));
        assert_eq!(host_char(0x03), Some('\n'));
        // Escape, Tab, backspace/delete, arrows, F-keys compose nothing.
        for cp in [0x1B, 0x09, 0x7F, 0xF700, 0xF704, 0xF728, 0x00, 0xD800] {
            assert_eq!(host_char(cp), None, "U+{cp:04X}");
        }
    }

    #[test]
    fn bundle_resource_dir_requires_bundle_layout() {
        let root = std::env::temp_dir().join(format!("buttre-bundle-{}", std::process::id()));
        let contents = root.join("Buttre.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        let exe = contents.join("MacOS/buttre");
        assert_eq!(bundle_resource_dir(&exe), None, "no Resources dir yet");
        std::fs::create_dir_all(contents.join("Resources")).unwrap();
        assert_eq!(bundle_resource_dir(&exe), Some(contents.join("Resources")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
