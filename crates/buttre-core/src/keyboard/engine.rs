//! Keyboard - Main keyboard struct
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/keyboard_tests.rs`.
//!
//! Uses buttre-engine pipeline for processing

use crate::state::learning::{LearningFile, LearningStore, PreferKind};
use crate::Action;
use buttre_engine::compose::{compose_closed, is_last_event_undo, ComposeOpts, Validator};
use buttre_engine::pipeline::validation::{is_attested_overlay, SyllableStructure};
use buttre_engine::pipeline::{PipelineConfig, PipelineExecutor};
use buttre_engine::types::Action as EngineAction;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

/// How many words the editable rolling window keeps (current + N-1 previous).
/// 3 = current word + 2 words back (matches "fix 1–2 previous words").
const MAX_WINDOW_WORDS: usize = 3;
/// Safety cap on the window's raw length (bounds recompute cost).
const MAX_WINDOW_RAW: usize = 64;

/// Backspace deletion granularity (event-sourcing-completion Phase 4).
///
/// `Grapheme` (default): delete the last DISPLAYED character — search for the
/// raw-key subset that recomputes to that shorter display. Unchanged existing
/// behavior (`backspace_multiword`/`backspace_legacy`).
///
/// `Raw`: delete the last RAW KEYSTROKE and recompute from what remains — the
/// trivially-correct inverse operation in an event-sourced buffer (no search
/// needed). Trade-off: can delete more or less than one visible grapheme
/// (undoing a tone key removes only the accent, for example) — that's the
/// whole point of the mode, for users who think in keystrokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackspaceMode {
    #[default]
    Grapheme,
    Raw,
}

impl BackspaceMode {
    /// Parse from the persisted `Settings::backspace_mode` string. Unknown or
    /// missing values fall back to `Grapheme` — a corrupt/foreign
    /// settings.toml must never disable the safe default.
    pub fn from_settings_str(s: &str) -> Self {
        match s {
            "raw" => Self::Raw,
            _ => Self::Grapheme,
        }
    }
}

/// One toggle event's learning signal (event-sourcing-completion Phase 4,
/// consumed by Phase 5): the raw keystrokes of the toggled word and which
/// direction the user chose. A user toggling `rết` → `reset` is the
/// strongest possible preference signal — Phase 5's collector drains these
/// via `Keyboard::drain_toggle_signals`. NOT implemented here; this type only
/// carries the signal so the seam exists without pulling learning logic into
/// this phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleSignal {
    /// The word's raw keystrokes, original case, as typed.
    pub raw_sequence: String,
    /// `true` if the user chose the literal(raw) projection, `false` for
    /// compose(raw).
    pub literal: bool,
}

/// Main keyboard struct
pub struct Keyboard {
    /// Pipeline executor from buttre-engine (used as a per-word compose helper
    /// in multi-word mode, or as the live engine in legacy/Nôm mode).
    executor: PipelineExecutor,

    /// Current displayed text the engine is tracking.
    buffer: String,

    /// Multi-word rolling-window mode (Telex/VNI).  When false (Nôm / native
    /// scripts), the legacy per-keystroke executor path is used unchanged.
    multiword: bool,

    /// Live window raw keystrokes (original case), including separators.
    /// Spans the last `MAX_WINDOW_WORDS` words — the editable region.
    raw: Vec<char>,

    /// Frozen display prefix: words that scrolled out of the window.  Never
    /// recomputed; only a common-prefix anchor for the screen diff.
    committed: String,

    /// Word-freezing toggle map (event-sourcing-completion Phase 4): raw
    /// char-index (exclusive end offset, i.e. the key a word run ends at) →
    /// `true` (render literal(raw), case-preserved) or `false` (render
    /// compose(raw) with `closed=true` — the word is closed BY the toggle,
    /// per P3's boundary-repair projection). A toggle also forces a word
    /// boundary at that offset even without a real separator character, so
    /// `word_runs`/`compose_window` split the run there — see their docs.
    /// Absent entries render via the normal (untouched) open/closed logic.
    ///
    /// INVARIANT: every key must be a valid word-run end for the CURRENT
    /// `raw` (see `word_runs`); any raw mutation that isn't a pure append
    /// must clear or re-anchor entries so a stale offset never attaches to
    /// the wrong word (see `backspace_multiword`, `backspace_multiword_raw`,
    /// `scroll_out_overflow`, `reset`).
    toggle_map: HashMap<usize, bool>,

    /// Pending learning signals for Phase 5's collector (see `ToggleSignal`).
    /// Only ever appended to by `toggle_last_word`; drained externally.
    toggle_signals: Vec<ToggleSignal>,

    /// Backspace deletion granularity (event-sourcing-completion Phase 4).
    /// Set from `Settings::backspace_mode` by the platform layer via
    /// `set_backspace_mode` — `Keyboard::new` always starts at the engine
    /// default (`Grapheme`) because `PipelineConfig` has no app-settings
    /// concept of its own.
    backspace_mode: BackspaceMode,

    /// Input method identifier (e.g. `"telex"`, `"vni"`) — captured at
    /// construction time since `config` is moved into the executor.
    /// Event-sourcing-completion Phase 5: scopes personal-learning
    /// records/snapshots per method (`LearningStore::record_pref`/
    /// `snapshot_for_method` are both method-keyed, so Telex/VNI never share
    /// a preference for the same raw sequence).
    method: String,

    /// A `Keyboard`-owned copy of the compose options the LIVE executor's
    /// compose stage derives from the same `PipelineConfig` (event-sourcing-
    /// completion Phase 5). `None` for non-Vietnamese-validator configs
    /// (native scripts, Hmong/Custom) — there is no attested-syllable table
    /// to learn against, mirroring `PipelineExecutor::boundary_repair_opts`'s
    /// own gate.
    ///
    /// Exists so the word-commit collector below can call
    /// `buttre_engine::compose::{compose_closed, is_last_event_undo}`
    /// directly, through the engine's PUBLIC API, without a new accessor
    /// into `PipelineExecutor`'s private state. Kept in sync with the live
    /// executor's own learning snapshot by `apply_learning_snapshot` — both
    /// must see the SAME `user_attested`/`raw_prefs` data, or the collector's
    /// notion of "already attested" could silently diverge from what the
    /// pipeline actually renders.
    compose_opts: Option<ComposeOpts>,

    /// Personal-learning store handle (event-sourcing-completion Phase 5).
    /// `None` (the `Keyboard::new` default) until `set_learning` is called —
    /// every collection/refresh path below is then a no-op, byte-identical
    /// to pre-Phase-5 behavior. The platform layer gates calling
    /// `set_learning` at all on `Settings::learning_enabled`.
    learning: Option<LearningHandle>,
}

/// Bundles the shared personal-learning store with its off-thread save
/// channel (event-sourcing-completion Phase 5) — see `Keyboard::
/// set_learning`. Both halves are cheap to clone (`Arc`/`mpsc::Sender`), so
/// the collector can release its borrow of `self.learning` before taking
/// `&mut self` again for the snapshot refresh.
#[derive(Clone)]
struct LearningHandle {
    store: Arc<Mutex<LearningStore>>,
    save_tx: Sender<LearningFile>,
}

impl Keyboard {
    /// Create a new keyboard from pipeline config
    pub fn new(config: PipelineConfig) -> anyhow::Result<Self> {
        // Multi-word editing applies to phonetic Latin methods (Telex/VNI/VIQR)
        // on the HOOK backend (Commit/Replace).  Excluded: TSF composition mode
        // (use_composition — it emits UpdateComposition and the IME owns the
        // composition span), Nôm (dictionary lookup), and native scripts.
        let multiword = !config.native_script_mode
            && !config.enable_lookup
            && config.name != "nom"
            && !config.pipeline.use_composition;

        // Captured BEFORE `config` moves into the executor below (event-
        // sourcing-completion Phase 5) — see `method`/`compose_opts`'s docs.
        let method = config.name.clone();
        let compose_opts = {
            let opts = ComposeOpts::from_config(&config);
            (opts.validator == Validator::Vietnamese).then_some(opts)
        };

        // Create executor directly from config
        let executor = PipelineExecutor::new(config);

        Ok(Self {
            executor,
            buffer: String::new(),
            multiword,
            raw: Vec::new(),
            committed: String::new(),
            toggle_map: HashMap::new(),
            toggle_signals: Vec::new(),
            backspace_mode: BackspaceMode::default(),
            method,
            compose_opts,
            learning: None,
        })
    }

    /// Inject the personal-learning store + off-thread save channel
    /// (event-sourcing-completion Phase 5). Callers gate calling this AT ALL
    /// on `Settings::learning_enabled` — never calling it (the
    /// `Keyboard::new` default) is what makes learning byte-identical to no
    /// store: `self.learning` stays `None` and every collection/refresh path
    /// below no-ops. See `buttre-platform/src/shared/input/keyboard_manager.rs`,
    /// the only caller, which re-injects on every method switch (a fresh
    /// `Keyboard` instance starts with no handle, same as `backspace_mode`).
    ///
    /// Immediately refreshes the compose snapshot from `store`'s CURRENT
    /// state (`LearningStore::snapshot_for_method`) so a freshly loaded
    /// store applies from the very next keystroke — the Combined Contract's
    /// "boundary only" rule governs when NEW signals are COLLECTED, not this
    /// one-time hand-off at construction/method-switch time.
    pub fn set_learning(
        &mut self,
        store: Arc<Mutex<LearningStore>>,
        save_tx: Sender<LearningFile>,
    ) {
        let snapshot = {
            let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
            guard.snapshot_for_method(&self.method)
        };
        self.apply_learning_snapshot(snapshot);
        self.learning = Some(LearningHandle { store, save_tx });
    }

    /// Detach the personal-learning store and clear the live snapshot — the
    /// runtime counterpart of never calling [`Self::set_learning`]: after
    /// this, collection, snapshot refresh, and pref/overlay consultation all
    /// no-op, byte-identical to a keyboard built with learning disabled.
    /// Used by the tray's live "Học thông minh" toggle.
    pub fn clear_learning(&mut self) {
        self.learning = None;
        self.apply_learning_snapshot(buttre_engine::compose::LearningSnapshot::default());
    }

    /// Push a fresh learning snapshot to both the live executor's compose
    /// stage AND this `Keyboard`'s own `compose_opts` copy (event-sourcing-
    /// completion Phase 5) — mirrors `PipelineExecutor::
    /// set_learning_snapshot`'s own "single consult point" requirement: the
    /// collector below calls `compose_closed`/`is_last_event_undo` directly
    /// against `compose_opts`, so it must see the SAME data the live
    /// pipeline does, or the two could silently diverge on what counts as
    /// attested/preferred.
    fn apply_learning_snapshot(&mut self, snapshot: buttre_engine::compose::LearningSnapshot) {
        if let Some(opts) = self.compose_opts.as_mut() {
            opts.user_attested = snapshot.user_attested.clone();
            opts.raw_prefs = snapshot.raw_prefs.clone();
        }
        self.executor.set_learning_snapshot(snapshot);
    }

    /// Remove and return every pending toggle signal (event-sourcing-
    /// completion Phase 4 seam, `ToggleSignal`) whose raw sequence exactly
    /// matches `word_raw` — the just-committed word's raw keys, same case.
    /// Toggling FREEZES a word (`toggle_last_word`'s doc: continued typing
    /// starts a NEW word), so its raw can never be mutated again before it
    /// scrolls out — an exact string match is therefore unambiguous.
    /// Non-matching signals (belonging to a DIFFERENT, still-in-window word)
    /// are left in place for a later commit.
    ///
    /// Called unconditionally at every word commit (event-sourcing-
    /// completion Phase 5), even when no learning store is wired — otherwise
    /// `toggle_signals` would grow unboundedly for a session that toggles
    /// words but never consumes the signal any other way.
    fn drain_matching_toggle_signals(&mut self, word_raw: &str) -> Vec<ToggleSignal> {
        let mut matched = Vec::new();
        self.toggle_signals.retain(|sig| {
            if sig.raw_sequence == word_raw {
                matched.push(sig.clone());
                false
            } else {
                true
            }
        });
        matched
    }

    /// Word-commit personal-learning collection (event-sourcing-completion
    /// Phase 5, Requirements (a)/(b)) — called ONCE per committed word from
    /// `scroll_out_overflow`, NEVER per keystroke (red-team M7: multiword
    /// replays the same window every keystroke, so collecting anywhere else
    /// would record the same event dozens of times).
    ///
    /// Ordering (Combined Contract: repair → collect → refresh): P3's
    /// word-boundary repair already ran inside `compose_window`'s call to
    /// `compose_one_word`, before the caller (`scroll_out_overflow`) reaches
    /// this — this function only collects signals and (last) refreshes the
    /// snapshot.
    ///
    /// Precedence: a P4 word toggle is the strongest, most deliberate signal
    /// for `word_raw` — when one (or more, if re-toggled before scrolling
    /// out) exists, it is recorded and nothing else is; recording an
    /// unrelated undo-shape/direct-typed guess for the SAME raw afterward
    /// could silently overwrite the user's explicit choice. Otherwise: a
    /// double-tap undo/toggle shape (`is_last_event_undo`) records nothing —
    /// the undo display re-derives deterministically from raw, and a Literal
    /// pref would replay raw-verbatim instead (see the branch comment);
    /// failing that, a clean direct-typed (no inferred marks,
    /// not gate-demoted, structurally valid, currently unattested) syllable
    /// feeds the overlay promotion counter (anti-feedback rule (i): an
    /// automatic demote is checked via `ComposeResult::demoted` and records
    /// NOTHING).
    fn collect_and_refresh_learning(&mut self, word_raw: &[char]) {
        if word_raw.is_empty() {
            return;
        }
        let word_str: String = word_raw.iter().collect();
        let toggles = self.drain_matching_toggle_signals(&word_str);

        let Some(handle) = self.learning.as_ref() else {
            return;
        };
        let Some(opts) = self.compose_opts.as_ref() else {
            return;
        };

        let has_trigger = opts.has_trigger_key(word_raw);
        let mut store = handle.store.lock().unwrap_or_else(PoisonError::into_inner);

        if !toggles.is_empty() {
            for sig in &toggles {
                let prefer = if sig.literal {
                    PreferKind::Literal
                } else {
                    PreferKind::Composed
                };
                // Record-replay invariant (ADR-0001): a Literal pref on an
                // undo-shaped raw can never replay ("yess" would render
                // "yes" via the lookup guard, not the literal the toggle
                // showed) — the honest record for this latest choice is no
                // pref at all, which also clears any stale opposite pref
                // instead of leaving it to win by default.
                if prefer == PreferKind::Literal && is_last_event_undo(word_raw, opts) {
                    store.remove_pref(&self.method, &word_str);
                } else {
                    store.record_pref(&self.method, &word_str, prefer, has_trigger);
                }
            }
        } else if is_last_event_undo(word_raw, opts) {
            // Deliberately record NOTHING. The display the user accepted here
            // is compose's own undo output ("yess" → "yes"), which re-derives
            // deterministically from raw; a Literal pref replays raw-verbatim
            // ("yess") instead, permanently hijacking the double-key escape
            // (the bug that made "yes" untypeable once this fired). Explicit
            // Ctrl+Shift+Z toggles (branch above) remain the only writer of
            // Literal prefs.
        } else {
            let result = compose_closed(word_raw, opts);
            let is_direct = !result.temp_english
                && !result.demoted
                && !result.applied_marks.iter().any(|m| m.non_adjacent)
                && SyllableStructure::parse(&result.text).is_valid()
                && !is_attested_overlay(&result.text, opts.user_attested.as_deref());
            if is_direct {
                store.record_direct_typed(&result.text);
            }
        }

        let snapshot = store.snapshot_for_method(&self.method);
        let save_file = store.is_dirty().then(|| store.snapshot_for_save());
        let save_tx = handle.save_tx.clone();
        drop(store);

        if let Some(file) = save_file {
            // Off-thread save (red-team C3): this only ENQUEUES the
            // snapshot — the actual disk write happens in
            // `buttre-platform/src/main.rs`'s event loop, never here (this
            // runs on the hook-callback thread). A full/disconnected
            // receiver is not an error worth surfacing on this hot path;
            // the next dirty commit will retry.
            let _ = save_tx.send(file);
        }

        self.apply_learning_snapshot(snapshot);
    }

    /// Override the backspace deletion mode (event-sourcing-completion Phase
    /// 4). `Keyboard::new` always starts at `BackspaceMode::Grapheme`; the
    /// platform layer calls this after loading `Settings` (and again after
    /// every method switch, since a new `Keyboard` instance restarts at the
    /// default) — see `buttre-platform/src/main.rs`.
    pub fn set_backspace_mode(&mut self, mode: BackspaceMode) {
        self.backspace_mode = mode;
    }

    /// Drain pending toggle learning signals (event-sourcing-completion
    /// Phase 5 seam — see `ToggleSignal`). Not implemented here; Phase 5's
    /// collector will poll this after each hook cycle.
    pub fn drain_toggle_signals(&mut self) -> Vec<ToggleSignal> {
        std::mem::take(&mut self.toggle_signals)
    }

    /// Process a keystroke
    ///
    /// Returns a vector of actions to perform. Usually contains 1-2 actions:
    /// - Main action (DoNothing/Commit/Replace/UpdateComposition)
    /// - Optional ShowCandidates/HideCandidates for Nôm input
    pub fn process(&mut self, key: char) -> anyhow::Result<Vec<Action>> {
        if self.multiword {
            return self.process_multiword(key);
        }
        self.process_legacy(key)
    }

    /// Multi-word rolling-window processing: append the key to the live window,
    /// scroll the oldest word out (to the frozen prefix) if over the cap,
    /// recompute the whole window, and emit the screen diff.  This keeps the
    /// last few words editable (re-tone after a typo) without desync, because
    /// the engine's tracked text always mirrors the on-screen recent text.
    fn process_multiword(&mut self, key: char) -> anyhow::Result<Vec<Action>> {
        let old = self.buffer.clone();
        self.raw.push(key);
        self.scroll_out_overflow();

        let raw = self.raw.clone();
        let window = self.compose_window(&raw);
        let new = format!("{}{}", self.committed, window);
        self.buffer = new.clone();

        Ok(vec![diff_to_action(&old, &new)])
    }

    fn process_legacy(&mut self, key: char) -> anyhow::Result<Vec<Action>> {
        // Process through engine pipeline
        let engine_actions = self.executor.process(key);

        // Convert engine actions to our actions
        let mut result = Vec::new();

        for action in &engine_actions {
            match action {
                EngineAction::DoNothing => {
                    // Character was buffered
                    self.buffer.push(key);
                    result.push(Action::DoNothing);
                }
                EngineAction::Commit(text) => {
                    // Append committed text to buffer
                    self.buffer.push_str(text);
                    result.push(Action::Commit(text.clone()));
                }
                EngineAction::Replace {
                    backspace_count,
                    text,
                } => {
                    // Update buffer
                    for _ in 0..*backspace_count {
                        self.buffer.pop();
                    }
                    self.buffer.push_str(text);

                    result.push(Action::Replace {
                        backspace_count: *backspace_count,
                        text: text.clone(),
                    });
                }
                EngineAction::UpdateComposition { text, cursor } => {
                    // Update buffer with current composition
                    self.buffer = text.clone();
                    result.push(Action::UpdateComposition {
                        text: text.clone(),
                        cursor: *cursor,
                    });
                }
                EngineAction::ConfirmComposition(text) => {
                    // Update buffer with confirmed text
                    self.buffer = text.clone();
                    result.push(Action::ConfirmComposition(text.clone()));
                }
                EngineAction::ShowCandidates { candidates, input } => {
                    result.push(Action::ShowCandidates {
                        candidates: candidates.clone(),
                        input: input.clone(),
                    });
                }
                EngineAction::HideCandidates => {
                    result.push(Action::HideCandidates);
                }
            }
        }

        if result.is_empty() {
            result.push(Action::DoNothing);
        }

        // ALWAYS synchronize buffer with engine's canonical state
        // This prevents "ignored" characters in PermutationStage from lingering in buffer
        self.buffer = self.executor.get_buffer().to_string();

        Ok(result)
    }

    /// Process backspace — delete the last displayed grapheme while KEEPING the
    /// current word's composition alive, so the user can keep editing it (e.g.
    /// re-apply a tone after fixing a fast-typing error).
    ///
    /// ## Why not just reset
    ///
    /// The engine is recompute-from-raw.  A naive "pop one raw key" is wrong
    /// because raw order ≠ display order (the tone key is often typed last:
    /// `vieetj` → `việt`, so popping `j` removes the tone, not the `t`).  And a
    /// hard reset loses the composition, so tones can no longer be applied to the
    /// edited word.
    ///
    /// Instead we search for the raw-key subset that recomputes to *the current
    /// display minus its last grapheme*, then replay it.  Because the target is
    /// always a prefix of the old display, the emitted action is a clean
    /// `Replace { backspace_count: 1 }` (delete one visible char), while the
    /// engine's `char_buffer`/`last_output` stay in sync with the screen — so no
    /// desync (the bug the old reset-on-backspace was guarding against) and the
    /// word remains editable.
    ///
    /// Scope: this edits the CURRENT word (everything since the last separator
    /// reset the engine).  Backspacing past the word start returns `DoNothing`
    /// and lets the host delete the separator / previous word.
    pub fn backspace(&mut self) -> anyhow::Result<Action> {
        if self.multiword {
            return match self.backspace_mode {
                BackspaceMode::Raw => self.backspace_multiword_raw(),
                BackspaceMode::Grapheme => self.backspace_multiword(),
            };
        }
        self.backspace_legacy()
    }

    /// Backspace over the multi-word window: delete the last displayed grapheme,
    /// keeping the window editable.  When the window empties, reset so further
    /// backspaces fall back to the host (the frozen prefix is not editable).
    fn backspace_multiword(&mut self) -> anyhow::Result<Action> {
        if self.raw.is_empty() {
            // Past the editable window — let the host delete the frozen text and
            // start fresh so the engine never desyncs with off-window content.
            self.reset();
            return Ok(Action::DoNothing);
        }
        let old = self.buffer.clone();
        let raw = self.raw.clone();
        let window = self.compose_window(&raw);

        let mut target_chars: Vec<char> = window.chars().collect();
        target_chars.pop();
        let target: String = target_chars.into_iter().collect();

        // Map invalidation (event-sourcing-completion Phase 4, red-team
        // M5/M3 CRITICAL): the search below (`find_window_backspace_raw`) can
        // remove a raw key from the MIDDLE of the window, or truncate to an
        // arbitrary prefix — either can renumber every offset after the edit,
        // so a stale `toggle_map` entry could silently attach to the WRONG
        // word once the array is renumbered. Clear before searching (so the
        // search's own `compose_window` probes never contaminate on a stale
        // boundary either). Conservative — this also drops toggle state for
        // OTHER, untouched words still in the window — but always correct;
        // the user just re-toggles. `backspace_multiword_raw` (below) is the
        // precise-invalidation counterpart for the pure-truncation case.
        self.toggle_map.clear();

        self.raw = self.find_window_backspace_raw(&raw, &target);
        let raw2 = self.raw.clone();
        let new_window = self.compose_window(&raw2);
        let new = format!("{}{}", self.committed, new_window);
        self.buffer = new.clone();

        Ok(diff_to_action(&old, &new))
    }

    /// Raw-space backspace (event-sourcing-completion Phase 4,
    /// `BackspaceMode::Raw`): pop the last RAW keystroke and recompute — the
    /// trivially-correct inverse for an event-sourced buffer, no search
    /// needed (unlike grapheme mode). May delete more or less than one
    /// visible grapheme (undoing a tone key removes only the accent, for
    /// example); that trade-off is the mode's whole purpose.
    fn backspace_multiword_raw(&mut self) -> anyhow::Result<Action> {
        if self.raw.is_empty() {
            self.reset();
            return Ok(Action::DoNothing);
        }
        let old = self.buffer.clone();
        self.raw.pop();

        // Map invalidation, precise case: a pure tail truncation never
        // renumbers surviving offsets (unlike grapheme mode's arbitrary
        // rewrite above), so only entries pointing past the new (shorter)
        // raw length are stale — drop exactly those, keep the rest.
        let len = self.raw.len();
        self.toggle_map.retain(|&end, _| end <= len);

        let raw = self.raw.clone();
        let new_window = self.compose_window(&raw);
        let new = format!("{}{}", self.committed, new_window);
        self.buffer = new.clone();

        Ok(diff_to_action(&old, &new))
    }

    fn backspace_legacy(&mut self) -> anyhow::Result<Action> {
        let raw: Vec<char> = self
            .executor
            .context()
            .char_buffer
            .iter()
            .map(|ci| ci.to_output_char())
            .collect();
        let old_display = self.executor.get_buffer().to_string();

        if raw.is_empty() || old_display.is_empty() {
            // Nothing composing here — let the host handle the backspace.
            self.reset();
            return Ok(Action::DoNothing);
        }

        // Target display = current display with its last grapheme removed.
        let mut target_chars: Vec<char> = old_display.chars().collect();
        target_chars.pop();
        let target: String = target_chars.into_iter().collect();

        let new_raw = self.find_backspace_raw(&raw, &target);

        // Replay the chosen raw keys so the engine state matches the new display.
        self.executor.reset();
        for &ch in &new_raw {
            self.executor.process(ch);
        }
        let new_display = self.executor.get_buffer().to_string();
        self.buffer = new_display.clone();

        Ok(diff_to_action(&old_display, &new_display))
    }

    /// Find the raw-key subset whose recomputed display equals `target`.
    ///
    /// Preference order keeps as much of the word editable as possible:
    /// 1. Remove a single raw key (keeps later tone/transform keys intact —
    ///    handles `vieetj`→`việt`: removing `t` keeps the `j` tone → `việ`).
    /// 2. Truncate to a trailing prefix (handles multi-key graphemes built at
    ///    the end, e.g. `vieej`→`việ`: drop `eej` → `vi`).
    /// 3. Fall back to dropping the last raw key (undo one keystroke).
    fn find_backspace_raw(&mut self, raw: &[char], target: &str) -> Vec<char> {
        for i in (0..raw.len()).rev() {
            let cand: Vec<char> = raw
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, c)| *c)
                .collect();
            if self.display_of(&cand) == target {
                return cand;
            }
        }
        for k in (0..raw.len()).rev() {
            if self.display_of(&raw[..k]) == target {
                return raw[..k].to_vec();
            }
        }
        raw[..raw.len() - 1].to_vec()
    }

    /// Recompute the display a given raw-key sequence would produce.
    /// Mutates the executor; callers replay the chosen sequence afterwards.
    fn display_of(&mut self, raw: &[char]) -> String {
        self.executor.reset();
        for &ch in raw {
            self.executor.process(ch);
        }
        self.executor.get_buffer().to_string()
    }

    /// Process backspace when candidates are showing (Nôm mode)
    ///
    /// This method properly syncs the executor state with the keyboard buffer
    /// after removing a character. It:
    /// 1. Pops one character from buffer
    /// 2. Resets the executor
    /// 3. Re-processes the remaining buffer through the executor
    /// 4. Returns the new candidates
    ///
    /// Returns: (remaining_buffer, candidates) or None if buffer is empty
    pub fn backspace_with_candidates(
        &mut self,
    ) -> Option<(String, Vec<buttre_engine::pipeline::Candidate>)> {
        if self.buffer.is_empty() {
            return None;
        }

        // Pop one character
        self.buffer.pop();

        // If buffer is now empty, reset and return empty
        if self.buffer.is_empty() {
            self.executor.reset();
            return Some((String::new(), vec![]));
        }

        // Reset executor to clear stale state
        self.executor.reset();

        // Re-process each character in the remaining buffer
        let buffer_copy = self.buffer.clone();

        // Process each character to rebuild executor state
        let mut last_candidates = vec![];
        for ch in buffer_copy.chars() {
            let actions = self.executor.process(ch);

            // Extract candidates from actions
            for action in actions {
                if let EngineAction::ShowCandidates { candidates, .. } = action {
                    last_candidates = candidates;
                }
            }
        }

        Some((self.buffer.clone(), last_candidates))
    }

    /// Reset state — hard reset (clears the whole window + frozen prefix).
    /// Called on word-boundary / cursor-relocation keys (Enter, arrows, mouse,
    /// modifiers) so the editable window never spans a cursor jump.
    pub fn reset(&mut self) {
        self.executor.reset();
        self.buffer.clear();
        self.raw.clear();
        self.committed.clear();
        // Map invalidation (event-sourcing-completion Phase 4): a hard reset
        // discards `raw` entirely, so every toggle offset is stale by
        // definition.
        self.toggle_map.clear();
        // Also drop any un-drained learning signals: a hard reset (Enter, mouse,
        // focus change) ends the editing session, so a pending signal whose word
        // never re-commits with a matching raw must not survive to be mis-matched
        // against an identical raw sequence typed later in a different context.
        self.toggle_signals.clear();
    }

    /// Word-boundary final repair probe (event-sourcing-completion Phase 3):
    /// see `buttre_engine::pipeline::PipelineExecutor::boundary_repair`.
    ///
    /// Meaningful only for the single-word/legacy path (TSF, Nôm) — a TSF
    /// commit point (Enter, or a buffer-reset key) ends the composition
    /// directly, bypassing `process()`/`ConfirmComposition` entirely, so the
    /// platform layer calls this explicitly BEFORE ending the composition to
    /// fold the correction in. `multiword` mode already threads `closed` per
    /// word through `compose_one_word` on every window recompute (see its
    /// doc) — a separate retroactive query there would be redundant, so this
    /// always returns `None` when `multiword` is active.
    pub fn boundary_repair(&self) -> Option<String> {
        if self.multiword {
            return None;
        }
        self.executor.boundary_repair()
    }

    // ── Multi-word window helpers ─────────────────────────────────────────────

    /// A window separator: anything that is not an alphanumeric key.  Telex tone
    /// keys (s/f/r/x/j/w/z) and VNI digits are alphanumeric → part of a word;
    /// spaces and punctuation split words and are emitted literally.
    fn is_window_separator(c: char) -> bool {
        !c.is_alphanumeric()
    }

    /// Count word runs (maximal alphanumeric runs) in `raw`.
    fn window_word_count(raw: &[char]) -> usize {
        let mut count = 0;
        let mut in_word = false;
        for &c in raw {
            if Self::is_window_separator(c) {
                in_word = false;
            } else if !in_word {
                count += 1;
                in_word = true;
            }
        }
        count
    }

    /// Scroll the oldest word(s) out of the live window into the frozen prefix
    /// when over the word/char cap.
    fn scroll_out_overflow(&mut self) {
        while Self::window_word_count(&self.raw) > MAX_WINDOW_WORDS
            || self.raw.len() > MAX_WINDOW_RAW
        {
            let n = self.raw.len();
            let mut i = 0;
            while i < n && Self::is_window_separator(self.raw[i]) {
                i += 1;
            }
            let word_start = i;
            while i < n && !Self::is_window_separator(self.raw[i]) {
                i += 1;
            }
            let word_end = i;
            while i < n && Self::is_window_separator(self.raw[i]) {
                i += 1;
            }
            if i == 0 {
                break; // nothing to drop
            }
            let head: Vec<char> = self.raw[..i].to_vec();
            let frozen = self.compose_window(&head);
            self.committed.push_str(&frozen);

            // Personal-learning word-commit signal collection (event-
            // sourcing-completion Phase 5) — ONLY for a word truly CLOSED by
            // a real separator (`i > word_end`, i.e. at least one trailing
            // separator char followed it within `head`), matching the exact
            // closed/open distinction `compose_window` already used to
            // render `frozen` above (see its doc): the pathological "no
            // separator anywhere, forced out by the raw-length safety cap"
            // case is left uncollected, same as it is left un-repaired.
            // Must run BEFORE `self.raw.drain` below — `word_start`/
            // `word_end` index into the CURRENT `self.raw`.
            if i > word_end {
                let word_raw: Vec<char> = self.raw[word_start..word_end].to_vec();
                self.collect_and_refresh_learning(&word_raw);
            }

            self.raw.drain(..i);

            // Map re-anchor (event-sourcing-completion Phase 4, red-team
            // M5/M3): entries fully inside the scrolled-out prefix (`end <=
            // i`) are now permanently baked into `committed` above (rendered
            // with the CURRENT map, before this drain) — drop them. Entries
            // for the surviving window shift left by `i` so they keep
            // pointing at the same word. Precise, not a wholesale clear:
            // scrolling is routine (happens on nearly every keystroke once
            // the window is full), so clearing here would make toggles
            // evaporate on ordinary continued typing, not just on edits.
            // NOTE: `filter` then `map` (not `filter_map(.., .then_some(...))`
            // or `.then(...)`) — `end - i` must only ever be evaluated for
            // entries that already passed the `end > i` guard, or it
            // underflows for any entry with `end <= i`.
            self.toggle_map = self
                .toggle_map
                .iter()
                .filter(|&(&end, _)| end > i)
                .map(|(&end, &literal)| (end - i, literal))
                .collect();
        }
    }

    /// Word-run boundaries in `raw`: `(start, end)` for each maximal
    /// alphanumeric span, split at natural separators AND at any forced
    /// toggle boundary (event-sourcing-completion Phase 4 — a toggle closes
    /// a word even without a typed separator, so later raw chars start a new
    /// run instead of extending the toggled one). Pure query; separators
    /// themselves are not returned, callers emit them verbatim.
    fn word_runs(&self, raw: &[char]) -> Vec<(usize, usize)> {
        let mut runs = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            if Self::is_window_separator(raw[i]) {
                i += 1;
                continue;
            }
            let start = i;
            let mut end = start;
            while end < raw.len() && !Self::is_window_separator(raw[end]) {
                end += 1;
            }
            if let Some(&forced) = self
                .toggle_map
                .keys()
                .filter(|&&b| b > start && b <= end)
                .min()
            {
                end = forced;
            }
            runs.push((start, end));
            i = end;
        }
        runs
    }

    /// The `(start, end)` bounds of the last word run in `raw` (see
    /// `word_runs`), or `None` if the window holds no word to act on (empty,
    /// or trailing separators only).
    fn last_word_run(&self, raw: &[char]) -> Option<(usize, usize)> {
        self.word_runs(raw).last().copied()
    }

    /// Compose a window's raw keys: compose each alphanumeric word run via the
    /// engine, emit separators literally, and concatenate.
    ///
    /// A word run is `closed` (word-boundary final repair,
    /// event-sourcing-completion Phase 3) iff a separator follows it within
    /// `raw` — the moment of complete evidence. The trailing word with no
    /// separator yet (`i == raw.len()`) is still open/editable, so it keeps
    /// the per-keystroke live-typing projection. Re-opening a closed word
    /// (backspacing its separator away) is automatic and needs no special
    /// casing: the very next call here simply finds `closed = false` for it.
    ///
    /// A run whose end is in `toggle_map` (event-sourcing-completion Phase 4)
    /// overrides this: `Some(true)` renders literal(raw) case-preserved (no
    /// compose at all); `Some(false)` renders compose(raw) with `closed =
    /// true` — the toggle itself closed the word, regardless of whether a
    /// real separator ever follows.
    fn compose_window(&mut self, raw: &[char]) -> String {
        let runs = self.word_runs(raw);
        let mut out = String::new();
        let mut cursor = 0;
        for (start, end) in runs {
            // Separators (or nothing) between the previous run and this one.
            out.push_str(&raw[cursor..start].iter().collect::<String>());
            let word = &raw[start..end];
            let rendered = match self.toggle_map.get(&end).copied() {
                Some(true) => word.iter().collect::<String>(),
                // Toggle → composed OVERRIDES any stored raw-pref (Combined
                // Contract: toggle > pref). Route through the prefs-suppressed
                // forced projection, not the pref-consulting `compose_one_word`,
                // so a stored `Pref::Literal` can't make this direction
                // unreachable.
                Some(false) => self.executor.compose_word_forced_composed(word, true),
                None => self.compose_one_word(word, end < raw.len()),
            };
            out.push_str(&rendered);
            cursor = end;
        }
        out.push_str(&raw[cursor..].iter().collect::<String>());
        out
    }

    /// Toggle the last (current) window word between `literal(raw)` and
    /// `compose(raw)` (event-sourcing-completion Phase 4) — the raw-log
    /// architecture's own bidirectional escape hatch, repeatable in either
    /// direction (unlike Unikey's one-shot Ctrl+Shift+Esc, which destroys the
    /// composed form and can't be pressed again to undo itself).
    ///
    /// Also FREEZES the word: from this point it is treated as closed
    /// regardless of whether a real separator ever follows, so continued
    /// typing starts a NEW word instead of extending the toggled one. This
    /// prevents a tone/transform key typed right after the toggle from
    /// silently mutating the frozen word (the junk-letter cascade) and from
    /// mis-attributing a later learning signal to the wrong raw span.
    ///
    /// No-op (`None`) when: not in multiword mode (TSF/Nôm/native — scope
    /// note, TSF deferred), or the window holds no word to toggle (empty, or
    /// only trailing separators).
    pub fn toggle_last_word(&mut self) -> Option<Action> {
        if !self.multiword {
            return None;
        }
        let raw = self.raw.clone();
        let (start, end) = self.last_word_run(&raw)?;
        if start >= end {
            return None;
        }

        let old = self.buffer.clone();
        // First toggle (no entry, or a prior `false`/compose entry) goes
        // literal; toggling an existing literal entry flips it back.
        let now_literal = !matches!(self.toggle_map.get(&end), Some(true));
        self.toggle_map.insert(end, now_literal);

        let raw_sequence: String = raw[start..end].iter().collect();
        self.toggle_signals.push(ToggleSignal {
            raw_sequence,
            literal: now_literal,
        });

        let window = self.compose_window(&raw);
        let new = format!("{}{}", self.committed, window);
        self.buffer = new.clone();

        Some(diff_to_action(&old, &new))
    }

    /// Compose a single separator-free word via the executor (recompute-from-raw).
    ///
    /// `closed`: `true` when this word has a separator after it in the caller's
    /// window (see `compose_window`) — after the normal per-keystroke
    /// recompute, probe the word-boundary CLOSED projection
    /// (`PipelineExecutor::boundary_repair`) and adopt it when it differs
    /// (event-sourcing-completion Phase 3). `false` leaves the live per-
    /// keystroke (open) projection untouched — this is the still-editable
    /// trailing word.
    fn compose_one_word(&mut self, word: &[char], closed: bool) -> String {
        self.executor.reset();
        for &c in word {
            self.executor.process(c);
        }
        if closed {
            if let Some(repaired) = self.executor.boundary_repair() {
                return repaired;
            }
        }
        self.executor.get_buffer().to_string()
    }

    /// Find the window raw-key subset that recomputes to `target` (the window
    /// display minus its last grapheme).  Same strategy as the single-word
    /// backspace: prefer single-key removal (keeps later tones), then a trailing
    /// prefix, then drop the last key.
    fn find_window_backspace_raw(&mut self, raw: &[char], target: &str) -> Vec<char> {
        for idx in (0..raw.len()).rev() {
            let cand: Vec<char> = raw
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != idx)
                .map(|(_, c)| *c)
                .collect();
            if self.compose_window(&cand) == target {
                return cand;
            }
        }
        for k in (0..raw.len()).rev() {
            if self.compose_window(&raw[..k]) == target {
                return raw[..k].to_vec();
            }
        }
        raw[..raw.len() - 1].to_vec()
    }

    /// Get current buffer
    pub fn buffer(&self) -> &str {
        &self.buffer
    }
}

/// Build the screen action to go from `old` display text to `new` via a
/// common-prefix diff (mirrors the output stage): backspace the differing tail
/// of `old`, then emit the differing tail of `new`.
fn diff_to_action(old: &str, new: &str) -> Action {
    let old_c: Vec<char> = old.chars().collect();
    let new_c: Vec<char> = new.chars().collect();
    let mut p = 0;
    while p < old_c.len() && p < new_c.len() && old_c[p] == new_c[p] {
        p += 1;
    }
    let backspace_count = old_c.len() - p;
    let text: String = new_c[p..].iter().collect();
    if backspace_count == 0 && text.is_empty() {
        Action::DoNothing
    } else if backspace_count == 0 {
        Action::Commit(text)
    } else {
        Action::Replace {
            backspace_count,
            text,
        }
    }
}
