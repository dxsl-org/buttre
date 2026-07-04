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
//! **Visibility gate:** the undo fires ONLY when the toned form of the base
//! is a plausible Vietnamese syllable (`could_be_vietnamese`, the same
//! validator gate as `compose_internal` step 6) — i.e. the tone was actually
//! on screen for there to be something to undo. English words ending in a
//! double tone key were already in literal English fallback before the repeat
//! (`glá`/`clá`/`succé` never displayed), so the repeat key is a plain letter:
//! `glass`/`class`/`success`/`press` compose to their full literal selves and
//! no keystroke is dropped. Valid-syllable collisions (`boss`: `bó` WAS on
//! screen) keep Unikey undo semantics (`boss` → "bos").
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
//!
//! ### Non-adjacent transform undo (Phase 4)
//!
//! Extends the retype-to-undo reflex to NON-ADJACENT marks (see
//! `segment::TransformMark::non_adjacent`) — the escape hatch for an accepted
//! attested collision (`"cana"` → `"cân"`; retype `a` → literal `"cana"`,
//! English latched). Unlike the adjacent toggles above, a non-adjacent mark's
//! trigger key is not necessarily the syllable's last RAW character at the
//! moment it fires (`"dodong"`'s đ fires on the 3rd raw char, well before the
//! word ends) — so "retype to undo" only makes sense as an IMMEDIATE reflex:
//! the very next keystroke after the firing one. `check_nonadjacent_transform_toggle`
//! enforces this precisely (see its doc).
//!
//! ## Ordering contract
//!
//! Runs strictly AFTER `check_tone_toggle` and `check_transform_toggle` in
//! `check_fallback` below. Adjacent toggles keep priority: `"aaa"` → `"aa"`
//! must always resolve via `check_transform_toggle`'s tail-triple match and
//! must never reach this check (verified by a regression test — adjacent
//! toggle claims `"aaa"`/`"canaa"`-shaped tails before this function ever
//! runs). This check runs BEFORE `segment`/`transform`/`assemble` — it is
//! step 1 of `compose_internal`, same as the other two toggles.
//!
//! ## Equivalence note (red-team AD-minor)
//!
//! On undo, the output is the LITERAL prefix raw keys, always — never a
//! recomposed form. Unlike `check_transform_toggle` above (which calls
//! `apply_transforms_only` to PRESERVE any earlier, unrelated completed
//! transform in the prefix — e.g. `"ddaaa"` → `"đaa"`, not `"ddaa"`), this
//! check does NOT recompose the prefix when reverting: an earlier, unrelated
//! transform that had already fired within the SAME prefix would also revert
//! to its literal keys for this one keystroke, rather than surviving in
//! composed form.
//!
//! This simplification is safe ONLY because `compose()` is a pure, stateless
//! recompute: the very next keystroke reprocesses the ENTIRE buffer from
//! scratch, so an independent earlier transform re-fires exactly as it would
//! typing from a cold start — there is no stale state for it to have been
//! lost FROM. It also is not a new class of transient loss this check
//! introduces: `compose_internal`'s own step 6 (English fallback,
//! `could_be_vietnamese`) already reverts an ENTIRE word — including any
//! earlier adjacent transforms — to literal `raw.iter().collect()` whenever
//! the whole composed result fails to look Vietnamese, on exactly the same
//! "self-heals next keystroke" assumption. `"dodongd"`-shaped inputs (an
//! earlier non-adjacent mark, followed later by an unrelated retype) do NOT
//! actually reach this code path in practice — the immediacy contract
//! (below) requires the retyped key to equal the raw key immediately
//! preceding it, which is never true once other keys have been typed in
//! between (that shape is exactly what `"vieteje"`'s "must NOT undo" case
//! demonstrates) — so this note documents the underlying design principle
//! for the general case, not a claim that `"dodongd"` itself exercises it.

use super::ComposeOpts;
use super::assemble;
use super::segment;
use super::segment::AppliedMark;
use super::transform;
use crate::tone;
use crate::pipeline::config::ToneMark;

// Test-only instrumentation (red-team M4, perf): counts how many times
// `recompute_prefix_marks` actually ran segment+transform on a prefix. Used
// to PROVE the non-vacuous pre-filter in `check_nonadjacent_transform_toggle`
// skips the prefix compose entirely for non-matching keystrokes, rather than
// merely returning the right answer after doing the work anyway.
#[cfg(test)]
thread_local! {
    static PREFIX_COMPOSE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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
///
/// `allow_nonadjacent` is the SAME recursion-guard flag `compose::mod` threads
/// through every re-entry (see `compose_internal`'s doc). It is forwarded
/// unchanged into the prefix-reconstruction helpers below so they run through
/// the attestation gate too — closing the red-team C2 bypass where
/// `apply_transforms_only`/`compose_base_and_transforms_with_tone` used to
/// rebuild a prefix straight from `segment`+`transform`, ungated, and return
/// from `compose()` before the gate ever ran (`"dataeee"` → `"dâtee"`,
/// `"vietess"` → `"viêts"`, `"databaaa"` → `"dâtbaa"`).
pub fn check_fallback(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> FallbackResult {
    // We only act when there are at least 2 keys (minimum for a toggle).
    if raw.len() < 2 {
        return FallbackResult::not_handled();
    }

    // Check tone toggle first (e.g. "a11", "a111", "a1111").
    if let Some(result) = check_tone_toggle(raw, opts, allow_nonadjacent) {
        return result;
    }

    // Check transform toggle (e.g. "aaa", "aww", "awww").
    if let Some(result) = check_transform_toggle(raw, opts, allow_nonadjacent) {
        return result;
    }

    // Check non-adjacent transform undo (e.g. "cana"+"a" → "cana" literal).
    // Must run LAST: adjacent toggles above always keep priority (ordering
    // contract, module doc).
    if let Some(result) = check_nonadjacent_transform_toggle(raw, opts, allow_nonadjacent) {
        return result;
    }

    FallbackResult::not_handled()
}

// ── Last-event parity fold (shared contract with P2) ───────────────────────────

/// Table-driven parity fold: is the tail of `raw` — AS TYPED SO FAR — a
/// just-fired undo/toggle event?
///
/// ## Why "table-driven"
///
/// Every detector this predicate goes through is driven entirely by the
/// method's OWN tables — `opts.tone_map` (the trailing tone-key-run parity in
/// `check_tone_toggle`'s Path 1, plus the same-tone-repress-after-coda Path
/// 2) and `opts.transform_rules` (the trailing transform-trigger-run parity
/// in `check_transform_toggle`, and the retype-immediacy check in
/// `check_nonadjacent_transform_toggle`) — never a hardcoded key list.
///
/// ## Why delegation, not a second formula
///
/// An earlier proposal collapsed multi-step undo/redo to a single rule —
/// "even trailing run of the same key ⇒ undo, odd ⇒ the mark stays on" — and
/// red-teamed it: `vie65t5`'s trailing run of `'5'` is exactly 1 (odd), yet
/// the syllable STILL undoes, via the separate same-tone-repress-after-coda
/// path (`check_tone_toggle`'s Path 2), not trailing parity. One formula
/// cannot express both paths at once. Delegating to the SAME functions
/// [`check_fallback`] already dispatches through — rather than re-deriving
/// the rule a second time — is what guarantees this predicate reproduces
/// every existing detector outcome EXACTLY, by construction, not by
/// coincidence: `a611`, `seess`, `vie65t5`, `aaa`, and `dessign`'s undo point
/// (`"dess"`) are all pinned by the unit tests below.
///
/// ## Consumer (P2)
///
/// This is the shared LAST-EVENT predicate `pipeline::stages::compose_stage`'s
/// evidence-based un-latch condition (d) consumes via
/// `compose::is_last_event_undo`: "is the word, as typed so far, sitting in a
/// just-undone state" (see `plan.md`'s Combined Contract — "(d) is a
/// LAST-EVENT parity fold sharing P6's rule"). This module defines and pins
/// the predicate only; deciding what to DO with an undone state (un-latch,
/// redo, …) lives in `compose_stage::should_unlatch` — no redo behavior is
/// implemented here, and `a6116` keeps its literal undo-is-final outcome
/// (see `tests/vni_edge_cases.rs`).
pub fn is_last_event_undo(raw: &[char], opts: &ComposeOpts) -> bool {
    check_fallback(raw, opts, true).is_handled
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
fn check_tone_toggle(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> Option<FallbackResult> {
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
            let transformed_base = apply_transforms_only(base_part, opts, allow_nonadjacent);

            // An undo is only meaningful when the tone was ever VISIBLE: the
            // frame BEFORE this repeat key (prefix = raw minus the final key,
            // recomposed through the same segment+transform+tone+gate steps
            // the screen ran) must be a plausible Vietnamese syllable — same
            // validator gate as compose_internal's step 6. English words
            // ending in a double tone key ("glass", "class", "success",
            // "press") were ALREADY in literal English fallback before the
            // repeat — "glá" never displayed, so there is no tone to undo and
            // the repeat key is just a letter. Returning None lets normal
            // compose run, whose step-6 fallback yields the FULL literal raw
            // ("glass", not "glas" — one keystroke must never vanish).
            // Valid-syllable collisions ("boss": "bó" was really on screen)
            // keep Unikey undo semantics unchanged, and delayed-mark words
            // ("vietess": the frame was attested "viết") still undo — the
            // prefix compose runs WITH its tone, so the attestation gate sees
            // the same text the screen did.
            if opts.validator == super::Validator::Vietnamese {
                let visible = compose_base_and_transforms_with_tone(
                    &raw[..raw.len() - 1],
                    opts,
                    allow_nonadjacent,
                )
                .is_some_and(|frame| super::could_be_vietnamese(&frame, opts));
                if !visible {
                    return None;
                }
            }

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
    let candidate = compose_base_and_transforms_with_tone(raw_without_last, opts, allow_nonadjacent)?;

    // Same visibility gate as Path 1: the toned candidate must be a plausible
    // Vietnamese syllable, or the earlier tone key never actually displayed a
    // tone (English word, e.g. "asks": "ák" was never on screen) and this
    // repeat is a plain letter, not a repress.
    if opts.validator == super::Validator::Vietnamese && !super::could_be_vietnamese(&candidate, opts) {
        return None;
    }

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
fn check_transform_toggle(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> Option<FallbackResult> {
    if raw.len() < 3 {
        return None;
    }

    // Detect the pattern: a 2-char transform rule key appears, then the second
    // char of that rule is repeated once more (the undo key).
    // Example: "aaa" — rule "aa"→"â", third 'a' is undo.
    // Example: "aww" — rule "aw"→"ă", second 'w' is undo.
    //
    // The trailing triple [rc1, rc2, rc2] pins the ONLY rule key that could
    // ever match — so this is one O(1) `pair_rules` lookup, not a scan. (The
    // old loop iterated EVERY `transform_rules` key allocating a lowercase
    // String + a Vec<char> per key — ~36 allocations on every ≥3-key
    // keystroke, the single largest allocation source in `compose()`.
    // `pair_rules` indexes exactly the all-lowercase 2-char keys the old
    // lowercased comparison could match, so the match set is identical.)
    let n = raw.len();
    let t = [
        raw[n - 3].to_ascii_lowercase(),
        raw[n - 2].to_ascii_lowercase(),
        raw[n - 1].to_ascii_lowercase(),
    ];
    if t[1] == t[2] && opts.pair_rules.contains_key(&(t[0], t[1])) {
        // The last 3 chars match the undo pattern.
        // Re-compose the prefix so earlier completed transforms (e.g. "dd"→"đ")
        // are preserved.  Only the undone cluster reverts to literal rc1+rc2.
        let (rc1, rc2) = (t[0], t[1]);
        let prefix_raw = &raw[..n - 3];
        let composed_prefix = apply_transforms_only(prefix_raw, opts, allow_nonadjacent);
        let text = format!("{composed_prefix}{rc1}{rc2}");
        return Some(FallbackResult::handled(text, true));
    }

    None
}

// ── Non-adjacent transform undo ────────────────────────────────────────────────

/// Detect the non-adjacent-mark undo reflex: retyping the trigger key of a
/// just-fired non-adjacent transform reverts it to literal keystrokes and
/// latches English passthrough.
///
/// ## Non-vacuous pre-filter (red-team M4, perf)
///
/// Before ever composing anything, two O(1)-on-`raw` checks must BOTH hold:
/// (a) the new key `K` repeats the raw key immediately before it, and
/// (b) that repeated key can ever trigger a transform at all (config-driven —
/// see [`is_transform_trigger_char`]). A non-matching keystroke — the
/// overwhelming majority typed — returns `None` right here, before
/// [`recompute_prefix_marks`] runs. `nonadjacent_undo_prefilter_skips_*`
/// below prove this with call-count instrumentation, not just the right
/// answer.
///
/// ## Immediacy contract (red-team M3)
///
/// Undo fires iff there is a mark in the prefix's OWN applied-marks report
/// (P2's [`AppliedMark`]) with `non_adjacent == true`, whose `raw_pos` is the
/// LAST index of the prefix (the fired mark's trigger WAS the prefix's final
/// keystroke), and whose `key` matches `K` case-insensitively. This is what
/// the pre-filter's condition (a) already establishes at the character level
/// — the deeper check additionally confirms the mark that fired there is the
/// one being retyped, not merely that some other mark fired earlier in the
/// word. `"vieteje"`: prefix `"vietej"`'s last raw key is tone `'j'`, not the
/// `'e'` mark (fired at position 4) — pre-filter condition (a) already fails
/// (`K='e' != prefix-last='j'`), so no prefix compose is attempted and no
/// undo fires.
///
/// ## Gate interaction (`"dataa"` — no double-strip)
///
/// The prefix is recomposed via [`recompute_prefix_marks`], which runs the
/// SAME attestation gate as the main pipeline. If the prefix's own
/// non-adjacent mark fails attestation (`"data"`'s `'a'` → unattested
/// `"dât"`), the gate demotes it to literal WITHIN the prefix recompute —
/// the report comes back with `applied_marks` empty, no eligible mark is
/// found, and this function no-ops (`None`). The main `compose_internal`
/// path then handles the FULL raw (`"dataa"`) on its own terms — no double
/// strip, because this function never touched the output.
///
/// ## Recursion bound
///
/// Only ever attempted from a genuine top-level call — identical guard to
/// `try_elongation_fallback` in `compose::mod`. A demoted recompute
/// (`allow_nonadjacent == false`) is already resolving a DIFFERENT gate
/// failure for this same raw buffer; re-entering here would double-process
/// the same keystroke. [`recompute_prefix_marks`] deliberately bypasses
/// `check_fallback` entirely (same pattern as `apply_transforms_only`), so
/// composing the prefix can NEVER re-enter this function — total additional
/// depth from the gate-demote inside it is bounded at 1, exactly like every
/// other re-entry point threaded on `allow_nonadjacent`.
fn check_nonadjacent_transform_toggle(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> Option<FallbackResult> {
    if !allow_nonadjacent {
        return None;
    }

    let k_lc = nonadjacent_undo_candidate(raw, opts)?;

    let prefix = &raw[..raw.len() - 1];
    let (_, applied) = recompute_prefix_marks(prefix, opts, true);
    let prefix_last_idx = prefix.len() - 1;
    let fired_here = applied.iter().any(|m| {
        m.non_adjacent && m.raw_pos == prefix_last_idx && m.key.to_ascii_lowercase() == k_lc
    });
    if !fired_here {
        return None;
    }

    // K is consumed (not appended): the literal is exactly the prefix's raw
    // keystrokes, never the prefix's composed/transformed text (equivalence
    // note, module doc).
    let literal: String = prefix.iter().collect();
    Some(FallbackResult::handled(literal, true))
}

/// The non-vacuous pre-filter's two O(1) checks (module doc, red-team M4).
/// Returns the lowercased candidate key when BOTH hold, else `None` without
/// touching `opts.transform_rules` more than once.
fn nonadjacent_undo_candidate(raw: &[char], opts: &ComposeOpts) -> Option<char> {
    let n = raw.len();
    if n < 2 {
        return None;
    }
    let k_lc = raw[n - 1].to_ascii_lowercase();
    if raw[n - 2].to_ascii_lowercase() != k_lc {
        return None;
    }
    if !is_transform_trigger_char(k_lc, opts) {
        return None;
    }
    Some(k_lc)
}

/// True when `key_lc` (already lowercased) can EVER trigger a transform: it
/// is the trigger — the second character of a 2-char rule, or the sole
/// character of a 1-char rule — of some entry in `opts.transform_rules`.
/// Config-driven: covers Telex doubling letters (a/e/o/d, via `"aa"`/`"dd"`/…),
/// Telex `'w'` (via `"aw"`/`"ow"`/`"uw"`/standalone `"w"`), and VNI digits (via
/// `"a6"`/`"o7"`/…) uniformly, with no hardcoded key set.
fn is_transform_trigger_char(key_lc: char, opts: &ComposeOpts) -> bool {
    // Precomputed hot-path tables (`ComposeOpts::from_config`) instead of
    // rescanning every rule key: a trigger is the second char of a 2-char
    // rule or the sole char of a 1-char rule — the same set the runtime
    // transform lookups can actually act on.
    opts.single_rules.contains_key(&key_lc)
        || opts.pair_rules.keys().any(|&(_, b)| b == key_lc)
}

/// Recompute `raw` through segment + transform + tone + the attestation gate
/// — the same steps `compose::mod::compose_internal` runs at its steps 2-5 —
/// WITHOUT step 1 (`check_fallback`) or step 6 (English-fallback).
///
/// Skipping step 1 is what bounds recursion: this is the dedicated
/// fallback-bypass helper (same pattern as `apply_transforms_only` /
/// `compose_base_and_transforms_with_tone` above), so composing a prefix here
/// can never re-enter [`check_nonadjacent_transform_toggle`] a second time.
///
/// Skipping step 6 (`could_be_vietnamese` English-fallback) is safe: any mark
/// that survives the attestation gate below already implies
/// `could_be_vietnamese(text)` would also hold — `is_attested`/
/// `is_shape_attested` both imply structural validity — so step 6 could never
/// additionally revert a mark this function reports as fired.
fn recompute_prefix_marks(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> (String, Vec<AppliedMark>) {
    #[cfg(test)]
    PREFIX_COMPOSE_CALLS.with(|c| c.set(c.get() + 1));

    if raw.is_empty() {
        return (String::new(), Vec::new());
    }
    let seg = segment::segment(raw, opts, allow_nonadjacent);
    let (transformed, applied) = transform::apply_transforms(&seg.base, &seg.transforms, opts);
    let text = if let Some(&last_tone_key) = seg.tones.last() {
        assemble::apply_tone(&transformed, last_tone_key, opts).unwrap_or(transformed)
    } else {
        transformed
    };
    // `closed=false` (open projection) always: these are mid-word undo/toggle
    // PREFIX reconstructions (detecting "was this raw tail a just-fired undo
    // event"), never the word-boundary final decision — that decision is
    // made once, by the top-level `compose_internal` call for this same
    // `raw`, via its own (possibly `closed=true`) gate check.
    if allow_nonadjacent
        && opts.attest_non_adjacent
        && !super::passes_attestation_gate(&text, &applied, false, opts)
    {
        return recompute_prefix_marks(raw, opts, false);
    }
    (text, applied)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Apply segment + transform steps to `raw` (no tone application), GATED on
/// attestation exactly like the main `compose_internal` path (red-team C2:
/// this prefix-reconstruction helper used to call `segment`+`transform`
/// directly and return from `compose()` before the gate ever ran, letting
/// `"dataeee"`/`"databaaa"` resurface the `"data"`→`"dât"` bug through the
/// tone/transform-toggle back door).
///
/// Used by the tone-undo path to reconstruct the transformed base vowels
/// (e.g. `[a, 6]` → `"â"`) before appending the literal tone key suffix.
///
/// Any tone keys that `segment()` collects from `raw` are intentionally
/// discarded — when this function is called, the caller has already decided
/// that all tone marks in `raw` should be stripped (the undo has fired).
///
/// `allow_nonadjacent=false` recurses at most once (mirrors
/// `compose_internal`'s demote bound): the recursive call passes `false`
/// again, and a `false` call never re-enters the gate.
fn apply_transforms_only(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let seg = segment::segment(raw, opts, allow_nonadjacent);
    let (text, applied) = transform::apply_transforms(&seg.base, &seg.transforms, opts);
    // `closed=false` (open projection) always: these are mid-word undo/toggle
    // PREFIX reconstructions (detecting "was this raw tail a just-fired undo
    // event"), never the word-boundary final decision — that decision is
    // made once, by the top-level `compose_internal` call for this same
    // `raw`, via its own (possibly `closed=true`) gate check.
    if allow_nonadjacent
        && opts.attest_non_adjacent
        && !super::passes_attestation_gate(&text, &applied, false, opts)
    {
        return apply_transforms_only(raw, opts, false);
    }
    text
}

/// Run segment + transform + tone on `raw` and return the result text, GATED
/// on attestation like `apply_transforms_only` above (same C2 concern).
///
/// Returns `None` if `raw` is empty or composition produces no output.
/// Used by the same-tone-repress path to evaluate `raw[..-1]`.
fn compose_base_and_transforms_with_tone(raw: &[char], opts: &ComposeOpts, allow_nonadjacent: bool) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let seg = segment::segment(raw, opts, allow_nonadjacent);
    let (transformed, applied) = transform::apply_transforms(&seg.base, &seg.transforms, opts);
    let text = if let Some(&last_tone_key) = seg.tones.last() {
        // Apply the last tone key (mirrors the compose() main path).
        assemble::apply_tone(&transformed, last_tone_key, opts).unwrap_or(transformed)
    } else {
        transformed
    };
    // `closed=false` (open projection) always: these are mid-word undo/toggle
    // PREFIX reconstructions (detecting "was this raw tail a just-fired undo
    // event"), never the word-boundary final decision — that decision is
    // made once, by the top-level `compose_internal` call for this same
    // `raw`, via its own (possibly `closed=true`) gate check.
    if allow_nonadjacent
        && opts.attest_non_adjacent
        && !super::passes_attestation_gate(&text, &applied, false, opts)
    {
        return compose_base_and_transforms_with_tone(raw, opts, false);
    }
    Some(text)
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
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "aaa should trigger undo");
        assert_eq!(result.text, "aa");
        assert!(result.temp_english);
    }

    #[test]
    fn aww_triggers_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "aww".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "aww should trigger undo");
        assert_eq!(result.text, "aw");
    }

    #[test]
    fn ass_triggers_tone_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "ass".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "ass should trigger tone undo");
        assert_eq!(result.text, "as");
        assert!(result.temp_english);
    }

    #[test]
    fn as_does_not_trigger() {
        let opts = telex_opts();
        let raw: Vec<char> = "as".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(!result.is_handled);
    }

    #[test]
    fn ddd_triggers_dd_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "ddd".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "ddd should trigger undo");
        assert_eq!(result.text, "dd");
    }

    #[test]
    fn a11_triggers_tone_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "a11".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "a11 should trigger tone undo");
        assert_eq!(result.text, "a1");
    }

    // ── Regression guards: transform-preserving tone undo ─────────────────────

    #[test]
    fn vni_a611_keep_circumflex() {
        // a6 → â, then 11 undo pair → strip tone, keep â. Output: â1.
        let opts = vni_opts();
        let raw: Vec<char> = "a611".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "a611 should trigger transform-preserving undo");
        assert_eq!(result.text, "â1", "a611: should keep â and strip tone to give â1");
    }

    #[test]
    fn vni_a822_keep_breve() {
        // a8 → ă, then 22 undo pair → ă2.
        let opts = vni_opts();
        let raw: Vec<char> = "a822".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "a822 should trigger transform-preserving undo");
        assert_eq!(result.text, "ă2", "a822: should keep ă and strip tone to give ă2");
    }

    #[test]
    fn vni_u733_keep_horn() {
        // u7 → ư, then 33 undo pair → ư3.
        let opts = vni_opts();
        let raw: Vec<char> = "u733".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "u733 should trigger transform-preserving undo");
        assert_eq!(result.text, "ư3", "u733: should keep ư and strip tone to give ư3");
    }

    #[test]
    fn vni_o744_keep_horn() {
        // o7 → ơ, then 44 undo pair → ơ4.
        let opts = vni_opts();
        let raw: Vec<char> = "o744".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "o744 should trigger transform-preserving undo");
        assert_eq!(result.text, "ơ4", "o744: should keep ơ and strip tone to give ơ4");
    }

    #[test]
    fn vni_u7o711_keep_compound_horn() {
        // u7o7 → ươ (compound), then 11 undo pair → ươ1.
        let opts = vni_opts();
        let raw: Vec<char> = "u7o711".chars().collect();
        let result = check_fallback(&raw, &opts, true);
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
        let result = check_fallback(&raw, &opts, true);
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
        let result = check_fallback(&raw, &opts, true);
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
        let result = check_fallback(&raw, &opts, true);
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
        let result = check_fallback(&raw, &opts, true);
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
        let result = check_fallback(&raw, &opts, true);
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
    fn telex_fanss_is_literal_no_undo() {
        // Visibility gate (double-s English fix): "fán" has an invalid 'f'
        // onset, so the tone from the first 's' never displayed — there is
        // nothing to undo, and the trailing "ss" must NOT eat a keystroke.
        // Fallback defers; compose's step-6 English fallback yields the full
        // literal "fanss". (This test originally guarded the trailing-run
        // counting fix with expected text "fans"; that algorithm is still
        // guarded by `telex_seess_triggers_tone_undo_with_ee_transform`,
        // whose toned prefix "sế" IS valid and still undoes.)
        let opts = telex_opts();
        let raw: Vec<char> = "fanss".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(!result.is_handled, "fanss: tone never displayed → repeat 's' is a plain letter");
    }

    #[test]
    fn telex_seess_triggers_tone_undo_with_ee_transform() {
        // "see" (ee→ê transform) typed with trailing "ss": 's' is Telex Acute key *and* consonant.
        // trailing run of 's' = 2, base_part = ['s','e','e'] → "sê" (ee transform applied), text = "sês".
        let opts = telex_opts_with_ee();
        let raw: Vec<char> = "seess".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "seess should trigger tone undo (trailing-run fix)");
        assert_eq!(result.text, "sês", "seess → sês + temp_english (ee transform preserved)");
        assert!(result.temp_english);
    }

    #[test]
    fn telex_sass_triggers_tone_undo() {
        // 's' as leading consonant, 'a' vowel, then "ss" undo pair. The frame
        // before the repeat ("sas") displayed valid "sá", so — unlike the
        // "glass"/"fanss" class — this undo is visible and must keep firing.
        let opts = telex_opts();
        let raw: Vec<char> = "sass".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "sass should trigger tone undo (frame 'sá' was visible)");
        assert_eq!(result.text, "sas");
        assert!(result.temp_english);
    }

    #[test]
    fn telex_ass_still_undoes_valid_tone() {
        // Control for the visibility gate: "á" IS a valid displayed syllable,
        // so the classic Unikey double-tone undo must keep firing.
        let opts = telex_opts();
        let raw: Vec<char> = "ass".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled);
        assert_eq!(result.text, "as");
    }

    #[test]
    fn telex_sinff_triggers_tone_undo() {
        // "sin" + "ff" undo pair: 'f' before vowel stays literal, trailing "ff" undoes the tone.
        // base_part = ['s','i','n'] → "sin", text = "sinf".
        let opts = telex_opts();
        let raw: Vec<char> = "sinff".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "sinff should trigger tone undo (trailing-run fix)");
        assert_eq!(result.text, "sinf", "sinff → sinf + temp_english");
        assert!(result.temp_english);
    }

    // ── Phase 4: non-adjacent transform undo ──────────────────────────────────
    // See the module-level deviation note in `compose::tests` for why "can6"
    // (not the matrix's "cana7") and "dand" (not "dodongd") are used here —
    // both verified empirically against this build's actual rule tables.

    #[test]
    fn nonadjacent_undo_cana_a_fires() {
        // Pre-condition: "cana" is an attested collision (verified in
        // compose::tests::medium_cana_collision_canal_self_heals: "cana"->"cân").
        let opts = telex_opts();
        let raw: Vec<char> = "canaa".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "canaa should undo the non-adjacent â mark");
        assert_eq!(result.text, "cana");
        assert!(result.temp_english);
    }

    #[test]
    fn nonadjacent_undo_uppercase_trigger_fires() {
        let opts = telex_opts();
        let raw: Vec<char> = "canaA".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "uppercase retype of the trigger must still undo");
        assert_eq!(result.text, "cana");
    }

    #[test]
    fn nonadjacent_undo_dand_d_consonant_class_fires() {
        // đ analogue of "cana"+"a": "dand" fires đ on ITS OWN final raw key
        // (base_ends_with_coda("dan")), so retyping 'd' right after satisfies
        // immediacy. Composes to attested "đan" (to knit/weave).
        let opts = telex_opts();
        let raw: Vec<char> = "dandd".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "dandd should undo the non-adjacent đ mark");
        assert_eq!(result.text, "dand");
        assert!(result.temp_english);
    }

    #[test]
    fn nonadjacent_undo_vni_digit_parity_fires() {
        // Method parity (S8): VNI digit-triggered equivalent of "cana"+"a".
        let opts = vni_opts();
        let raw: Vec<char> = "can66".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled, "VNI digit retype must undo exactly like Telex");
        assert_eq!(result.text, "can6");
        assert!(result.temp_english);
    }

    #[test]
    fn nonadjacent_undo_dataa_no_ops_no_double_strip() {
        // "data"'s own 'a' mark is already gate-demoted at the top level
        // (see compose::tests::critical_data_stays_literal). The prefix
        // recompute inside the undo check independently re-derives the same
        // demotion (fresh count-of-'a' == 2 there, but "dât" fails
        // attestation), finds zero eligible marks, and no-ops — this whole
        // check must return `not_handled` so compose_internal's normal path
        // (not this check) decides the final literal text, dropping no keys.
        let opts = telex_opts();
        let raw: Vec<char> = "dataa".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(!result.is_handled, "dataa's prefix mark was already demoted — no-op, no double-strip");
    }

    #[test]
    fn nonadjacent_undo_vieteje_immediacy_violated() {
        // "vietej" fires the 'e' mark at raw index 4, but "vieteje"'s prefix
        // ("vietej") ends in tone key 'j' (index 5) — the pre-filter's
        // same-key-repeat condition already fails (K='e' != prefix-last='j'),
        // so no undo may fire.
        let opts = telex_opts();
        let raw: Vec<char> = "vieteje".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(!result.is_handled, "vieteje must not undo: immediacy violated");
    }

    #[test]
    fn nonadjacent_undo_aaa_adjacent_toggle_claims_tail_first() {
        // Ordering contract: a crafted input where the LAST THREE raw chars
        // form an adjacent undo triple ("aaa") must be claimed by
        // `check_transform_toggle` before this check ever runs, even when a
        // non-adjacent mark exists earlier in the same word ("cana"'s â).
        let opts = telex_opts();
        let raw: Vec<char> = "canaaa".chars().collect();
        let result = check_fallback(&raw, &opts, true);
        assert!(result.is_handled);
        assert_eq!(result.text, "canaa",
            "adjacent toggle must claim the trailing 'aaa' triple; must not be 'cana' (non-adjacent undo's answer)");
    }

    // ── Non-vacuous pre-filter (red-team M4, perf) ────────────────────────────
    // Prove — via call-count instrumentation, not just the returned answer —
    // that a non-matching keystroke performs ZERO prefix composes.

    #[test]
    fn nonadjacent_undo_prefilter_skips_compose_for_non_repeat_key() {
        let opts = telex_opts();
        PREFIX_COMPOSE_CALLS.with(|c| c.set(0));
        let raw: Vec<char> = "canan".chars().collect(); // 'n' does not repeat 'a'
        let _ = check_fallback(&raw, &opts, true);
        assert_eq!(PREFIX_COMPOSE_CALLS.with(|c| c.get()), 0,
            "a non-repeating final key must never trigger a prefix compose");
    }

    #[test]
    fn nonadjacent_undo_prefilter_skips_compose_for_non_trigger_repeat() {
        let opts = telex_opts();
        PREFIX_COMPOSE_CALLS.with(|c| c.set(0));
        let raw: Vec<char> = "cannn".chars().collect(); // 'n' repeats but can never trigger a transform
        let _ = check_fallback(&raw, &opts, true);
        assert_eq!(PREFIX_COMPOSE_CALLS.with(|c| c.get()), 0,
            "repeating a non-transform-capable key must never trigger a prefix compose");
    }

    #[test]
    fn nonadjacent_undo_prefilter_allows_compose_for_matching_key() {
        // Sanity check for the instrumentation itself: a genuinely eligible
        // retype DOES perform (at least) one prefix compose.
        let opts = telex_opts();
        PREFIX_COMPOSE_CALLS.with(|c| c.set(0));
        let raw: Vec<char> = "canaa".chars().collect();
        let _ = check_fallback(&raw, &opts, true);
        assert!(PREFIX_COMPOSE_CALLS.with(|c| c.get()) > 0,
            "an eligible retype must perform at least one prefix compose");
    }

    // ── Last-event parity fold: base cases (P6) ───────────────────────────────
    // `is_last_event_undo` must reproduce every existing detector outcome
    // EXACTLY — these mirror the regression-critical cases from the module
    // doc / phase spec, including the red-team `vie65t5` counter-case that
    // falsified a naive single-parity-rule design.

    #[test]
    fn parity_fold_a611_is_undo() {
        let opts = vni_opts();
        let raw: Vec<char> = "a611".chars().collect();
        assert!(is_last_event_undo(&raw, &opts), "a611's tail is a fired tone-undo (-> â1)");
    }

    #[test]
    fn parity_fold_seess_is_undo() {
        let opts = telex_opts_with_ee();
        let raw: Vec<char> = "seess".chars().collect();
        assert!(is_last_event_undo(&raw, &opts), "seess's tail is a fired tone-undo (-> sês)");
    }

    #[test]
    fn parity_fold_vie65t5_same_tone_repress_is_undo() {
        // Red-team counter-case: trailing run of '5' is exactly 1 (odd) — a
        // naive one-rule parity formula would say "tone stays on", but this
        // fires via the same-tone-repress-after-coda path (check_tone_toggle
        // Path 2), not trailing-run parity.
        let opts = vni_opts();
        let raw: Vec<char> = "vie65t5".chars().collect();
        assert!(is_last_event_undo(&raw, &opts),
            "vie65t5 undoes via same-tone repress (-> viêt5), not trailing parity");
    }

    #[test]
    fn parity_fold_aaa_is_undo() {
        let opts = telex_opts();
        let raw: Vec<char> = "aaa".chars().collect();
        assert!(is_last_event_undo(&raw, &opts), "aaa's tail is a fired transform-undo (-> aa)");
    }

    #[test]
    fn parity_fold_dessign_undo_fires_at_dess_not_full_word() {
        // "dessign": the undo fires the instant raw == "dess" (2nd 's').
        // Further typing ("ign") no longer forms an undo pattern at the
        // tail — those keys are a literal append at the EXECUTOR level
        // (temp_english_mode, `ComposeStage`), not a repeated undo event,
        // so the predicate correctly says "no" once the tail has moved on.
        let opts = telex_opts();
        assert!(is_last_event_undo(&"dess".chars().collect::<Vec<_>>(), &opts),
            "the 2nd 's' in dessign is the undo-firing instant");
        assert!(!is_last_event_undo(&"dessign".chars().collect::<Vec<_>>(), &opts),
            "the full word's tail is no longer an undo pattern");
    }

    #[test]
    fn parity_fold_plain_compose_is_not_undo() {
        let opts = telex_opts();
        assert!(!is_last_event_undo(&"viet".chars().collect::<Vec<_>>(), &opts));
        assert!(!is_last_event_undo(&"a".chars().collect::<Vec<_>>(), &opts));
        assert!(!is_last_event_undo(&"cana".chars().collect::<Vec<_>>(), &opts),
            "cana composes to the attested collision 'cân' — not itself an undo event");
    }
}
