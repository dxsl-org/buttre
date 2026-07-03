//! Purity-invariant enforcement (event-sourcing-completion Phase 8).
//!
//! `AGENTS.md`'s "Event-sourcing purity (INVARIANT)" rule: the raw keystroke
//! buffer is the immutable event log; displayed text is a pure projection.
//! No one-way flag/latch that, once set, prevents recomputation from raw.
//! `TypingContext` gaining a new persistent `bool` field is the textbook red
//! flag this invariant exists to catch — every past purity bug in this
//! codebase (sticky `temp_english`, the retired dual-engine's leftover
//! flags) started life as "just one more bool".
//!
//! ## Enforcement mechanism — not grep theater
//!
//! This is not a human tally that silently drifts from the real source: the
//! test below parses the ACTUAL `struct TypingContext { ... }` body out of
//! `pipeline/context.rs` (via `include_str!`, scoped to the real struct's
//! brace-delimited body, not a blind whole-file scan) and mechanically
//! counts `pub <name>: bool,` declarations within it. Any PR that adds,
//! removes, or renames a bool field changes this count and fails CI until
//! the change is deliberate — bump [`EXPECTED_BOOL_FIELD_COUNT`] AND add a
//! justification row to the table below in the SAME commit.
//!
//! See also `scripts/check-purity.ps1` — a companion deny-script that
//! enforces two orthogonal rules (temp_english_mode's assignment sites, and
//! a frozen count of `_mode: bool` fields workspace-wide) that this
//! single-struct, single-crate test cannot see.

const CONTEXT_SRC: &str = include_str!("../src/pipeline/context.rs");

/// Number of `pub <field>: bool` fields on `TypingContext`, frozen at 3 by
/// the event-sourcing-completion Phase 8 purity audit.
///
/// Deleted 5 dead one-way bools left over from the retired dual-engine
/// (Permutation/Reconciliation/Retrofix stages, removed in an earlier
/// phase) — grep-verified ZERO production readers anywhere in the crate
/// (only ever written at `TypingContext::new()`/`clear()`, never consulted
/// by `ComposeStage` or any other pipeline stage):
/// `last_was_undo`, `just_undid`, `has_pending_marks`,
/// `had_successful_transform`, `used_permutation_result`. Three test files
/// outside this struct's own module referenced `last_was_undo` in
/// debug-print / tautological-fixture assertions only (no behavioral
/// coverage lost) and were updated alongside this deletion.
///
/// ## Field-justification table (every SURVIVING bool + why it is not a
/// purity violation)
///
/// | Field | Why it survives |
/// |---|---|
/// | `temp_english_mode` | The ONE legacy latch the purity invariant explicitly calls out (`AGENTS.md`) — but it is DERIVED, not one-way: `pipeline::stages::compose_stage`'s evidence-based un-latch (event-sourcing-completion Phase 2) re-probes `compose(&full_raw)` on every trigger-eligible keystroke while set, and clears it the instant the evidence says Vietnamese (`should_unlatch`). It is a transient CACHE of `compose()`'s own most recent verdict, re-evaluated from the full raw buffer, never a valve that blocks recomputation. |
/// | `showing_candidates` | UI presentation state for Stage 6 (Nôm dictionary lookup) — mirrors whether `candidates` is non-empty at the moment of the last lookup. Transient per-session UI bookkeeping, not a decision about how to interpret raw keystrokes. |
/// | `learning_enabled` | A session-level USER SETTING (mirrors `Settings::learning_enabled`), not something derived from raw at all — same category as any other config-derived field on `ComposeOpts` (e.g. `tone_enabled`). Configuration is data, not a re-derivation target. |
///
/// ## Other legitimate non-raw-derived state (outside `TypingContext`, for
/// completeness — these are NOT counted here, but exist for the same
/// structural reason `learning_enabled` does: they are DATA, not decisions)
///
/// - **P4's `Keyboard::toggle_map`** (`buttre_core::keyboard::Keyboard`) —
///   a map of USER EVENTS (deliberate toggle actions), not an inference
///   over raw. The toggle itself IS part of the event log in spirit (a
///   user-initiated projection override); it is folded into `compose_window`
///   by parity every recompute, never mutating composed text outside the
///   normal diff path.
/// - **P5's `LearningStore` (overlay + prefs)** (`buttre_core::state::learning`)
///   — CROSS-SESSION data (what the user has taught the engine over time),
///   entering `compose()` as `Arc` snapshots inside `ComposeOpts`, never as
///   a global or a hidden side channel. `compose()` stays pure: same
///   raw + same opts (incl. the snapshot) always yields the same result.
const EXPECTED_BOOL_FIELD_COUNT: usize = 3;

/// Extract the brace-delimited body of `pub struct <name> { ... }` from
/// `src`. Scoped extraction (not a whole-file scan) so `bool` mentions
/// elsewhere in the file — doc comments, other structs, the `flags:
/// HashMap<String, bool>` field — can never contaminate the count.
fn struct_body<'a>(src: &'a str, struct_name: &str) -> &'a str {
    let marker = format!("pub struct {struct_name} {{");
    let start = src
        .find(&marker)
        .unwrap_or_else(|| panic!("`pub struct {struct_name}` not found in context.rs — has it been renamed or moved?"))
        + marker.len();
    let rest = &src[start..];
    // Every struct in this file is formatted with its closing brace alone
    // on a line at column 0 — safe to scan for the first such occurrence.
    let end = rest
        .find("\n}")
        .unwrap_or_else(|| panic!("no closing brace found for `struct {struct_name}` — malformed source?"));
    &rest[..end]
}

/// Count field declarations in `body` whose type is exactly `bool`.
///
/// Hardened per the adversarial review (the naive `starts_with("pub ") &&
/// ends_with(": bool,")` form was evadable by a PRIVATE bool, a last field
/// with no trailing comma, or `pub x: bool ,` spacing): this version skips
/// comments/attributes, splits each remaining line at its FIRST `:` (the
/// field separator — path colons like `std::collections::…` only ever
/// appear in the TYPE half), strips a trailing comma, and requires the type
/// to be exactly `bool` after trimming. Composite types
/// (`HashMap<String, bool>`, `Option<bool>`) still never match, and
/// visibility no longer matters — a private one-way bool can't slip past.
fn count_bool_fields(body: &str) -> usize {
    body.lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("#[")
            {
                return false;
            }
            let Some((_name, ty)) = trimmed.split_once(':') else {
                return false;
            };
            ty.trim().trim_end_matches(',').trim() == "bool"
        })
        .count()
}

#[test]
fn typing_context_bool_field_count_is_frozen() {
    let body = struct_body(CONTEXT_SRC, "TypingContext");
    let actual = count_bool_fields(body);
    assert_eq!(
        actual, EXPECTED_BOOL_FIELD_COUNT,
        "TypingContext's `bool` field count changed ({actual} found, {EXPECTED_BOOL_FIELD_COUNT} expected). \
         A new bool is a purity red flag (AGENTS.md's event-sourcing invariant): if it can be derived from \
         raw, derive it instead of adding a field. If it is genuinely a legitimate exception (like \
         `temp_english_mode`'s derived-every-keystroke cache), add a justification row to this file's \
         doc comment table AND bump EXPECTED_BOOL_FIELD_COUNT in the SAME commit."
    );
}

/// Sanity-checks the extraction helpers themselves against a synthetic
/// struct, independent of `TypingContext`'s actual current shape — proves
/// `struct_body`/`count_bool_fields` do the right thing on ordinary,
/// unambiguous input before trusting them against the real source.
#[test]
fn extraction_helpers_are_correct_on_synthetic_input() {
    let synthetic = "\
pub struct Other {
    pub not_this_one: String,
}

pub struct Sample {
    pub flag_a: bool,
    pub name: String,
    pub flags: std::collections::HashMap<String, bool>,
    pub maybe: Option<bool>,
    pub flag_b: bool,
    // the three evasion shapes the naive counter missed (review LOW):
    hidden_private: bool,
    pub weird_spacing: bool ,
    pub no_trailing_comma: bool
}
";
    let body = struct_body(synthetic, "Sample");
    assert_eq!(
        count_bool_fields(body),
        5,
        "must count flag_a, flag_b, AND the three evasion shapes (private, \
         `bool ,` spacing, no trailing comma) — never the HashMap/Option<bool> composites"
    );
}

/// Guards against the count silently reading 0 because the struct marker or
/// field-line format drifted (e.g. a formatting change to one-per-line
/// trailing commas) — a `0` result must never be mistaken for "no bool
/// fields left", since `TypingContext` currently has several.
#[test]
fn typing_context_struct_body_is_actually_found() {
    let body = struct_body(CONTEXT_SRC, "TypingContext");
    assert!(body.contains("temp_english_mode"), "extracted body must contain the struct's real fields");
    assert!(count_bool_fields(body) > 0, "must find at least one bool field — a 0 result means the parser broke, not that purity was achieved");
}
