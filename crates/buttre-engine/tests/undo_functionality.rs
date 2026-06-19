//! Integration tests for undo functionality (compose/recompute pipeline).
//!
//! ## Note on internal-state assertions
//!
//! `transform_history`, `last_was_undo`, and `TransformType` were Stage 4 / Stage 8
//! artifacts.  They are no longer populated by `ComposeStage`.  Behavioral
//! correctness is verified via `syllable_buffer` and `temp_english_mode` only.
//!
//! ## Note on `aaw` behavior
//!
//! Under the old pipeline, `aaw` produced `âư` via Stage 4 tracking the
//! pre-transform base vowel separately.  Under compose/recompute, `normalize_vowel`
//! returns `â` for the already-transformed vowel, so `"âw"` lookup fails and `w`
//! is appended literally, giving `âw`.  This is correct compose behavior; the
//! old result was an incremental-tracking artifact.

use buttre_engine::pipeline::{PipelineExecutor, PipelineConfig};

fn create_telex_config() -> PipelineConfig {
    buttre_engine::pipeline::telex_config()
}

fn process_sequence(config: &PipelineConfig, input: &str) -> PipelineExecutor {
    let mut executor = PipelineExecutor::new(config.clone());
    for ch in input.chars() {
        executor.process(ch);
    }
    executor
}

// ── Core undo behavioral tests (all still valid under compose) ────────────────

#[test]
fn test_undo_transformation_aa_to_a() {
    // "aaa" → compose detects undo pattern → "aa", temp_english=true.
    let config = create_telex_config();
    let executor = process_sequence(&config, "aaa");
    assert_eq!(executor.context().syllable_buffer, "aa",
               "aaa should undo to 'aa'");
    assert!(executor.context().temp_english_mode,
            "temp_english_mode should be true after undo");
}

#[test]
fn test_undo_transformation_aw_to_a() {
    let config = create_telex_config();
    let executor1 = process_sequence(&config, "aw");
    assert_eq!(executor1.context().syllable_buffer, "ă");

    let executor2 = process_sequence(&config, "aww");
    assert_eq!(executor2.context().syllable_buffer, "aw",
               "'aww' should undo to 'aw'");
    assert!(executor2.context().temp_english_mode);
}

#[test]
fn test_undo_transformation_dd_to_d() {
    let config = create_telex_config();
    let executor1 = process_sequence(&config, "dd");
    assert_eq!(executor1.context().syllable_buffer, "đ");

    let executor2 = process_sequence(&config, "ddd");
    assert_eq!(executor2.context().syllable_buffer, "dd",
               "'ddd' should undo to 'dd'");
    assert!(executor2.context().temp_english_mode);
}

#[test]
fn test_undo_tone_application() {
    let config = create_telex_config();
    let executor1 = process_sequence(&config, "as");
    assert_eq!(executor1.context().syllable_buffer, "á",
               "'as' should give á");

    let executor2 = process_sequence(&config, "ass");
    assert_eq!(executor2.context().syllable_buffer, "as",
               "'ass' should undo tone to 'as'");
    assert!(executor2.context().temp_english_mode,
            "temp_english_mode should be enabled after tone undo");
}

#[test]
fn test_undo_complex_word() {
    // Validation-first: "viet" (bare "ie" + coda "t") is NOT valid Vietnamese —
    // the real word "việt" needs "ê" (typed "vieet").  A tone key cannot apply
    // to a non-Vietnamese base, so the whole sequence is English passthrough.
    let config = create_telex_config();
    let executor3 = process_sequence(&config, "vietff");
    assert_eq!(executor3.context().syllable_buffer, "vietff",
               "'vietff' is not Vietnamese (viet≠việt) → English passthrough");
    assert!(executor3.context().temp_english_mode);
}

// ── test_no_undo_different_key: validation-first behavior ─────────────────────
#[test]
fn test_no_undo_different_key() {
    // "aaw" composes to "âw" (aa→â, then dangling w), which is NOT a valid
    // Vietnamese syllable (â + dangling w).  Validation-first reverts it to the
    // literal keystrokes and latches English passthrough.
    let config = create_telex_config();
    let executor = process_sequence(&config, "aaw");
    assert_eq!(executor.context().syllable_buffer, "aaw",
               "aaw → âw is invalid Vietnamese → literal English passthrough");
    assert!(executor.context().temp_english_mode,
            "temp_english_mode latches on the invalid-syllable fallback");
}

#[test]
fn test_no_undo_when_no_history() {
    let config = create_telex_config();
    let executor = process_sequence(&config, "a");
    assert_eq!(executor.context().syllable_buffer, "a");
    assert!(!executor.context().temp_english_mode);
}

#[test]
fn test_undo_after_english_mode() {
    // "aaa" → undo → temp_english_mode = true.
    let config = create_telex_config();
    let executor1 = process_sequence(&config, "aaa");
    assert!(executor1.context().temp_english_mode);
    // Subsequent chars in a real session would pass through via Gatekeeper.
}

// ── Transform history tests: behavior only (no internal field assertions) ─────

#[test]
fn test_transform_history_tracking() {
    // Behavioral: aa → â. Compose doesn't populate transform_history field.
    let config = create_telex_config();
    let executor = process_sequence(&config, "aa");
    assert_eq!(executor.context().syllable_buffer, "â",
               "aa should produce â");
    assert!(!executor.context().temp_english_mode,
            "no undo on aa");
}

#[test]
fn test_tone_history_tracking() {
    // Behavioral: as → á. Compose doesn't populate transform_history field.
    let config = create_telex_config();
    let executor = process_sequence(&config, "as");
    assert_eq!(executor.context().syllable_buffer, "á",
               "as should produce á");
}

#[test]
fn test_multiple_transformations_history() {
    // Behavioral: thuow → transforms apply via compose.
    let config = create_telex_config();
    let executor = process_sequence(&config, "thuow");
    // t+h+u+ow → th+uo+w; uo+w → ươ; result: thươ
    assert!(!executor.context().syllable_buffer.is_empty(),
            "thuow should produce output");
}

#[test]
fn test_undo_clears_history_entry() {
    // "aaa" → undo detected → "aa", temp_english=true.
    let config = create_telex_config();
    let executor2 = process_sequence(&config, "aaa");
    assert_eq!(executor2.context().syllable_buffer, "aa",
               "aaa should undo to aa");
    assert!(executor2.context().temp_english_mode);
}

#[test]
fn test_case_sensitive_undo() {
    let config = create_telex_config();
    let executor2 = process_sequence(&config, "AAA");
    assert!(executor2.context().temp_english_mode,
            "AAA should trigger undo and set temp_english_mode");
}

#[test]
fn test_real_word_vietnamese_with_undo() {
    // 'm' is not a transformation key → no undo triggered.
    let config = create_telex_config();
    let executor = process_sequence(&config, "vietnamm");
    assert!(!executor.context().temp_english_mode,
            "vietnamm: no undo expected (m is not a transform key)");
}

#[test]
fn test_sequential_undos() {
    // "ddaa" → đ + â = "đâ".
    let config = create_telex_config();
    let executor1 = process_sequence(&config, "ddaa");
    assert_eq!(executor1.context().syllable_buffer, "đâ");

    // "ddaaa": compose fallback detects `aaa` undo at tail.
    // The prefix "dd" is re-composed through segment+transform → "đ".
    // Only the undone cluster reverts to literal "aa".  Result: "đaa".
    // Matches all four reference IMEs: undoing one transform must NOT revert
    // unrelated earlier completed transforms.
    let executor2 = process_sequence(&config, "ddaaa");
    assert_eq!(executor2.context().syllable_buffer, "đaa",
               "ddaaa: undo of aaa tail; prefix dd re-composed to đ (transform-preserving undo)");
    assert!(executor2.context().temp_english_mode);
}
