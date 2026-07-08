//! Integration tests for the compose module.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::compose::{compose, compose_closed, ComposeOpts, ComposeResult, Pref};
use crate::pipeline::config::{PipelineConfig, ToneMark};
use crate::pipeline::validation::{bit_index, decompose_ids};

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

fn raw(s: &str) -> Vec<char> {
    s.chars().collect()
}

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
    assert_eq!(
        compose(&[], &telex_opts()),
        ComposeResult {
            text: String::new(),
            temp_english: false,
            applied_marks: Vec::new(),
            consumed_tone: None,
            demoted: false,
        }
    );
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

// ── Regression: double-u horn placement (bugs: luuw Telex / luu7 VNI) ────────
//
// The rightmost 'u' in "luu" produces invalid nucleus "uư"; the fix must
// retry leftward to place the horn on the first 'u' giving valid "lưu".

#[test]
fn telex_luuw_yields_luu_horn() {
    assert_eq!(
        compose(&raw("luuw"), &telex_opts()).text,
        "lưu",
        "Telex luuw must produce lưu"
    );
}

#[test]
fn vni_luu7_yields_luu_horn() {
    assert_eq!(
        compose(&raw("luu7"), &vni_opts()).text,
        "lưu",
        "VNI luu7 must produce lưu"
    );
}

// ── Regression: tone-before-transform ordering (bug: mieng16/mieng26 VNI) ────
//
// "mieng16": '1' (tone sắc) typed before '6' (e→ê transform).
// The intermediate form "miéng" has nucleus "ie" + coda "ng"; this must pass
// validation so English fallback does NOT latch before '6' is processed.

#[test]
fn vni_mieng16_yields_mieng_acute() {
    assert_eq!(
        compose(&raw("mieng16"), &vni_opts()).text,
        "miếng",
        "VNI mieng16 (tone before transform) must produce miếng"
    );
}

#[test]
fn vni_mieng26_yields_mieng_grave() {
    assert_eq!(
        compose(&raw("mieng26"), &vni_opts()).text,
        "miềng",
        "VNI mieng26 must produce miềng"
    );
}

#[test]
fn vni_mieng61_still_works() {
    // Transform before tone: already worked before the fix; guard the regression.
    assert_eq!(
        compose(&raw("mieng61"), &vni_opts()).text,
        "miếng",
        "VNI mieng61 (transform before tone) must still produce miếng"
    );
}

// ── Regression: fast-typing onset 'd' before doubling key (Telex) ─────────────
//
// When typing fast, the onset 'd' and first vowel can slip in before the
// doubling keys.  Unikey-compatible: second 'd' = "dd"→"đ", second 'o' = "oo"→"ô".

#[test]
fn telex_dodong_yields_dong() {
    assert_eq!(
        compose(&raw("dodong"), &telex_opts()).text,
        "đông",
        "Telex dodong (fast-type, no tone) must produce đông"
    );
}

#[test]
fn telex_dodongf_yields_dong_grave() {
    assert_eq!(
        compose(&raw("dodongf"), &telex_opts()).text,
        "đồng",
        "Telex dodongf (fast-type onset slip) must produce đồng"
    );
}

// ── Phase 2: attestation gate on non-adjacent transforms ─────────────────────
// Test Scenario Matrix from phase-02-attestation-gate-compose.md.

#[test]
fn critical_data_stays_literal() {
    // The flagship bug: "data" — non-adjacent 'a' would produce unattested
    // "dât" — must demote to the literal keystrokes.
    let r = compose(&raw("data"), &telex_opts());
    assert_eq!(
        r.text, "data",
        "unattested 'dât' must demote to literal 'data'"
    );
}

#[test]
fn critical_vietej_fires_attested() {
    assert_eq!(
        compose(&raw("vietej"), &telex_opts()).text,
        "việt",
        "flexible non-adjacent typing must still produce attested 'việt'"
    );
}

#[test]
fn critical_nasa_stays_literal() {
    // "nấ" (raw-adjacency bug: tone key 's' sits between the two 'a's) is
    // unattested — must demote to literal, not leak a spurious diacritic
    // through the elongation fallback either (see `try_elongation_fallback`).
    let r = compose(&raw("nasa"), &telex_opts());
    assert_eq!(
        r.text, "nasa",
        "unattested 'nấ' must demote to literal 'nasa'"
    );
}

#[test]
fn critical_luuw_huuw_no_demote() {
    // Retry inherits the segment-level adjacent flag unchanged — no demote.
    assert_eq!(compose(&raw("luuw"), &telex_opts()).text, "lưu");
    assert_eq!(compose(&raw("huuw"), &telex_opts()).text, "hưu");
}

#[test]
fn critical_vni_nhat61_shape_attested_no_flicker() {
    // Non-alphabetic (VNI digit) trigger relaxes to shape-attestation: the
    // intermediate "nhât" (no tone yet) is not itself attested, but its
    // SHAPE is (nhất exists) — no literal flicker before '1' arrives.
    assert_eq!(compose(&raw("nhat61"), &vni_opts()).text, "nhất");
    assert_eq!(
        compose(&raw("nhat6"), &vni_opts()).text,
        "nhât",
        "mid-typing 'nhât' must not flicker to literal before the tone key"
    );
}

#[test]
fn high_reset_accepted_attested_collision() {
    // "rết" (centipede) happens to be a real word — the gate cannot and must
    // not distinguish this from a deliberate non-adjacent transform. Escape
    // is via undo (Phase 4) or adjacent retyping.
    assert_eq!(compose(&raw("reset"), &telex_opts()).text, "rết");
}

#[test]
fn high_data_class_words_stay_literal() {
    for word in ["meme", "photo", "papa"] {
        let r = compose(&raw(word), &telex_opts());
        assert_eq!(
            r.text, word,
            "'{word}' must stay literal (unattested non-adjacent result)"
        );
    }
}

// ── Phase 3: guard simplification — segment-level rejection moved here ──────
// `fallback_real_word_no_transform`/`implement_real_word_no_transform`/
// `implemeent_no_nonadjacent_transform` used to assert REJECTION in
// `compose::segment::tests` via `count_vowel_groups`/`coda_after_last_vowel_is_valid`.
// Those two guards are now bypassed at the segment layer for Vietnamese
// configs (see `segment::tests::vietnamese_config_bypasses_legacy_shape_guards`) —
// the SAME end-to-end outcome (literal output) is now produced by the P2
// attestation gate instead. Zero scenarios dropped; only the layer moved.

#[test]
fn high_fallback_implement_class_words_stay_literal() {
    for word in ["fallback", "implement", "impleme", "salsa"] {
        let r = compose(&raw(word), &telex_opts());
        assert_eq!(
            r.text, word,
            "'{word}' must stay literal via the attestation gate (unattested non-adjacent result)"
        );
    }
}

#[test]
fn high_banana_stays_literal_via_repeat_count_guard() {
    // "banana" has THREE 'a's — blocked by the exactly-2-occurrence rule
    // (KEEP, independent of attestation), never even reaches the gate.
    let r = compose(&raw("banana"), &telex_opts());
    assert_eq!(r.text, "banana");
}

#[test]
fn high_tuongw_no_misflag_or_underflow() {
    // "tuongw": the compound trigger 'w' is separated from the vowel cluster
    // by the coda "ng" — flagged non-adjacent, but "tương" is attested so it
    // passes the gate unchanged. Must not panic/underflow either way.
    assert_eq!(compose(&raw("tuongw"), &telex_opts()).text, "tương");
}

#[test]
fn high_vni_6a_prefix_forward_apply() {
    // base_len_at_typing == 0 prefix mark: adjacent by definition, no underflow.
    assert_eq!(compose(&raw("6a"), &vni_opts()).text, "â");
}

#[test]
fn high_uw_no_misflag() {
    assert_eq!(compose(&raw("uw"), &telex_opts()).text, "ư");
}

#[test]
fn high_adjacent_typing_unchanged() {
    // Adjacent-typing behavior must be byte-for-byte unchanged by the gate.
    // Default ToneStyle is Old, so "hoas" → "hóa" (not "hoá").
    assert_eq!(compose(&raw("vieet"), &telex_opts()).text, "viêt");
    assert_eq!(compose(&raw("hoas"), &telex_opts()).text, "hóa");
    assert_eq!(compose(&raw("how"), &telex_opts()).text, "hơ");
}

#[test]
fn medium_cana_collision_canal_self_heals() {
    // "cân" is attested — the gate cannot distinguish this collision from a
    // deliberate transform (accepted, matches the "reset" row). Continuing to
    // type "canal" makes the composed form "cânl" (invalid), so the whole
    // mark demotes and the word self-heals to the literal keystrokes.
    assert_eq!(compose(&raw("cana"), &telex_opts()).text, "cân");
    assert_eq!(compose(&raw("canal"), &telex_opts()).text, "canal");
}

#[test]
fn medium_elongation_unchanged_by_gate() {
    // "không" is attested, so the elongation fallback's own attestation
    // check (added to close the "nasa" false-positive, see
    // `try_elongation_fallback`) does not affect this legitimate case.
    assert_eq!(compose(&raw("khoongggg"), &telex_opts()).text, "khôngggg");
}

#[test]
fn medium_hmong_config_gate_off() {
    // `attest_non_adjacent` is false for non-Vietnamese validators — the
    // exact same raw sequence that demotes under Telex/Vietnamese ("nasa")
    // must fire UNGATED under a Hmong-validated config (re-entry + gate-off).
    use crate::pipeline::config::ValidationSettings;
    let mut cfg = PipelineConfig::new("hmong-test");
    cfg.add_transform("aa", "â");
    cfg.add_tone('s', ToneMark::Acute);
    cfg.validation = Some(ValidationSettings {
        syllable_structure: "hmong".to_string(),
        allow_invalid: true,
    });
    let opts = ComposeOpts::from_config(&cfg);
    assert!(
        !opts.attest_non_adjacent,
        "Hmong validator must not enable the attestation gate"
    );
    assert_eq!(
        compose(&raw("nasa"), &opts).text,
        "nấ",
        "gate-off: the non-adjacent mark fires ungated, unaffected by attestation"
    );
}

// ── Fallback bypass regression (red-team C2) ──────────────────────────────────
// `check_tone_toggle`/`check_transform_toggle`'s prefix reconstruction must be
// gated exactly like the main compose() path — no â/ê leaking through.

#[test]
fn c2_dataeee_no_bypass() {
    let r = compose(&raw("dataeee"), &telex_opts());
    assert_eq!(r.text, "dataee");
    assert!(
        !r.text.contains(['â', 'ê']),
        "no diacritic must leak through the transform-toggle bypass"
    );
}

#[test]
fn c2_vietess_no_bypass() {
    // "viet" + demoted literal 'e' + literal 's' suffix = "vietes".
    let r = compose(&raw("vietess"), &telex_opts());
    assert_eq!(r.text, "vietes");
    assert!(
        !r.text.contains(['â', 'ê']),
        "no diacritic must leak through the tone-toggle bypass"
    );
}

#[test]
fn c2_databaaa_no_bypass() {
    let r = compose(&raw("databaaa"), &telex_opts());
    assert_eq!(r.text, "databaa");
    assert!(
        !r.text.contains(['â', 'ê']),
        "no diacritic must leak through the transform-toggle bypass"
    );
}

// ── Recursion bound: demote pass cannot itself demote ─────────────────────────

#[test]
fn demote_pass_cannot_recurse_twice() {
    // A word with TWO independently-unattested non-adjacent marks must still
    // terminate in a single demote pass (both suppressed at once — per-mark
    // subset search is explicitly out of scope, see phase Risk Notes).
    // "papa" already covers one flavor (Telex 'a' x2); this forces multiple
    // marks by combining an unattested đ with an unattested vowel double.
    let r = compose(&raw("dedeng"), &telex_opts());
    // Must not panic/loop; result must be a plain literal (no stray marks).
    assert!(
        !r.text.contains(['â', 'ê', 'đ']),
        "demoted output must carry no leftover diacritics: {}",
        r.text
    );
}

// ── Phase 4: non-adjacent transform undo ─────────────────────────────────────
// Test Scenario Matrix from phase-04-nonadjacent-undo.md.
//
// Deviations from the matrix's literal example strings (verified empirically
// against this build, same spirit as the phase's own "verify cana->cân first"
// instruction):
//
// - VNI parity row: the matrix's "cana7"+"7" does not exercise any transform
//   at all — this codebase's canonical VNI digit for â is '6', not '7'
//   (`crate::pipeline::presets::vni_config`: "a6"->"â"; '7' is only ever
//   registered for o7/u7 horn). Substituted with "can6"+"6" (digit-triggered
//   equivalent of Telex "cana"+"a": non-adjacent '6' fires â on "can6" ->
//   attested "cân", exactly like the Telex case).
// - "dodongd" row: does not satisfy the immediacy contract as literally
//   written — đ fires on "dodong"'s 3rd raw char ('d' at index 2), but
//   "dodong" (the prefix once the trailing retype 'd' is removed) ends in
//   'g' (index 5), not 'd'. Retyping 'd' at the very end of an already-
//   completed "dodong" is NOT an immediate retype of the đ trigger — it is
//   exactly the same non-immediacy shape as "vieteje" (whose own row
//   confirms this must NOT undo). Substituted with "dand"+"d": đ fires on
//   "dand"'s FINAL raw char (open/coda-final backward-referring đ, same
//   mechanism, composes to attested "đan" — a real word, "to knit/weave"),
//   so retyping 'd' immediately after DOES satisfy immediacy. This is the
//   correct analogue of the đ/consonant-class escape hatch.
// - "cana"+"a"+"n" latch-semantics row: at the PURE `compose()` layer there
//   is no persistent state — re-running compose on the grown raw buffer
//   "canaan" recomputes from scratch. The interior spent-undo fold (step
//   1.5, `check_spent_undo`) recognizes the "canaa" undo event from raw
//   alone and appends the tail literally, so compose returns the SAME
//   display-trimmed "canan" the live latched executor shows (the undoing
//   keystroke is consumed). What Phase 4 guarantees at this layer — and
//   what the test below asserts — is the "Gatekeeper passthrough"
//   invariant: no diacritic leaks back in and no incorrect re-fire happens.

#[test]
fn critical_cana_a_undoes_to_literal_latched() {
    // Verified: compose("cana") == "cân" (attested collision, see
    // `medium_cana_collision_canal_self_heals` above) — the pre-undo state
    // this escape hatch targets.
    assert_eq!(compose(&raw("cana"), &telex_opts()).text, "cân");
    let r = compose(&raw("canaa"), &telex_opts());
    assert_eq!(
        r.text, "cana",
        "retyping 'a' must undo the non-adjacent â mark"
    );
    assert!(r.temp_english, "undo must latch English passthrough");
    assert!(r.applied_marks.is_empty(), "undo result carries no marks");
}

#[test]
fn critical_dataa_no_double_strip() {
    // "dataa": the gate already demoted "data"'s own 'a' mark to literal at
    // the top level (count-of-'a' == 3 blocks the non-adjacent branch
    // entirely — see `critical_data_stays_literal`). The undo check's own
    // internal prefix recompute ALSO finds the mark demoted (fresh count == 2
    // there), so it reports zero eligible marks and no-ops. Compose's normal
    // path then handles the full 5-key raw buffer untouched: no keystroke is
    // dropped ("no double-strip").
    let r = compose(&raw("dataa"), &telex_opts());
    assert_eq!(
        r.text, "dataa",
        "no keystroke may be dropped when the prefix's mark was already gate-demoted"
    );
    assert!(r.applied_marks.is_empty());
}

#[test]
fn critical_aaa_adjacent_priority_unchanged() {
    // Adjacent toggle (check_transform_toggle) must still claim "aaa" before
    // the non-adjacent check ever runs.
    let r = compose(&raw("aaa"), &telex_opts());
    assert_eq!(r.text, "aa");
    assert!(r.temp_english);
}

#[test]
fn critical_vieteje_immediacy_violated_no_undo() {
    // "vietej" already fires the attested non-adjacent 'e' mark (-> "việt",
    // see `critical_vietej_fires_attested`). Appending one more 'e' makes the
    // prefix's LAST raw key the tone 'j', not the 'e' mark's trigger position
    // — immediacy fails at the pre-filter (K='e' != prefix-last='j') before
    // any prefix compose is attempted. Must NOT undo: "việt" does not
    // resurface at all, and the result is NOT the undone literal "vietej"
    // either — the extra 'e' just runs through the ordinary English-fallback
    // path (independent of Phase 4), yielding the full literal 7-key buffer.
    let r = compose(&raw("vieteje"), &telex_opts());
    assert_eq!(r.text, "vieteje");
    assert!(
        !r.text.contains(['ệ', 'ê']),
        "no undo-related diacritic must leak"
    );
}

#[test]
fn high_vni_can6_digit_parity_undoes_to_literal_latched() {
    // Method parity (S8): the VNI digit-triggered equivalent of "cana"+"a".
    // See module-level deviation note for why '6' (not '7') is the correct
    // trigger digit for â in this codebase's VNI table.
    assert_eq!(
        compose(&raw("can6"), &vni_opts()).text,
        "cân",
        "can6 must be the VNI attested collision analogous to Telex cana"
    );
    let r = compose(&raw("can66"), &vni_opts());
    assert_eq!(
        r.text, "can6",
        "retyping the VNI digit trigger must undo exactly like Telex"
    );
    assert!(r.temp_english);
}

#[test]
fn high_cana_uppercase_trigger_case_insensitive() {
    // Retyping the trigger in the OPPOSITE case must still undo.
    let r = compose(&raw("canaA"), &telex_opts());
    assert_eq!(
        r.text, "cana",
        "uppercase retype of the trigger key must still undo"
    );
    assert!(r.temp_english);
}

#[test]
fn high_cana_latch_survives_recompute_no_reentry() {
    // "cana"+"a"+"n": the interior spent-undo fold (step 1.5) now folds the
    // "canaa" undo event and appends the tail literally, so pure compose()
    // returns exactly what the live latched display shows — "canan" (the
    // undoing keystroke is consumed, same as the executor path). Previously
    // compose() answered the full literal "canaan" and display-level
    // trimming was a caller concern; the fold moved that concern into
    // compose so probe/boundary/replay all agree. The invariant this test
    // exists for is unchanged: the undone â mark must never re-fire.
    let r = compose(&raw("canaan"), &telex_opts());
    assert_eq!(r.text, "canan");
    assert!(r.temp_english);
    assert!(
        !r.text.contains(['â']),
        "the undone â mark must never re-fire on further typing"
    );
}

#[test]
fn high_dand_d_consonant_undo_equivalence_note() {
    // Substitute for the matrix's "dodongd" row (see module-level deviation
    // note): "dand" fires the backward-referring đ mark on its OWN final raw
    // key (base_ends_with_coda("dan") makes it a "committed syllable"), so
    // retyping 'd' immediately after DOES satisfy immediacy.
    assert_eq!(
        compose(&raw("dand"), &telex_opts()).text,
        "đan",
        "dand must be an attested đ collision (đan = to knit/weave)"
    );
    let r = compose(&raw("dandd"), &telex_opts());
    assert_eq!(
        r.text, "dand",
        "retyping 'd' right after dand must undo the đ mark"
    );
    assert!(r.temp_english);
}

#[test]
fn medium_existing_toggles_unaffected_by_ordering_change() {
    assert_eq!(compose(&raw("a611"), &vni_opts()).text, "â1");
    let opts_with_ee = {
        let mut cfg = PipelineConfig::new("telex");
        cfg.add_transform("aa", "â");
        cfg.add_transform("ee", "ê");
        cfg.add_tone('s', ToneMark::Acute);
        ComposeOpts::from_config(&cfg)
    };
    let r = compose(&raw("seess"), &opts_with_ee);
    assert_eq!(r.text, "sês");
}

#[test]
fn medium_latched_then_more_keys_no_reentry() {
    // Once undone, further identical keystrokes must not oscillate back into
    // firing the mark again.
    let r = compose(&raw("canaaa"), &telex_opts());
    assert!(
        !r.text.contains('â'),
        "repeated retypes must never resurrect the diacritic: {}",
        r.text
    );
}

// ── Phase 6: coda "k" (Đắk Lắk class) ─────────────────────────────────────────

#[test]
fn p6_telex_ddawks_yields_dak_acute() {
    // dd→đ, aw→ă (both adjacent, ungated), literal 'k' coda, s→sắc tone.
    // "đắk" is now attested (P6 re-embed) and structurally valid (per-nucleus
    // coda-k row for "ă") — no English revert.
    assert_eq!(
        compose(&raw("ddawks"), &telex_opts()).text,
        "đắk",
        "ddawks (đ + ă + k + sắc) must compose to đắk"
    );
}

#[test]
fn p6_vni_dak_acute_via_digits() {
    // VNI equivalent: d9 -> đ, a8 -> ă, literal 'k', '1' -> sắc.
    assert_eq!(
        compose(&raw("d9a8k1"), &vni_opts()).text,
        "đắk",
        "VNI d9a8k1 must compose to đắk exactly like Telex ddawks"
    );
}

// ── Phase 6: gate hardening — trigger classification (digit vs. everything else) ──

/// Synthetic custom config: apostrophe (`'`) registered as a non-alphabetic,
/// non-digit transform trigger — same non-adjacent-firing shape as VNI's
/// digit `'6'` in `"nhat6"`, but punctuation instead of a digit. No shipped
/// preset does this; it exists purely to exercise `passes_attestation_gate`'s
/// trigger classification in isolation.
fn punctuation_trigger_opts() -> ComposeOpts {
    let mut cfg = PipelineConfig::new("custom-punct");
    cfg.add_transform("a'", "â");
    ComposeOpts::from_config(&cfg)
}

#[test]
fn p6_gate_hardening_punctuation_trigger_gets_exact_check() {
    // "nhat'" fires â non-adjacently, giving "nhât" — whose SHAPE is attested
    // (via "nhất") but whose EXACT (toneless) form is not (see
    // `critical_vni_nhat61_shape_attested_no_flicker`'s doc for the same
    // shape/exact distinction). Before the P6 hardening, the gate's
    // classification was `is_alphabetic() -> exact, else -> shape`, which
    // wrongly relaxed ANY non-alphabetic trigger — including punctuation —
    // to the shape check. After hardening (`is_ascii_digit() -> shape, else
    // -> exact`), only digit triggers get that relaxation; a punctuation
    // trigger must get the EXACT check and demote to literal.
    let opts = punctuation_trigger_opts();
    let r = compose(&raw("nhat'"), &opts);
    assert_eq!(
        r.text, "nhat'",
        "punctuation trigger must get the EXACT check, not shape-relaxed"
    );
    assert!(
        !r.text.contains('â'),
        "no diacritic may leak through a punctuation trigger's shape-relaxation"
    );
}

#[test]
fn p6_coda_k_invalid_nucleus_reverts_to_literal() {
    // "ddik"/"ddok": dd->đ fires (attested prefix "đi"/"đo" alone), but the
    // FULL word "đik"/"đok" has coda 'k' on nucleus "i"/"o" — no per-nucleus
    // row for those (only "u"/"ă" do) — so `could_be_vietnamese` rejects it
    // and the whole word reverts to the literal raw keys. Guards against the
    // blanket-allowance risk called out in the phase's Risk Notes (English
    // "-ik"/"-ok" endings must not start composing just because coda 'k'
    // exists in the structural table).
    for word in ["ddik", "ddok"] {
        let r = compose(&raw(word), &telex_opts());
        assert_eq!(
            r.text, word,
            "'{word}' must revert to literal — coda 'k' invalid for this nucleus"
        );
        assert!(
            !r.text.contains('đ'),
            "no partial đ transform may leak through: {}",
            r.text
        );
    }
}

// ── Phase 3: word-boundary final repair (`compose_closed`) ───────────────────
// Test Scenario Matrix from phase-03-boundary-repair.md, compose-level rows.

#[test]
fn critical_vni_nhat6_closed_restores_literal() {
    // Open projection: digit trigger relaxes to shape-attestation, so the
    // tone-not-yet-typed intermediate "nhât" survives (see
    // critical_vni_nhat61_shape_attested_no_flicker). At the word boundary
    // (closed=true), the SAME shape-only form must demote to literal raw —
    // "nhât" is not itself a word.
    let opts = vni_opts();
    assert_eq!(
        compose(&raw("nhat6"), &opts).text,
        "nhât",
        "open projection unaffected (regression guard)"
    );
    let r = compose_closed(&raw("nhat6"), &opts);
    assert_eq!(
        r.text, "nhat6",
        "closed projection must restore the literal raw"
    );
    assert!(
        r.applied_marks.is_empty(),
        "the demoted digit mark carries no applied mark"
    );
}

#[test]
fn critical_vni_toned_shape_attested_survives_closed_boundary() {
    // "d9ech56": delayed digit order — the '6' mark is non-adjacent, so the
    // gate is consulted. The composed "đệch" is NOT exact-attested (only
    // "đếch" is in the dictionary) but its tone HAS been deliberately typed
    // ('5' = nặng), so the closed boundary must keep the shape-attested
    // composition instead of reverting to raw. Regression: the old
    // unconditional exact clamp committed "d9ech56" while the ADJACENT key
    // order "d9e6ch5" (marks unflagged, gate never consulted) committed
    // "đệch" — commit results must not depend on key order.
    let opts = vni_opts();
    assert_eq!(
        compose(&raw("d9ech56"), &opts).text,
        "đệch",
        "open projection"
    );
    assert_eq!(
        compose_closed(&raw("d9ech56"), &opts).text,
        "đệch",
        "toned shape-attested word must survive the closed boundary"
    );
    // Key-order parity: the adjacent order commits the same word.
    assert_eq!(compose_closed(&raw("d9e6ch5"), &opts).text, "đệch");
}

// ── Informal/slang supplement (data/attested-syllables.txt) ──────────────────
// The base ibus-bamboo dictionary already attests most established slang
// (đéo, vãi, khum, quẩy, toang, ngáo, khịa, bựa, nhây, lầy, chảnh, hông, hem);
// the buttre-curated supplement adds the missing phonologically-valid ones.
// These tests pin BOTH projections so a future gate change can never silently
// revert an informal word at the word boundary again (the "d9ech56" class).

#[test]
fn slang_supplement_composes_and_survives_boundary_telex() {
    let opts = telex_opts();
    for (input, expected) in [
        ("ddeechj", "đệch"), // adjacent
        ("ddechej", "đệch"), // delayed non-adjacent 'e' — needs the dictionary entry
        ("chowif", "chời"),
        ("dzoo", "dzô"),
        ("dzaayj", "dzậy"),
        ("dzui", "dzui"),
        // Pre-attested slang from the base dictionary (pin protection).
        ("vaix", "vãi"),
        ("quaayr", "quẩy"),
        ("ngaos", "ngáo"),
        ("laayf", "lầy"),
    ] {
        assert_eq!(compose(&raw(input), &opts).text, expected, "open: {input}");
        assert_eq!(
            compose_closed(&raw(input), &opts).text,
            expected,
            "closed: {input}"
        );
    }
}

#[test]
fn slang_supplement_composes_and_survives_boundary_vni() {
    let opts = vni_opts();
    for (input, expected) in [
        ("d9ech56", "đệch"), // delayed digit + tone (the reported bug's shape)
        ("d9e6ch5", "đệch"), // adjacent parity
        ("cho72i", "chời"),
        ("dzo6", "dzô"),
        ("dza6y5", "dzậy"),
    ] {
        assert_eq!(compose(&raw(input), &opts).text, expected, "open: {input}");
        assert_eq!(
            compose_closed(&raw(input), &opts).text,
            expected,
            "closed: {input}"
        );
    }
}

#[test]
fn critical_vni_nhat61_closed_untouched_exact_attested() {
    // Exact-attested already under the OPEN gate (the tone arrived) — the
    // closed projection must be byte-identical.
    let opts = vni_opts();
    let open = compose(&raw("nhat61"), &opts);
    let closed = compose_closed(&raw("nhat61"), &opts);
    assert_eq!(open.text, "nhất");
    assert_eq!(
        closed.text, "nhất",
        "exact-attested word must be untouched by the closed projection"
    );
}

#[test]
fn critical_vietej_closed_untouched_telex_exact_path() {
    // Telex's non-digit trigger is ALREADY exact-attested under the open
    // gate (no digit shape-relaxation to begin with) — closed must agree.
    let opts = telex_opts();
    assert_eq!(compose_closed(&raw("vietej"), &opts).text, "việt");
}

#[test]
fn critical_data_closed_no_double_repair() {
    // Already literal under the open gate (unattested "dât" demotes) —
    // closed must not do anything additional/different.
    let opts = telex_opts();
    assert_eq!(compose_closed(&raw("data"), &opts).text, "data");
}

#[test]
fn high_reset_closed_accepted_collision_untouched() {
    // "rết" is a real (if coincidental) word — exact-attested under both
    // projections; the gate cannot and must not distinguish this from a
    // deliberate transform.
    let opts = telex_opts();
    assert_eq!(compose_closed(&raw("reset"), &opts).text, "rết");
}

#[test]
fn high_adjacent_vieet_closed_never_repaired() {
    // No inferred (non-adjacent) mark fired at all — `passes_attestation_gate`
    // short-circuits before ever consulting `closed`.
    let opts = telex_opts();
    assert_eq!(compose_closed(&raw("vieet"), &opts).text, "viêt");
}

#[test]
fn medium_closed_flag_is_a_pure_parameter_not_shared_state() {
    // `compose` and `compose_closed` on the SAME raw/opts must never
    // interfere with each other — both are pure functions of their inputs.
    let opts = vni_opts();
    let raw_buf = raw("nhat6");
    for _ in 0..3 {
        assert_eq!(compose(&raw_buf, &opts).text, "nhât");
        assert_eq!(compose_closed(&raw_buf, &opts).text, "nhat6");
    }
}

// ── Phase 5: learning — user-attested overlay + preference memory ────────────
// Test Scenario Matrix from phase-05-learning-stores.md, compose-level rows.
// Empty-store determinism is checked FIRST, per the phase's own ordering.

#[test]
fn p5_empty_store_determinism_byte_identical() {
    // `ComposeOpts::from_config` always starts with `user_attested: None,
    // raw_prefs: None` — every OTHER test in this file (Telex/VNI/Cham,
    // undo, gate, boundary-repair) already proves this exhaustively by
    // never touching either field. This test pins the contract explicitly:
    // an opts value with the fields SET but EMPTY (not `None`) must ALSO be
    // byte-identical — an empty overlay/pref map is observationally the
    // same as no store at all (Success Criteria: "goldens unaffected").
    let base = telex_opts();
    let with_empty_store = ComposeOpts {
        user_attested: Some(Arc::new(HashSet::new())),
        raw_prefs: Some(Arc::new(HashMap::new())),
        ..base.clone()
    };
    for word in ["data", "reset", "vietje", "aaa", "cana", "nasa"] {
        assert_eq!(
            compose(&raw(word), &base),
            compose(&raw(word), &with_empty_store),
            "an empty (but Some) learning store must be byte-identical to no store at all for '{word}'"
        );
    }
}

#[test]
fn p5_overlay_or_check_rescues_unattested_non_adjacent_mark() {
    // "data" demotes to literal because "dât" is unattested
    // (critical_data_stays_literal). Adding "dât"'s bit to the overlay must
    // make it survive the gate instead — via the SAME single consult point
    // (`pipeline::validation::is_attested_overlay`) P3's closed gate and
    // P2's probe also use.
    let (o, n, c, t) = decompose_ids("dât").expect("'dât' must be a decomposable shape");
    let mut overlay = HashSet::new();
    overlay.insert(bit_index(o, n, c, t) as u32);
    let opts = ComposeOpts {
        user_attested: Some(Arc::new(overlay)),
        ..telex_opts()
    };

    let r = compose(&raw("data"), &opts);
    assert_eq!(
        r.text, "dât",
        "an overlay-attested syllable must survive the non-adjacent gate"
    );
    assert!(!r.demoted, "a gate PASS is not a demote");
    assert!(!r.temp_english);

    // Sanity: WITHOUT the overlay, the original demote behavior is unchanged.
    assert_eq!(compose(&raw("data"), &telex_opts()).text, "data");
}

#[test]
fn p5_pref_literal_short_circuits_before_fallback() {
    // "reset" naturally composes to attested "rết"
    // (high_reset_accepted_attested_collision). A stored `Pref::Literal`
    // for this exact raw must short-circuit to the literal keystrokes
    // instead, from the very top of `compose_internal` — before
    // fallback/segment/transform/tone ever run.
    let mut prefs = HashMap::new();
    prefs.insert("reset".to_string(), Pref::Literal);
    let opts = ComposeOpts {
        raw_prefs: Some(Arc::new(prefs)),
        ..telex_opts()
    };

    let r = compose(&raw("reset"), &opts);
    assert_eq!(
        r.text, "reset",
        "a literal pref must short-circuit compose entirely"
    );

    // Unaffected: a DIFFERENT raw sequence still composes normally.
    assert_eq!(compose(&raw("viet"), &opts).text, "viet");
}

#[test]
fn p5_pref_composed_short_circuits_undo_detection() {
    // "ass" normally undoes via `check_fallback` (telex_ass_undo: "ass" ->
    // "as" + literal 's', temp_english). A stored `Pref::Composed` for this
    // exact raw must bypass fallback/gate/English-check entirely and force
    // the plain segment->transform->tone pipeline: 'a' + tone 's' -> "á".
    let mut prefs = HashMap::new();
    prefs.insert("ass".to_string(), Pref::Composed);
    let opts = ComposeOpts {
        raw_prefs: Some(Arc::new(prefs)),
        ..telex_opts()
    };

    let r = compose(&raw("ass"), &opts);
    assert_eq!(r.text, "á", "a composed pref must bypass undo detection");
    assert!(!r.temp_english);
    assert!(!r.demoted);

    // Sanity: WITHOUT the pref, the original undo behavior is unchanged.
    let plain = compose(&raw("ass"), &telex_opts());
    assert_eq!(plain.text, "as");
    assert!(plain.temp_english);
}

#[test]
fn p5_pref_lookup_is_exact_full_raw_never_prefix() {
    // The pref short-circuit is gated on `allow_nonadjacent` (never
    // consulted inside a demoted recursive call) and is structurally
    // unreachable from `fallback`'s internal prefix-reconstruction helpers
    // (`apply_transforms_only` et al. call segment/transform/assemble
    // directly, never re-entering `compose_internal`). This test pins BOTH
    // guarantees at once: storing a pref for the PREFIX "dd" alone must
    // have zero effect on `"ddaaa"` (whose full raw is "ddaaa", not "dd") —
    // the transform-preserving undo still reconstructs the prefix as the
    // TRANSFORMED "đ", exactly as it does with no pref at all
    // (telex_ddaaa_preserves_dstroke in `compose::fallback`'s own tests).
    let mut prefs = HashMap::new();
    prefs.insert("dd".to_string(), Pref::Literal);
    let opts = ComposeOpts {
        raw_prefs: Some(Arc::new(prefs)),
        ..telex_opts()
    };

    let r = compose(&raw("ddaaa"), &opts);
    assert_eq!(r.text, "đaa", "a pref stored for a raw PREFIX must never leak into a longer raw's compose, nor into fallback's own prefix reconstruction");
}
