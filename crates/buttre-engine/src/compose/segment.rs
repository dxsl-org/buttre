//! Segment step — split raw key buffer into (base, transform marks, tone marks).
//!
//! ## Two modes
//!
//! ### `MarkBased` (Telex / VNI / VIQR / …)
//!
//! Port of `PermutationStage::extract_base_and_marks` (stage6, lines 111-196).
//! Context-aware: r/s/x/j are only treated as tone keys **after** a vowel.
//! Adjacent double-letter detection: `aa`→`â`, `ee`→`ê`, `oo`→`ô`, `dd`→`đ`.
//! A guard prevents false triggers in English words where the same vowel letter
//! appears on both sides of a consonant (e.g. "fallbaack", "implemeent").
//!
//! ### `DirectMap` (Cham, Khmer, …)
//!
//! Every key is a base key; double-key digraphs are resolved via the transform
//! table (e.g. "kk" → "ꩀ"). No mark extraction at all.

use std::collections::HashMap;
use crate::vowel::cluster::is_vowel;
use super::ComposeOpts;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Segmentation mode — chosen per config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentMode {
    /// Telex/VNI/VIQR: extract base + transform marks + tone marks.
    MarkBased,
    /// Cham/Khmer: every key maps to a glyph; double-key via transform table.
    DirectMap,
}

/// A single transform mark with context about the base position it was typed at.
#[derive(Debug, Clone)]
pub struct TransformMark {
    /// The raw key pressed.
    pub key: char,
    /// Number of base chars that had been typed BEFORE this mark key.
    /// Used by `transform::apply_transforms` to find the right-most vowel
    /// in `base[..base_pos_at_typing]` for the mark to apply to.
    pub base_len_at_typing: usize,
}

/// Output of the segment step.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Base string (consonants + vowels without marks).
    pub base: String,
    /// Transform marks in typing order, with positional context.
    pub transforms: Vec<TransformMark>,
    /// Tone keys in order (only the last one is used by assemble).
    pub tones: Vec<char>,
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Split raw keys into base + transform marks + tone marks.
pub fn segment(raw: &[char], opts: &ComposeOpts) -> Segment {
    match opts.segment_mode {
        SegmentMode::MarkBased  => segment_mark_based(raw, opts),
        SegmentMode::DirectMap  => segment_direct_map(raw, opts),
    }
}

// ── MarkBased ─────────────────────────────────────────────────────────────────

fn segment_mark_based(raw: &[char], opts: &ComposeOpts) -> Segment {
    let mut base = String::new();
    let mut transforms: Vec<TransformMark> = Vec::new();
    let mut tones: Vec<char> = Vec::new();
    let mut has_seen_vowel = false;

    // Pre-scan: count each potentially-doubling char (a/e/o/d).
    // Non-adjacent flexible typing fires ONLY when count == 2 — meaning the
    // raw buffer has exactly one base char + one transform mark intended.
    // Three or more occurrences (e.g. "implemeent" has 3 'e') indicate an
    // English word with accidental repeats, not a Vietnamese transform intent.
    let mut double_candidates: HashMap<char, usize> = HashMap::new();
    for &ch in raw {
        let lc = ch.to_ascii_lowercase();
        if matches!(lc, 'a' | 'e' | 'o' | 'd') {
            *double_candidates.entry(lc).or_insert(0) += 1;
        }
    }
    let mut vowel_in_base: HashMap<char, bool> = HashMap::new();

    for &ch in raw {
        let lc = ch.to_ascii_lowercase();

        // Track vowel presence (for ambiguous-consonant gating).
        if is_vowel(lc) {
            has_seen_vowel = true;
        }

        // ── Adjacent double-letter transform (Telex: aa/ee/oo/dd) ──────────
        // Fires when the current key equals the last base char and is in the
        // doubling set.  A guard prevents false triggers in English words where
        // the same vowel appears on both sides of a consonant boundary
        // (e.g. "fallbaack": earlier 'a' at pos 1, consonants "llb" before the
        // adjacent "aa"; "implemeent": earlier 'e' at pos 4, 'm' before "ee").
        if !base.is_empty() {
            let last_base_lc = base.chars().last().unwrap().to_ascii_lowercase();
            if last_base_lc == lc && matches!(lc, 'a' | 'e' | 'o' | 'd') {
                if !has_earlier_vowel_with_consonants(&base, lc) {
                    transforms.push(TransformMark { key: ch, base_len_at_typing: base.chars().count() });
                    continue;
                }
                // Guard fired — same vowel already exists with consonants between;
                // fall through to treat this key as a literal base character.
            }
        }

        // ── Non-adjacent double (flexible typing: "vietej" → "việt") ───────
        // The repeated vowel refers back to the nucleus of an already-complete
        // syllable.  This is only legitimate when the earlier part really IS one
        // complete Vietnamese syllable, which requires BOTH:
        //   1. exactly one contiguous vowel group (one nucleus) — rejects
        //      "implem" ('i' … 'e' = two groups, an English word); AND
        //   2. the consonants after the rightmost matching vowel form a VALID
        //      Vietnamese coda — rejects "fallb" (coda "llb" is invalid, so
        //      "fallback" stays literal instead of becoming "fâllback").
        // For "viet": one group + coda "t" (valid) → fires → "việt".
        // count != 2 also disables non-adjacent (English word with repeats).
        if matches!(lc, 'a' | 'e' | 'o' | 'd') {
            let count = double_candidates.get(&lc).copied().unwrap_or(0);
            if count == 2
                && *vowel_in_base.get(&lc).unwrap_or(&false)
                && count_vowel_groups(&base) <= 1
                && coda_after_last_vowel_is_valid(&base, lc)
            {
                transforms.push(TransformMark { key: ch, base_len_at_typing: base.chars().count() });
                continue;
            }
        }

        // ── Classify mark keys ─────────────────────────────────────────────
        // A key is a *standalone* transform key when it exclusively acts as a
        // modifier and never as a base letter — e.g. Telex 'w', VNI digits.
        // Keys like 'a', 'e', 'o', 'd' are base letters first; they become
        // transform marks only via the double/non-adjacent detection above.
        let is_standalone_transform = is_standalone_transform_key(ch, opts);
        let is_tone_key_char = opts.tone_map.contains_key(&lc);

        // Ambiguous consonants (r/s/x/j) are both valid initial consonants and
        // tone keys in Telex.  The non-ambiguous tone keys (f, z in Telex;
        // 1-5 in VNI) are never consonants, but they still have nothing to
        // act on when no vowel has been seen yet — a leading tone key has no
        // nucleus, so it must remain literal in the base rather than be
        // collected as a tone mark.
        //
        // Rule: a tone key occurrence is only collected as a tone mark when at
        // least one vowel precedes it in the raw sequence.  Otherwise it falls
        // through to the literal base path.  This unifies the guard for
        // ambiguous consonants and non-ambiguous-but-leading tone keys (e.g.
        // leading 'f' in "fan", leading 'j' in "jin").
        //
        // Standalone transform keys (e.g. 'w') do NOT need this guard because
        // they carry diacritic intent independently of vowel position
        // (e.g. "win" → 'w' transforms the implicit nucleus, giving "ưin").

        if is_standalone_transform {
            // Record base length at time of this mark so transform can pick the right vowel.
            transforms.push(TransformMark { key: ch, base_len_at_typing: base.chars().count() });
        } else if is_tone_key_char {
            if !has_seen_vowel {
                // No vowel yet — this tone key has no nucleus to act on; treat as literal.
                base.push(ch);
                if matches!(lc, 'a' | 'e' | 'o' | 'd') {
                    vowel_in_base.insert(lc, true);
                }
            } else {
                tones.push(ch);
            }
        } else {
            base.push(ch);
            if matches!(lc, 'a' | 'e' | 'o' | 'd') {
                vowel_in_base.insert(lc, true);
            }
        }
    }

    Segment { base, transforms, tones }
}

// ── DirectMap ─────────────────────────────────────────────────────────────────

/// DirectMap: resolve each key (or double-key) through the transform table.
/// The result is a fully-assembled base string; no separate mark extraction.
fn segment_direct_map(raw: &[char], opts: &ComposeOpts) -> Segment {
    let mut base = String::new();
    let rules = &opts.transform_rules;

    let mut i = 0;
    while i < raw.len() {
        let ch = raw[i];
        // Try double-key first (e.g. "kk").
        if i + 1 < raw.len() {
            let pair: String = [ch, raw[i + 1]].iter().collect();
            if let Some(result) = rules.get(&pair) {
                base.push_str(result);
                i += 2;
                continue;
            }
        }
        // Single-key lookup.
        let single = ch.to_string();
        if let Some(result) = rules.get(&single) {
            base.push_str(result);
        } else {
            // Pass through unmapped characters as-is.
            base.push(ch);
        }
        i += 1;
    }

    // DirectMap never produces separate marks.
    Segment { base, transforms: Vec::new(), tones: Vec::new() }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` when `base` already contains an earlier occurrence of `vowel`
/// that is separated from the last character of `base` by at least one consonant.
///
/// This guards the adjacent-double transform against English words like
/// "fallbaack" (earlier 'a' at pos 1, consonants "llb" before the adjacent "aa")
/// or "implemeent" (earlier 'e' at pos 4, consonant 'm' before the adjacent "ee").
fn has_earlier_vowel_with_consonants(base: &str, vowel: char) -> bool {
    let chars: Vec<char> = base.chars().collect();
    let last_idx = match chars.len().checked_sub(1) {
        Some(i) => i,
        None => return false,
    };
    // For each earlier position with the same vowel, check if there is a
    // consonant between that position and the last position.
    chars[..last_idx].iter().enumerate().any(|(i, &c)| {
        c.to_ascii_lowercase() == vowel
            && chars[i + 1..last_idx]
                .iter()
                .any(|&x| !is_vowel(x.to_ascii_lowercase()))
    })
}

/// True when the consonants after the rightmost occurrence of `vowel` in `base`
/// form a valid Vietnamese coda (or are empty).
///
/// The non-adjacent transform targets that rightmost vowel; for the earlier
/// portion to be a complete syllable, its tail must be a legal coda.
/// "viet" → tail after 'e' is "t" (valid). "fallb" → tail after 'a' is "llb"
/// (invalid → not a syllable → keep "fallback" literal).
fn coda_after_last_vowel_is_valid(base: &str, vowel: char) -> bool {
    let chars: Vec<char> = base.chars().collect();
    let Some(pos) = chars.iter().rposition(|&c| c.to_ascii_lowercase() == vowel) else {
        return false;
    };
    let tail: String = chars[pos + 1..].iter().collect::<String>().to_ascii_lowercase();
    // Valid Vietnamese codas (single + 2-char); empty = open syllable.
    matches!(
        tail.as_str(),
        "" | "c" | "m" | "n" | "p" | "t" | "ch" | "ng" | "nh"
    )
}

/// Count maximal runs of consecutive vowels in `s`.
///
/// A valid Vietnamese syllable has exactly one vowel nucleus (one group).
/// More than one group means the base spans a consonant-separated vowel
/// boundary — not a single syllable.
fn count_vowel_groups(s: &str) -> usize {
    let mut groups = 0;
    let mut in_vowel = false;
    for c in s.chars() {
        if is_vowel(c.to_ascii_lowercase()) {
            if !in_vowel {
                groups += 1;
                in_vowel = true;
            }
        } else {
            in_vowel = false;
        }
    }
    groups
}

/// A key is a *standalone* transform key when it:
/// 1. Is NOT a tone key, AND
/// 2. Appears as the second (modifier) character of 2-char rules, OR as the
///    sole character of a 1-char rule (e.g. Telex "w"→"ư" for prefix use).
///    It must never be a vowel, a consonant in base position, or 'd'.
///
/// This distinguishes 'w' (modifier in "aw"/"ow"/"uw" or standalone "w"→"ư")
/// from 'a' (base letter first, reaches transform only via double-detection).
///
/// VNI digits (6/7/8/9) are also standalone transform keys because they are
/// not Vietnamese letters.
fn is_standalone_transform_key(ch: char, opts: &ComposeOpts) -> bool {
    let lc = ch.to_ascii_lowercase();

    // Tone keys are not transform keys.
    if opts.tone_map.contains_key(&lc) {
        return false;
    }

    // ASCII letters that are vowels are never standalone transform keys —
    // they reach transform role only via the double-detection path above.
    if is_vowel(lc) {
        return false;
    }

    // 'd' is a consonant/vowel in Vietnamese — not standalone.
    if lc == 'd' {
        return false;
    }

    // Check both:
    // a) 2-char rules where this char is the second (modifier) char.
    // b) 1-char rules where this char is the sole key (e.g. "w"→"ư").
    opts.transform_rules.keys().any(|k| {
        let kl: String = k.to_lowercase();
        (kl.len() == 2 && kl.ends_with(lc))
            || (kl.len() == 1 && kl.chars().next() == Some(lc))
    })
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::ComposeOpts;
    use crate::pipeline::config::{PipelineConfig, ToneMark, ToneStyle};

    fn telex_opts() -> ComposeOpts {
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
        cfg.add_tone('r', ToneMark::Hook);
        cfg.add_tone('x', ToneMark::Tilde);
        cfg.add_tone('j', ToneMark::Dot);
        ComposeOpts::from_config(&cfg)
    }

    fn transform_keys(seg: &Segment) -> Vec<char> {
        seg.transforms.iter().map(|t| t.key).collect()
    }

    #[test]
    fn basic_tone_key_after_vowel() {
        let opts = telex_opts();
        let raw: Vec<char> = "as".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(seg.base, "a");
        assert!(seg.transforms.is_empty());
        assert_eq!(seg.tones, vec!['s']);
    }

    #[test]
    fn s_as_initial_consonant() {
        let opts = telex_opts();
        // "sinh" — 's' before vowel is a consonant
        let raw: Vec<char> = "sinh".chars().collect();
        let seg = segment(&raw, &opts);
        // 's' before vowel → base
        assert!(seg.base.contains('s'));
        assert!(seg.tones.is_empty());
    }

    // ── Positional tone-key guard (leading tone = literal) ────────────────────

    #[test]
    fn f_before_vowel_is_literal_not_tone() {
        // "fan": 'f' is a tone key (grave) but no vowel precedes it → literal base.
        // Segment must place 'f' in base, NOT in tones.
        let opts = telex_opts();
        let raw: Vec<char> = "fan".chars().collect();
        let seg = segment(&raw, &opts);
        assert!(seg.tones.is_empty(), "leading 'f' must not be collected as tone: {:?}", seg.tones);
        assert!(seg.base.starts_with('f'), "leading 'f' must be in base: '{}'", seg.base);
    }

    #[test]
    fn f_after_vowel_is_tone() {
        // "af": 'a' is vowel first → 'f' is a tone (grave).
        let opts = telex_opts();
        let raw: Vec<char> = "af".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(seg.tones, vec!['f'], "post-vowel 'f' must be tone");
        assert_eq!(seg.base, "a");
    }

    #[test]
    fn j_before_vowel_is_literal() {
        // "jin": 'j' is a tone key (dot-below) but leads the syllable → literal.
        let opts = telex_opts();
        let raw: Vec<char> = "jin".chars().collect();
        let seg = segment(&raw, &opts);
        assert!(seg.tones.is_empty(), "leading 'j' must not be collected as tone: {:?}", seg.tones);
        assert!(seg.base.starts_with('j'));
    }

    #[test]
    fn adjacent_double_transform() {
        let opts = telex_opts();
        let raw: Vec<char> = "aa".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(seg.base, "a");
        assert_eq!(transform_keys(&seg), vec!['a']);
    }

    #[test]
    fn w_is_transform_not_tone() {
        let opts = telex_opts();
        let raw: Vec<char> = "ow".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(seg.base, "o");
        assert_eq!(transform_keys(&seg), vec!['w']);
        assert!(seg.tones.is_empty());
    }

    // ── English fallback guard (vowel-consonant-vowel boundary) ──────────────

    #[test]
    fn fallbaack_no_transform() {
        // "fallbaack": 'aa' at positions 5-6, but earlier 'a' at pos 1 with
        // consonants "llb" between — guard must prevent transform.
        let opts = telex_opts();
        let raw: Vec<char> = "fallbaack".chars().collect();
        let seg = segment(&raw, &opts);
        assert!(seg.transforms.is_empty(), "guard must block transform in 'fallbaack': {:?}", seg.transforms);
        assert_eq!(seg.base, "fallbaack");
    }

    #[test]
    fn implemeent_no_transform() {
        // "implemeent": 'ee' at positions 7-8, but earlier 'e' at pos 4 with
        // consonant 'm' between — guard must prevent transform.
        let opts = telex_opts();
        let raw: Vec<char> = "implemeent".chars().collect();
        let seg = segment(&raw, &opts);
        assert!(seg.transforms.is_empty(), "guard must block transform in 'implemeent': {:?}", seg.transforms);
        assert_eq!(seg.base, "implemeent");
    }

    #[test]
    fn fallback_real_word_no_transform() {
        // Typing the real word "fallback": second 'a' must NOT transform the
        // first ('fallb' has invalid coda "llb") — output stays "fallback".
        let opts = telex_opts();
        let raw: Vec<char> = "fallback".chars().collect();
        let seg = segment(&raw, &opts);
        assert!(seg.transforms.is_empty(), "no transform in 'fallback': {:?}", seg.transforms);
        assert_eq!(seg.base, "fallback");
    }

    #[test]
    fn implement_real_word_no_transform() {
        // Typing the real word "implement": no transform at all.
        let opts = telex_opts();
        let raw: Vec<char> = "implement".chars().collect();
        let seg = segment(&raw, &opts);
        assert!(seg.transforms.is_empty(), "no transform in 'implement': {:?}", seg.transforms);
        assert_eq!(seg.base, "implement");
    }

    #[test]
    fn implemeent_no_nonadjacent_transform() {
        // "impleme" has two vowel groups ('i' … 'e') → not one Vietnamese
        // syllable → non-adjacent 'e' transform must NOT fire.
        let opts = telex_opts();
        let raw: Vec<char> = "impleme".chars().collect();
        let seg = segment(&raw, &opts);
        assert!(seg.transforms.is_empty(), "non-adjacent must not fire in 'impleme': {:?}", seg.transforms);
        assert_eq!(seg.base, "impleme");
    }

    #[test]
    fn vietej_nonadjacent_transform_fires() {
        // "viet" is a single vowel group ('ie') → non-adjacent 'e' fires.
        let opts = telex_opts();
        let raw: Vec<char> = "viete".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(transform_keys(&seg), vec!['e'], "non-adjacent must fire in 'viete'");
    }

    #[test]
    fn viet_ee_transform_fires() {
        // "vieetj": 'ee' adjacent with NO earlier 'e' before it → must fire.
        let opts = telex_opts();
        let raw: Vec<char> = "vieet".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(transform_keys(&seg), vec!['e'], "ee transform must fire in 'vieet'");
    }

    #[test]
    fn baan_aa_transform_fires() {
        // "baan": no earlier 'a' before the adjacent pair → must fire.
        let opts = telex_opts();
        let raw: Vec<char> = "baan".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(transform_keys(&seg), vec!['a'], "aa transform must fire in 'baan'");
    }

    #[test]
    fn direct_map_double_key() {
        let mut cfg = PipelineConfig::new("cham");
        cfg.native_script_mode = true;
        cfg.add_transform("k", "ꨆ");
        cfg.add_transform("kk", "ꩀ");
        let opts = ComposeOpts::from_config(&cfg);

        let raw: Vec<char> = "kk".chars().collect();
        let seg = segment(&raw, &opts);
        assert_eq!(seg.base, "ꩀ");
    }
}
