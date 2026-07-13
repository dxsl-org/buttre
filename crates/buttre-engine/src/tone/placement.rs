//! Vietnamese tone placement: canonical rule for which vowel in a nucleus receives the tone.
//!
//! ## Rules (in priority order, from VIETNAMESE_ACCENT.md / stage6:309-352)
//!
//! 1. Super-vowel (ê, ô, ơ, ă, â, and their toned variants) — always wins.
//! 2. Triple vowel — middle vowel (index 1).
//! 3. Double vowel with final consonant — second vowel (index 1).
//! 4. Double vowel, open syllable:
//!    - "ia" / "ua" → first vowel (index 0).
//!    - "ie" / "ye" (bare transients of iê/yê) → second vowel (index 1).
//!    - "oa" / "oe" / "uy" → depends on `ToneStyle` (Old→0, New→1).
//!    - Anything else → first vowel (index 0).
//! 5. Single vowel → index 0.

use crate::pipeline::config::ToneStyle;
use crate::vowel::normalize_vowel;

// ── Public API ────────────────────────────────────────────────────────────────

/// Return the index within `nucleus` of the vowel that should receive the tone.
///
/// `nucleus` is a slice of **vowel** characters that form the core of a Vietnamese syllable
/// (initial consonants and final consonants must already be excluded by the caller).
///
/// Returns `None` only if `nucleus` is empty.
///
/// ## Contract
///
/// - Characters in `nucleus` may already carry tone diacritics; they are normalised
///   internally via `normalize_vowel`.
/// - The returned index is always `< nucleus.len()`.
pub fn place(nucleus: &[char], tone_style: ToneStyle, has_final_consonant: bool) -> Option<usize> {
    if nucleus.is_empty() {
        return None;
    }

    // ── PRIORITY 1: Super-vowel ───────────────────────────────────────────────
    // Scan from the end so that the last super-vowel wins when there are two
    // (mirrors stage6 which used `rev().find()`).
    if let Some(idx) = nucleus.iter().rposition(|&c| is_super_vowel(c)) {
        return Some(idx);
    }

    // ── PRIORITY 2: Triple vowel ──────────────────────────────────────────────
    if nucleus.len() == 3 {
        let v0 = normalize_vowel(nucleus[0]);
        let v1 = normalize_vowel(nucleus[1]);
        let v2 = normalize_vowel(nucleus[2]);
        let triple = [v0, v1, v2];
        match triple {
            ['i', 'e', 'u']
            | ['y', 'e', 'u']
            | ['u', 'o', 'i']
            | ['u', 'o', 'u']
            | ['o', 'a', 'i']
            | ['o', 'a', 'y']
            | ['u', 'a', 'y']
            | ['u', 'y', 'a']
            | ['u', 'y', 'u'] => return Some(1),
            _ => {} // fall through
        }
    }

    // ── PRIORITY 3: Double vowel ──────────────────────────────────────────────
    if nucleus.len() >= 2 {
        if has_final_consonant {
            // Closed syllable → tone on second vowel
            return Some(1);
        }

        // Open syllable
        let v0 = normalize_vowel(nucleus[0]);
        let v1 = normalize_vowel(nucleus[1]);
        match (v0, v1) {
            // ia / ua (ưa normalises to ua because ư→u) → first vowel
            ('i', 'a') | ('u', 'a') => return Some(0),
            // ie / ye are the bare transients of iê / yê (the ê not typed
            // yet): the tone belongs on the e — where the super-vowel rule
            // will keep it once ê arrives — never on the glide ("yes" must
            // show "yé" like Unikey, not "ýe").
            ('i', 'e') | ('y', 'e') => return Some(1),
            // oa / oe / uy → ToneStyle-dependent
            ('o', 'a') | ('o', 'e') | ('u', 'y') => {
                return Some(match tone_style {
                    ToneStyle::Old => 0,
                    ToneStyle::New => 1,
                });
            }
            // All other open diphthongs → first vowel
            _ => return Some(0),
        }
    }

    // ── PRIORITY 4: Single vowel ──────────────────────────────────────────────
    Some(0)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Characters that carry their own diacritic (circumflex/breve/horn) and therefore
/// always receive the tone mark regardless of position.
///
/// This matches `is_super_vowel` in stage5 and stage6, using `ă | â | ê | ô | ơ`.
/// Note: stage6's free-standing `is_super_vowel` also includes `ư`, but stage5's
/// method-based version does NOT include `ư`. The placement canonical source is
/// stage6:309 which uses the free-standing version — so `ư` IS included here.
#[inline]
fn is_super_vowel(ch: char) -> bool {
    let lower = ch.to_lowercase().next().unwrap_or(ch);
    matches!(
        lower,
        'ă' | 'â' | 'ê' | 'ô' | 'ơ' | 'ư' |
        // toned variants
        'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' |
        'ấ' | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' |
        'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' |
        'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' |
        'ớ' | 'ờ' | 'ở' | 'ỡ' | 'ợ' |
        'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự'
    )
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::config::ToneStyle;

    // hoà (Old) / hòa (New) — "oa" open, no final consonant
    #[test]
    fn oa_tone_style() {
        let nucleus = ['o', 'a'];
        assert_eq!(
            place(&nucleus, ToneStyle::Old, false),
            Some(0),
            "Old: tone on 'o'"
        );
        assert_eq!(
            place(&nucleus, ToneStyle::New, false),
            Some(1),
            "New: tone on 'a'"
        );
    }

    // tuần / tuấn — "ua" + final consonant → second vowel
    #[test]
    fn ua_with_final_consonant() {
        let nucleus = ['u', 'a'];
        assert_eq!(place(&nucleus, ToneStyle::Old, true), Some(1));
        assert_eq!(place(&nucleus, ToneStyle::New, true), Some(1));
    }

    // ua open (vua) — ia/ua pattern → first vowel
    #[test]
    fn ua_open_syllable() {
        let nucleus = ['u', 'a'];
        assert_eq!(place(&nucleus, ToneStyle::Old, false), Some(0));
        assert_eq!(place(&nucleus, ToneStyle::New, false), Some(0));
    }

    // ia open — first vowel
    #[test]
    fn ia_open_syllable() {
        let nucleus = ['i', 'a'];
        assert_eq!(place(&nucleus, ToneStyle::Old, false), Some(0));
    }

    // ye / ie open — bare transients of yê/iê → tone on e (yé, not ýe)
    #[test]
    fn ye_ie_open_tone_on_e() {
        assert_eq!(place(&['y', 'e'], ToneStyle::Old, false), Some(1));
        assert_eq!(place(&['y', 'e'], ToneStyle::New, false), Some(1));
        assert_eq!(place(&['i', 'e'], ToneStyle::Old, false), Some(1));
    }

    // super-vowel always wins (ê, ô, ơ, ă, â)
    #[test]
    fn super_vowel_priority() {
        // "uê" → ê wins (index 1)
        assert_eq!(place(&['u', 'ê'], ToneStyle::Old, false), Some(1));
        // "ươ" → ơ wins (index 1), ư is also super so rev() finds ơ last → index 1
        assert_eq!(place(&['ư', 'ơ'], ToneStyle::Old, false), Some(1));
        // "uâ" → â wins
        assert_eq!(place(&['u', 'â'], ToneStyle::Old, false), Some(1));
        // "oa" with ô in nucleus
        assert_eq!(place(&['o', 'â'], ToneStyle::Old, false), Some(1));
        // single ă
        assert_eq!(place(&['ă'], ToneStyle::Old, false), Some(0));
    }

    // 3-vowel: "uyê" → index 1
    #[test]
    fn triple_vowel_uye() {
        // "uya" matches 'u','y','a' — not in table, falls through to 2-vowel logic
        // but "uye" = ['u','y','e'] — also not in table
        // The canonical triple is ['u','y','u'] etc.
        // "iêu" normalises to ['i','e','u'] → index 1
        assert_eq!(place(&['i', 'ê', 'u'], ToneStyle::Old, false), Some(1));
        assert_eq!(place(&['u', 'ô', 'i'], ToneStyle::Old, false), Some(1));
    }

    #[test]
    fn empty_nucleus_returns_none() {
        assert_eq!(place(&[], ToneStyle::Old, false), None);
    }

    #[test]
    fn single_vowel_returns_zero() {
        assert_eq!(place(&['a'], ToneStyle::Old, false), Some(0));
        assert_eq!(place(&['i'], ToneStyle::New, true), Some(0));
    }

    // ư is treated as super-vowel — prevents it from being skipped in "ưa" open
    #[test]
    fn u_horn_is_super_vowel() {
        // "ưa" open — ư is super, wins at index 0
        assert_eq!(place(&['ư', 'a'], ToneStyle::Old, false), Some(0));
    }
}
