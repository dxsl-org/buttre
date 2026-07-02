//! Shared attested-syllable bitset generation logic.
//!
//! **Single source of truth** for turning `data/attested-syllables.txt` into
//! the `pipeline::attested_data` bitset source. Included as a module by both
//! `examples/gen_attested_syllables.rs` (writes the result to disk) and
//! `tests/attested_repro_test.rs` (regenerates in-memory and byte-compares
//! against the checked-in file), so the two can never diverge — mirrors the
//! `tests/golden/corpus_data` pattern used by `buttre-core`'s gen_golden.
//!
//! The onset/nucleus/coda/tone id decomposition itself is NOT duplicated
//! here — every id comes from `pipeline::validation`, which is the only
//! place that owns the phonology tables.

use buttre_engine::pipeline::validation::{
    bit_index, decompose_ids, normalize_vietnamese, SyllableStructure, NUM_CODAS, NUM_NUCLEI,
    NUM_ONSETS, NUM_TONES,
};

/// Why a dict line failed to decompose into a structurally valid syllable.
///
/// P6 retired the `KCoda` category: coda "k" now has table entries
/// (`pipeline::validation::VALID_CODAS`), so every word that used to fail
/// decomposition ONLY because of a trailing "k" (e.g. "đắk") now decomposes
/// successfully and is never passed to [`classify_skip`] at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// No Vietnamese vowel letter anywhere in the word — a consonant-only
    /// abbreviation (e.g. "đtbxh").
    VowelLess,
    /// Everything else: loanwords, typos, non-standard consonant clusters.
    Other,
}

/// Result of one full generation pass over the dict text.
pub struct GenerationResult {
    /// Full Rust source for `pipeline/attested_data.rs`.
    pub source: String,
    /// Number of unique (onset, nucleus, coda, tone) tuples embedded.
    pub popcount: usize,
    /// Total non-comment, non-blank lines read from the dict.
    pub total_lines: usize,
    /// "gi"-family words (gì, gìn, gích, gíp, …) embedded thanks to the
    /// `extract_onset` fix — informational only, these are not skips.
    pub gi_family_fixed: usize,
    /// Words that failed to decompose, paired with why.
    pub skipped: Vec<(String, SkipReason)>,
}

/// Parse `dict_text` (one syllable per line; blank lines and `#`-comment
/// lines ignored) and build the attested-syllable bitset.
///
/// Returns `Err` if any non-comment line contains a non-letter character —
/// a Trojan-source guard so a poisoned dict line (digits, punctuation, bidi
/// control characters) aborts generation instead of being silently absorbed
/// or classified as an ordinary skip.
pub fn generate(dict_text: &str) -> Result<GenerationResult, String> {
    let total_bits = NUM_ONSETS * NUM_NUCLEI * NUM_CODAS * NUM_TONES;
    let words = total_bits.div_ceil(64);
    let mut bits = vec![0u64; words];
    let mut popcount = 0usize;
    let mut gi_family_fixed = 0usize;
    let mut skipped: Vec<(String, SkipReason)> = Vec::new();
    let mut total_lines = 0usize;

    for (line_no, raw_line) in dict_text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.chars().all(|c| c.is_alphabetic()) {
            return Err(format!(
                "line {}: non-letter character in {line:?} (Trojan-source guard)",
                line_no + 1
            ));
        }
        total_lines += 1;

        let structure = SyllableStructure::parse(line);
        match decompose_ids(line) {
            Some((o, n, c, t)) => {
                if structure.onset == "g" && structure.nucleus.starts_with('i') {
                    gi_family_fixed += 1;
                }
                let idx = bit_index(o, n, c, t);
                let mask = 1u64 << (idx % 64);
                if bits[idx / 64] & mask == 0 {
                    popcount += 1;
                }
                bits[idx / 64] |= mask;
            }
            None => skipped.push((line.to_string(), classify_skip(line))),
        }
    }

    Ok(GenerationResult {
        source: render_source(&bits, popcount),
        popcount,
        total_lines,
        gi_family_fixed,
        skipped,
    })
}

/// Categorize a decompose failure using the same structural signal a human
/// reviewer would use — never a per-word lookup table. The only remaining
/// category (`VowelLess`) is a property of the raw word text; P6 retired the
/// structural `KCoda` category (see `SkipReason` doc), so this no longer
/// needs the parsed `SyllableStructure`.
fn classify_skip(word: &str) -> SkipReason {
    let has_vowel = normalize_vietnamese(word).chars().any(|c| {
        matches!(
            c,
            'a' | 'ă' | 'â' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y'
        )
    });
    if !has_vowel {
        return SkipReason::VowelLess;
    }
    SkipReason::Other
}

/// Render the generated `attested_data.rs` source. Emits numerals only — no
/// dict strings are echoed into the built artifact. Dimension constants are
/// imported from `pipeline::validation` rather than re-declared, so this file
/// cannot silently drift out of sync with the phonology tables.
fn render_source(bits: &[u64], popcount: usize) -> String {
    let mut out = String::new();
    out.push_str("//! GENERATED FILE — DO NOT EDIT BY HAND.\n");
    out.push_str("//!\n");
    out.push_str("//! Source: `data/attested-syllables.txt`. Regenerate with:\n");
    out.push_str("//!     cargo run -p buttre-engine --example `gen_attested_syllables`\n");
    out.push_str("//!\n");
    out.push_str("//! Bitset over (`onset_id`, `nucleus_id`, `coda_id`, `tone_id`). Dimensions and\n");
    out.push_str("//! ids come from `pipeline::validation` (`NUM_ONSETS`/`NUM_NUCLEI`/`NUM_CODAS`/\n");
    out.push_str("//! `NUM_TONES`, `onset_id`/`nucleus_id`/`coda_id`/`tone_id`) — imported, not\n");
    out.push_str("//! re-declared, so this file can never drift out of sync with those tables\n");
    out.push_str("//! without regenerating. `tests/attested_repro_test.rs` enforces that this file\n");
    out.push_str("//! matches a fresh run.\n\n");
    // This module is declared `mod attested_data;` (private) in `pipeline/mod.rs`,
    // so plain `pub` items here are already crate-limited by the module wrapper —
    // marking them `pub(crate)` too would be redundant (clippy::redundant_pub_crate).
    // The bitset literals are opaque packed data, not values a human reads
    // digit-group-by-digit — grouping them with `_` would add noise, not
    // readability (clippy::unreadable_literal).
    out.push_str("#![allow(clippy::unreadable_literal)]\n\n");
    out.push_str(
        "use super::validation::{bit_index, NUM_CODAS, NUM_NUCLEI, NUM_ONSETS, NUM_TONES};\n\n",
    );
    out.push_str("const TOTAL_BITS: usize = NUM_ONSETS * NUM_NUCLEI * NUM_CODAS * NUM_TONES;\n");
    out.push_str("const WORDS: usize = TOTAL_BITS.div_ceil(64);\n\n");
    out.push_str("/// Provenance marker: embedded tuple count as of the last regeneration.\n");
    out.push_str("/// Not read at runtime — `tests/attested_repro_test.rs` recomputes and\n");
    out.push_str("/// asserts this independently; kept here for binary-level introspection.\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str(&format!("pub const POPCOUNT: usize = {popcount};\n\n"));
    out.push_str("static BITS: [u64; WORDS] = [\n");
    for chunk in bits.chunks(6) {
        out.push_str("    ");
        for w in chunk {
            out.push_str(&format!("0x{w:016x}, "));
        }
        out.push('\n');
    }
    out.push_str("];\n\n");
    out.push_str("/// Test whether bit (`onset_id`, `nucleus_id`, `coda_id`, `tone_id`) is set.\n");
    out.push_str("/// Out-of-range ids return `false` (fail-open, never panics or indexes OOB).\n");
    out.push_str(
        "pub fn is_set(onset_id: usize, nucleus_id: usize, coda_id: usize, tone_id: usize) -> bool {\n",
    );
    out.push_str(
        "    if onset_id >= NUM_ONSETS || nucleus_id >= NUM_NUCLEI || coda_id >= NUM_CODAS || tone_id >= NUM_TONES {\n",
    );
    out.push_str("        return false;\n");
    out.push_str("    }\n");
    out.push_str("    let idx = bit_index(onset_id, nucleus_id, coda_id, tone_id);\n");
    out.push_str("    (BITS[idx / 64] >> (idx % 64)) & 1 != 0\n");
    out.push_str("}\n");
    out
}
