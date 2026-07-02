//! Reproducibility gate for the embedded attested-syllable bitset.
//!
//! Regenerates `pipeline/attested_data.rs` in-memory from the checked-in
//! `data/attested-syllables.txt` (via the shared `attested_gen` logic) and
//! asserts byte-identical output. A failure here means either the generated
//! file was hand-edited, or `pipeline::validation`'s onset/nucleus/coda
//! tables changed without regenerating.
//!
//! Regenerate with: `cargo run -p buttre-engine --example gen_attested_syllables`

mod attested_gen;

use std::fs;
use std::path::PathBuf;

/// Embedded tuple count as of the last regeneration (7,884 upstream entries,
/// 166 skipped — see `data/attested-syllables.txt`'s header for the
/// category breakdown). Bump intentionally when the dict or the phonology
/// tables change; an unintentional change here means the dataset drifted.
///
/// P6: +9 vs. the previous 7,642 — the former "k-coda place names" skip
/// category (búk, lăk, lắk, măk, úk, ăk, đăk, đắk, ắk) is now embedded, since
/// coda "k" has per-nucleus table rows (`pipeline::validation`, nuclei "u"
/// and "ă" only — not a blanket allowance).
const EXPECTED_POPCOUNT: usize = 7651;

fn manifest_path(parts: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for part in parts {
        path.push(part);
    }
    path
}

#[test]
fn attested_data_is_reproducible() {
    let dict_path = manifest_path(&["data", "attested-syllables.txt"]);
    let generated_path = manifest_path(&["src", "pipeline", "attested_data.rs"]);

    let dict_text = fs::read_to_string(&dict_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dict_path.display()));
    let checked_in = fs::read_to_string(&generated_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", generated_path.display()));

    let result =
        attested_gen::generate(&dict_text).unwrap_or_else(|e| panic!("generation failed: {e}"));

    assert_eq!(
        result.source, checked_in,
        "pipeline/attested_data.rs is stale — run: \
         cargo run -p buttre-engine --example gen_attested_syllables"
    );
    assert_eq!(
        result.popcount, EXPECTED_POPCOUNT,
        "embedded tuple count changed from {EXPECTED_POPCOUNT} to {} — \
         update EXPECTED_POPCOUNT if this is intentional",
        result.popcount
    );
}

/// The two skip categories the generator can classify plus the "gi"-family
/// fix must stay within the ranges observed for the current dataset — a
/// large jump would indicate a parsing regression rather than dataset noise.
#[test]
fn skip_categories_within_expected_bounds() {
    let dict_path = manifest_path(&["data", "attested-syllables.txt"]);
    let dict_text = fs::read_to_string(&dict_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dict_path.display()));
    let result =
        attested_gen::generate(&dict_text).unwrap_or_else(|e| panic!("generation failed: {e}"));

    assert_eq!(result.total_lines, 7884, "upstream dict line count changed");
    assert_eq!(result.gi_family_fixed, 11, "gi-family fixed count changed");
    // `classify_skip` is exhaustive over {VowelLess, Other} — every skip is
    // categorized by construction, so "unexplained" is 0/7884 (0.000%), well
    // under the 0.5% blocker threshold. A jump in this total is the signal
    // to re-review (see data/attested-syllables.txt's header for the current
    // per-category breakdown: 24 vowel-less, 142 loan/typo). P6 retired the
    // former "k-coda" category (9 entries) — coda "k" now decomposes via
    // per-nucleus table rows, so those 9 lines no longer reach `classify_skip`.
    assert_eq!(result.skipped.len(), 166, "total skip count changed");
}
