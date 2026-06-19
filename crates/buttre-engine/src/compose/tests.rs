//! Integration tests for the compose module.

use crate::compose::{compose, ComposeOpts, ComposeResult};
use crate::pipeline::config::{PipelineConfig, ToneMark, ToneStyle};

fn telex_opts() -> ComposeOpts {
    let mut cfg = PipelineConfig::new("telex");
    cfg.add_transform("aa", "â");
    cfg.add_transform("aw", "ă");
    cfg.add_transform("ee", "ê");
    cfg.add_transform("oo", "ô");
    cfg.add_transform("ow", "ơ");
    cfg.add_transform("uw", "ư");
    cfg.add_transform("dd", "đ");
    // Standalone single-char: 'w' as prefix maps to 'ư' (e.g. "win"→"ưin").
    // Mirrors the stage4 hardcoded 'w'→"ư" rule used by the live pipeline.
    cfg.add_transform("w", "ư");
    cfg.add_transform("W", "Ư");
    cfg.add_tone('s', ToneMark::Acute);
    cfg.add_tone('f', ToneMark::Grave);
    cfg.add_tone('r', ToneMark::Hook);
    cfg.add_tone('x', ToneMark::Tilde);
    cfg.add_tone('j', ToneMark::Dot);
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

fn raw(s: &str) -> Vec<char> { s.chars().collect() }

// ── Telex basic ───────────────────────────────────────────────────────────────

#[test]
fn telex_a_acute() {
    assert_eq!(compose(&raw("as"), &telex_opts()).text, "á");
}

#[test]
fn telex_a_circumflex() {
    assert_eq!(compose(&raw("aa"), &telex_opts()).text, "â");
}

#[test]
fn telex_a_breve() {
    assert_eq!(compose(&raw("aw"), &telex_opts()).text, "ă");
}

#[test]
fn telex_aa_acute() {
    assert_eq!(compose(&raw("aas"), &telex_opts()).text, "ấ");
}

#[test]
fn telex_aw_grave() {
    assert_eq!(compose(&raw("awf"), &telex_opts()).text, "ằ");
}

#[test]
fn telex_dd() {
    assert_eq!(compose(&raw("dd"), &telex_opts()).text, "đ");
}

#[test]
fn telex_tuong_grave() {
    // "tuongwf" → base="tuong", transform='w' → "tương", tone='f' → "tường"
    assert_eq!(compose(&raw("tuongwf"), &telex_opts()).text, "tường");
}

// ── Telex undo/toggle ─────────────────────────────────────────────────────────

#[test]
fn telex_aaa_undo() {
    let r = compose(&raw("aaa"), &telex_opts());
    assert_eq!(r.text, "aa");
    assert!(r.temp_english);
}

#[test]
fn telex_aww_undo() {
    assert_eq!(compose(&raw("aww"), &telex_opts()).text, "aw");
}

#[test]
fn telex_ass_undo() {
    let r = compose(&raw("ass"), &telex_opts());
    assert_eq!(r.text, "as");
    assert!(r.temp_english);
}

#[test]
fn telex_ddd_undo() {
    assert_eq!(compose(&raw("ddd"), &telex_opts()).text, "dd");
}

// ── VNI basic ─────────────────────────────────────────────────────────────────

#[test]
fn vni_a1_acute() {
    assert_eq!(compose(&raw("a1"), &vni_opts()).text, "á");
}

#[test]
fn vni_a6_circumflex() {
    assert_eq!(compose(&raw("a6"), &vni_opts()).text, "â");
}

#[test]
fn vni_a61_acute_on_circumflex() {
    assert_eq!(compose(&raw("a61"), &vni_opts()).text, "ấ");
}

#[test]
fn vni_a11_undo() {
    let r = compose(&raw("a11"), &vni_opts());
    assert_eq!(r.text, "a1");
    assert!(r.temp_english);
}

// ── DirectMap (Cham) ──────────────────────────────────────────────────────────

#[test]
fn cham_double_kk() {
    let mut cfg = PipelineConfig::new("cham");
    cfg.native_script_mode = true;
    cfg.add_transform("k", "ꨆ");
    cfg.add_transform("kk", "ꩀ");
    let opts = ComposeOpts::from_config(&cfg);
    assert_eq!(compose(&raw("kk"), &opts).text, "ꩀ");
}

#[test]
fn cham_single_k() {
    let mut cfg = PipelineConfig::new("cham");
    cfg.native_script_mode = true;
    cfg.add_transform("k", "ꨆ");
    cfg.add_transform("kk", "ꩀ");
    let opts = ComposeOpts::from_config(&cfg);
    assert_eq!(compose(&raw("k"), &opts).text, "ꨆ");
}

// ── Regression guards: leading tone keys must stay literal ───────────────────
//
// In Telex, tone keys (s/f/r/x/j) that appear BEFORE any vowel in the syllable
// have no nucleus to act on and must be treated as literal consonant/base chars.
// Violating this rule caused "fan"→"àn", "fin"→"ìn" (f incorrectly consumed as
// grave tone applied to the following vowel).

#[test]
fn telex_fan_stays_fan() {
    // 'f' before any vowel: must be literal, compose must not drop 'f'.
    assert_eq!(compose(&raw("fan"), &telex_opts()).text, "fan");
}

#[test]
fn telex_fin_stays_fin() {
    // Same: leading 'f' is not a tone mark.
    assert_eq!(compose(&raw("fin"), &telex_opts()).text, "fin");
}

#[test]
fn telex_win_stays_win() {
    // Leading bare 'w' has no preceding a/o/u vowel to modify, so it is a literal
    // consonant — English w-words type naturally ("win" → "win", not "ưin").
    // 'ư' at word start is typed as "uw".
    assert_eq!(compose(&raw("win"), &telex_opts()).text, "win");
}

#[test]
fn telex_uw_still_yields_uhorn() {
    // 'ư' at word start is reached via "uw" (the w modifies the preceding u).
    assert_eq!(compose(&raw("uwng"), &telex_opts()).text, "ưng");
}

#[test]
fn telex_af_yields_a_grave() {
    // Post-vowel 'f' must still function as a tone key (grave).
    assert_eq!(compose(&raw("af"), &telex_opts()).text, "à");
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[test]
fn empty_raw() {
    assert_eq!(compose(&[], &telex_opts()), ComposeResult {
        text: String::new(),
        temp_english: false,
    });
}

#[test]
fn telex_uwow_yields_uhorn_ohorn() {
    // "uwow" = uw (u→ư) + ow (o→ơ) = "ươ"
    assert_eq!(compose(&raw("uwow"), &telex_opts()).text, "ươ");
}

#[test]
fn telex_thuees_yields_thue_acute() {
    // "thuees" = "thue" + ee(e→ê) + s(tone acute) = "thuế"
    assert_eq!(compose(&raw("thuees"), &telex_opts()).text, "thuế");
}
