//! Keyboard - Main keyboard struct
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/keyboard_tests.rs`.
//!
//! Uses buttre-engine pipeline for processing

use crate::Action;
use buttre_engine::pipeline::{PipelineExecutor, PipelineConfig};
use buttre_engine::types::Action as EngineAction;

/// How many words the editable rolling window keeps (current + N-1 previous).
/// 3 = current word + 2 words back (matches "fix 1–2 previous words").
const MAX_WINDOW_WORDS: usize = 3;
/// Safety cap on the window's raw length (bounds recompute cost).
const MAX_WINDOW_RAW: usize = 64;

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
        })
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
            return self.backspace_multiword();
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

        self.raw = self.find_window_backspace_raw(&raw, &target);
        let raw2 = self.raw.clone();
        let new_window = self.compose_window(&raw2);
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
        }
    }

    /// Compose a window's raw keys: compose each alphanumeric word run via the
    /// engine, emit separators literally, and concatenate.
    fn compose_window(&mut self, raw: &[char]) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < raw.len() {
            if Self::is_window_separator(raw[i]) {
                out.push(raw[i]);
                i += 1;
            } else {
                let start = i;
                while i < raw.len() && !Self::is_window_separator(raw[i]) {
                    i += 1;
                }
                let word: Vec<char> = raw[start..i].to_vec();
                out.push_str(&self.compose_one_word(&word));
            }
        }
        out
    }

    /// Compose a single separator-free word via the executor (recompute-from-raw).
    fn compose_one_word(&mut self, word: &[char]) -> String {
        self.executor.reset();
        for &c in word {
            self.executor.process(c);
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
