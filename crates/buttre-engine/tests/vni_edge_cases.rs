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
    // Lenient VNI (Unikey-style): "ie" + coda is accepted as a valid intermediate
    // form so that tone-before-transform works (e.g. "mieng16" → "miếng").
    // As a consequence, "viet5" applies nặng to the bare 'e' without English
    // fallback, and "viet55" triggers the tone-undo path → "viet5" (temp_english).
    let result = process_sequence("viet55");
    assert_eq!(result, "viet5", "Expected 'viet5' after tone undo on bare 'ie'+'t', got '{}'", result);
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

// Priority 4: Phase 3 regression guard — VNI "ie" exception must stay (KEEP)
//
// `could_be_vietnamese`'s "ie"+coda exception (compose/mod.rs) exists for the
// TONE-BEFORE-TRANSFORM ordering on digit-triggered nuclei: typing the tone key
// before the circumflex/horn digit leaves an intermediate structural nucleus
// ("ie" instead of "iê") that fails `SyllableStructure::is_valid()`. Unlike
// `nhat6` (nucleus "a" is already a fully valid standalone syllable with any
// coda, so no exception is needed there), a digit-nucleus base like "mieng"
// genuinely needs the exception mid-typing — this is a DIFFERENT code path
// from the P2 non-adjacent attestation gate (which only fires when a transform
// mark has actually been extracted; at this intermediate point none has).
//
// Phase 3's original plan called for DELETING this exception as "subsumed by
// P2's shape-attestation". Deleting it and running these two tests proved
// that claim wrong: `mieng1`→(latches English)→`6` produced literal
// "mieng16" instead of "miếng". The exception was restored; these tests are
// the permanent regression guard against re-attempting that deletion. The
// compose()-level `vni_mieng16_yields_mieng_acute` test alone would NOT catch
// this — it only asserts the FINAL string, never this intermediate state.

#[test]
fn test_vni_mieng16_incremental_no_flicker() {
    use buttre_engine::pipeline::PipelineExecutor;
    // Typing "mieng16" one keystroke at a time: tone '1' arrives BEFORE the
    // circumflex digit '6'. Mid-typing, after "mieng1", the nucleus is still
    // bare "ie" (not yet "iê") — this must not latch English fallback, or the
    // trailing '6' would append literally instead of completing the transform.
    let config = vni_config();
    let mut executor = PipelineExecutor::new(config);
    for ch in "mieng1".chars() {
        executor.process(ch);
    }
    assert!(!executor.is_temp_english_mode(),
        "mid-typing 'mieng1' (bare 'ie' nucleus + tone) must not latch English fallback");
    executor.process('6');
    assert_eq!(executor.context().syllable_buffer, "miếng",
        "incremental mieng->1->6 must complete to 'miếng', got '{}'", executor.context().syllable_buffer);
}

#[test]
fn test_vni_nhat61_incremental_no_flicker() {
    use buttre_engine::pipeline::PipelineExecutor;
    // Digit-before-tone ordering (the P2 shape-attestation case): confirm the
    // executor-level, character-by-character path matches the compose()-level
    // `critical_vni_nhat61_shape_attested_no_flicker` assertion.
    let config = vni_config();
    let mut executor = PipelineExecutor::new(config);
    for ch in "nhat6".chars() {
        executor.process(ch);
    }
    assert!(!executor.is_temp_english_mode(),
        "mid-typing 'nhat6' must not latch English fallback");
    executor.process('1');
    assert_eq!(executor.context().syllable_buffer, "nhất",
        "incremental nhat->6->1 must complete to 'nhất', got '{}'", executor.context().syllable_buffer);
}

// Priority 5: Multi-step undo — LITERAL / undo-is-final semantics (P6)
//
// a6 → â
// a61 → ấ
// a611 → â1 (tone undo fires: trailing "11" is an even-parity tone-undo pair;
//            transform-preserving — â survives, tone strips, literal "1" appended)
// a6116 → â16 (undo-is-final: temp_english_mode is now latched by the a611
//              step, so the trailing '6' is a LITERAL append at the executor
//              level, not a redo of the â transform)
//
// This is the P6 parity-fold adjudication (plan.md Combined Contract, "(d) is
// a LAST-EVENT parity fold sharing P6's rule"): red-team review confirmed
// executor-level `a6116`→`ấ` (full redo) is only reachable through a FUTURE
// evidence-based un-latch probe (P2, descoped from this phase) — the
// literal-append path (`ComposeStage`/`GatekeeperStage`'s existing
// `temp_english_mode` handling) never re-runs `compose()` once latched, so it
// cannot re-derive the â+sắc redo on its own. User-confirmed adjudication:
// literal wins (simpler, undo-is-final) — do NOT attempt to produce "ấ" here.

#[test]
fn test_multi_step_undo_a6116() {
    let result = process_sequence("a6116");
    assert_eq!(result, "â16",
        "a6116: undo-is-final — a611 undoes the tone (-> â1, temp_english_mode \
         latches), then the trailing '6' is a literal append (-> â16), not a redo. \
         Got '{}'", result);
}
