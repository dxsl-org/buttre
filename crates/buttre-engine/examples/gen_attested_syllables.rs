//! Generator for the embedded attested-Vietnamese-syllable bitset.
//!
//! ## Usage
//!
//!     cargo run -p buttre-engine --example gen_attested_syllables
//!
//! Reads `data/attested-syllables.txt`, decomposes each syllable into
//! (onset, nucleus, coda, tone) ids via `pipeline::validation`, and writes
//! `src/pipeline/attested_data.rs` — a numerals-only bitset.
//!
//! ## Single source of truth
//!
//! The generation logic lives in `tests/attested_gen/mod.rs`, included here
//! via `#[path]` so `tests/attested_repro_test.rs` can regenerate the exact
//! same output in-memory and byte-compare it against the checked-in file
//! (mirrors `buttre-core`'s `gen_golden.rs` / `tests/golden/corpus_data`).

use std::fs;
use std::path::PathBuf;

#[path = "../tests/attested_gen/mod.rs"]
mod attested_gen;

use attested_gen::SkipReason;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dict_path = manifest_dir.join("data").join("attested-syllables.txt");
    let out_path = manifest_dir
        .join("src")
        .join("pipeline")
        .join("attested_data.rs");

    let dict_text = fs::read_to_string(&dict_path)
        .unwrap_or_else(|e| fatal(&format!("cannot read {}: {e}", dict_path.display())));

    let result = attested_gen::generate(&dict_text).unwrap_or_else(|e| fatal(&e));

    report(&result);

    fs::write(&out_path, &result.source)
        .unwrap_or_else(|e| fatal(&format!("cannot write {}: {e}", out_path.display())));

    eprintln!("[gen_attested_syllables] wrote {}", out_path.display());
}

/// Print per-category skip accounting to stderr, plus the full skipped-word
/// lists so the counts can be cross-checked against `data/attested-syllables.txt`'s header.
fn report(result: &attested_gen::GenerationResult) {
    let (mut vowel_less, mut other) = (0usize, 0usize);
    for (_, reason) in &result.skipped {
        match reason {
            SkipReason::VowelLess => vowel_less += 1,
            SkipReason::Other => other += 1,
        }
    }
    let total_skipped = vowel_less + other;
    let unexplained_pct = 100.0 * other as f64 / result.total_lines.max(1) as f64;

    eprintln!("[gen_attested_syllables] dict lines: {}", result.total_lines);
    eprintln!(
        "[gen_attested_syllables] embedded tuples (popcount): {}",
        result.popcount
    );
    eprintln!(
        "[gen_attested_syllables] gi-family fixed (embedded, not skipped): {}",
        result.gi_family_fixed
    );
    eprintln!("[gen_attested_syllables] skipped vowel-less: {vowel_less}");
    eprintln!("[gen_attested_syllables] skipped other (loan/typo, review manually): {other} ({unexplained_pct:.3}%)");
    eprintln!(
        "[gen_attested_syllables] total skipped: {total_skipped} ({:.3}% of {})",
        100.0 * total_skipped as f64 / result.total_lines.max(1) as f64,
        result.total_lines
    );

    print_words("other (loan/typo)", &result.skipped, SkipReason::Other);
    print_words("vowel-less", &result.skipped, SkipReason::VowelLess);
}

fn print_words(label: &str, skipped: &[(String, SkipReason)], want: SkipReason) {
    let words: Vec<&str> = skipped
        .iter()
        .filter(|(_, r)| *r == want)
        .map(|(w, _)| w.as_str())
        .collect();
    if words.is_empty() {
        return;
    }
    eprintln!("[gen_attested_syllables] {label} entries ({}):", words.len());
    eprintln!("    {}", words.join(", "));
}

fn fatal(msg: &str) -> ! {
    eprintln!("[gen_attested_syllables] FATAL: {msg}");
    std::process::exit(1);
}
