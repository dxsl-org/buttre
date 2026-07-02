//! Compose engine — pure recompute-from-raw core (Phase 3).
//!
//! ## Design
//!
//! One call to [`compose`] rebuilds the entire syllable from the raw key buffer.
//! No hidden state, no history flags. Each sub-module owns one pipeline step:
//!
//! | Step | Module | What it does |
//! |------|--------|-------------|
//! | Segment | [`segment`] | Raw keys → (base, transform marks, tone marks) |
//! | Transform | [`transform`] | Apply diacritic marks to base, validation-gated |
//! | Assemble | [`assemble`] | Place + apply tone mark onto nucleus |
//! | Fallback | [`fallback`] | Undo / toggle / English-fallback from key counts |
//!
//! ## Phase scope
//!
//! This module is a **pure library** — it does NOT touch `PipelineExecutor`
//! or any existing stage.  Wiring happens in Phase 4.

mod segment;
mod transform;
mod assemble;
mod fallback;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use crate::pipeline::config::{PipelineConfig, ToneMark, ToneStyle};
use crate::pipeline::validation::{is_attested, is_shape_attested};

// Re-export public types only.
pub use segment::{AppliedMark, SegmentMode};

// ── Configuration ─────────────────────────────────────────────────────────────

/// Validator strategy for transformation gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validator {
    /// Full Vietnamese syllable structure check via `SyllableStructure`.
    Vietnamese,
    /// Hmong syllable rules (placeholder — currently same gate as Vietnamese).
    Hmong,
    /// Custom validator registered in config — treated as no-gate for now.
    Custom,
    /// No validation gate (native scripts: Cham, Khmer, …).
    None,
}

/// All configuration needed for one `compose()` call.
///
/// Derived from `PipelineConfig` via `ComposeOpts::from_config`.
/// Does not borrow from config — owned copies of the tables.
#[derive(Debug, Clone)]
pub struct ComposeOpts {
    /// Raw key → result string (e.g. "aa" → "â").
    /// Keys are lowercase; lookup is done case-insensitively.
    pub transform_rules: HashMap<String, String>,

    /// Tone key → `ToneMark` (e.g. 's' → Acute for Telex, '1' → Acute for VNI).
    pub tone_map: HashMap<char, ToneMark>,

    /// Old vs. New tone positioning style.
    pub tone_style: ToneStyle,

    /// How raw keys are segmented into base + marks.
    pub segment_mode: SegmentMode,

    /// Validation strategy used when gating transforms.
    pub validator: Validator,

    /// `true` when `tone_map` is non-empty.  When `false`, the tone step is
    /// skipped entirely (DirectMap / native scripts).
    pub tone_enabled: bool,

    /// Non-alphabetic chars that act as transform triggers (e.g. VNI digits 6–9).
    ///
    /// Used by `apply_case_mask` in `ComposeStage` to exclude these chars from
    /// the "content chars" count, so `VIE65T` produces `VIỆT` (not `Việt`).
    /// Only non-alphabetic trigger chars are included; letter triggers (like Telex
    /// `w`, `a`) are omitted because they are content chars in their own right.
    pub transform_trigger_chars: HashSet<char>,

    /// Gate INFERRED NON-ADJACENT transforms (see [`segment::TransformMark::non_adjacent`])
    /// on attestation of the composed syllable — fixes the `"data"` → `"dât"`
    /// class of false transforms without touching adjacent-typing behavior.
    ///
    /// `true` only for `Validator::Vietnamese` (the attested-syllable table is
    /// Vietnamese-lexical by definition): the `telex`/`vni`/`simple_telex` presets,
    /// AND any custom `MarkBased` config whose `validation.syllable_structure`
    /// is unset or unrecognized — `from_config` defaults those to
    /// `Validator::Vietnamese` too (documented there). `false` for
    /// `Hmong`/`Custom`/`None`, which have no attested-syllable table to
    /// check against.
    pub attest_non_adjacent: bool,
}

impl ComposeOpts {
    /// Derive `ComposeOpts` from a `PipelineConfig`.
    ///
    /// - `segment_mode`: `DirectMap` when `config.native_script_mode`, else `MarkBased`.
    /// - `validator`: parsed from `config.validation.syllable_structure`
    ///   ("vietnamese" → Vietnamese, "hmong" → Hmong, "none" → None, else Vietnamese).
    /// - `tone_enabled`: `!config.tone_map.is_empty()`.
    /// - `tone_style`: via `config.get_tone_style()`.
    pub fn from_config(config: &PipelineConfig) -> Self {
        let segment_mode = if config.native_script_mode {
            SegmentMode::DirectMap
        } else {
            SegmentMode::MarkBased
        };

        let validator = match config
            .validation
            .as_ref()
            .map(|v| v.syllable_structure.as_str())
            .unwrap_or("vietnamese")
        {
            "hmong"  => Validator::Hmong,
            "custom" => Validator::Custom,
            "none"   => Validator::None,
            _        => Validator::Vietnamese,
        };
        let attest_non_adjacent = validator == Validator::Vietnamese;

        // Collect non-alphabetic transform trigger chars (e.g. VNI '6', '7', '8', '9').
        // Letter triggers (Telex 'w', 'a', etc.) are intentionally excluded — they are
        // also content chars and must not be stripped from the uppercase count.
        let transform_trigger_chars: HashSet<char> = config
            .transform_rules
            .keys()
            .filter_map(|k| k.chars().last())
            .filter(|c| !c.is_alphabetic())
            .collect();

        Self {
            transform_rules: config.transform_rules.clone(),
            tone_map: config.tone_map.clone(),
            tone_style: config.get_tone_style(),
            segment_mode,
            validator,
            tone_enabled: !config.tone_map.is_empty(),
            transform_trigger_chars,
            attest_non_adjacent,
        }
    }
}

// ── Output ────────────────────────────────────────────────────────────────────

/// Result of a single `compose()` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeResult {
    /// The syllable text after all transforms, tone placement, and fallback.
    pub text: String,

    /// `true` when the key sequence looks like an English/non-Vietnamese word
    /// (after undo/toggle detection).  Phase 4 executor uses this to passthrough.
    pub temp_english: bool,

    /// Marks that successfully fired to produce `text` (see
    /// [`transform::apply_transforms`]). Empty for the undo/toggle fallback
    /// paths and for empty raw input — those outputs are not the result of
    /// marks firing against the CURRENT raw buffer. A later phase's undo
    /// detection consumes this to test "was the fired mark's trigger the
    /// last key of the raw prefix".
    pub applied_marks: Vec<AppliedMark>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Recompute the syllable output from the full raw key buffer.
///
/// ## Contract
///
/// - Pure: no global state read or written, no I/O.
/// - Deterministic: same `raw` + `opts` always yields the same `ComposeResult`.
/// - `raw` may be empty; returns `ComposeResult { text: "", temp_english: false }`.
///
/// ## Steps (Vietnamese `MarkBased` mode)
///
/// 1. [`fallback::check_undo`] — detect double-key undo / toggle patterns first
///    so later steps never see the undo key as a real mark.
/// 2. [`segment::segment`] — split raw into (base, transforms, tones).
/// 3. [`transform::apply_transforms`] — apply diacritic marks, validation-gated.
/// 4. [`assemble::apply_tone`] — place + apply the last tone key (if any).
/// 5. Attestation gate on INFERRED NON-ADJACENT transforms (see module doc).
/// 6. Existing structural English-fallback gate (`could_be_vietnamese`).
///
/// ## DirectMap mode (native scripts)
///
/// Segment returns the full base from the transform table; no tone step.
pub fn compose(raw: &[char], opts: &ComposeOpts) -> ComposeResult {
    compose_internal(raw, opts, true)
}

/// Core recompute engine, parameterized by `allow_nonadjacent` — the ONE
/// recursion-guard flag threaded through every re-entry point: the
/// attestation-gate demote below, [`try_elongation_fallback`]'s internal
/// recompute, and the fallback prefix reconstruction in
/// [`fallback::check_fallback`] (closing the red-team C2 bypass: those
/// helpers no longer rebuild prefixes ungated).
///
/// `false` means "this call is already a demoted/gated recompute — do not
/// gate or demote again". Since [`segment::segment`] with
/// `allow_nonadjacent=false` extracts NO mark that would be flagged
/// non-adjacent, the gate condition below (`applied.iter().any(non_adjacent)`)
/// can never be true inside a `false` call — recursion is bounded at depth 1.
fn compose_internal(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> ComposeResult {
    if raw.is_empty() {
        return ComposeResult { text: String::new(), temp_english: false, applied_marks: Vec::new() };
    }

    // Step 1 — fallback / undo detection (reads raw counts only).
    let fb = fallback::check_fallback(raw, opts, allow_nonadjacent);
    if fb.is_handled {
        return ComposeResult {
            text: fb.text,
            temp_english: fb.temp_english,
            applied_marks: Vec::new(),
        };
    }

    // Step 2 — segment.
    let seg = segment::segment(raw, opts, allow_nonadjacent);

    // Step 3 — apply transform marks.
    let (transformed, applied) = transform::apply_transforms(&seg.base, &seg.transforms, opts);

    // Step 4 — apply tone (skip for DirectMap / no tone_map).
    let text = if opts.tone_enabled && !seg.tones.is_empty() {
        // Vietnamese: last tone wins.
        let last_tone_key = *seg.tones.last().unwrap();
        assemble::apply_tone(&transformed, last_tone_key, opts)
            .unwrap_or(transformed)
    } else {
        transformed
    };

    // Step 5 — attestation gate on INFERRED NON-ADJACENT transforms.
    //
    // A mark whose trigger was NOT typed immediately after its target (in RAW
    // order — see `segment::TransformMark::non_adjacent`) survives only if the
    // composed syllable is a real, attested word. This is what fixes
    // `"data"` → `"dât"`: the non-adjacent `a` produces a structurally valid
    // but nonexistent syllable, so it demotes back to a literal `'a'`.
    //
    // `allow_nonadjacent` guards re-entry: this branch is only ever taken from
    // a call where it is still `true`, and it always recurses with `false`,
    // so it can fire at most once per top-level `compose()` call.
    if allow_nonadjacent
        && opts.attest_non_adjacent
        && !passes_attestation_gate(&text, &applied)
    {
        // Demote: recompose with non-adjacent mark extraction disabled at the
        // source (segment). This re-derives the base/marks split from raw —
        // it does NOT mutate `text` — so already-completed ADJACENT
        // transforms elsewhere in the word are preserved untouched.
        return compose_internal(raw, opts, false);
    }

    // Step 6 — validation-first English fallback.
    //
    // When a tone or transform key was consumed but the composed result is NOT a
    // plausible Vietnamese syllable, the user is typing an English/non-Vietnamese
    // word whose r/s/f/j/x/w keys were mis-applied as marks (e.g. "water" → "wảte",
    // "wonder" → "wỏnde", "window" → "windơ").  Revert to the literal keystrokes
    // and latch English passthrough.
    //
    // The "a mark was consumed" guard is essential: base-only sequences are never
    // reverted, so partial Telex states on the way to a valid word are safe —
    // e.g. "vie" (no transform yet, before the "ee"→"ê" doubling) stays "vie"
    // instead of being misread as English.
    if opts.validator == Validator::Vietnamese
        && (!seg.transforms.is_empty() || !seg.tones.is_empty())
        && !could_be_vietnamese(&text, opts)
    {
        // Leniency first (Unikey-style, not aggressive spell-check): before
        // reverting the whole word to English, try stylistic elongation — keep a
        // valid leading syllable and append a repeated tail literally
        // ("veofoo" → "vèooo", not "veofoo").  Only the full English fallback
        // remains for genuinely non-Vietnamese input ("water", "result").
        if let Some(elong) = try_elongation_fallback(raw, opts, allow_nonadjacent) {
            return elong;
        }
        let literal: String = raw.iter().collect();
        return ComposeResult { text: literal, temp_english: true, applied_marks: Vec::new() };
    }

    ComposeResult { text, temp_english: false, applied_marks: applied }
}

/// True when `text` passes the non-adjacent attestation gate: either no
/// applied mark is flagged non-adjacent, or the composed syllable is
/// attested.
///
/// Telex-style alphabetic triggers require an EXACT match (`is_attested`,
/// whatever tone `text` currently carries) — they can never collide with an
/// English word, since a real Telex mark key is also a base letter or a tone
/// key, so a false alphabetic match would already have to look like a real
/// Vietnamese word. VNI-style non-alphabetic (digit) triggers relax to a
/// SHAPE match (`is_shape_attested`, any tone): the tone key often arrives
/// AFTER the digit mark (`nhat61`), so the exact tone is not yet known when
/// the gate must decide. Mixed marks (any non-alphabetic trigger present)
/// use the relaxed check, per the phase spec.
fn passes_attestation_gate(text: &str, applied: &[AppliedMark]) -> bool {
    let mut flagged = applied.iter().filter(|m| m.non_adjacent).peekable();
    if flagged.peek().is_none() {
        return true;
    }
    if flagged.all(|m| m.key.is_alphabetic()) {
        is_attested(text)
    } else {
        is_shape_attested(text)
    }
}

/// Stylistic elongation fallback: when the syllable is invalid, check whether it
/// is a valid syllable followed by a run (≥2) of one repeated character — the
/// way people lengthen words in chat/fiction ("vèoooo", "khôngggg", "trờiii").
///
/// Returns the valid syllable with the repeated tail appended literally, and
/// latches English passthrough so further repeats also append.  Returns `None`
/// for genuinely non-Vietnamese input (e.g. "result": the tail "t" does not
/// repeat, and "resul" is not a valid syllable), which then takes the full
/// English fallback.
///
/// The two guards together protect English words: the tail must REPEAT (English
/// rarely ends in ≥2 identical letters after a valid Vietnamese prefix), and the
/// prefix must itself be valid Vietnamese.
///
/// Only attempted from a TOP-LEVEL call (`allow_nonadjacent == true`) — never
/// from within an attestation-gate demote. A demoted pass has already
/// conceded "this raw sequence, once the unattested mark is stripped, is not
/// clean Vietnamese"; re-discovering a DIFFERENT, unrelated short word via
/// elongation's own heuristics is not a case this phase needs to support, and
/// doing so is actively wrong for inputs like `"nasa"`: demoting the flagged
/// `a` leaves literal base `"naa"` + tone `'s'`, `assemble::apply_tone` places
/// the tone on the FIRST of the two literal a's (open 2-vowel syllable, no
/// special-cased pair), giving `"ná"` — which is itself a real, attested word
/// (`"ná"` = slingshot) — and the trailing lone `'a'` then satisfies the
/// `lengthens_final` single-repeat allowance below, producing the spurious
/// `"náa"`. Restricting elongation to top-level calls closes this without
/// touching the heuristic itself (legitimate top-level elongation — a
/// non-demoted `"khoongggg"` — never sets `allow_nonadjacent = false`).
fn try_elongation_fallback(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> Option<ComposeResult> {
    use crate::vowel::cluster::normalize_vowel;

    if !allow_nonadjacent {
        return None;
    }

    let &last = raw.last()?;
    let run = raw.iter().rev().take_while(|&&c| c == last).count();
    let base_raw = &raw[..raw.len() - run];
    if base_raw.is_empty() {
        return None;
    }
    let base = compose_internal(base_raw, opts, allow_nonadjacent);
    if base.temp_english || !could_be_vietnamese(&base.text, opts) {
        return None;
    }
    // Elongation is recognised when EITHER the tail repeats ≥2 times, OR it is a
    // single repeat of the base syllable's final vowel/letter (lengthening the
    // last sound — "vèo"+"o").  The latter is essential because a single extra
    // vowel makes the syllable invalid, and without it the first extra key would
    // latch the full English fallback and the elongation could never grow.
    // The "matches the final letter" test still rejects English tails like
    // "feel" (base "fê" ends in 'ê', tail 'l' ≠ 'ê').
    let lengthens_final = base
        .text
        .chars()
        .last()
        .is_some_and(|c| normalize_vowel(c) == last);
    if run < 2 && !lengthens_final {
        return None;
    }
    let suffix: String = std::iter::repeat(last).take(run).collect();
    Some(ComposeResult {
        text: format!("{}{}", base.text, suffix),
        temp_english: true,
        applied_marks: Vec::new(),
    })
}

/// True when `text` is a valid Vietnamese syllable OR a valid in-progress prefix
/// of one (an onset typed with the nucleus not yet reached, e.g. "đ", "ng",
/// "ngh", "th").  Used to gate the English fallback so a mark applied to a real
/// Vietnamese base is never reverted.
///
/// Stylistic elongation (`khôngggg`, `trờiii`, `ơiii`) is accepted: the validity
/// check runs on the syllable with runs of repeated identical characters
/// collapsed.  This is safe because no valid Vietnamese syllable has two
/// identical adjacent letters in its final orthographic form — the diacritic
/// carries the distinction (`ô`, not `oo`; `â`, not `aa`).  The displayed text
/// keeps the elongation; only the validity decision sees the collapsed form.
fn could_be_vietnamese(text: &str, opts: &ComposeOpts) -> bool {
    use crate::pipeline::validation::SyllableStructure;
    let collapsed = collapse_adjacent_repeats(text);
    let s = SyllableStructure::parse(&collapsed);
    if s.is_valid() {
        return true;
    }
    // Consonant-only prefix: onset present, nucleus/coda not yet typed.
    if s.nucleus.is_empty() && s.coda.is_empty() && !s.onset.is_empty() {
        return true;
    }
    // VNI intermediate form: when the method uses non-alphabetic transform keys
    // (digit '6' for e→ê, '7' for u→ư, etc.), the user may type a tone before
    // the vowel-mark key (e.g. "mieng16": '1'=sắc, then '6'=e→ê).  The
    // intermediate state after '1' has nucleus "ie" + coda, which is not a
    // final valid Vietnamese form but IS a plausible in-progress syllable.
    // Accept it so English-fallback does not latch before '6' is pressed.
    //
    // This path is skipped for Telex (transform_trigger_chars is empty — all
    // Telex mark keys are alphabetic), so "vietf" (tone on bare 'e' in Telex)
    // continues to fall through to English passthrough as intended.
    if !opts.transform_trigger_chars.is_empty()
        && s.nucleus == "ie"
        && matches!(s.coda.as_str(), "c" | "m" | "n" | "ng" | "p" | "t")
    {
        return true;
    }
    false
}

/// Collapse runs of consecutive identical characters down to one
/// ("khôngggg" → "không", "trờiii" → "trời").
fn collapse_adjacent_repeats(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if Some(c) != prev {
            out.push(c);
            prev = Some(c);
        }
    }
    out
}
