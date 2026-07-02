//! Stage 2: Smart Gatekeeper
//!
//! **Tests**: Integration tests for this stage are located in `crates/buttre-engine/tests/stage2_gatekeeper_tests.rs`.
//!
//! This stage implements intelligent English fallback detection.
//!
//! ## Algorithm
//!
//! 1. Check if temp_english_mode is active
//!    - If yes, pass through the input and reset mode on non-alphabetic
//! 2. Check if input should trigger English mode
//!    - Non-alphabetic characters always pass through
//! 3. Otherwise, continue to next stage for Vietnamese processing
//!
//! ## Rationale
//!
//! The gatekeeper prevents unwanted Vietnamese transformations when:
//! - User is typing English words
//! - An undo operation was detected (temp_english_mode set by Stage 6)
//! - Non-alphabetic input is encountered

use crate::pipeline::{PipelineStage, StageResult, TypingContext};

/// Stage 2: Smart Gatekeeper
///
/// Controls whether input should be processed as Vietnamese or passed through as English.
///
/// ## Algorithm
///
/// This stage implements the "temporary English mode" feature from Unikey:
///
/// 1. **Check Temporary English Mode**:
///    - If `temp_english_mode` is true, pass through input as-is
///    - Reset mode when encountering non-alphabetic character
///    - This allows typing English after an undo operation
///
/// 2. **Check Non-Alphabetic Input**:
///    - Numbers, spaces, punctuation → PassThrough
///    - These characters should never be transformed
///
/// 3. **Check Buffer State**:
///    - If buffer is empty and input is alphabetic → Continue
///    - This starts a new Vietnamese syllable
///
/// 4. **Default Behavior**:
///    - Continue to next stage for Vietnamese processing
///
/// ## Example Scenarios
///
/// ### Scenario 1: Temporary English Mode
/// ```text
/// State: temp_english_mode = true
/// Input: 'f', 'i', 'l', 'e'
/// Output: PassThrough for each (outputs "file")
/// Input: ' ' (space)
/// Action: Reset temp_english_mode, PassThrough
/// ```
///
/// ### Scenario 2: Non-Alphabetic Input
/// ```text
/// Input: '1', '2', '3'
/// Output: PassThrough (outputs "123")
/// ```
///
/// ### Scenario 3: Normal Vietnamese
/// ```text
/// State: temp_english_mode = false
/// Input: 't', 'h', 'u'
/// Output: Continue (proceed to transformation stages)
/// ```
#[derive(Debug, Clone)]
pub struct GatekeeperStage {
    /// Input method name (for VNI-specific optimizations)
    method_name: String,
    /// Native Script Mode (e.g. Cham) - disables English fallback features
    native_script_mode: bool,
    // Future: English dictionary for word detection
    // english_dict: Option<HashSet<String>>,
}

impl GatekeeperStage {
    /// Create a new gatekeeper stage
    pub fn new() -> Self {
        Self {
            method_name: String::new(),
            native_script_mode: false,
        }
    }

    /// Create a gatekeeper stage with method name
    pub fn with_method(method_name: String) -> Self {
        Self {
            method_name,
            native_script_mode: false,
        }
    }

    /// Create from config
    pub fn from_config(config: &crate::pipeline::PipelineConfig) -> Self {
        Self {
            method_name: config.name.clone(),
            native_script_mode: config.native_script_mode,
        }
    }

    /// Check if a character is a separator (space, newline, etc.)
    ///
    /// ## Algorithm
    ///
    /// Separators are characters that:
    /// - End the current word/syllable
    /// - Should always pass through unchanged
    /// - Reset temporary English mode
    pub fn is_separator(&self, ch: char) -> bool {
        ch.is_whitespace() || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | '-' | '_')
    }

    /// Check if a character is a vowel (basic Vietnamese vowels)
    pub fn is_vowel(&self, ch: char) -> bool {
        let lower = ch.to_lowercase().next().unwrap_or(ch);
        matches!(lower, 
            // Base vowels
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' |
            // Transformed vowels
            'ă' | 'â' | 'ê' | 'ô' | 'ơ' | 'ư' |
            // Toned vowels - all combinations
            'á' | 'à' | 'ả' | 'ã' | 'ạ' |
            'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' |
            'ấ' | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' |
            'é' | 'è' | 'ẻ' | 'ẽ' | 'ẹ' |
            'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' |
            'í' | 'ì' | 'ỉ' | 'ĩ' | 'ị' |
            'ó' | 'ò' | 'ỏ' | 'õ' | 'ọ' |
            'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' |
            'ớ' | 'ờ' | 'ở' | 'ỡ' | 'ợ' |
            'ú' | 'ù' | 'ủ' | 'ũ' | 'ụ' |
            'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' |
            'ý' | 'ỳ' | 'ỷ' | 'ỹ' | 'ỵ'
        )
    }
}

impl Default for GatekeeperStage {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineStage for GatekeeperStage {
    fn process(&self, ctx: &mut TypingContext, input: char) -> StageResult {
        // Algorithm Step 1: Check temporary English mode
        // If temp_english_mode is active, pass through input as-is
        if ctx.temp_english_mode {
            // NATIVE SCRIPT FIX: If native script mode, IGNORE temp_english_mode for alphabetic chars.
            // We want to process them regardless of undo history.
            if self.native_script_mode && input.is_alphabetic() {
                 // Fall through to normal processing (don't return PassThrough)
            } else {
                // Check if we should exit temp English mode.
                // Numbers are intentionally NOT reset triggers: after a VNI undo, the user
                // may press the same tone-key digit again (e.g. a111 → á1). Stage 4 appends
                // the number literally when temp_english_mode is active.
                // Only separators (space, punctuation) and non-alphabetic/non-numeric symbols
                // reset the mode.
                if self.is_separator(input) || (!input.is_alphabetic() && !input.is_numeric()) {
                    // Reset temp English mode on separator or non-alphabetic/non-numeric
                    ctx.temp_english_mode = false;
                    // PassThrough for separators/symbols (triggers context reset in executor)
                    return StageResult::PassThrough;
                }
                
                // CRITICAL: For alphabetic chars in temp_english_mode:
                // - Return CONTINUE to let Stage 4 handle the append
                // - Don't append here - Stage 4 will append when no transformation is found
                // - This prevents double append (Stage 2 + Stage 4)
                // Example: after "des", typing "ign" → Stage 4 appends each char
                return StageResult::Continue;
            }
        }

        // ========================================
        // VNI Optimization #3: Context Detection
        // ========================================
        // PROBLEM: "Windows 10" → "Windows 1ò" (VNI treats '1' as tone key after 'o')
        // SOLUTION: Detect numeric context (digit after space/separator) → don't transform
        //
        // Pattern detection:
        // 1. If VNI method
        // 2. AND input is a digit (0-9)
        // 3. AND last character in syllable_buffer is a space/separator or buffer is empty
        // → This is likely part of a number (like "10", "2025"), not Vietnamese
        // → PassThrough to avoid transformation
        // TECH-DEBT: hardcoded to the "vni" method name / numeric triggers.
        // The descoped VIQR preset (phase-07, preserved as a design record in
        // `.agents/`) would have needed this same numeric-context detection
        // generalized to an arbitrary config-driven trigger-class check (any
        // non-alphabetic transform trigger, not just VNI digits) — descoped
        // along with VIQR itself. Generalize here if a future preset adds a
        // non-alphabetic, non-digit trigger set that also needs this guard.
        if self.method_name == "vni" && input.is_numeric() {
            // Check if we're starting a number (after space or at start)
            if ctx.syllable_buffer.is_empty() || 
               ctx.syllable_buffer.chars().last().map_or(false, |c| c.is_whitespace()) {
                // This is likely a number at the start of a word
                // Example: "Windows 10", "năm 2025"
                return StageResult::PassThrough;
            }
            
            // Check if previous character is also a digit
            if let Some(last_char) = ctx.syllable_buffer.chars().last() {
                if last_char.is_numeric() {
                    // Check the context of this number sequence
                    // Scan backwards to find what this number is attached to
                    let chars: Vec<char> = ctx.syllable_buffer.chars().collect();
                    let mut is_attached_to_word = false;
                    
                    // Iterate backwards from the character before the last digit
                    // chars has the full buffer content including the last digit
                    for i in (0..chars.len() - 1).rev() {
                        let c = chars[i];
                        if !c.is_numeric() {
                            // Found non-numeric terminator
                            if c.is_alphabetic() {
                                is_attached_to_word = true;
                            }
                            break;
                        }
                    }
                    
                    if is_attached_to_word {
                        // This is a number attached to a word (e.g. "H200", "a6")
                        // Allow it to proceed through pipeline for VNI processing
                        // Stage 5 will handle appending numbers if they are not invalid tones
                    } else {
                        // This is a standalone number or attached to symbol (e.g. "10", "2025")
                        return StageResult::PassThrough;
                    }
                }
            }
            
            // Otherwise, this is VNI transformation (e.g., "a1" → "á")
            // Let it continue to transformation stages
        }

        // Algorithm Step 2: Check for non-alphabetic input
        // IMPORTANT: Allow numbers if buffer is not empty (for VNI support)
        // VNI uses numbers like 6,7,0 for transformations (a6 → â)
        if !input.is_alphabetic() {
            // If raw_buffer is not empty and input is a digit, allow it to continue
            // This enables VNI-style transformations
            // NOTE: Use raw_buffer instead of is_empty() because syllable_buffer
            // may still be empty at this point (Stage 1 only updates raw_buffer)
            if !ctx.raw_buffer().is_empty() && input.is_numeric() {
                return StageResult::Continue;
            }
            
            // SPECIAL CASE: Space with candidates showing (for multi-keyword Nôm search)
            // When candidates are showing and space is pressed, let it continue
            // This allows "thien thuong" multi-keyword search
            if input == ' ' && ctx.showing_candidates {
                return StageResult::Continue;
            }
            
            // Otherwise, pass through (spaces, punctuation, etc.)
            return StageResult::PassThrough;
        }

        // Algorithm Step 3: Check if this is Vietnamese input
        // At this point:
        // - temp_english_mode is false
        // - input is alphabetic
        // - We should proceed with Vietnamese processing
        StageResult::Continue
    }

    fn name(&self) -> &'static str {
        "GatekeeperStage"
    }

    fn reset(&mut self) {
        // No internal state to reset
        // Note: temp_english_mode is in TypingContext, not here
    }
}
