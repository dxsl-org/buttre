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

use super::ComposeOpts;
use crate::vowel::cluster::is_vowel;

/// Slot index for the per-call a/e/o/d counters (`double_candidates`,
/// `vowel_in_base`) — fixed 4-char domain, so plain arrays beat per-call
/// HashMap allocations on the keystroke hot path.
#[inline]
fn aeod_idx(c: char) -> Option<usize> {
    match c {
        'a' => Some(0),
        'e' => Some(1),
        'o' => Some(2),
        'd' => Some(3),
        _ => None,
    }
}

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
    /// Index of `key` within the raw buffer that produced this mark.
    /// Carried through unchanged by `transform::apply_transforms` (including
    /// through the leftward retry) into `AppliedMark::raw_pos` for Phase 4.
    pub raw_pos: usize,
    /// `true` when `key` was NOT typed immediately after its target in RAW
    /// key order — see `mark_non_adjacent` for the exact rule. Set once here
    /// at extraction time and never recomputed from the (possibly retried)
    /// commit position in `transform::apply_one_transform`.
    pub non_adjacent: bool,
}

/// A transform mark that successfully changed the composed text (as opposed
/// to an unmatched mark whose trigger key was appended literally).
///
/// Reported via `ComposeResult::applied_marks` so:
/// - the attestation gate (`compose::mod`) can decide whether the composed
///   syllable needs to pass `is_attested`/`is_shape_attested`;
/// - Phase 4's undo detection can test "was the fired mark's trigger the
///   last key of the raw prefix".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedMark {
    /// The raw key that triggered this transform.
    pub key: char,
    /// Index of the trigger key within the raw buffer.
    pub raw_pos: usize,
    /// Mirrors `TransformMark::non_adjacent`.
    pub non_adjacent: bool,
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
///
/// `allow_nonadjacent`: when `false` (the demote pass, see `compose::mod`),
/// any mark that would be flagged `non_adjacent` is suppressed at the source
/// — its trigger key is pushed to `base` as a literal character instead of
/// becoming a `TransformMark`. This is the "flag-driven toggle" demote
/// mechanism: it re-derives the base/marks split from scratch rather than
/// mutating a previously composed string, so completed ADJACENT transforms
/// elsewhere in the same word are unaffected.
pub fn segment(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> Segment {
    match opts.segment_mode {
        SegmentMode::MarkBased => segment_mark_based(raw, opts, allow_nonadjacent),
        SegmentMode::DirectMap => segment_direct_map(raw, opts),
    }
}

// ── MarkBased ─────────────────────────────────────────────────────────────────

fn segment_mark_based(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> Segment {
    let mut base = String::new();
    let mut transforms: Vec<TransformMark> = Vec::new();
    let mut tones: Vec<char> = Vec::new();
    let mut has_seen_vowel = false;

    // Pre-scan: count each potentially-doubling char (a/e/o/d).
    // Non-adjacent flexible typing fires ONLY when count == 2 — meaning the
    // raw buffer has exactly one base char + one transform mark intended.
    // Three or more occurrences (e.g. "implemeent" has 3 'e') indicate an
    // English word with accidental repeats, not a Vietnamese transform intent.
    //
    // KEEP (phase-03 adjudication table): attestation cannot replace this.
    // A 3rd repeat is the signal Phase 4's undo/toggle detection and the
    // English-fallback path rely on — it is orthogonal to whether the
    // COMPOSED syllable happens to be a real word. E.g. "aaa" must undo to
    // "aa" regardless of attestation; the count==2 rule is what tells segment
    // "this looks like one intentional mark", not "the result is attested".
    let mut double_candidates = [0usize; 4];
    for &ch in raw {
        if let Some(idx) = aeod_idx(ch.to_ascii_lowercase()) {
            double_candidates[idx] += 1;
        }
    }

    // For open-syllable non-adjacent đ: fire only when a vowel follows the
    // second 'd' in raw.  This lets "dodong"→"đông" fire (vowel 'o' follows)
    // while preserving English "dad"/"dads" (no vowel after the trailing 'd').
    //
    // KEEP (phase-03 adjudication table): "dad"→"đa" IS an attested Vietnamese
    // syllable, so the attestation gate cannot tell it apart from a deliberate
    // đ transform — this guard is the ONLY thing protecting English "dad"/
    // "dads". Applies to every đ-path guard below (this fn, `base_ends_with_coda`,
    // and the open-syllable vowel check at the đ branch's call site), not just
    // this helper.
    let has_vowel_after_second_d = {
        let mut d_count = 0usize;
        let second_d_pos = raw.iter().position(|&c| {
            if c.eq_ignore_ascii_case(&'d') {
                d_count += 1;
            }
            d_count == 2
        });
        second_d_pos.is_some_and(|pos| {
            raw.get(pos + 1..)
                .unwrap_or(&[])
                .iter()
                .any(|&c| is_vowel(c.to_ascii_lowercase()))
        })
    };

    let mut vowel_in_base = [false; 4];

    for (i, &ch) in raw.iter().enumerate() {
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
        //
        // KEEP (phase-03 adjudication table): this ADJACENT path is deliberately
        // ungated by the attestation gate — the gate (`compose::mod`) only ever
        // demotes marks flagged `non_adjacent`. Removing this guard would let
        // every adjacent English double ("fallbaack", "implemeent") transform
        // unconditionally; leniency here means "typed exactly like a real
        // Vietnamese double" still gets one structural sanity check, not a
        // lexical one.
        if !base.is_empty() {
            let last_base_lc = base.chars().last().unwrap().to_ascii_lowercase();
            if last_base_lc == lc
                && matches!(lc, 'a' | 'e' | 'o' | 'd')
                && !has_earlier_vowel_with_consonants(&base, lc)
            {
                let base_len = base.chars().count();
                let non_adjacent = mark_non_adjacent(raw, i, lc, base_len, opts);
                if allow_nonadjacent || !non_adjacent {
                    transforms.push(TransformMark {
                        key: ch,
                        base_len_at_typing: base_len,
                        raw_pos: i,
                        non_adjacent,
                    });
                    continue;
                }
                // Demote pass suppressing a non-adjacent mark: fall through
                // to treat this key as a literal base character (below).
            }
            // Guard fired (or the double-letter/doubling-set check failed) —
            // fall through to treat this key as a literal base character.
        }

        // ── Non-adjacent double (flexible typing: "vietej" → "việt") ───────
        // The repeated vowel refers back to the nucleus of an already-complete
        // syllable.  `vowel_in_base` (KEEP, phase-03 adjudication table) is a
        // structural precondition independent of attestation: without an
        // earlier occurrence of `lc` in `base` at all, there is nothing for a
        // non-adjacent mark to target — this is not a "is it a real word"
        // question, it is "does the shape even make sense to attempt".
        //
        // The remaining two checks (KEEP for non-Vietnamese, gate-bypassed for
        // Vietnamese — see `legacy_shape_guards_pass` below) used to ALSO gate
        // Vietnamese configs:
        //   1. exactly one contiguous vowel group (one nucleus) — rejected
        //      "implem" ('i' … 'e' = two groups, an English word); AND
        //   2. the consonants after the rightmost matching vowel form a VALID
        //      Vietnamese coda — rejected "fallb" (coda "llb" is invalid, so
        //      "fallback" stayed literal instead of becoming "fâllback").
        // For "viet": one group + coda "t" (valid) → fires → "việt".
        //
        // DELETE for Vietnamese (phase-03 adjudication table, conditional-keep
        // rule / red-team M1): the composed result of "implêm"/"fâllb"/"sâls"
        // is unattested, so `compose::mod`'s attestation gate demotes it after
        // the fact — these two structural pre-checks are now redundant work on
        // that path. But the gate is Vietnamese-only (`opts.attest_non_adjacent`
        // is false for Hmong/Custom/None), so for those validators the legacy
        // guards must keep running exactly as before — there is no attestation
        // table to catch a bad shape post-hoc.
        // count != 2 also disables non-adjacent (English word with repeats).
        if let Some(idx) = aeod_idx(lc).filter(|_| lc != 'd') {
            let count = double_candidates[idx];
            let legacy_shape_guards_pass = opts.attest_non_adjacent
                || (count_vowel_groups(&base) <= 1 && coda_after_last_vowel_is_valid(&base, lc));
            if count == 2 && vowel_in_base[idx] && legacy_shape_guards_pass {
                let base_len = base.chars().count();
                let non_adjacent = mark_non_adjacent(raw, i, lc, base_len, opts);
                if allow_nonadjacent || !non_adjacent {
                    transforms.push(TransformMark {
                        key: ch,
                        base_len_at_typing: base_len,
                        raw_pos: i,
                        non_adjacent,
                    });
                    continue;
                }
                // Demote pass: suppressed, fall through to literal (đ-check
                // below never matches a/e/o, so this reaches the final `else`).
            }
        }

        // ── Non-adjacent đ (flexible typing: "datjd" → "đạt") ──────────────
        // The trailing 'd' turns the onset 'd' into 'đ'.  Unlike the vowel case,
        // the coda/nucleus guards do not apply (đ is a consonant transform on the
        // onset).  To avoid mangling English ("dad" → "đa"), fire ONLY when the
        // syllable is "committed" — it already has a coda consonant OR a tone —
        // which signals genuine Vietnamese intent.  So "datjd"/"datd"/"datdj"
        // → đạt/đat, but bare "dad" stays "dad".
        //
        // KEEP unconditionally, for ALL validators (phase-03 adjudication
        // table): "đa" (from bare "dad") IS an attested Vietnamese syllable —
        // the attestation gate in `compose::mod` cannot distinguish this from
        // a deliberate transform, so it can never protect English "d…d" words.
        // This whole đ branch is NOT subject to the conditional-keep bypass
        // used above for the vowel branch.
        if lc == 'd'
            && double_candidates[3] == 2
            && vowel_in_base[3]
            && (!tones.is_empty() || base_ends_with_coda(&base)
                // Fast-typing: onset 'd' followed by a vowel (open syllable) before
                // the doubling key — "dodong"→"đông", "dodongf"→"đồng".
                // Guard: a vowel must follow the second 'd' in raw; otherwise
                // English words ending in 'd' ("dad", "dads") are preserved.
                || (is_vowel(base.chars().last().unwrap_or('_').to_ascii_lowercase())
                    && has_vowel_after_second_d))
        {
            let base_len = base.chars().count();
            let non_adjacent = mark_non_adjacent(raw, i, lc, base_len, opts);
            if allow_nonadjacent || !non_adjacent {
                transforms.push(TransformMark {
                    key: ch,
                    base_len_at_typing: base_len,
                    raw_pos: i,
                    non_adjacent,
                });
                continue;
            }
            // Demote pass: suppressed, fall through to literal push below.
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
        // An *alphabetic* standalone modifier (Telex 'w') acts as a transform
        // ONLY when a compatible base vowel precedes it — i.e. some earlier base
        // char `v` forms a 2-char rule `"{v}w"` (aw/ow/uw).  A leading bare 'w'
        // (or 'w' after a consonant with no a/o/u) is a literal consonant, so
        // English w-words ("won", "with", "will", "want") are typed naturally and
        // 'ư' at word start is reached via "uw".  Non-alphabetic standalone keys
        // (VNI digits 6–9) keep their unconditional behaviour.

        let fires_via_preceding_vowel =
            is_standalone_transform && standalone_modifier_has_vowel(ch, &base, opts);
        let fires_via_onset_insertion = is_standalone_transform
            && !fires_via_preceding_vowel
            && onset_only_insertion_fires(ch, &base, opts);
        if fires_via_preceding_vowel || fires_via_onset_insertion {
            // Record base length at time of this mark so transform can pick the right vowel.
            let base_len = base.chars().count();
            let non_adjacent = mark_non_adjacent(raw, i, lc, base_len, opts);
            if allow_nonadjacent || !non_adjacent {
                transforms.push(TransformMark {
                    key: ch,
                    base_len_at_typing: base_len,
                    raw_pos: i,
                    non_adjacent,
                });
                // The onset insertion's expansion ("w"→"ư") IS the syllable's
                // nucleus: later tone keys must see a vowel ("twf"→"từ",
                // "swj"→"sự") even though no raw vowel char exists in the
                // buffer. Only set when the mark actually fired — the demote
                // pass keeps 'w' literal and tone keys literal with it.
                if fires_via_onset_insertion {
                    has_seen_vowel = true;
                }
            } else {
                // Demote pass: suppressed non-adjacent standalone mark (VNI
                // digit or Telex 'w') stays literal in the base.
                base.push(ch);
            }
        } else if is_standalone_transform {
            // Alphabetic modifier with no compatible preceding vowel → literal.
            base.push(ch);
        } else if is_tone_key_char {
            if !has_seen_vowel {
                // No vowel yet — this tone key has no nucleus to act on; treat as literal.
                base.push(ch);
                if let Some(idx) = aeod_idx(lc) {
                    vowel_in_base[idx] = true;
                }
            } else {
                tones.push(ch);
            }
        } else {
            base.push(ch);
            if let Some(idx) = aeod_idx(lc) {
                vowel_in_base[idx] = true;
            }
        }
    }

    Segment {
        base,
        transforms,
        tones,
    }
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
    Segment {
        base,
        transforms: Vec::new(),
        tones: Vec::new(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute the `non_adjacent` flag for a mark about to fire at raw index `i`.
///
/// `base_len == 0` marks are prefix transforms (VNI `"6a"`: the digit is typed
/// BEFORE any base char, and `transform::apply_one_transform` applies it
/// forward across the whole base once typed) — ADJACENT by definition, since
/// there is no "previous base char" to be non-adjacent from. This also avoids
/// ever computing `base_len - 1`, which would underflow `usize` for this
/// exact case.
///
/// Otherwise, delegates to [`is_adjacent_trigger`] for the raw-order check.
fn mark_non_adjacent(
    raw: &[char],
    i: usize,
    trigger_lc: char,
    base_len: usize,
    opts: &ComposeOpts,
) -> bool {
    if base_len == 0 {
        return false;
    }
    !is_adjacent_trigger(raw, i, trigger_lc, opts)
}

/// True when the transform trigger at raw index `i` was typed immediately
/// (in RAW key order, not the mark-stripped `base` string) after a character
/// with which it forms a 2-char transform rule — i.e. genuinely "adjacent"
/// typing: `aa`/`ee`/`oo`/`dd` doubling, `aw`/`ow`/`uw` (also covers the
/// `uo`+`w` compound, since its trigger is always immediately preceded by
/// the compound's final vowel), and VNI `a6`/`u7`/etc.
///
/// Deliberately computed from RAW positions, not from `apply_one_transform`'s
/// eventual commit index: a tone key or an unrelated letter sitting between
/// the target and the trigger — as in `reset` (tone key `s` between the two
/// `e`s) or `nasa` (`s` between the two `a`s) — must NOT count as adjacent
/// even though the base-relative (mark-stripped) index would suggest
/// otherwise. This is what the attestation gate in `compose::mod` uses to
/// decide which marks need to prove the composed syllable is a real word.
fn is_adjacent_trigger(raw: &[char], i: usize, trigger_lc: char, opts: &ComposeOpts) -> bool {
    let Some(prev) = i.checked_sub(1).map(|j| raw[j]) else {
        return false;
    };
    let prev_lc = prev.to_ascii_lowercase();
    opts.pair_rules.contains_key(&(prev_lc, trigger_lc))
}

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
///
/// Phase-03: reachable only when `opts.attest_non_adjacent` is `false`
/// (Hmong/Custom/None) — see `legacy_shape_guards_pass` at the call site.
/// For Vietnamese configs the attestation gate in `compose::mod` catches the
/// same class of false positive downstream, on the composed result rather
/// than this structural pre-check.
fn coda_after_last_vowel_is_valid(base: &str, vowel: char) -> bool {
    let chars: Vec<char> = base.chars().collect();
    let Some(pos) = chars.iter().rposition(|&c| c.to_ascii_lowercase() == vowel) else {
        return false;
    };
    let tail: String = chars[pos + 1..]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    // Valid Vietnamese codas (single + 2-char); empty = open syllable.
    matches!(
        tail.as_str(),
        "" | "c" | "m" | "n" | "p" | "t" | "ch" | "ng" | "nh"
    )
}

/// True when a standalone modifier key may act as a transform at this position.
///
/// Non-alphabetic keys (VNI digits) always may.  An alphabetic modifier (Telex
/// 'w') may only when some earlier base char `v` forms a 2-char transform rule
/// `"{v}{key}"` (i.e. aw/ow/uw) — otherwise a leading bare 'w' would wrongly
/// become 'ư' and break English w-words.
fn standalone_modifier_has_vowel(ch: char, base: &str, opts: &ComposeOpts) -> bool {
    if !ch.is_alphabetic() {
        return true;
    }
    let key = ch.to_ascii_lowercase();
    base.chars()
        .any(|c| opts.pair_rules.contains_key(&(c.to_ascii_lowercase(), key)))
}

/// Onset-only standalone-modifier shorthand (UniKey-habit compatibility:
/// "nhw" → "như", "lwu" → "lưu", "trwong" → "trương", "chwowng" → "chương"):
/// an alphabetic modifier with a 1-char rule ("w" → "ư") fires as an
/// inferred INSERTION mark when the base so far is a non-empty,
/// pure-consonant onset.
///
/// Safety net, in order:
/// - Word-initial stays literal (`base` must be non-empty), so English
///   w-words ("won", "with", "will", "want") keep typing naturally.
/// - The mark is inherently non-adjacent (a pure-consonant base means no
///   preceding char forms a 2-char rule with the trigger), so the attestation
///   gate in `compose::mod` demotes any composition that is not a real word:
///   "swim" → "sưim" (unattested) → literal "swim".
/// - Gated on `attest_non_adjacent` because that demote net is exactly what
///   makes the inference safe — validators without an attested-syllable
///   table (Hmong/Custom/None) never take this branch.
fn onset_only_insertion_fires(ch: char, base: &str, opts: &ComposeOpts) -> bool {
    opts.attest_non_adjacent
        && ch.is_alphabetic()
        && opts.single_rules.contains_key(&ch.to_ascii_lowercase())
        && !base.is_empty()
        && !base.chars().any(|c| is_vowel(c.to_ascii_lowercase()))
}

/// True when `base` ends with a consonant that follows a vowel — i.e. the
/// syllable has a coda (e.g. "dat" → coda 't'; "da" → none; "d" → none).
fn base_ends_with_coda(base: &str) -> bool {
    let chars: Vec<char> = base.chars().collect();
    match chars.last() {
        Some(&last) if !is_vowel(last.to_ascii_lowercase()) => {
            // A consonant tail is a coda only if some vowel precedes it.
            chars[..chars.len() - 1]
                .iter()
                .any(|&c| is_vowel(c.to_ascii_lowercase()))
        }
        _ => false,
    }
}

/// Count maximal runs of consecutive vowels in `s`.
///
/// A valid Vietnamese syllable has exactly one vowel nucleus (one group).
/// More than one group means the base spans a consonant-separated vowel
/// boundary — not a single syllable.
///
/// Phase-03: reachable only when `opts.attest_non_adjacent` is `false`
/// (Hmong/Custom/None) — see `legacy_shape_guards_pass` at the call site.
/// For Vietnamese configs the attestation gate in `compose::mod` catches the
/// same class of false positive downstream, on the composed result rather
/// than this structural pre-check.
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
    // Precomputed in `ComposeOpts::from_config`: modifier chars of 2-char /
    // 1-char rules, minus tone keys, vowels, and 'd' (perf: this used to
    // rescan and re-lowercase every rule key on every keystroke).
    opts.standalone_keys.contains(&ch.to_ascii_lowercase())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::ComposeOpts;
    use crate::pipeline::config::{PipelineConfig, ToneMark, ValidationSettings};

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

    /// Same doubling/tone rules as `telex_opts`, but with a non-Vietnamese
    /// validator — `opts.attest_non_adjacent` is `false`, so the legacy
    /// `count_vowel_groups`/`coda_after_last_vowel_is_valid` shape guards must
    /// stay active (phase-03 conditional-keep rule).
    fn hmong_opts() -> ComposeOpts {
        let mut cfg = PipelineConfig::new("hmong-test");
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
        cfg.validation = Some(ValidationSettings {
            syllable_structure: "hmong".to_string(),
            allow_invalid: true,
            ..Default::default()
        });
        ComposeOpts::from_config(&cfg)
    }

    fn transform_keys(seg: &Segment) -> Vec<char> {
        seg.transforms.iter().map(|t| t.key).collect()
    }

    #[test]
    fn basic_tone_key_after_vowel() {
        let opts = telex_opts();
        let raw: Vec<char> = "as".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(seg.base, "a");
        assert!(seg.transforms.is_empty());
        assert_eq!(seg.tones, vec!['s']);
    }

    #[test]
    fn s_as_initial_consonant() {
        let opts = telex_opts();
        // "sinh" — 's' before vowel is a consonant
        let raw: Vec<char> = "sinh".chars().collect();
        let seg = segment(&raw, &opts, true);
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
        let seg = segment(&raw, &opts, true);
        assert!(
            seg.tones.is_empty(),
            "leading 'f' must not be collected as tone: {:?}",
            seg.tones
        );
        assert!(
            seg.base.starts_with('f'),
            "leading 'f' must be in base: '{}'",
            seg.base
        );
    }

    #[test]
    fn f_after_vowel_is_tone() {
        // "af": 'a' is vowel first → 'f' is a tone (grave).
        let opts = telex_opts();
        let raw: Vec<char> = "af".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(seg.tones, vec!['f'], "post-vowel 'f' must be tone");
        assert_eq!(seg.base, "a");
    }

    #[test]
    fn j_before_vowel_is_literal() {
        // "jin": 'j' is a tone key (dot-below) but leads the syllable → literal.
        let opts = telex_opts();
        let raw: Vec<char> = "jin".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert!(
            seg.tones.is_empty(),
            "leading 'j' must not be collected as tone: {:?}",
            seg.tones
        );
        assert!(seg.base.starts_with('j'));
    }

    #[test]
    fn adjacent_double_transform() {
        let opts = telex_opts();
        let raw: Vec<char> = "aa".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(seg.base, "a");
        assert_eq!(transform_keys(&seg), vec!['a']);
    }

    #[test]
    fn w_is_transform_not_tone() {
        let opts = telex_opts();
        let raw: Vec<char> = "ow".chars().collect();
        let seg = segment(&raw, &opts, true);
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
        let seg = segment(&raw, &opts, true);
        assert!(
            seg.transforms.is_empty(),
            "guard must block transform in 'fallbaack': {:?}",
            seg.transforms
        );
        assert_eq!(seg.base, "fallbaack");
    }

    #[test]
    fn implemeent_no_transform() {
        // "implemeent": 'ee' at positions 7-8, but earlier 'e' at pos 4 with
        // consonant 'm' between — guard must prevent transform.
        let opts = telex_opts();
        let raw: Vec<char> = "implemeent".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert!(
            seg.transforms.is_empty(),
            "guard must block transform in 'implemeent': {:?}",
            seg.transforms
        );
        assert_eq!(seg.base, "implemeent");
    }

    // ── Phase 3: conditional-keep rule (adjudication table DELETE¹ rows) ─────
    //
    // `count_vowel_groups(&base) <= 1` and `coda_after_last_vowel_is_valid`
    // used to ALSO block "fallback"/"implement"/"impleme" at THIS layer for
    // every validator. For Vietnamese configs they are now bypassed here —
    // the attestation gate in `compose::mod` demotes the unattested result
    // downstream instead (see `compose::tests::high_fallback_implement_class_words_stay_literal`
    // for the end-to-end assertion that these words still end up literal).
    // Zero scenarios dropped: the segment-level rejection assertion these
    // tests used to make now lives at the compose level; what's asserted HERE
    // is the new segment-level contract (mark fires, gate handles the rest).

    #[test]
    fn vietnamese_config_bypasses_legacy_shape_guards() {
        let opts = telex_opts();
        for word in ["fallback", "implement", "impleme"] {
            let raw: Vec<char> = word.chars().collect();
            let seg = segment(&raw, &opts, true);
            assert!(!seg.transforms.is_empty(),
                "Vietnamese config must bypass the legacy shape guards for '{word}' at segment level (gate demotes downstream): {:?}", seg.transforms);
        }
    }

    #[test]
    fn hmong_config_legacy_shape_guards_still_block() {
        // Non-Vietnamese-config regression guard (phase-03 conditional-keep
        // rule): `opts.attest_non_adjacent` is `false` for Hmong/Custom/None
        // validators, which have no attested-syllable table — the legacy
        // structural guards must keep running EXACTLY as before for these.
        let opts = hmong_opts();
        for word in ["fallback", "implement", "impleme"] {
            let raw: Vec<char> = word.chars().collect();
            let seg = segment(&raw, &opts, true);
            assert!(
                seg.transforms.is_empty(),
                "Hmong config must keep the legacy shape guards active for '{word}': {:?}",
                seg.transforms
            );
            assert_eq!(seg.base, word);
        }
    }

    #[test]
    fn vietej_nonadjacent_transform_fires() {
        // "viet" is a single vowel group ('ie') → non-adjacent 'e' fires.
        let opts = telex_opts();
        let raw: Vec<char> = "viete".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            transform_keys(&seg),
            vec!['e'],
            "non-adjacent must fire in 'viete'"
        );
    }

    #[test]
    fn viet_ee_transform_fires() {
        // "vieetj": 'ee' adjacent with NO earlier 'e' before it → must fire.
        let opts = telex_opts();
        let raw: Vec<char> = "vieet".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            transform_keys(&seg),
            vec!['e'],
            "ee transform must fire in 'vieet'"
        );
    }

    #[test]
    fn baan_aa_transform_fires() {
        // "baan": no earlier 'a' before the adjacent pair → must fire.
        let opts = telex_opts();
        let raw: Vec<char> = "baan".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            transform_keys(&seg),
            vec!['a'],
            "aa transform must fire in 'baan'"
        );
    }

    #[test]
    fn dodong_d_fires_as_onset_transform() {
        // "dodongf": fast-typing "đồng" — 'd' and 'o' typed before their doubling
        // keys.  The second 'd' must fire as a non-adjacent onset transform (base
        // open, tone key 'f' follows), and the second 'o' must follow via the
        // existing non-adjacent vowel path.
        let opts = telex_opts();
        let raw: Vec<char> = "dodongf".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            seg.base, "dong",
            "base must be 'dong' after both transforms extracted"
        );
        assert_eq!(
            transform_keys(&seg),
            vec!['d', 'o'],
            "both 'd' and 'o' must be transform marks"
        );
        assert_eq!(seg.tones, vec!['f']);
    }

    #[test]
    fn direct_map_double_key() {
        let mut cfg = PipelineConfig::new("cham");
        cfg.native_script_mode = true;
        cfg.add_transform("k", "ꨆ");
        cfg.add_transform("kk", "ꩀ");
        let opts = ComposeOpts::from_config(&cfg);

        let raw: Vec<char> = "kk".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(seg.base, "ꩀ");
    }

    // ── Raw-adjacency flag: the core of Phase 2 ───────────────────────────────

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

    fn non_adjacent_flags(seg: &Segment) -> Vec<bool> {
        seg.transforms.iter().map(|t| t.non_adjacent).collect()
    }

    #[test]
    fn vieet_adjacent_double_is_not_flagged() {
        // "vieet": 'ee' typed back-to-back → adjacent, must NOT be flagged.
        let opts = telex_opts();
        let raw: Vec<char> = "vieet".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            non_adjacent_flags(&seg),
            vec![false],
            "adjacent 'ee' must not be flagged non-adjacent"
        );
    }

    #[test]
    fn how_standalone_w_is_not_flagged() {
        // "how": 'w' typed immediately after its target vowel 'o' → adjacent.
        let opts = telex_opts();
        let raw: Vec<char> = "how".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            non_adjacent_flags(&seg),
            vec![false],
            "how's 'w' must not be flagged non-adjacent"
        );
    }

    #[test]
    fn viete_nonadjacent_double_is_flagged() {
        // "viete": the second 'e' is separated from the first by 't' → non-adjacent.
        let opts = telex_opts();
        let raw: Vec<char> = "viete".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            non_adjacent_flags(&seg),
            vec![true],
            "viete's 'e' must be flagged non-adjacent"
        );
    }

    #[test]
    fn reset_tone_key_between_vowels_is_flagged() {
        // "reset": a tone key ('s') sits between the two 'e's in RAW order.
        // The mark-stripped base index would wrongly suggest adjacency — the
        // flag must be computed from raw positions instead.
        let opts = telex_opts();
        let raw: Vec<char> = "reset".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(transform_keys(&seg), vec!['e']);
        assert_eq!(
            non_adjacent_flags(&seg),
            vec![true],
            "reset's 'e' must be flagged non-adjacent (tone key between)"
        );
    }

    #[test]
    fn nasa_tone_key_between_vowels_is_flagged() {
        // "nasa": same shape as "reset" — 's' between the two 'a's.
        let opts = telex_opts();
        let raw: Vec<char> = "nasa".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(transform_keys(&seg), vec!['a']);
        assert_eq!(
            non_adjacent_flags(&seg),
            vec![true],
            "nasa's 'a' must be flagged non-adjacent (tone key between)"
        );
    }

    #[test]
    fn dodongf_both_marks_flagged_non_adjacent() {
        // Backward-referring đ and the non-adjacent 'o' are both flagged, even
        // though the composed "đồng" is attested and survives the gate.
        let opts = telex_opts();
        let raw: Vec<char> = "dodongf".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(transform_keys(&seg), vec!['d', 'o']);
        assert_eq!(
            non_adjacent_flags(&seg),
            vec![true, true],
            "both đ and o must be flagged non-adjacent"
        );
    }

    #[test]
    fn luuw_retry_target_inherits_adjacent_flag() {
        // "luuw": 'w' is typed immediately after the second 'u' → adjacent at
        // the SEGMENT level, regardless of which 'u' `transform::apply_transforms`
        // eventually commits the horn to via its leftward retry.
        let opts = telex_opts();
        let raw: Vec<char> = "luuw".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(
            non_adjacent_flags(&seg),
            vec![false],
            "luuw's 'w' must not be flagged non-adjacent"
        );
    }

    #[test]
    fn vni_prefix_digit_base_len_zero_is_adjacent() {
        // VNI "6a": the digit is typed BEFORE any base char (base_len_at_typing
        // == 0, a forward-applying prefix transform). Must be classified
        // ADJACENT by definition — and must never underflow computing base_len - 1.
        let opts = vni_opts();
        let raw: Vec<char> = "6a".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(seg.transforms.len(), 1);
        assert_eq!(seg.transforms[0].base_len_at_typing, 0);
        assert!(
            !seg.transforms[0].non_adjacent,
            "base_len==0 prefix mark must be adjacent by definition"
        );
    }

    #[test]
    fn vni_nhat_digit_after_coda_is_flagged_non_adjacent() {
        // VNI "nhat6": '6' typed after coda 't', not immediately after the
        // target vowel 'a' → non-adjacent (non-alphabetic trigger, so the
        // gate relaxes to shape-attestation — see compose::mod tests).
        let opts = vni_opts();
        let raw: Vec<char> = "nhat6".chars().collect();
        let seg = segment(&raw, &opts, true);
        assert_eq!(transform_keys(&seg), vec!['6']);
        assert_eq!(non_adjacent_flags(&seg), vec![true]);
    }

    // ── allow_nonadjacent=false suppresses flagged marks at the source ────────

    #[test]
    fn demote_suppresses_nonadjacent_mark_keeps_literal() {
        // "viete" with allow_nonadjacent=false: the 'e' mark must not be
        // extracted at all — it stays a literal base character.
        let opts = telex_opts();
        let raw: Vec<char> = "viete".chars().collect();
        let seg = segment(&raw, &opts, false);
        assert!(
            seg.transforms.is_empty(),
            "demote pass must extract no marks: {:?}",
            seg.transforms
        );
        assert_eq!(seg.base, "viete");
    }

    #[test]
    fn demote_preserves_adjacent_marks() {
        // Adjacent marks are untouched by the demote toggle — only marks that
        // WOULD be flagged non-adjacent are suppressed.
        let opts = telex_opts();
        let raw: Vec<char> = "vieet".chars().collect();
        let seg = segment(&raw, &opts, false);
        assert_eq!(
            transform_keys(&seg),
            vec!['e'],
            "adjacent mark must still fire when demoted"
        );
    }

    #[test]
    fn demote_suppresses_backward_referring_d() {
        // 'f' is still classified as a tone key (independent of the
        // non-adjacent mark suppression) — only the đ/o marks are demoted.
        let opts = telex_opts();
        let raw: Vec<char> = "dodongf".chars().collect();
        let seg = segment(&raw, &opts, false);
        assert!(
            seg.transforms.is_empty(),
            "demote pass must extract no marks: {:?}",
            seg.transforms
        );
        assert_eq!(seg.base, "dodong");
        assert_eq!(seg.tones, vec!['f']);
    }

    #[test]
    fn demote_suppresses_vni_standalone_digit() {
        // Regression guard (redteam F3): VNI digit marks classify via
        // `is_standalone_transform_key`, not the vowel/đ branches — the demote
        // toggle must suppress them too, not just Telex doubles.
        let opts = vni_opts();
        let raw: Vec<char> = "nhat6".chars().collect();
        let seg = segment(&raw, &opts, false);
        assert!(
            seg.transforms.is_empty(),
            "demote pass must suppress VNI standalone digit: {:?}",
            seg.transforms
        );
        assert_eq!(seg.base, "nhat6");
    }
}
