//! Keyboard - Main keyboard struct
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/keyboard_tests.rs`.
//!
//! Uses buttre-engine pipeline for processing

use crate::Action;
use buttre_engine::pipeline::{PipelineExecutor, PipelineConfig};
use buttre_engine::types::Action as EngineAction;
use std::collections::HashMap;

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
        })
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
                    self.buffer.push_str(&text);
                    result.push(Action::Commit(text.clone()));
                }
                EngineAction::Replace { backspace_count, text } => {
                    // Update buffer
                    for _ in 0..*backspace_count {
                        self.buffer.pop();
                    }
                    self.buffer.push_str(&text);
                    
                    result.push(Action::Replace {
                        backspace_count: *backspace_count,
                        text: text.clone(),
                    });
                }
                EngineAction::UpdateComposition { text, cursor } => {
                    // Update buffer with current composition
                    self.buffer = text.clone();
                    result.push(Action::UpdateComposition { text: text.clone(), cursor: *cursor });
                }
                EngineAction::ConfirmComposition(text) => {
                    // Update buffer with confirmed text
                    self.buffer = text.clone();
                    result.push(Action::ConfirmComposition(text.clone()));
                }
                EngineAction::ShowCandidates { candidates, input } => {
                    result.push(Action::ShowCandidates { candidates: candidates.clone(), input: input.clone() });
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
    pub fn backspace_with_candidates(&mut self) -> Option<(String, Vec<buttre_engine::pipeline::Candidate>)> {
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
            while i < n && !Self::is_window_separator(self.raw[i]) {
                i += 1;
            }
            while i < n && Self::is_window_separator(self.raw[i]) {
                i += 1;
            }
            if i == 0 {
                break; // nothing to drop
            }
            let head: Vec<char> = self.raw[..i].to_vec();
            let frozen = self.compose_window(&head);
            self.committed.push_str(&frozen);
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
            if let Some(&forced) = self.toggle_map.keys().filter(|&&b| b > start && b <= end).min() {
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
                Some(false) => self.compose_one_word(word, true),
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
        self.toggle_signals.push(ToggleSignal { raw_sequence, literal: now_literal });

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
        Action::Replace { backspace_count, text }
    }
}
