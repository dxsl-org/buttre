use buttre_engine::pipeline::{PipelineExecutor, vni_config};

fn process_sequence(input: &str) -> String {
    let config = vni_config();
    let mut executor = PipelineExecutor::new(config);
    
    for ch in input.chars() {
        executor.process(ch);
    }
    
    executor.context().syllable_buffer.clone()
}

// Priority 1: Tone-then-transform
// User applies tone BEFORE transform (a1 then 6 → ấ)

#[test]
fn test_tone_then_transform_a16() {
    let result = process_sequence("a16");
    assert_eq!(result, "ấ", "Expected 'ấ' (a + tone1 + circumflex), got '{}'", result);
}

#[test]
fn test_tone_then_transform_e26() {
    let result = process_sequence("e26");
    assert_eq!(result, "ề", "Expected 'ề' (e + tone2 + circumflex), got '{}'", result);
}

#[test]
fn test_tone_then_transform_o36() {
    let result = process_sequence("o36");
    assert_eq!(result, "ổ", "Expected 'ổ' (o + tone3 + circumflex), got '{}'", result);
}

#[test]
fn test_tone_then_transform_u47() {
    let result = process_sequence("u47");
    assert_eq!(result, "ữ", "Expected 'ữ' (u + tone4 + horn), got '{}'", result);
}

#[test]
fn test_tone_then_transform_o57() {
    let result = process_sequence("o57");
    assert_eq!(result, "ợ", "Expected 'ợ' (o + tone5 + horn), got '{}'", result);
}

// Priority 2: Sequential tone undo
// Multiple consecutive tone toggles.
//
// Phase 4 / recompute model: `a111` → fallback detects `11` undo-pair at tail
// → temp_english_mode; third `1` is a literal append → `a11`.
//
// This matches Unikey `tempVietOff` behaviour: after tone-undo, subsequent
// same-key taps are literal (no re-apply).  This is the correct reference
// standard, not a missing feature.

#[test]
fn test_sequential_tone_undo_a111() {
    let result = process_sequence("a111");
    // Unikey standard: undo pair (11) → temp_english; third 1 is literal → a11.
    assert_eq!(result, "a11",
        "a111 → a11: Unikey standard (tempVietOff after undo, no re-apply), got '{}'", result);
}

#[test]
fn test_sequential_tone_undo_e222() {
    let result = process_sequence("e222");
    // Unikey standard: undo pair (22) → temp_english; third 2 is literal → e22.
    assert_eq!(result, "e22",
        "e222 → e22: Unikey standard (tempVietOff after undo, no re-apply), got '{}'", result);
}

#[test]
fn test_sequential_tone_undo_o333() {
    let result = process_sequence("o333");
    // Unikey standard: undo pair (33) → temp_english; third 3 is literal → o33.
    assert_eq!(result, "o33",
        "o333 → o33: Unikey standard (tempVietOff after undo, no re-apply), got '{}'", result);
}

// Priority 3: Word-level undo
// Undo tone in completed word
// In VNI: typing tone key twice should toggle the tone

#[test]
fn test_word_level_undo_viet5() {
    // Validation-first: VNI "viet" (bare "ie" + coda "t") is NOT valid — the real
    // word "việt" needs "ê" (e6: "viet65"/"vie65t").  The dot tone can't apply to
    // a non-Vietnamese base, so "viet55" is English passthrough (both 5s literal).
    let result = process_sequence("viet55");
    assert_eq!(result, "viet55", "viet is not Vietnamese (≠việt) → English passthrough, got '{}'", result);
}

#[test]
fn test_word_level_undo_hoa2() {
    // hoa2 → hòa, then hoa22 → hoa2 (undo)
    let result = process_sequence("hoa22");
    assert_eq!(result, "hoa2", "Expected 'hoa2' after tone undo, got '{}'", result);
}

#[test]
fn test_word_level_undo_toi1() {
    // toi1 → tói, then toi11 → toi1 (undo)
    let result = process_sequence("toi11");
    assert_eq!(result, "toi1", "Expected 'toi1' after tone undo, got '{}'", result);
}

// Test case we're skipping (as per user request)
// This would require complex multi-step history tracking

#[test]
#[ignore = "Skipped as per user request - too complex, low value"]
fn test_multi_step_undo_a6116() {
    let result = process_sequence("a6116");
    // Expected behavior (if implemented):
    // a6 → â
    // a61 → ấ
    // a611 → â (undo tone)
    // a6116 → ấ (redo transform with tone)
    assert_eq!(result, "ấ", "Expected 'ấ' after multi-step undo, got '{}'", result);
}
