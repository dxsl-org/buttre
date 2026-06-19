//! Stage: Compose — recompute-from-raw core (Phase 4).
//!
//! Replaces stages 4 (Transform), 5 (Tone), 6 (Permutation), 7 (Reconciliation),
//! and 8 (Retrofix) with a single pure call to [`compose`].
//!
//! ## Contract
//!
//! - When `ctx.temp_english_mode` is `false` at entry: full recompute from the
//!   entire `ctx.char_buffer` via [`compose`].  The case mask is applied to the
//!   result so the output respects the user's capitalisation.
//!
//! - When `ctx.temp_english_mode` is `true` at entry: the buffer is already in
//!   English/fallback mode (set by a previous compose call).  We do NOT recompute
//!   from raw — that would re-interpret the buffer as Vietnamese.  Instead we
//!   append the new character literally to `syllable_buffer`, preserving original
//!   case.  `temp_english_mode` stays `true`.
//!
//! ## Case handling (normal mode)
//!
//! `compose` is case-agnostic: it receives lowercase chars and returns a
//! lowercase-anchored string.  We then apply the case mask from `char_buffer`:
//! - All chars uppercase → uppercase the whole result.
//! - Any leading uppercase → capitalise only the first output char.
//! - No uppercase → return as-is.

use crate::compose::{compose, ComposeOpts};
use crate::pipeline::config::PipelineConfig;
use crate::pipeline::context::{CharInfo, CharInfoBufferExt};
use crate::pipeline::{PipelineStage, StageResult, TypingContext};

/// Maximum raw keystrokes a single Vietnamese syllable can occupy before the
/// recompute path treats the buffer as run-on input and falls back to literal
/// passthrough.  Set generously above the real maximum (~10: "nghieengf",
/// "truwowngf", VNI "nghie6ng2") so it never clips a legitimate syllable.
const MAX_VIET_SYLLABLE_RAW: usize = 16;

/// Stage: Compose (replaces stages 4–8).
///
/// Built once from `PipelineConfig`; holds the derived `ComposeOpts` for the
/// lifetime of the executor.
#[derive(Debug, Clone)]
pub struct ComposeStage {
    opts: ComposeOpts,
}

impl ComposeStage {
    /// Build a `ComposeStage` from a pipeline configuration.
    pub fn from_config(config: &PipelineConfig) -> Self {
        Self {
            opts: ComposeOpts::from_config(config),
        }
    }
}

impl PipelineStage for ComposeStage {
    fn process(&self, ctx: &mut TypingContext, _input: char) -> StageResult {
        // ── English / fallback passthrough ────────────────────────────────────
        // If temp_english_mode is already true (set by a prior compose call that
        // decided the buffer is an undo/English sequence), keep appending chars
        // literally.  Recomputing from raw would re-interpret the raw buffer and
        // overwrite the correct fallback text.
        if ctx.temp_english_mode {
            if let Some(last) = ctx.char_buffer.last() {
                // Preserve original case for the appended char.
                let ch = if last.is_uppercase {
                    last.ch.to_uppercase().next().unwrap_or(last.ch)
                } else {
                    last.ch
                };
                ctx.syllable_buffer.push(ch);
            }
            // temp_english_mode stays true — subsequent chars also append literally.
            return StageResult::Continue;
        }

        // ── Normal recompute path ─────────────────────────────────────────────
        // Extract lowercase raw keys from the char buffer Stage 1 populated.
        let raw: Vec<char> = ctx.char_buffer.to_char_vec(); // normalized (lowercase)

        // ── Defensive syllable-length cap ─────────────────────────────────────
        // A single Vietnamese syllable never exceeds ~10 raw keystrokes, even at
        // its longest with tone + transform keys ("nghieengf", "truwowngf",
        // VNI "nghie6ng2").  Past a generous cap the buffer is unavoidably run-on
        // input — multiple syllables typed with no separator.  Recomputing the
        // whole thing from raw on every keystroke is wasted O(n²) work, and it
        // makes the entire long buffer a single desync unit (one leaked/dropped
        // key corrupts all of it).  Switch to literal passthrough — the same path
        // temp_english uses — to bound both the cost and the blast radius.
        //
        // Skipped in Nôm multi-keyword candidate mode, where a space-joined query
        // ("thien thuong …") legitimately grows past the cap.
        if !ctx.showing_candidates
            && raw.len() > MAX_VIET_SYLLABLE_RAW
            && !raw.iter().any(|c| c.is_whitespace())
        {
            if let Some(last) = ctx.char_buffer.last() {
                let ch = if last.is_uppercase {
                    last.ch.to_uppercase().next().unwrap_or(last.ch)
                } else {
                    last.ch
                };
                ctx.syllable_buffer.push(ch);
            }
            // Latch passthrough so the rest of the run-on word also appends
            // literally until the next separator resets the engine.
            ctx.temp_english_mode = true;
            return StageResult::Continue;
        }

        // Run the pure recompute engine.
        let result = compose(&raw, &self.opts);

        // Apply the case mask from the original keystrokes.
        let text = apply_case_mask(&result.text, &ctx.char_buffer, &self.opts);

        ctx.syllable_buffer = text;
        ctx.temp_english_mode = result.temp_english;

        StageResult::Continue
    }

    fn name(&self) -> &'static str {
        "ComposeStage"
    }

    fn reset(&mut self) {
        // No internal state — compose is stateless.
    }
}

// ── Case application ──────────────────────────────────────────────────────────

/// Apply the original case mask to a compose-produced (lowercase-anchored) string.
///
/// ## Algorithm
///
/// 1. No uppercase chars in buffer → return as-is (pure lowercase).
/// 2. Build a list of "case-bearing" input chars: those that are NOT
///    tone-map keys and NOT non-alphabetic transform triggers (VNI digits).
///    These are the chars whose case should influence the output.
/// 3. All case-bearing chars uppercase → full uppercase result.
/// 4. Otherwise: walk output chars left-to-right, assigning each the case of
///    the case-bearing input char at the same index, with two special rules:
///    - **Doubling collapse**: when two consecutive case-bearing chars have the
///      SAME character (e.g. `DD`, `AA`) they form a doubling transform and both
///      map to a single output char; consume both but use the first one's case.
///    - **Overflow**: output chars beyond the case-bearing list default to
///      lowercase.
///
/// This produces correct case for:
/// - All-caps (`NGUOI+f` → `NGƯỜI`)
/// - Mixed leading-caps (`NGuwow+f` → `NGười`)
/// - Title-case (`Nguow+f` → `Người`)
/// - Doubling transforms (`DDaay` → `Đây`, not `ĐÂy`)
/// - VNI digit triggers (`VIE65T` → `VIỆT`)
fn apply_case_mask(text: &str, char_buffer: &[CharInfo], opts: &ComposeOpts) -> String {
    if text.is_empty() || char_buffer.is_empty() {
        return text.to_string();
    }

    let upper_count = char_buffer.iter().filter(|c| c.is_uppercase).count();
    if upper_count == 0 {
        return text.to_string();
    }

    // Build list of case-bearing chars.
    //
    // Strip:
    //   • Lowercase tone-map keys (e.g. Telex 's','f','r','x','j' when lowercase):
    //     These are tone markers and carry no content case.  When the user types
    //     uppercase 'R' they mean the consonant R, not the hook tone key 'r'.
    //   • Non-alphabetic transform triggers (e.g. VNI digits '6','7','8','9'):
    //     These are pure trigger characters with no inherent case.
    let case_chars: Vec<&CharInfo> = char_buffer
        .iter()
        .filter(|c| {
            let is_lowercase_tone_key = !c.is_uppercase && opts.tone_map.contains_key(&c.ch);
            let is_non_alpha_trigger = opts.transform_trigger_chars.contains(&c.ch);
            !is_lowercase_tone_key && !is_non_alpha_trigger
        })
        .collect();

    if case_chars.is_empty() {
        return text.to_string();
    }

    // Fast path: all case-bearing chars are uppercase → full uppercase result.
    if case_chars.iter().all(|c| c.is_uppercase) {
        return text.to_uppercase();
    }

    // Per-output-char case application with doubling-collapse.
    //
    // Walk case_chars with a cursor. For each output char:
    //   - Check if the current and next case_char are the same character
    //     (doubling, e.g. 'D'+'D' or 'A'+'A'). If so, use the first char's
    //     case and advance the cursor by 2.
    //   - Otherwise, use the current char's case and advance by 1.
    //   - If we run off the end of case_chars, output lowercase.
    let output_chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut ci = 0usize; // index into case_chars

    for &out_ch in &output_chars {
        let is_upper = if ci < case_chars.len() {
            let cur = case_chars[ci];
            // Detect doubling: same char appears consecutively in case_chars.
            // This means two input chars merged into one output char.
            let is_doubling = ci + 1 < case_chars.len()
                && case_chars[ci + 1].ch == cur.ch
                && opts.transform_rules.contains_key(&format!("{0}{0}", cur.ch));
            if is_doubling {
                ci += 2; // consume both
            } else {
                ci += 1; // consume one
            }
            cur.is_uppercase
        } else {
            false
        };

        if is_upper {
            for uc in out_ch.to_uppercase() {
                result.push(uc);
            }
        } else {
            result.push(out_ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::config::PipelineConfig;
    use crate::pipeline::context::CharInfo;

    fn make_buf(s: &str) -> Vec<CharInfo> {
        s.chars().map(CharInfo::new).collect()
    }

    fn no_tone_opts() -> ComposeOpts {
        ComposeOpts::from_config(&PipelineConfig::new("test"))
    }

    fn telex_opts() -> ComposeOpts {
        ComposeOpts::from_config(&crate::pipeline::presets::telex_config())
    }

    #[test]
    fn lowercase_unchanged() {
        let buf = make_buf("aa");
        assert_eq!(apply_case_mask("â", &buf, &no_tone_opts()), "â");
    }

    #[test]
    fn first_upper_capitalizes_first_char() {
        // "Aa" — content chars: a(upper), a(lower) → NOT all-content-upper → first-cap
        let buf = vec![CharInfo::with_case('a', true), CharInfo::with_case('a', false)];
        assert_eq!(apply_case_mask("â", &buf, &no_tone_opts()), "Â");
    }

    #[test]
    fn all_caps_uppercases_result() {
        let buf = vec![CharInfo::with_case('a', true), CharInfo::with_case('a', true)];
        assert_eq!(apply_case_mask("â", &buf, &no_tone_opts()), "Â");
    }

    #[test]
    fn all_caps_multi_char() {
        let buf = vec![
            CharInfo::with_case('t', true),
            CharInfo::with_case('h', true),
            CharInfo::with_case('u', true),
        ];
        assert_eq!(apply_case_mask("thu", &buf, &no_tone_opts()), "THU");
    }

    #[test]
    fn mixed_case_per_char() {
        // T(upper), H(upper), u(lower): per-char alignment → TH uppercase, ư lowercase.
        // New algorithm: per-char with doubling-collapse; no doubling here → direct 1:1.
        let buf = vec![
            CharInfo::with_case('t', true),
            CharInfo::with_case('h', true),
            CharInfo::with_case('u', false),
        ];
        assert_eq!(apply_case_mask("thư", &buf, &no_tone_opts()), "THư");
    }

    #[test]
    fn mixed_case_title_only() {
        // T(upper), h(lower), u(lower) → only first char uppercase.
        let buf = vec![
            CharInfo::with_case('t', true),
            CharInfo::with_case('h', false),
            CharInfo::with_case('u', false),
        ];
        assert_eq!(apply_case_mask("thư", &buf, &no_tone_opts()), "Thư");
    }

    #[test]
    fn doubling_collapse_dd() {
        // DDa: D+D detected as doubling (transform_rules has "dd") → both D's consumed
        // for output[0]='Đ'; then 'a' at output[1] gets case_chars[2]=a(F) → lowercase.
        // Expected: Đa (not ĐA).
        let buf = vec![
            CharInfo::with_case('d', true),
            CharInfo::with_case('d', true),
            CharInfo::with_case('a', false),
        ];
        assert_eq!(apply_case_mask("đa", &buf, &telex_opts()), "Đa",
                   "DDa: DD collapse → Đ; a stays lowercase");
    }

    #[test]
    fn partial_caps_ng_prefix() {
        // NGuwowif → NGười: N+G are different chars (no doubling), so 1:1 → N(T)→N, G(T)→G.
        // Then remaining content chars u,w,o,w,i are lowercase → người → NGười.
        let buf = vec![
            CharInfo::with_case('n', true),
            CharInfo::with_case('g', true),
            CharInfo::with_case('u', false),
            CharInfo::with_case('w', false),
            CharInfo::with_case('o', false),
            CharInfo::with_case('w', false),
            CharInfo::with_case('i', false),
            CharInfo::with_case('f', false), // tone key — excluded from case_chars
        ];
        assert_eq!(apply_case_mask("người", &buf, &telex_opts()), "NGười",
                   "NGuwowif: N+G prefix uppercase preserved");
    }

    #[test]
    fn tone_key_not_counted_as_content() {
        // NGUOI + f (tone key) — content chars N,G,U,O,I are all upper → ALL-CAPS
        let buf = vec![
            CharInfo::with_case('n', true),
            CharInfo::with_case('g', true),
            CharInfo::with_case('u', true),
            CharInfo::with_case('o', true),
            CharInfo::with_case('i', true),
            CharInfo::with_case('f', false), // tone key
        ];
        assert_eq!(apply_case_mask("người", &buf, &telex_opts()), "NGƯỜI");
    }
}
