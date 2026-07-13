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
//! ## Wiring
//!
//! [`compose`] is a **pure** function (no hidden state, no I/O), but it is the
//! live production core: `pipeline::stages::compose_stage` calls it on every
//! keystroke, replacing the former incremental transform/tone/permutation
//! stages. Purity is what lets the recompute-from-raw model work — each
//! keystroke rebuilds the syllable from the raw buffer with no accumulated
//! inter-stage state.

mod assemble;
mod fallback;
mod segment;
mod transform;

#[cfg(test)]
mod tests;

use crate::pipeline::config::{PipelineConfig, ToneMark, ToneStyle};
use crate::pipeline::validation::{is_attested_overlay, is_shape_attested};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// Re-export public types only.
pub use segment::{AppliedMark, SegmentMode};

// Re-export: the P6 last-event parity fold, consumed by P2's evidence-based
// un-latch condition (d) in `pipeline::stages::compose_stage::should_unlatch`
// (see `plan.md`'s Combined Contract) AND by P5's word-commit signal
// collector (`buttre_core::keyboard::Keyboard`) to detect a deliberate
// "undo at commit" preference signal. Public (widened from `pub(crate)` in
// Phase 5) specifically so buttre-core — the only external consumer — can
// call it directly rather than re-deriving the same classification.
pub use fallback::is_last_event_undo;

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
    ///
    /// `Arc`-wrapped (like `tone_map`/`transform_trigger_chars`) so that
    /// cloning a whole `ComposeOpts` is O(1) refcount bumps, not HashMap
    /// deep-copies — `ComposeStage::process` clones the opts once per
    /// keystroke to merge the live learning snapshot, and the multiword
    /// window replays that per word (review follow-up: with large custom
    /// rule tables the per-key deep-copy would erode the <1 ms headroom).
    /// The tables are immutable after `from_config`, so sharing is safe.
    pub transform_rules: Arc<HashMap<String, String>>,

    /// Tone key → `ToneMark` (e.g. 's' → Acute for Telex, '1' → Acute for VNI).
    pub tone_map: Arc<HashMap<char, ToneMark>>,

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
    pub transform_trigger_chars: Arc<HashSet<char>>,

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

    /// User-attested overlay (event-sourcing-completion Phase 5): bit-indices
    /// (via [`crate::pipeline::validation::bit_index`] — the SAME id space
    /// the static attested-syllable bitset uses) for syllables the user
    /// typed directly and committed ≥3 distinct times. Consulted by ORing
    /// it with the static table at the SINGLE shared consult point,
    /// [`crate::pipeline::validation::is_attested_overlay`] — never
    /// duplicate that OR-check elsewhere. `None` (the `from_config`
    /// default, and every existing caller that doesn't opt in) is
    /// byte-identical to having no overlay at all: goldens and every
    /// pre-Phase-5 test run this way. Populated by
    /// `buttre_core::state::learning::LearningStore`; never mutated by
    /// `compose()` itself (purity: stores enter as DATA, never as a
    /// pipeline side-effect).
    pub user_attested: Option<Arc<HashSet<u32>>>,

    /// Hot-path lookup tables derived once from `transform_rules` in
    /// `from_config` (perf: `segment`/`transform` used to rebuild `format!`
    /// keys and rescan `transform_rules.keys()` on every keystroke):
    ///
    /// `(prev, trigger)` for every ALL-LOWERCASE 2-char rule key → first char
    /// of the replacement (e.g. `('a','w')` → `'ă'`). Matches the exact
    /// semantics of the old `transform_rules.get(&format!("{lc}{lc}"))`
    /// lookups, which could only ever hit all-lowercase keys.
    pub pair_rules: Arc<HashMap<(char, char), char>>,

    /// ALL-LOWERCASE 1-char rule key → full replacement string
    /// (e.g. `'w'` → `"ư"` in the Telex preset). Same lowercase-key-only
    /// contract as `pair_rules`.
    pub single_rules: Arc<HashMap<char, String>>,

    /// Keys that act as standalone transform modifiers (Telex `w`, VNI
    /// digits): second char of a 2-char rule or sole char of a 1-char rule
    /// (any case, lowercased), minus tone keys, vowels, and `'d'`.
    /// Precomputed form of the old per-keystroke `is_standalone_transform_key`
    /// scan over every rule key.
    pub standalone_keys: Arc<HashSet<char>>,

    /// Raw-sequence preference memory (event-sourcing-completion Phase 5):
    /// exact raw sequence (lowercase, already scoped to THIS method by the
    /// caller — see `LearningStore::prefs_snapshot_for_method`) → the
    /// preferred projection. Consulted at the very top of
    /// [`compose_internal`], before fallback/segment/transform/tone/gate
    /// ever run (Combined Contract). `None` by default — same determinism
    /// guarantee as `user_attested`.
    pub raw_prefs: Option<Arc<HashMap<String, Pref>>>,
}

/// A learned preference for one raw sequence (event-sourcing-completion
/// Phase 5). `Literal` outputs the raw keystrokes verbatim, exactly like
/// every other literal-output path in this module (case is re-applied
/// afterward by `apply_case_mask`, unaffected by this choice). `Composed`
/// forces the normal segment → transform → tone pipeline (steps 2-4 of
/// [`compose_internal`]), skipping BOTH the undo/toggle fallback detection
/// (step 1) and the attestation gate / English-fallback (steps 5-6) — a
/// deliberate user override must never be silently re-demoted by the same
/// heuristics it is overriding. See [`compose_forced`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pref {
    /// Prefer the literal raw keystrokes over any composed projection.
    ///
    /// Exception: IGNORED when the raw is undo-shaped
    /// (`fallback::is_last_event_undo`) — replaying raw-verbatim there
    /// ("yess") differs from the undo display the user accepted ("yes")
    /// and would hijack the double-key escape. See `compose_internal`
    /// step 0.
    Literal,
    /// Prefer the composed Vietnamese projection, ungated.
    Composed,
}

/// The two learning-derived `ComposeOpts` fields, bundled for a single
/// atomic hand-off from `buttre_core::keyboard::Keyboard` to the running
/// pipeline (event-sourcing-completion Phase 5 — see
/// `pipeline::executor::PipelineExecutor::set_learning_snapshot`).
/// `Default` (`None`/`None`) is byte-identical to having no learning store
/// at all — every golden/regression test that never touches this type
/// exercises exactly that path.
#[derive(Debug, Clone, Default)]
pub struct LearningSnapshot {
    /// See [`ComposeOpts::user_attested`].
    pub user_attested: Option<Arc<HashSet<u32>>>,
    /// See [`ComposeOpts::raw_prefs`].
    pub raw_prefs: Option<Arc<HashMap<String, Pref>>>,
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
            "hmong" => Validator::Hmong,
            "custom" => Validator::Custom,
            "none" => Validator::None,
            _ => Validator::Vietnamese,
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

        // Hot-path tables (see the field docs). `pair_rules`/`single_rules`
        // index ONLY all-lowercase rule keys — the runtime lookups they
        // replace always queried lowercased keys, so uppercase preset
        // variants ("AA"→"Â") were unreachable through them by construction.
        let mut pair_rules: HashMap<(char, char), char> = HashMap::new();
        let mut single_rules: HashMap<char, String> = HashMap::new();
        let mut standalone_keys: HashSet<char> = HashSet::new();
        for (k, v) in &config.transform_rules {
            let kl: Vec<char> = k.to_lowercase().chars().collect();
            let is_lowercase_key = !k.chars().any(char::is_uppercase);
            let modifier = match kl.as_slice() {
                [a, b] => {
                    if is_lowercase_key {
                        if let Some(first) = v.chars().next() {
                            pair_rules.insert((*a, *b), first);
                        }
                    }
                    Some(*b)
                }
                [a] => {
                    if is_lowercase_key {
                        single_rules.insert(*a, v.clone());
                    }
                    Some(*a)
                }
                _ => None,
            };
            if let Some(c) = modifier {
                if !config.tone_map.contains_key(&c)
                    && !crate::vowel::cluster::is_vowel(c)
                    && c != 'd'
                {
                    standalone_keys.insert(c);
                }
            }
        }

        Self {
            transform_rules: Arc::new(config.transform_rules.clone()),
            tone_map: Arc::new(config.tone_map.clone()),
            tone_style: config.get_tone_style(),
            segment_mode,
            validator,
            tone_enabled: !config.tone_map.is_empty(),
            transform_trigger_chars: Arc::new(transform_trigger_chars),
            pair_rules: Arc::new(pair_rules),
            single_rules: Arc::new(single_rules),
            standalone_keys: Arc::new(standalone_keys),
            attest_non_adjacent,
            user_attested: None,
            raw_prefs: None,
        }
    }

    /// True when ANY character in `raw` (any case) can trigger a tone or
    /// transform mark under this method's tables — config-driven, no
    /// hardcoded key set (mirrors the private per-call idioms in
    /// `compose::fallback`/`pipeline::stages::compose_stage`, generalized
    /// to a whole slice). Used by `buttre_core::state::learning`'s
    /// preference min-specificity floor (event-sourcing-completion Phase 5,
    /// red-team M4): a learned preference's raw sequence must contain at
    /// least one such key, or a single stray undo on an ultra-common short
    /// word (`"ba"`) could force-literal it forever.
    pub fn has_trigger_key(&self, raw: &[char]) -> bool {
        raw.iter().any(|&c| {
            let lc = c.to_ascii_lowercase();
            self.tone_map.contains_key(&lc)
                || self.transform_rules.keys().any(|rule| {
                    rule.chars()
                        .last()
                        .is_some_and(|last| last.to_ascii_lowercase() == lc)
                })
        })
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

    /// The tone key that was actually applied to produce `text`, paired with
    /// its index in `raw` — the tone counterpart to `applied_marks` for P2's
    /// evidence-based un-latch condition (c) ("was the completing keystroke
    /// the LAST key of the raw buffer"). `None` whenever no tone fired,
    /// including every fallback/undo/English-revert output (mirrors
    /// `applied_marks` being empty on those same paths) — this is what stops
    /// a plain literal-append letter from ever satisfying condition (c)
    /// (red-team m1).
    ///
    /// `Segment::tones` is a bare `Vec<char>` with no position tracking, so
    /// the index is recovered here as the LAST raw position whose
    /// (lowercased) value matches the applied tone key. This is exact for
    /// every shipped config: `has_seen_vowel` is monotonic during `segment`,
    /// and a genuine tone-key character is never intercepted by the a/e/o/d
    /// doubling branches (disjoint key sets in every shipped Telex/VNI
    /// preset) — so the applied tone key's own occurrence is necessarily the
    /// last raw index carrying that character value.
    pub consumed_tone: Option<(char, usize)>,

    /// `true` when this result passed through the non-adjacent attestation
    /// gate's demote branch (Step 5) — a non-adjacent mark was ATTEMPTED
    /// and then stripped, as opposed to no candidate mark ever existing.
    /// Distinguishes "the algorithm inferred and then rejected a mark" from
    /// "nothing was ever inferred at all", which `applied_marks` alone
    /// cannot: after a demote, `applied_marks` reports the marks of the
    /// RE-derived (mark-free) parse, so an empty `applied_marks` is
    /// ambiguous between the two cases (event-sourcing-completion Phase 5,
    /// consumed by `buttre_core::keyboard::Keyboard`'s anti-feedback rule
    /// (i): an automatic demote must never be mistaken for genuinely
    /// direct, uninferred typing). Propagated through
    /// [`try_elongation_fallback`]'s recursive base compose; always
    /// `false` for [`compose_forced`] (a deliberate override never
    /// consults the gate at all).
    pub demoted: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Recompute the syllable output from the full raw key buffer.
///
/// ## Contract
///
/// - Pure: no global state read or written, no I/O.
/// - Deterministic: same `raw` + `opts` always yields the same `ComposeResult`.
/// - `raw` may be empty; returns `ComposeResult { text: "", temp_english: false, .. }`.
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
    compose_internal(raw, opts, true, false)
}

/// Recompute the syllable output as a **closed word-boundary projection**
/// (event-sourcing-completion Phase 3 — "word-boundary final repair").
///
/// Identical to [`compose`] except the attestation gate ([`passes_attestation_gate`])
/// clamps digit triggers whose tone NEVER ARRIVED to exact attestation — the
/// shape-relaxation `compose` grants digit triggers exists (mid-word) to
/// avoid flicker while a tone key is still expected on the next keystroke.
/// A word that has reached a boundary (separator, Enter, or any other commit
/// point) expects no further keystrokes, so a shape-only inferred mark whose
/// tone never arrived (VNI `"nhat6"` → open-projection `"nhât"`) is demoted to
/// its literal raw form (`"nhat6"`) instead of staying on the shape-attested
/// intermediate. When the tone HAS arrived, shape attestation still suffices
/// at the boundary (`"d9ech56"` → `"đệch"` commits even though only `"đếch"`
/// is dictionary-attested) — see [`passes_attestation_gate`]'s digit branch.
///
/// Callers compare this against the currently-displayed (open-projection)
/// text and only emit a correction when the two differ — see
/// `pipeline::executor::PipelineExecutor::boundary_repair` and
/// `buttre_core::keyboard::Keyboard::compose_one_word`.
pub fn compose_closed(raw: &[char], opts: &ComposeOpts) -> ComposeResult {
    compose_internal(raw, opts, true, true)
}

/// Core recompute engine, parameterized by `allow_nonadjacent` — the ONE
/// recursion-guard flag threaded through every re-entry point: the
/// attestation-gate demote below, [`try_elongation_fallback`]'s internal
/// recompute, and the fallback prefix reconstruction in
/// [`fallback::check_fallback`] (closing the red-team C2 bypass: those
/// helpers no longer rebuild prefixes ungated) — and by `closed`, the
/// word-boundary projection flag consumed by [`passes_attestation_gate`]
/// (phase-03-boundary-repair; see [`compose_closed`]).
///
/// `false` means "this call is already a demoted/gated recompute — do not
/// gate or demote again". Since [`segment::segment`] with
/// `allow_nonadjacent=false` extracts NO mark that would be flagged
/// non-adjacent, the gate condition below (`applied.iter().any(non_adjacent)`)
/// can never be true inside a `false` call: the demote/gate re-entry is
/// bounded at depth 1. (The one exception is [`try_elongation_fallback`],
/// which re-enters on a STRICTLY SHORTER buffer — so it always terminates,
/// but its depth is O(raw-len), not 1. This is only reachable from a
/// top-level call and is bounded by the ≤16-char syllable cap.)
fn compose_internal(
    raw: &[char],
    opts: &ComposeOpts,
    allow_nonadjacent: bool,
    closed: bool,
) -> ComposeResult {
    if raw.is_empty() {
        return ComposeResult {
            text: String::new(),
            temp_english: false,
            applied_marks: Vec::new(),
            consumed_tone: None,
            demoted: false,
        };
    }

    // Step 0 — P5 preference short-circuit (Combined Contract: pref lookup
    // runs before fallback). Gated on `allow_nonadjacent` — the SAME
    // top-level recursion-guard threaded through every other re-entry
    // point: never re-consulted inside the attestation-gate demote
    // recursion below (that call is re-deriving THIS SAME raw's
    // already-decided top-level outcome), and structurally unreachable from
    // `fallback`'s prefix-reconstruction helpers (they call
    // segment/transform/assemble directly and never re-enter
    // `compose_internal` at all — see `compose::fallback`'s module doc).
    if allow_nonadjacent {
        if let Some(pref) = lookup_pref(raw, opts) {
            match pref {
                // An undo-shaped raw never replays a Literal pref: the display
                // the user accepted when it was recorded is the pipeline's own
                // undo output ("yess" → "yes"), while the raw-verbatim
                // projection ("yess") would permanently hijack the double-key
                // escape — the exact contract pinned by telex-fallback.txt.
                // Falling through re-derives that undo from raw (step 1).
                Pref::Literal if fallback::is_last_event_undo(raw, opts) => {}
                Pref::Literal => {
                    return ComposeResult {
                        text: raw.iter().collect(),
                        temp_english: true,
                        applied_marks: Vec::new(),
                        consumed_tone: None,
                        demoted: false,
                    }
                }
                Pref::Composed => return compose_forced(raw, opts),
            }
        }
    }

    // Step 1 — fallback / undo detection (reads raw counts only).
    let fb = fallback::check_fallback(raw, opts, allow_nonadjacent);
    if fb.is_handled {
        return ComposeResult {
            text: fb.text,
            temp_english: fb.temp_english,
            applied_marks: Vec::new(),
            consumed_tone: None,
            demoted: false,
        };
    }

    // Step 1.5 — interior spent-undo fold: a deliberate undo (tone OR
    // transform) that fired EARLIER in raw (no longer at the tail) makes
    // everything after it a literal append (Unikey multi-level toggle: undo
    // is final for the rest of the word). Without this, segment would
    // re-collect the spent pair as live marks and resurrect the very mark
    // the user removed ("resse" → "rế" instead of "rese", "rowws" → "rớ"
    // instead of "rows"). See `fallback::check_spent_undo`.
    //
    // Gated on `allow_nonadjacent` like step 0: whether the word is in the
    // post-undo literal zone is decided ONCE, at top level (with the
    // more-permissive gate inside the prefix visibility check) — the
    // attestation-gate demote recursion below must not re-litigate it with
    // the stricter flag and diverge from that decision.
    if allow_nonadjacent {
        if let Some(fb) = fallback::check_spent_undo(raw, opts, true) {
            return ComposeResult {
                text: fb.text,
                temp_english: fb.temp_english,
                applied_marks: Vec::new(),
                consumed_tone: None,
                demoted: false,
            };
        }
    }

    // Step 2 — segment.
    let seg = segment::segment(raw, opts, allow_nonadjacent);

    // Step 3 — apply transform marks.
    let (transformed, applied) = transform::apply_transforms(&seg.base, &seg.transforms, opts);

    // Step 4 — apply tone (skip for DirectMap / no tone_map).
    let mut consumed_tone = None;
    let text = if opts.tone_enabled && !seg.tones.is_empty() {
        // Vietnamese: last tone wins.
        let last_tone_key = *seg.tones.last().unwrap();
        match assemble::apply_tone(&transformed, last_tone_key, opts) {
            Some(toned) => {
                consumed_tone =
                    last_tone_raw_pos(raw, last_tone_key).map(|pos| (last_tone_key, pos));
                toned
            }
            None => transformed,
        }
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
        && !passes_attestation_gate(&text, &applied, closed, opts)
    {
        // Demote: recompose with non-adjacent mark extraction disabled at the
        // source (segment). This re-derives the base/marks split from raw —
        // it does NOT mutate `text` — so already-completed ADJACENT
        // transforms elsewhere in the word are preserved untouched.
        //
        // `demoted` is forced `true` on whatever this recursive call
        // returns (event-sourcing-completion Phase 5): it distinguishes "a
        // non-adjacent mark was attempted and stripped" from "nothing was
        // ever inferred" — see `ComposeResult::demoted`'s doc. Forcing it
        // HERE, at the single call site, correctly covers every path the
        // recursive call might take internally (including through
        // `try_elongation_fallback`), without needing to thread it through
        // each of those paths individually.
        let mut demoted_result = compose_internal(raw, opts, false, closed);
        demoted_result.demoted = true;
        return demoted_result;
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
        if let Some(elong) = try_elongation_fallback(raw, opts, allow_nonadjacent, closed) {
            return elong;
        }
        let literal: String = raw.iter().collect();
        return ComposeResult {
            text: literal,
            temp_english: true,
            applied_marks: Vec::new(),
            consumed_tone: None,
            demoted: false,
        };
    }

    ComposeResult {
        text,
        temp_english: false,
        applied_marks: applied,
        consumed_tone,
        demoted: false,
    }
}

/// Step 0 (Combined Contract) — a raw sequence with a stored, deliberate
/// preference short-circuits straight to the preferred projection, before
/// fallback/segment/transform/tone/gate ever run. Lookup key is the raw
/// buffer collected verbatim: `compose()`'s callers always hand it
/// already-lowercased raw (see `ComposeStage`'s doc on `char_buffer`
/// normalization), and `raw_prefs` snapshots
/// (`buttre_core::state::learning::LearningStore::prefs_snapshot_for_method`)
/// are built pre-lowercased and pre-scoped to the current method — this
/// function does no further normalization itself.
fn lookup_pref(raw: &[char], opts: &ComposeOpts) -> Option<Pref> {
    let prefs = opts.raw_prefs.as_ref()?;
    let key: String = raw.iter().collect();
    prefs.get(&key).copied()
}

/// Apply a `Pref::Composed` override: run segment → transform → tone
/// (steps 2-4 of [`compose_internal`]) UNGATED — no undo/toggle detection,
/// no attestation gate, no English-fallback. See [`Pref`]'s doc for why a
/// deliberate override must not be re-demoted by the same heuristics it is
/// overriding.
fn compose_forced(raw: &[char], opts: &ComposeOpts) -> ComposeResult {
    let seg = segment::segment(raw, opts, true);
    let (transformed, applied) = transform::apply_transforms(&seg.base, &seg.transforms, opts);
    let mut consumed_tone = None;
    let text = if opts.tone_enabled {
        match seg.tones.last() {
            Some(&last_tone_key) => match assemble::apply_tone(&transformed, last_tone_key, opts) {
                Some(toned) => {
                    consumed_tone =
                        last_tone_raw_pos(raw, last_tone_key).map(|pos| (last_tone_key, pos));
                    toned
                }
                None => transformed,
            },
            None => transformed,
        }
    } else {
        transformed
    };
    ComposeResult {
        text,
        temp_english: false,
        applied_marks: applied,
        consumed_tone,
        demoted: false,
    }
}

/// Recover the raw index of the tone key actually consumed by
/// `assemble::apply_tone` — see [`ComposeResult::consumed_tone`] for why a
/// value-based search on `raw` is exact for every shipped config.
fn last_tone_raw_pos(raw: &[char], tone_key: char) -> Option<usize> {
    let tone_lc = tone_key.to_ascii_lowercase();
    raw.iter().rposition(|&c| c.to_ascii_lowercase() == tone_lc)
}

/// True when `text` passes the non-adjacent attestation gate: either no
/// applied mark is flagged non-adjacent, or the composed syllable is
/// attested.
///
/// ## Word-boundary closed projection (Phase 3)
///
/// `closed` is the word-boundary flag from [`compose_closed`]. When `true`,
/// the digit-trigger shape-relaxation below is disabled for TONELESS text —
/// a closed word expects no further keystrokes, so a shape-only match whose
/// tone never arrived has no excuse left to survive on ("nhat6" → "nhât"
/// restores its literal raw). When the composed text ALREADY carries a tone,
/// shape attestation still suffices at the boundary: the "tone hasn't
/// arrived yet" rationale is moot, and clamping to exact attestation there
/// rejected deliberately-typed informal words ("d9ech56" → "đệch", shape
/// attested via "đếch") while the adjacent key order ("d9e6ch5", marks
/// unflagged) committed the same word — commit results must not depend on
/// key order. The demote path taken when the gate fails is byte-identical
/// to the open-projection gate's (`compose_internal` recurses with
/// `allow_nonadjacent=false`, unaffected by `closed`).
///
/// ## Trigger classification (P6 gate hardening)
///
/// ASCII-digit triggers relax to a SHAPE match (`is_shape_attested`, any
/// tone): the tone key often arrives AFTER the digit mark (`nhat61`), so the
/// exact tone is not yet known when the gate must decide, and a real VNI
/// digit trigger cannot occur inside an English word anyway. EVERYTHING ELSE
/// — Telex-style alphabetic triggers AND any non-alphabetic, non-digit
/// trigger (punctuation, in a hypothetical custom config) — requires an EXACT
/// match (`is_attested`, whatever tone `text` currently carries): a real
/// Telex mark key is also a base letter or a tone key, so a false alphabetic
/// match would already have to look like a real Vietnamese word, and a
/// punctuation trigger has no VNI-style "tone hasn't arrived yet" excuse for
/// relaxing to shape.
///
/// Before this hardening, the classification was inverted — `is_alphabetic()
/// → exact, else → shape` — which correctly handled Telex (alphabetic) and
/// VNI (digit) but wrongly RELAXED any non-digit, non-alphabetic trigger
/// (e.g. a punctuation trigger in a custom config) to shape-attestation too.
/// Classifying on `is_ascii_digit()` instead closes that gap while leaving
/// Telex and VNI byte-identical (Telex triggers are never digits; VNI
/// triggers are always digits).
///
/// ## Intrinsic trade-off (delayed-mark Telex live feedback)
///
/// The exact-match requirement for non-digit triggers means a Telex
/// delayed/non-adjacent mark whose toneless form is unattested does NOT show
/// its diacritic until the tone key is typed: `viete` composes to literal
/// `viete` at that frame (bare `viêt` is not a word), then `+j` recomputes to
/// `việt`. The FINAL output is always correct; only the intermediate frame of
/// the less-common delayed-mark style defers feedback. This cannot be
/// "fixed" by relaxing Telex to shape-attestation — `viete→viêt` and
/// `data→dât` are the identical non-adjacent operation, and `dât`'s shape is
/// attested (via `dật`/`dất`), so shape-relaxing Telex would reopen the very
/// `data→dât` bug this gate exists to close. VNI escapes the trade-off only
/// because its digit trigger cannot occur inside an English word.
///
/// ## Assumption
///
/// Relies on each method being single-trigger-kind (Telex: all alphabetic;
/// VNI: all digit). A hypothetical custom config mixing digit and non-digit
/// triggers would relax a co-occurring non-digit mark to the shape check. No
/// shipped preset does this.
///
/// ## Overlay (event-sourcing-completion Phase 5)
///
/// The EXACT-match branch consults `opts.user_attested` through
/// [`is_attested_overlay`] — the single shared consult point P2's
/// evidence-based un-latch probe (`pipeline::stages::compose_stage::
/// should_unlatch`) also uses, so both the word-boundary closed gate and
/// the un-latch probe agree on what counts as attested. The SHAPE-relaxed
/// branch (digit triggers, open projection only) is deliberately NOT
/// overlay-aware: overlay entries are individual exact syllables (a
/// specific onset/nucleus/coda/TONE tuple), not shape families, so
/// widening a shape check to the overlay would double-relax an
/// already-permissive path for no corresponding user signal.
fn passes_attestation_gate(
    text: &str,
    applied: &[AppliedMark],
    closed: bool,
    opts: &ComposeOpts,
) -> bool {
    let mut flagged = applied.iter().filter(|m| m.non_adjacent).peekable();
    if flagged.peek().is_none() {
        return true;
    }
    // Digit (VNI) triggers relax to SHAPE attestation — mid-word always, and
    // at the closed boundary too PROVIDED the composed text already carries a
    // tone. The boundary's exact-attestation clamp exists solely for the
    // "tone never arrived" intermediates ("nhat6" → shape-only "nhât" must
    // restore its literal raw at commit); once a tone HAS been deliberately
    // typed, demoting a shape-attested word because its exact tone combo is
    // not in the dictionary would be aggressive spell-check — it rejected
    // real informal words ("d9ech56" → "đệch", shape attested via "đếch"),
    // and it made commit results depend on KEY ORDER: adjacent "d9e6ch5"
    // (marks unflagged, gate never consulted) committed "đệch" while the
    // delayed order reverted to raw. A digit trigger cannot occur inside an
    // English word, so this relaxation has no English-safety cost.
    if flagged.all(|m| m.key.is_ascii_digit()) && (!closed || carries_tone(text)) {
        is_shape_attested(text)
    } else {
        is_attested_overlay(text, opts.user_attested.as_deref())
    }
}

/// True when any char of `text` carries one of the five Vietnamese tone
/// marks — i.e. a tone key has already been applied to this composition.
fn carries_tone(text: &str) -> bool {
    text.chars()
        .any(|c| crate::tone::strip(c).1 != ToneMark::None)
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
fn try_elongation_fallback(
    raw: &[char],
    opts: &ComposeOpts,
    allow_nonadjacent: bool,
    closed: bool,
) -> Option<ComposeResult> {
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
    let base = compose_internal(base_raw, opts, allow_nonadjacent, closed);
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
    let suffix: String = std::iter::repeat_n(last, run).collect();
    Some(ComposeResult {
        text: format!("{}{}", base.text, suffix),
        temp_english: true,
        applied_marks: Vec::new(),
        consumed_tone: None,
        // Propagate the base compose's own demote status (event-sourcing-
        // completion Phase 5): elongation recurses into `compose_internal`
        // on a shorter, still-top-level buffer, which may itself have gone
        // through the gate/demote branch — an honest `demoted` flag must
        // reflect that, not silently reset to `false`.
        demoted: base.demoted,
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
    use crate::pipeline::validation::{
        normalize_vietnamese, normalize_vietnamese_into, parts_are_valid, split_parts,
    };
    let collapsed = collapse_adjacent_repeats(text);
    // Zero-alloc path (perf: this runs on nearly every composed keystroke):
    // normalize into a stack buffer and borrow the (onset, nucleus, coda)
    // slices — the SyllableStructure form cost 4 heap allocations per call.
    let mut buf = [0u8; 64];
    let heap_norm; // slow-path storage, lives as long as `normalized`
    let normalized: &str = match normalize_vietnamese_into(&collapsed, &mut buf) {
        Some(n) => n,
        None => {
            heap_norm = normalize_vietnamese(&collapsed);
            &heap_norm
        }
    };
    let (onset, nucleus, coda) = split_parts(normalized);
    if parts_are_valid(onset, nucleus, coda) {
        return true;
    }
    // Consonant-only prefix: onset present, nucleus/coda not yet typed.
    if nucleus.is_empty() && coda.is_empty() && !onset.is_empty() {
        return true;
    }
    // KEEP (phase-03 adjudication table — REVISED from the original plan's
    // DELETE verdict; re-adjudicated after a confirmed regression, see below).
    //
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
    //
    // NOT subsumed by the P2 non-adjacent attestation gate: that gate (Step 5
    // in `compose_internal`) only evaluates `applied_marks`, which is empty at
    // this intermediate point — the tone fired via `assemble::apply_tone`, but
    // the digit transform mark has not been typed yet, so nothing has been
    // "applied" for the gate to check. This function (Step 6) is the ONLY
    // place that sees this state. Confirmed by direct executor-level replay
    // (`vni_edge_cases::test_vni_mieng16_incremental_no_flicker`): deleting
    // this branch made `mieng1`→(latches English)→`6` produce literal
    // "mieng16" instead of "miếng" — a real regression, not merely a
    // theoretical one, since the golden corpus does not happen to exercise
    // this specific tone-before-transform keystroke ordering.
    if !opts.transform_trigger_chars.is_empty()
        && nucleus == "ie"
        && matches!(coda, "c" | "m" | "n" | "ng" | "p" | "t")
    {
        return true;
    }
    false
}

/// Collapse runs of consecutive identical characters down to one
/// ("khôngggg" → "không", "trờiii" → "trời").
///
/// Borrows when there is nothing to collapse — the overwhelmingly common
/// case on the per-keystroke hot path (a valid Vietnamese syllable never has
/// two identical adjacent letters in its final form).
fn collapse_adjacent_repeats(s: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    let mut prev: Option<char> = None;
    let has_repeat = s.chars().any(|c| {
        let repeat = prev == Some(c);
        prev = Some(c);
        repeat
    });
    if !has_repeat {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if Some(c) != prev {
            out.push(c);
            prev = Some(c);
        }
    }
    Cow::Owned(out)
}
