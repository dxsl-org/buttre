//! Transform step — apply diacritic marks to the base string.
//!
//! ## Position-aware transform
//!
//! Each `TransformMark` carries `base_len_at_typing` — the number of base chars
//! that existed when the mark key was pressed.  The mark applies to the
//! RIGHT-MOST vowel in `base[..base_len_at_typing]`, matching incremental
//! behaviour (the mark was typed immediately after that vowel).
//!
//! Example: "ngoaw" → base="ngoat"(sic "ngoa"), mark='w' with base_len=4.
//! Search `base[..4]="ngoa"` right-to-left → 'a' matches "aw"→"ă". ✓
//!
//! ## UO compound rule (port of stage6 lines 204-240)
//!
//! `uo + w/7`:
//! - When "uo" is followed by more chars (has a coda) → ươ (both transformed).
//! - When "uo" is at word end → uơ (only o transformed).
//! - SUPPRESSED when another compound-trigger mark follows: "uwow" → first mark
//!   applies to 'u' individually, second mark applies to 'o' individually.

use crate::vowel::cluster::normalize_vowel;
use super::{ComposeOpts, segment::{AppliedMark, TransformMark}};

// ── Public API ────────────────────────────────────────────────────────────────

/// Apply all `transforms` to `base` in order.
///
/// Returns the transformed string plus a report of the marks that actually
/// changed it (an unmatched mark falls back to appending its trigger key
/// literally and is NOT included in the report — there is nothing for the
/// attestation gate or Phase 4's undo to act on).
///
/// The report's `non_adjacent`/`raw_pos` fields are copied unchanged from
/// each `TransformMark` — `apply_one_transform`'s leftward retry (see module
/// doc) may change WHICH vowel receives the diacritic, but it never changes
/// whether the trigger key itself was typed adjacently.
pub fn apply_transforms(base: &str, transforms: &[TransformMark], opts: &ComposeOpts) -> (String, Vec<AppliedMark>) {
    let mut result = base.to_string();
    let mut applied = Vec::new();
    for (idx, tm) in transforms.iter().enumerate() {
        // Remaining marks (for the compound-suppression check) are passed as
        // a borrowed sub-slice — no per-mark Vec<char>.
        let remaining = &transforms[idx + 1..];
        let new = apply_one_transform(&result, tm.key, tm.base_len_at_typing, remaining, opts);
        if let Some(new_result) = new {
            result = new_result;
            applied.push(AppliedMark { key: tm.key, raw_pos: tm.raw_pos, non_adjacent: tm.non_adjacent });
        } else {
            result.push(tm.key);
        }
    }
    (result, applied)
}

// ── Internal ──────────────────────────────────────────────────────────────────

/// Apply a single transform mark to `result`.
///
/// `base_len_at_typing`: char count of the base when the mark was typed.
/// The mark targets the rightmost matching vowel in `result[..base_len_at_typing]`.
///
/// `remaining`: later marks (for compound-suppression decision).
fn apply_one_transform(
    result: &str,
    mark: char,
    base_len_at_typing: usize,
    remaining: &[TransformMark],
    opts: &ComposeOpts,
) -> Option<String> {
    let mark_lc = mark.to_ascii_lowercase();
    let mut chars: Vec<char> = result.chars().collect();

    // Cap the search range to base_len_at_typing (right-most vowel before mark was typed).
    // If the result has grown beyond the original base (e.g. after a previous transform
    // that expanded the string), we use the full result length to avoid out-of-bounds.
    let search_end = base_len_at_typing.min(chars.len());

    // ── UO compound rule ──────────────────────────────────────────────────────
    // Only within the base slice that was present when this mark was typed.
    let triggers_compound = is_compound_trigger(mark, opts);
    let has_later_compound = remaining.iter()
        .any(|t| t.key.to_ascii_lowercase() == mark_lc && is_compound_trigger(t.key, opts));

    if triggers_compound && !has_later_compound {
        let base_slice: String = chars[..search_end].iter().collect();
        let base_slice_lower = base_slice.to_lowercase();
        if let Some(pos) = find_uo_pos(&base_slice_lower) {
            let u_ch = chars[pos];
            let o_ch = chars[pos + 1];
            let u_is_base = normalize_vowel(u_ch) == 'u' && !is_u_horn(u_ch);
            let o_is_base = normalize_vowel(o_ch) == 'o'
                && !matches!(normalize_vowel(o_ch), 'ơ' | 'ô');
            if u_is_base && o_is_base {
                let has_following = pos + 2 < chars.len();
                if has_following {
                    chars[pos]     = preserve_case(u_ch, 'ư');
                    chars[pos + 1] = preserve_case(o_ch, 'ơ');
                } else {
                    chars[pos + 1] = preserve_case(o_ch, 'ơ');
                }
                return Some(chars.into_iter().collect());
            }
        }
    }

    // ── Data-driven single-vowel transform ────────────────────────────────────
    // Normal case: scan RIGHT-TO-LEFT within base[..search_end] to find the
    // rightmost vowel that has a matching rule.  This honours the "mark applies
    // to the vowel typed immediately before it" contract.
    //
    // Prefix-transform case (base_len_at_typing == 0): the mark key was the
    // FIRST key typed, so the entire base was typed AFTER it.  Two sub-cases:
    //
    // 1. The mark has a 1-char rule for itself (e.g. Telex "w"→"ư"): prepend
    //    the result to the base.  This handles "win" → base="in", mark='w' at
    //    pos 0 → single-char rule "w"→"ư" → prepend → "ưin".
    //
    // 2. No 1-char rule: scan left-to-right for the first vowel that has a
    //    2-char rule with the mark (e.g. a future config where "iw"→"ị").
    if search_end == 0 && !chars.is_empty() {
        // Sub-case 1: 1-char rule for the mark key itself (standalone prefix).
        // NOTE: no shipped preset registers a 1-char rule (leading bare 'w'
        // stays literal so English w-words type naturally); reachable only by
        // a custom config whose 1-char-rule key passes segment.
        if let Some(first) = opts
            .single_rules
            .get(&mark_lc)
            .and_then(|repl| repl.chars().next())
        {
            let prefix = preserve_case(mark, first);
            let rest: String = chars.into_iter().collect();
            return Some(format!("{prefix}{rest}"));
        }

        // Sub-case 2: Forward scan for a vowel + mark 2-char rule.
        for i in 0..chars.len() {
            let ch    = chars[i];
            let ch_lc = normalize_vowel(ch);
            if let Some(&new_char) = opts.pair_rules.get(&(ch_lc, mark_lc)) {
                chars[i] = preserve_case(ch, new_char);
                return Some(chars.into_iter().collect());
            }
        }
        return None;
    }

    // Scan right-to-left for a matching vowel.  When multiple vowels match (e.g.
    // both 'u' positions in "luu"), prefer the one whose result is a valid
    // Vietnamese syllable — placing the horn on the LEFTMOST 'u' gives "lưu"
    // (valid nucleus "ưu") while the rightmost gives "luư" (invalid "uư").
    // Save the first (rightmost) candidate as a fallback in case validation
    // cannot resolve the ambiguity (unknown/partial buffer).
    let mut first_candidate: Option<String> = None;

    for i in (0..search_end).rev() {
        let ch    = chars[i];
        let ch_lc = normalize_vowel(ch); // base vowel (strips tone diacritics)

        if let Some(&new_char) = opts.pair_rules.get(&(ch_lc, mark_lc)) {
            // Try the replacement in place and revert before the next
            // (leftward) probe — no per-candidate Vec<char> clone.
            chars[i] = preserve_case(ch, new_char);
            let candidate_str: String = chars.iter().collect();
            chars[i] = ch;
            if is_valid_syllable(&candidate_str) {
                return Some(candidate_str);
            }
            if first_candidate.is_none() {
                first_candidate = Some(candidate_str);
            }
        }

        // Idempotent repeat: the target vowel ALREADY carries this mark's
        // diacritic — an earlier mark's insertion+compound may have horned it
        // first ("chwowng": by the time the second 'w' applies, its target is
        // already 'ơ'/'ư'). Treat the mark as applied with no text change,
        // instead of falling through to a literal trailing 'w'.
        //
        // Scoped to marks that ALSO have a 1-char rule (the insertion
        // shorthand family, i.e. Telex 'w') — a repeated plain mark key with
        // no insertion behavior must stay a literal append, or it would eat
        // the post-undo literal in VNI "a6116" (undo latches, trailing '6'
        // appends as "â16" — see vni_edge_cases::test_multi_step_undo_a6116).
        if opts.single_rules.contains_key(&mark_lc)
            && opts
                .pair_rules
                .iter()
                .any(|(&(_, m), &out)| m == mark_lc && out == ch_lc)
        {
            return Some(result.to_string());
        }
    }

    // ── Onset-only insertion (see `segment::onset_only_insertion_fires`) ──────
    // No vowel in the search range matched a 2-char rule, but the mark has a
    // standalone 1-char expansion ("w"→"ư"): insert it at the position the
    // mark was typed — "lwu" → base "lu", 'w' at base_len 1 → "lưu".
    // Only reachable for a pure-consonant prefix: every other path into this
    // function has a matching vowel in range or no 1-char rule for the mark.
    if first_candidate.is_none() {
        if let Some(first) = opts
            .single_rules
            .get(&mark_lc)
            .and_then(|repl| repl.chars().next())
        {
            let pos = search_end.min(chars.len());
            let ins = preserve_case(mark, first);
            chars.insert(pos, ins);
            // Orthography: inserted ư directly before a PLAIN 'o' with a coda
            // after it forms the "ươ" compound — mirrors the uo+w rule above
            // ("trwong" → "trưong" would otherwise stay an invalid nucleus).
            // Deliberately NOT mirroring the uo+w no-coda "→ uơ" arm: a
            // coda-less "ưo" ("trwo" mid-word frame) is an invalid nucleus
            // that demotes to literal cleanly, and recovers on the next key.
            if matches!(ins, 'ư' | 'Ư')
                && pos + 2 < chars.len()
                && matches!(chars[pos + 1], 'o' | 'O')
            {
                chars[pos + 1] = preserve_case(chars[pos + 1], 'ơ');
            }
            return Some(chars.into_iter().collect());
        }
    }

    first_candidate
}

/// True when `s` is a valid (or structurally plausible) Vietnamese syllable.
///
/// Used by the rightmost-vowel scan to prefer a position whose result passes
/// phonological validation over one that produces an invalid nucleus cluster
/// (e.g. "lưu" valid vs "luư" invalid for the "luu"+horn transform).
fn is_valid_syllable(s: &str) -> bool {
    // Zero-alloc probe (stack-buffer normalize + borrowed parts) — this runs
    // once per candidate vowel per mark on the compose hot path.
    crate::pipeline::validation::is_valid_syllable_fast(s)
}

/// True when the mark key is the compound-trigger in the transform table.
fn is_compound_trigger(mark: char, opts: &ComposeOpts) -> bool {
    let ml = mark.to_ascii_lowercase();
    opts.pair_rules.contains_key(&('o', ml)) && opts.pair_rules.contains_key(&('u', ml))
}

/// Find the char index of "uo" in a lowercase string.
fn find_uo_pos(lower: &str) -> Option<usize> {
    let chars: Vec<char> = lower.chars().collect();
    for i in 0..chars.len().saturating_sub(1) {
        if normalize_vowel(chars[i]) == 'u' && normalize_vowel(chars[i + 1]) == 'o' {
            return Some(i);
        }
    }
    None
}

/// True when `ch` is 'ư' (already has the u-horn diacritic).
fn is_u_horn(ch: char) -> bool {
    matches!(ch.to_lowercase().next(), Some('ư'))
}

/// Preserve the case of `original` on `new_char`.
#[inline]
fn preserve_case(original: char, new_char: char) -> char {
    if original.is_uppercase() {
        new_char.to_uppercase().next().unwrap_or(new_char)
    } else {
        new_char
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::compose::{ComposeOpts, segment::TransformMark};
    use crate::pipeline::config::{PipelineConfig, ToneMark};

    fn telex_opts() -> ComposeOpts {
        let mut cfg = PipelineConfig::new("telex");
        cfg.add_transform("aw", "ă");
        cfg.add_transform("aa", "â");
        cfg.add_transform("ee", "ê");
        cfg.add_transform("oo", "ô");
        cfg.add_transform("ow", "ơ");
        cfg.add_transform("uw", "ư");
        cfg.add_transform("dd", "đ");
        cfg.add_tone('s', ToneMark::Acute);
        ComposeOpts::from_config(&cfg)
    }

    fn tm(key: char, base_len: usize) -> TransformMark {
        TransformMark { key, base_len_at_typing: base_len, raw_pos: base_len, non_adjacent: false }
    }

    /// Test-only convenience: most tests here only care about the resulting
    /// text, not the applied-marks report (that report is covered by
    /// `compose::mod`'s gate tests).
    fn apply_transforms(base: &str, transforms: &[TransformMark], opts: &ComposeOpts) -> String {
        super::apply_transforms(base, transforms, opts).0
    }

    #[test]
    fn a_plus_w_yields_breve() {
        let opts = telex_opts();
        // base="a", mark='w', base_len=1 (base had 1 char when 'w' was typed)
        let result = apply_transforms("a", &[tm('w', 1)], &opts);
        assert_eq!(result, "ă");
    }

    #[test]
    fn a_plus_a_yields_circumflex() {
        let opts = telex_opts();
        let result = apply_transforms("a", &[tm('a', 1)], &opts);
        assert_eq!(result, "â");
    }

    #[test]
    fn uo_w_has_coda_yields_uhorn_ohorn() {
        // base="tuong", mark='w' typed after full base → base_len=5
        let opts = telex_opts();
        let result = apply_transforms("tuong", &[tm('w', 5)], &opts);
        assert_eq!(result, "tương");
    }

    #[test]
    fn uo_w_no_coda_yields_only_ohorn() {
        let opts = telex_opts();
        let result = apply_transforms("thuo", &[tm('w', 4)], &opts);
        assert_eq!(result, "thuơ");
    }

    #[test]
    fn ngoa_w_transforms_a_not_o() {
        // "ngoaw" → 'w' typed after 'a' (base_len=4 for "ngoa") → 'a'→'ă'
        let opts = telex_opts();
        let result = apply_transforms("ngoa", &[tm('w', 4)], &opts);
        assert_eq!(result, "ngoă");
    }

    #[test]
    fn uwa_w_transforms_u_not_a() {
        // "uwa": base="ua", 'w' typed after 'u' (base_len=1) → 'u'→'ư', 'a' untouched
        let opts = telex_opts();
        let result = apply_transforms("ua", &[tm('w', 1)], &opts);
        assert_eq!(result, "ưa");
    }

    #[test]
    fn dd_transform() {
        let opts = telex_opts();
        let result = apply_transforms("d", &[tm('d', 1)], &opts);
        assert_eq!(result, "đ");
    }

    #[test]
    fn unknown_mark_appended_literally() {
        let opts = telex_opts();
        let result = apply_transforms("ban", &[tm('z', 3)], &opts);
        assert_eq!(result, "banz");
    }

    // ── Regression: validity-gated leftward retry (bugs: luuw / luu7) ─────────
    //
    // These tests use the PRODUCTION rule set (no synthetic "w"→"ư" rule).
    // The rightmost 'u' in "luu" would produce invalid nucleus "uư"; the fix
    // must retry leftward and land on the first 'u', giving valid nucleus "ưu".

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
        ComposeOpts::from_config(&cfg)
    }

    #[test]
    fn telex_luu_w_yields_luu_horn() {
        // "luuw": base="luu", 'w' typed after both u's (base_len=3).
        // Rightmost 'u' (index 2) gives invalid "luư"; leftward retry picks
        // index 1 giving valid "lưu".
        let opts = telex_opts();
        let result = apply_transforms("luu", &[tm('w', 3)], &opts);
        assert_eq!(result, "lưu", "luuw must produce lưu, not luư");
    }

    #[test]
    fn vni_luu_7_yields_luu_horn() {
        // "luu7": same as above but via VNI '7' key.
        let opts = vni_opts();
        let result = apply_transforms("luu", &[tm('7', 3)], &opts);
        assert_eq!(result, "lưu", "luu7 must produce lưu, not luư");
    }

    #[test]
    fn telex_huu_w_yields_huu_horn() {
        // "huuw": similar double-u case — horn on first u gives valid "hưu".
        let opts = telex_opts();
        let result = apply_transforms("huu", &[tm('w', 3)], &opts);
        assert_eq!(result, "hưu", "huuw must produce hưu, not huư");
    }
}
