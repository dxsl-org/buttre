//! Fallback step — undo / toggle / English-fallback derived from raw key counts.
//!
//! ## Design: stateless from raw keys
//!
//! All decisions are derived by counting repeated keys in `raw`.  No history
//! flags, no transform records.
//!
//! ## Rules (mirroring stage4/stage8 behaviour)
//!
//! ### Transform undo / toggle
//!
//! A transform mark (`aa→â`, `aw→ă`, `dd→đ`, etc.) is undone when the trigger
//! key is typed **one extra time** beyond what the transform consumes.
//!
//! Pattern: the *last* 2-char rule in the buffer was applied (2 same keys → 1
//! result), and now a *third* identical key arrives.
//!
//! Rule: if the last two identical-and-transform-related chars in raw are
//! followed immediately by a third identical char → output the two literal keys
//! and set `temp_english = true`.
//!
//! Example: `aaa` → "aa" (literal), `aww` → "aw" (literal).
//!
//! ### Tone undo / toggle
//!
//! A tone key typed twice removes the tone and yields a literal tone key suffix.
//! Example: `ass` → "as" (literal), `a11` → "a1" (raw undo, current engine).
//!
//! **Partial undo (transform-preserving):** when the base before the tone key
//! contains transform triggers (e.g. `a611`: `a6` → `â`, then `11` undo pair),
//! the undo strips ONLY the tone and keeps the diacritic transform.  The
//! transformed vowel is recomputed by running the segment + transform steps on
//! the base portion without any tone.
//!
//!   - `a611` → `â1`  (â preserved, tone removed, literal `1` appended)
//!   - `a822` → `ă2`  (ă preserved)
//!   - `u733` → `ư3`  (ư preserved)
//!   - `o744` → `ơ4`  (ơ preserved)
//!   - `u7o711` → `ươ1`  (compound transform preserved)
//!
//! ### Same-tone repress (coda-interleaved undo)
//!
//! When the user presses a tone key that is already applied to the current
//! syllable (even with a coda consonant between the original tone key and the
//! re-press), this is an undo: strip the tone from the composed result and
//! append the literal tone key.  Matches Unikey `tempVietOff` behaviour.
//!
//!   - `vie65t5` → `viêt5`  (strip dot tone from việt, keep ê transform, literal 5)
//!
//! ### Multi-level toggle (Unikey standard)
//!
//! After a tone-undo pair, `temp_english` mode engages and subsequent same-key
//! taps are literal (no re-apply).  This matches Unikey behaviour — it is NOT
//! a missing feature, NOT a bug.  See: `a111` → `a11`, `a222` → `a22`.
//!
//! ### English fallback
//!
//! After an undo resolves to a literal key sequence with no valid Vietnamese
//! transforms remaining, `temp_english = true` signals the executor to pass
//! subsequent input through (Phase 4 only — this module just sets the flag).

use super::ComposeOpts;
use super::segment;
use super::transform;
use crate::tone;
use crate::pipeline::config::ToneMark;

// ── Output ────────────────────────────────────────────────────────────────────

/// Result of the fallback check.
#[derive(Debug, Clone)]
pub struct FallbackResult {
    /// True when this module has fully handled the input (caller should not
    /// proceed to segment/transform/assemble).
    pub is_handled: bool,
    /// The resulting text (only meaningful when `is_handled = true`).
    pub text: String,
    /// Whether to set temp_english mode.
    pub temp_english: bool,
}

impl FallbackResult {
    fn not_handled() -> Self {
        Self { is_handled: false, text: String::new(), temp_english: false }
    }

    fn handled(text: impl Into<String>, temp_english: bool) -> Self {
        Self { is_handled: true, text: text.into(), temp_english }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Check whether the raw key sequence triggers an undo/toggle pattern.
///
/// Returns `FallbackResult::not_handled()` when normal compose should proceed.
pub fn check_fallback(raw: &[char], opts: &ComposeOpts) -> FallbackResult {
    // We only act when there are at least 2 keys (minimum for a toggle).
    if raw.len() < 2 {
        return FallbackResult::not_handled();
    }

    // Check tone toggle first (e.g. "a11", "a111", "a1111").
    if let Some(result) = check_tone_toggle(raw, opts) {
        return result;
    }

    // Check transform toggle (e.g. "aaa", "aww", "awww").
    if let Some(result) = check_transform_toggle(raw, opts) {
        return result;
    }

    FallbackResult::not_handled()
}

// ── Tone toggle ───────────────────────────────────────────────────────────────

/// Detect patterns like "as", "ass", "a11", "a111", …  and also the
/// "same-tone repress after coda" pattern like "vie65t5".
///
/// ## Contiguous-suffix pattern (a11, a611, u7o711, seess, fanss)
///
/// Counts the **trailing** identical tone keys at the END of raw (not from the
/// first tone key occurrence).  This is critical for Telex words like "seess"
/// or "fanss" where the same letter is both a leading consonant and a tone key:
/// trailing_count("seess", 's') = 2, base_part = ['s','e','e'] → "sês".
///
/// When trailing_count is even → undo fires: apply transforms to base_part
/// (no tone), then append `n/2` literal tone key chars.
///
/// ## Same-tone repress after coda (vie65t5)
///
/// When trailing_count == 1 and the same tone key appeared earlier in raw
/// (with non-tone chars in between), compose raw-without-last to see if it
/// produced a vowel with the same tone mark.  If so, strip that tone and
/// append the literal key.
fn check_tone_toggle(raw: &[char], opts: &ComposeOpts) -> Option<FallbackResult> {
    let last = *raw.last()?;
    let last_lc = last.to_ascii_lowercase();

    // Only proceed if the last character is a tone key.
    if !opts.tone_map.contains_key(&last_lc) {
        return None;
    }

    // ── Path 1: trailing contiguous run of identical tone keys ────────────────
    // Count from the END, not from the first occurrence.  This correctly handles
    // words like "seess" (Telex 's' is tone key *and* leading consonant) where
    // the first-occurrence approach would find index 0 and fail to detect the
    // trailing "ss" undo pair.
    let trailing_count = raw.iter().rev()
        .take_while(|&&c| c.to_ascii_lowercase() == last_lc)
        .count();

    if trailing_count >= 2 {
        let n = trailing_count;
        let base_part = &raw[..raw.len() - n];

        if n % 2 == 0 {
            // Even count → undo: apply transforms to base_part (no tone) to
            // preserve diacritics, then append n/2 literal tone key chars.
            let transformed_base = apply_transforms_only(base_part, opts);
            let suffix: String = std::iter::repeat(last_lc).take(n / 2).collect();
            let text = format!("{transformed_base}{suffix}");
            return Some(FallbackResult::handled(text, true));
        }
        // Odd n >= 3: let normal compose handle (applies tone from last key).
        return None;
    }

    // trailing_count == 1: fall through to Path 2.

    // ── Path 2: same-tone repress after coda consonant(s) ────────────────────
    // Pattern: last char is tone key `tk` and the same key appeared earlier in
    // raw with non-tone chars in between (e.g. "vie65t5").
    let raw_without_last = &raw[..raw.len() - 1];
    if raw_without_last.is_empty() {
        return None;
    }

    // The raw-without-last must contain the same tone key somewhere earlier.
    let same_tone_earlier = raw_without_last.iter()
        .any(|&c| c.to_ascii_lowercase() == last_lc);
    if !same_tone_earlier {
        return None;
    }

    // trailing_count == 1 already guarantees second_last != last_lc, but keep
    // the check to make the invariant explicit and guard against future changes.
    let second_last = raw.get(raw.len() - 2).copied()?;
    if second_last.to_ascii_lowercase() == last_lc {
        return None;
    }

    // Compose raw-without-last to get the candidate toned syllable.
    let candidate = compose_base_and_transforms_with_tone(raw_without_last, opts)?;

    let expected_tone = *opts.tone_map.get(&last_lc)?;
    if expected_tone == ToneMark::None {
        return None;
    }

    let stripped = strip_tone_from_text(&candidate, expected_tone)?;
    let text = format!("{stripped}{last_lc}");
    Some(FallbackResult::handled(text, true))
}

// ── Transform toggle ──────────────────────────────────────────────────────────

/// Detect patterns like "aaa" (aa→â, third 'a' → undo), "aww", "dddd".
///
/// ## Transform-preserving prefix re-composition
///
/// When the undo triple `[rc1, rc2, rc2]` is at the tail but there is a prefix
/// before it (e.g. `ddaaa` → prefix `dd`, undo cluster `aaa`), the prefix must
/// be re-composed through segment + transform so that any completed transforms
/// there (`dd`→`đ`, an earlier `ee`→`ê`, etc.) survive.  Only the specifically
/// undone cluster reverts to its literal keys.
///
/// This is the same orthogonal-transform principle applied by the tone-undo path:
/// undoing ONE transform must NEVER revert unrelated earlier transforms.
fn check_transform_toggle(raw: &[char], opts: &ComposeOpts) -> Option<FallbackResult> {
    if raw.len() < 3 {
        return None;
    }

    // Detect the pattern: a 2-char transform rule key appears, then the second
    // char of that rule is repeated once more (the undo key).
    // Example: "aaa" — rule "aa"→"â", third 'a' is undo.
    // Example: "aww" — rule "aw"→"ă", second 'w' is undo.

    // Find any 2-char transform rule whose trigger chars appear consecutively
    // in raw, followed by the same second char once more.
    for rule_key in opts.transform_rules.keys() {
        let rk: Vec<char> = rule_key.to_lowercase().chars().collect();
        if rk.len() != 2 {
            continue;
        }
        let (rc1, rc2) = (rk[0], rk[1]);

        // Look for the pattern [..., rc1, rc2, rc2] anywhere in raw.
        // After the triple, nothing else may appear (the triple is at the end).
        let n = raw.len();
        if n < 3 {
            continue;
        }
        let tail = &raw[n - 3..];
        let t = [
            tail[0].to_ascii_lowercase(),
            tail[1].to_ascii_lowercase(),
            tail[2].to_ascii_lowercase(),
        ];
        if t[0] == rc1 && t[1] == rc2 && t[2] == rc2 {
            // The last 3 chars match the undo pattern.
            // Re-compose the prefix so earlier completed transforms (e.g. "dd"→"đ")
            // are preserved.  Only the undone cluster reverts to literal rc1+rc2.
            let prefix_raw = &raw[..n - 3];
            let composed_prefix = apply_transforms_only(prefix_raw, opts);
            let text = format!("{composed_prefix}{rc1}{rc2}");
            return Some(FallbackResult::handled(text, true));
        }
    }

    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Apply segment + transform steps to `raw` (no tone application).
///
/// Used by the tone-undo path to reconstruct the transformed base vowels
/// (e.g. `[a, 6]` → `"â"`) before appending the literal tone key suffix.
///
/// Any tone keys that `segment()` collects from `raw` are intentionally
/// discarded — when this function is called, the caller has already decided
/// that all tone marks in `raw` should be stripped (the undo has fired).
fn apply_transforms_only(raw: &[char], opts: &ComposeOpts) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let seg = segment::segment(raw, opts);
    transform::apply_transforms(&seg.base, &seg.transforms, opts)
}

/// Run segment + transform + tone on `raw` and return the result text.
///
/// Returns `None` if `raw` is empty or composition produces no output.
/// Used by the same-tone-repress path to evaluate `raw[..-1]`.
fn compose_base_and_transforms_with_tone(raw: &[char], opts: &ComposeOpts) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let seg = segment::segment(raw, opts);
    let transformed = transform::apply_transforms(&seg.base, &seg.transforms, opts);
    if seg.tones.is_empty() {
        return Some(transformed);
    }
    // Apply the last tone key (mirrors the compose() main path).
    let last_tone_key = *seg.tones.last().unwrap();
    use super::assemble;
    assemble::apply_tone(&transformed, last_tone_key, opts)
        .or(Some(transformed))
}

/// Strip the first vowel carrying `expected_tone` in `text` (walk chars left
/// to right, apply `ToneMark::None` to strip).
///
/// Returns `None` when no vowel with that tone is found (prevents false undo).
fn strip_tone_from_text(text: &str, expected_tone: ToneMark) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        let (_base, found_tone) = tone::strip(ch);
        if found_tone == expected_tone {
            // Strip the tone from this vowel.
            let stripped_char = tone::apply(ch, ToneMark::None);
            let mut result: Vec<char> = chars.clone();
            result[i] = stripped_char;
            return Some(result.into_iter().collect());
        }
    }
    None
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::ComposeOpts;
    use crate::pipeline::config::{PipelineConfig, ToneMark};

    fn telex_opts() -> ComposeOpts {
        let mut cfg = PipelineConfig::new("telex");
        cfg.add_transform("aa", "â");
        cfg.add_transform("aw", "ă");
        cfg.add_transform("ow", "ơ");
        cfg.add_transform("uw", "ư");
        cfg.add_transform("dd", "đ");
        cfg.add_tone('s', ToneMark::Acute);
        cfg.add_tone('f', ToneMark::Grave);
        cfg.add_tone('1', ToneMark::Acute); // VNI-style for test
        ComposeOpts::from_config(&cfg)
    }

    fn vni_opts() -> ComposeOpts {
        let mut cfg = PipelineConfig::new("vni");
        cfg.add_transform("a6", "â");
        cfg.add_transform("a8", "ă");
        cfg.add_transform("e6", "ê");
        cfg.add_transform("o6", "ô");
        cfg.add_transform("o7", "ơ");
        cfg.add_transform("u7", "ư");
        cfg.add_transform("d9", "đ");
        cfg.add_tone('1', ToneMark::Acute);
        cfg.add_tone('2', ToneMark::Grave);
        cfg.add_tone('3', ToneMark::Hook);
        cfg.add_tone('4', ToneMark::Tilde);
        cfg.add_tone('5', ToneMark::Dot);
        ComposeOpts::from_config(&cfg)
    }

    #[test]
    fn aaa_triggers_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "aaa".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "aaa should trigger undo");
        assert_eq!(result.text, "aa");
        assert!(result.temp_english);
    }

    #[test]
    fn aww_triggers_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "aww".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "aww should trigger undo");
        assert_eq!(result.text, "aw");
    }

    #[test]
    fn ass_triggers_tone_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "ass".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "ass should trigger tone undo");
        assert_eq!(result.text, "as");
        assert!(result.temp_english);
    }

    #[test]
    fn as_does_not_trigger() {
        let opts = telex_opts();
        let raw: Vec<char> = "as".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(!result.is_handled);
    }

    #[test]
    fn ddd_triggers_dd_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "ddd".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "ddd should trigger undo");
        assert_eq!(result.text, "dd");
    }

    #[test]
    fn a11_triggers_tone_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "a11".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "a11 should trigger tone undo");
        assert_eq!(result.text, "a1");
    }

    // ── Regression guards: transform-preserving tone undo ─────────────────────

    #[test]
    fn vni_a611_keep_circumflex() {
        // a6 → â, then 11 undo pair → strip tone, keep â. Output: â1.
        let opts = vni_opts();
        let raw: Vec<char> = "a611".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "a611 should trigger transform-preserving undo");
        assert_eq!(result.text, "â1", "a611: should keep â and strip tone to give â1");
    }

    #[test]
    fn vni_a822_keep_breve() {
        // a8 → ă, then 22 undo pair → ă2.
        let opts = vni_opts();
        let raw: Vec<char> = "a822".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "a822 should trigger transform-preserving undo");
        assert_eq!(result.text, "ă2", "a822: should keep ă and strip tone to give ă2");
    }

    #[test]
    fn vni_u733_keep_horn() {
        // u7 → ư, then 33 undo pair → ư3.
        let opts = vni_opts();
        let raw: Vec<char> = "u733".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "u733 should trigger transform-preserving undo");
        assert_eq!(result.text, "ư3", "u733: should keep ư and strip tone to give ư3");
    }

    #[test]
    fn vni_o744_keep_horn() {
        // o7 → ơ, then 44 undo pair → ơ4.
        let opts = vni_opts();
        let raw: Vec<char> = "o744".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "o744 should trigger transform-preserving undo");
        assert_eq!(result.text, "ơ4", "o744: should keep ơ and strip tone to give ơ4");
    }

    #[test]
    fn vni_u7o711_keep_compound_horn() {
        // u7o7 → ươ (compound), then 11 undo pair → ươ1.
        let opts = vni_opts();
        let raw: Vec<char> = "u7o711".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "u7o711 should trigger transform-preserving undo");
        assert_eq!(result.text, "ươ1", "u7o711: should keep ươ and strip tone to give ươ1");
    }

    // ── Multi-level toggle stays Unikey-standard (no re-apply) ────────────────

    #[test]
    fn vni_a111_is_a11_not_reapply() {
        // Matches Unikey: after tone-undo pair (11), temp_english_mode engages.
        // Third `1` is a literal append. Result: a11 (NOT á1).
        // This is intentional standard behaviour, not a missing feature.
        let opts = vni_opts();
        let raw: Vec<char> = "a111".chars().collect();
        let result = check_fallback(&raw, &opts);
        // Odd n=3: fallback returns not_handled; compose handles it as tone application
        // on the base, which then gives a11 via temp_english in PipelineExecutor.
        // The unit-level fallback check for n=3 returns None (let compose handle).
        assert!(!result.is_handled, "a111 odd count: fallback defers to compose (which gives a11)");
    }

    #[test]
    fn vni_a66_fires_transform_undo() {
        // a66: undo pair → output "a6", temp_english=true.
        // The executor then passes the third `6` as literal (→ "a66" in end-to-end).
        // Matches Unikey: no re-apply after undo.
        let opts = vni_opts();
        let raw: Vec<char> = "a66".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "a66 should fire transform undo");
        assert_eq!(result.text, "a6", "a66 → a6 + temp_english (Unikey standard)");
    }

    // ── Regression guards: transform-preserving transform undo ───────────────
    // Undoing one transform cluster must NOT revert unrelated earlier transforms
    // in the prefix.  Matches all four reference IMEs.

    #[test]
    fn telex_ddaaa_preserves_dstroke() {
        // dd→đ (prefix), then aaa undo: â reverts to aa, đ survives. Output: đaa.
        let opts = telex_opts();
        let raw: Vec<char> = "ddaaa".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "ddaaa should trigger transform undo");
        assert_eq!(result.text, "đaa",
            "ddaaa → đaa: dd prefix re-composed to đ, aa literal (transform-preserving undo)");
        assert!(result.temp_english);
    }

    #[test]
    fn telex_ddeee_preserves_dstroke() {
        // dd→đ (prefix), then eee undo (ee→ê, third e reverts): ê→ee, đ survives. Output: đee.
        let opts = telex_opts_with_ee();
        let raw: Vec<char> = "ddeee".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "ddeee should trigger transform undo");
        assert_eq!(result.text, "đee",
            "ddeee → đee: dd prefix re-composed to đ, ee literal (transform-preserving undo)");
        assert!(result.temp_english);
    }

    #[test]
    fn telex_ddooo_preserves_dstroke() {
        // dd→đ (prefix), then ooo undo (oo→ô, third o reverts): ô→oo, đ survives. Output: đoo.
        let opts = telex_opts_with_oo();
        let raw: Vec<char> = "ddooo".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "ddooo should trigger transform undo");
        assert_eq!(result.text, "đoo",
            "ddooo → đoo: dd prefix re-composed to đ, oo literal (transform-preserving undo)");
        assert!(result.temp_english);
    }

    fn telex_opts_with_ee() -> ComposeOpts {
        let mut cfg = PipelineConfig::new("telex");
        cfg.add_transform("aa", "â");
        cfg.add_transform("aw", "ă");
        cfg.add_transform("ee", "ê");
        cfg.add_transform("oo", "ô");
        cfg.add_transform("ow", "ơ");
        cfg.add_transform("uw", "ư");
        cfg.add_transform("dd", "đ");
        cfg.add_tone('s', ToneMark::Acute);
        cfg.add_tone('f', ToneMark::Grave);
        cfg.add_tone('1', ToneMark::Acute);
        ComposeOpts::from_config(&cfg)
    }

    fn telex_opts_with_oo() -> ComposeOpts {
        telex_opts_with_ee() // same config includes oo
    }

    // ── Regression guards: leading-consonant == tone-key (the trailing-run fix) ─
    // Words where the same letter is both a leading consonant and a Telex tone key.
    // The old first-occurrence algorithm found index 0 and failed to detect the
    // trailing "ss"/"ff" undo pair.  These tests guard the trailing-count fix.

    #[test]
    fn telex_fanss_triggers_tone_undo() {
        // "fans" typed with trailing "ss": 'f' is Telex Grave key *and* consonant.
        // trailing run of 's' = 2, base_part = ['f','a','n'] → "fan", text = "fans".
        let opts = telex_opts();
        let raw: Vec<char> = "fanss".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "fanss should trigger tone undo (trailing-run fix)");
        assert_eq!(result.text, "fans", "fanss → fans + temp_english");
        assert!(result.temp_english);
    }

    #[test]
    fn telex_seess_triggers_tone_undo_with_ee_transform() {
        // "see" (ee→ê transform) typed with trailing "ss": 's' is Telex Acute key *and* consonant.
        // trailing run of 's' = 2, base_part = ['s','e','e'] → "sê" (ee transform applied), text = "sês".
        let opts = telex_opts_with_ee();
        let raw: Vec<char> = "seess".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "seess should trigger tone undo (trailing-run fix)");
        assert_eq!(result.text, "sês", "seess → sês + temp_english (ee transform preserved)");
        assert!(result.temp_english);
    }

    #[test]
    fn telex_sass_triggers_tone_undo() {
        // Simple case: 's' as leading consonant, 'a' vowel, then "ss" undo pair.
        let opts = telex_opts();
        let raw: Vec<char> = "sass".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "sass should trigger tone undo");
        assert_eq!(result.text, "sas");
        assert!(result.temp_english);
    }

    #[test]
    fn telex_sinff_triggers_tone_undo() {
        // "sin" + "ff" undo pair: 'f' before vowel stays literal, trailing "ff" undoes the tone.
        // base_part = ['s','i','n'] → "sin", text = "sinf".
        let opts = telex_opts();
        let raw: Vec<char> = "sinff".chars().collect();
        let result = check_fallback(&raw, &opts);
        assert!(result.is_handled, "sinff should trigger tone undo (trailing-run fix)");
        assert_eq!(result.text, "sinf", "sinff → sinf + temp_english");
        assert!(result.temp_english);
    }
}
