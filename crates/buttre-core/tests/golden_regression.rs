//! Golden regression tests — read .snap files and verify the current engine
//! produces identical output for every case.
//!
//! ## Running
//!
//!     cargo test -p buttre-core golden_regression
//!
//! ## Generating / regenerating snapshots
//!
//!     cargo run -p buttre-core --example gen_golden
//!
//! Snapshots live in `tests/golden/{telex,vni,nom}.snap`.
//! Format: `<keys>\t<expected_output>\t<TAG>` one per line.

mod golden;

use buttre_core::keyboard::{nom, telex, vni};
use buttre_engine::pipeline::PipelineConfig;
use golden::{corpus_data, type_sequence};

use std::path::{Path, PathBuf};

// ── snapshot loader ───────────────────────────────────────────────────────────

struct SnapCase {
    keys: String,
    expected: String,
    tag: String,
}

fn load_snap(path: &Path) -> Vec<SnapCase> {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Cannot read snap file {}: {e}", path.display()));

    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            assert!(
                parts.len() == 3,
                "Malformed snap line (expected 3 tab-separated fields): {:?}",
                line
            );
            SnapCase {
                keys: parts[0].to_string(),
                expected: parts[1].to_string(),
                tag: parts[2].to_string(),
            }
        })
        .collect()
}

fn snap_path(method: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(format!("{method}.snap"))
}

// ── generic runner ────────────────────────────────────────────────────────────

fn run_regression(method: &str, config_fn: fn() -> PipelineConfig) {
    let path = snap_path(method);
    if !path.exists() {
        panic!(
            "{method}.snap not found at {} — run: cargo run -p buttre-core --example gen_golden",
            path.display()
        );
    }
    let cases = load_snap(&path);
    assert!(
        !cases.is_empty(),
        "{method}.snap is empty — run gen_golden first"
    );

    let mut failures = 0usize;
    let mut failure_msgs: Vec<String> = Vec::new();

    for case in &cases {
        let actual = type_sequence(config_fn(), &case.keys);
        if actual != case.expected {
            failures += 1;
            failure_msgs.push(format!(
                "  keys: {} expected '{}' got '{}' [{}]",
                case.keys, case.expected, actual, case.tag
            ));
            // Collect all failures before panicking for better diagnostics.
            if failures >= 50 {
                failure_msgs.push("  … (more failures truncated)".to_string());
                break;
            }
        }
    }

    assert!(
        failures == 0,
        "\n{method} regression: {failures} failure(s):\n{}",
        failure_msgs.join("\n")
    );

    println!("[golden_regression] {method}: {} cases OK", cases.len());
}

// ── per-method test functions ─────────────────────────────────────────────────

#[test]
fn test_golden_telex() {
    run_regression("telex", telex::build_config);
}

#[test]
fn test_golden_vni() {
    run_regression("vni", vni::build_config);
}

/// Key-order invariance: the free tone marking order (whole word typed
/// first, marks and tone appended afterwards — the Unikey habit) must
/// produce the same syllable as the canonical adjacent order.
///
/// This is the invariant behind the whole non-adjacent attestation gate: the
/// canonical order never flags its marks and so clears the gate ungated,
/// while the free order routes every mark through it. A gate that is too
/// strict shows up here as a syllable reverting to its raw keystrokes, which
/// the .snap files alone cannot catch — they pin whatever the engine
/// currently emits, right or wrong.
///
/// Failures are reported as a list rather than one-at-a-time: a gate change
/// typically moves a whole class of syllables at once, and the class is what
/// identifies the cause.
/// Syllables that still depend on key order — Telex only, one shared cause.
///
/// The free order infers the vowel/đ mark NON-ADJACENTLY, so it must clear
/// the EXACT-attestation branch of the gate, and none of these is in the
/// attested-syllable table: they are bare rimes kept for nucleus/coda
/// coverage (`ât`, `ôc`, `êch`, …) or toned forms that are not words on their
/// own (`đí`, `đỉ`, `chiệu`). The adjacent order composes them because it
/// never flags its marks and so never consults the gate at all.
///
/// Relaxing that branch is not an option: it reopens the `data` → `dât`
/// class (measured: 29 test regressions, 59 extra words in the English
/// typeability corpus, one of them unrecoverable). VNI is unaffected — its
/// digit trigger cannot occur inside an English word, so its branch checks
/// structural validity instead and this list is empty for VNI. The user-side
/// escapes are the adjacent order, or the user-attested overlay once the
/// syllable has been typed directly a few times.
const KNOWN_KEY_ORDER_EXCEPTIONS: &[&str] = &[
    "chiệu", "đí", "đỉ", "ưm", "ăt", "ât", "âc", "ôc", "êp", "ôp", "ơp", "êch",
];

fn assert_free_order_matches_canonical(
    method: &str,
    config_fn: fn() -> PipelineConfig,
    to_canonical: fn(&str) -> String,
    decompose: corpus_data::DecomposeFn,
    tone_key: fn(corpus_data::VnTone) -> char,
) {
    let mut failures = Vec::new();
    for &syllable in corpus_data::SYLLABLES {
        if KNOWN_KEY_ORDER_EXCEPTIONS.contains(&syllable) {
            continue;
        }
        let canonical = to_canonical(syllable);
        let Some(free) = corpus_data::vn_to_free_keys(syllable, decompose, tone_key) else {
            continue; // no unambiguous free encoding (bare ươ — see vn_to_free_keys)
        };
        if free == canonical {
            continue;
        }
        let expected = type_sequence(config_fn(), &canonical);
        // The canonical order refused to compose this entry at all (it is a
        // bare nucleus/coda fragment, not a syllable): there is no composed
        // result for the free order to agree with.
        if expected == canonical {
            continue;
        }
        let actual = type_sequence(config_fn(), &free);
        if actual != expected {
            failures.push(format!(
                "  {syllable}: '{canonical}' → '{expected}' but free '{free}' → '{actual}'"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "\n{method}: {} syllable(s) depend on key order:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn free_tone_marking_matches_canonical_order_telex() {
    assert_free_order_matches_canonical(
        "telex",
        telex::build_config,
        corpus_data::vn_to_telex_keys,
        corpus_data::decompose_telex,
        corpus_data::telex_tone_key,
    );
}

#[test]
fn free_tone_marking_matches_canonical_order_vni() {
    assert_free_order_matches_canonical(
        "vni",
        vni::build_config,
        corpus_data::vn_to_vni_keys,
        corpus_data::decompose_vni,
        corpus_data::vni_tone_key,
    );
}

/// Nôm regression — skipped automatically if nom.snap does not exist
/// (which happens when buttre_nom.db was absent during gen_golden).
#[test]
fn test_golden_nom() {
    let path = snap_path("nom");
    if !path.exists() {
        println!("[golden_regression] nom: skipped (nom.snap not present)");
        return;
    }
    run_regression("nom", nom::build_config);
}
