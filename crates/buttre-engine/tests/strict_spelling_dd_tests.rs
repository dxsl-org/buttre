//! Deliberate-đ abbreviation leniency (`ValidationSettings::strict_spelling`):
//! a vowel-less consonant cluster starting with `đ` ("đt", "đc", "đkkd") can
//! only arise from an explicit transform keystroke ("dd" Telex, "d9" VNI), so
//! lenient mode (the default) keeps it composed — Unikey parity — instead of
//! reverting to raw via the Step-6 English fallback. Strict mode restores the
//! revert. See `compose::could_be_vietnamese`'s deliberate-đ branch.

use buttre_engine::compose::{compose, compose_closed, ComposeOpts};
use buttre_engine::pipeline::{telex_config, vni_config, PipelineConfig, PipelineExecutor};

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn strict_telex_config() -> PipelineConfig {
    let mut config = telex_config();
    // The preset leaves `validation` at `None` (→ all defaults, lenient);
    // materialize it to flip just the strict switch.
    let mut validation = config.validation.clone().unwrap_or_default();
    validation.strict_spelling = true;
    config.validation = Some(validation);
    config
}

/// Type a whole word through the live pipeline and return the final open
/// projection (what the user sees mid-word, before any separator).
fn type_word(input: &str, config: &PipelineConfig) -> String {
    let mut executor = PipelineExecutor::new(config.clone());
    for ch in input.chars() {
        executor.process(ch);
    }
    executor.syllable().to_string()
}

#[test]
fn lenient_default_keeps_dd_abbreviations_composed() {
    let config = telex_config();
    assert_eq!(type_word("ddt", &config), "đt", "điện thoại shorthand");
    assert_eq!(type_word("ddc", &config), "đc", "được shorthand");
    assert_eq!(type_word("ddk", &config), "đk", "điều kiện shorthand");
    assert_eq!(type_word("ddkkd", &config), "đkkd", "đăng ký kinh doanh");
}

#[test]
fn lenient_default_survives_the_closed_projection_too() {
    // The word-boundary (separator/Enter) decision runs through
    // `compose_closed` — it must agree with the open projection, or the
    // committed word would flicker back to raw at the separator.
    let opts = ComposeOpts::from_config(&telex_config());
    let raw = chars("ddt");
    assert_eq!(compose(&raw, &opts).text, "đt");
    assert_eq!(compose_closed(&raw, &opts).text, "đt");
    assert!(!compose_closed(&raw, &opts).temp_english);
}

#[test]
fn vni_d9_reaches_the_same_leniency() {
    let config = vni_config();
    assert_eq!(type_word("d9t", &config), "đt", "VNI d9 = Telex dd");
}

#[test]
fn strict_mode_restores_the_raw_revert() {
    let config = strict_telex_config();
    assert_eq!(
        type_word("ddt", &config),
        "ddt",
        "strict spelling must revert the vowel-less đ-cluster to raw"
    );
    let opts = ComposeOpts::from_config(&config);
    let closed = compose_closed(&chars("ddt"), &opts);
    assert_eq!(closed.text, "ddt");
    assert!(closed.temp_english, "strict revert latches English fallback");
}

#[test]
fn executor_set_strict_spelling_flips_live() {
    // The runtime path (`Keyboard::set_strict_spelling` →
    // `PipelineExecutor::set_strict_spelling`) must flip BOTH the live
    // compose stage and the boundary-repair opts without a rebuild.
    let mut executor = PipelineExecutor::new(telex_config());
    for ch in "ddt".chars() {
        executor.process(ch);
    }
    assert_eq!(executor.syllable(), "đt", "default lenient");

    let mut strict = PipelineExecutor::new(telex_config());
    strict.set_strict_spelling(true);
    for ch in "ddt".chars() {
        strict.process(ch);
    }
    assert_eq!(strict.syllable(), "ddt", "strict after live flip");
}

#[test]
fn non_vietnamese_letters_after_dd_still_revert() {
    // 'w' can never follow đ in a Vietnamese abbreviation — the leniency
    // must not swallow it ("ddw" would otherwise render as "đư"-less junk).
    let config = telex_config();
    let result = type_word("ddw", &config);
    assert!(
        result == "ddw" || !result.starts_with('đ') || result == "đư",
        "unexpected ddw projection: {result:?}"
    );
    // đ + vowel continues down the normal (non-lenient) path: an invalid
    // syllable with a vowel is NOT covered by the đ branch.
    assert_eq!(type_word("ddta", &config), "ddta", "vowel present → normal gate");
}

#[test]
fn plain_consonant_clusters_without_dd_are_untouched() {
    // No đ, no leniency: "vs", "bdds" (dd inside a cluster never composes)
    // behave exactly as before.
    let config = telex_config();
    assert_eq!(type_word("vs", &config), "vs");
    assert_eq!(type_word("bdds", &config), "bdds");
}
