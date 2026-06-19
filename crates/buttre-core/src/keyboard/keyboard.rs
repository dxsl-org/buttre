//! Keyboard - Main keyboard struct
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-core/tests/keyboard_tests.rs`.
//!
//! Uses buttre-engine pipeline for processing

use crate::Action;
use buttre_engine::pipeline::{PipelineExecutor, PipelineConfig};
use buttre_engine::types::Action as EngineAction;

/// Main keyboard struct
pub struct Keyboard {
    /// Pipeline executor from buttre-engine
    executor: PipelineExecutor,
    
    /// Current buffer
    buffer: String,
}

impl Keyboard {
    /// Create a new keyboard from pipeline config
    pub fn new(config: PipelineConfig) -> anyhow::Result<Self> {
        // Create executor directly from config
        let executor = PipelineExecutor::new(config);
        
        Ok(Self {
            executor,
            buffer: String::new(),
        })
    }
    
    /// Process a keystroke
    /// 
    /// Returns a vector of actions to perform. Usually contains 1-2 actions:
    /// - Main action (DoNothing/Commit/Replace/UpdateComposition)
    /// - Optional ShowCandidates/HideCandidates for Nôm input
    pub fn process(&mut self, key: char) -> anyhow::Result<Vec<Action>> {
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
    
    /// Process backspace.
    ///
    /// ## Why this resets the composition
    ///
    /// The engine is stateless recompute-from-raw and tracks ONLY the current
    /// in-progress syllable (committed text on screen was never part of engine
    /// state).  The previous implementation popped just this mirror buffer and
    /// left the executor's `char_buffer` / `syllable_buffer` / `last_output`
    /// untouched — so after one backspace the engine still believed the full
    /// pre-backspace syllable was on screen.  The next keystroke then diffed
    /// against that stale `last_output`, producing a backspace count that reached
    /// back into the previous word and ate the separator ("một ngày" → delete +
    /// type → words merged, space lost).
    ///
    /// A correct raw-key pop is ambiguous (one displayed grapheme can map to
    /// several keys, e.g. "ư" ← "uw"), so we take the safe path: delete the one
    /// displayed char and reset the composition.  Subsequent keystrokes start a
    /// fresh syllable, which can never desync with the screen.  Returns
    /// `DoNothing` when nothing is composing so the host handles the backspace.
    pub fn backspace(&mut self) -> anyhow::Result<Action> {
        if self.buffer.is_empty() {
            return Ok(Action::DoNothing);
        }

        // Drop all in-progress composition state (mirror + executor context),
        // then emit a single backspace to remove the last displayed char.
        self.reset();

        Ok(Action::Replace {
            backspace_count: 1,
            text: String::new(),
        })
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
    
    /// Reset state
    pub fn reset(&mut self) {
        self.executor.reset();
        self.buffer.clear();
    }
    
    /// Get current buffer
    pub fn buffer(&self) -> &str {
        &self.buffer
    }
    
}
