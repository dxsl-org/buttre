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
        applied_marks: Vec::new(),
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

// ── Regression: double-u horn placement (bugs: luuw Telex / luu7 VNI) ────────
//
// The rightmost 'u' in "luu" produces invalid nucleus "uư"; the fix must
// retry leftward to place the horn on the first 'u' giving valid "lưu".

#[test]
fn telex_luuw_yields_luu_horn() {
    assert_eq!(compose(&raw("luuw"), &telex_opts()).text, "lưu",
        "Telex luuw must produce lưu");
}

#[test]
fn vni_luu7_yields_luu_horn() {
    assert_eq!(compose(&raw("luu7"), &vni_opts()).text, "lưu",
        "VNI luu7 must produce lưu");
}

// ── Regression: tone-before-transform ordering (bug: mieng16/mieng26 VNI) ────
//
// "mieng16": '1' (tone sắc) typed before '6' (e→ê transform).
// The intermediate form "miéng" has nucleus "ie" + coda "ng"; this must pass
// validation so English fallback does NOT latch before '6' is processed.

#[test]
fn vni_mieng16_yields_mieng_acute() {
    assert_eq!(compose(&raw("mieng16"), &vni_opts()).text, "miếng",
        "VNI mieng16 (tone before transform) must produce miếng");
}

#[test]
fn vni_mieng26_yields_mieng_grave() {
    assert_eq!(compose(&raw("mieng26"), &vni_opts()).text, "miềng",
        "VNI mieng26 must produce miềng");
}

#[test]
fn vni_mieng61_still_works() {
    // Transform before tone: already worked before the fix; guard the regression.
    assert_eq!(compose(&raw("mieng61"), &vni_opts()).text, "miếng",
        "VNI mieng61 (transform before tone) must still produce miếng");
}

// ── Regression: fast-typing onset 'd' before doubling key (Telex) ─────────────
//
// When typing fast, the onset 'd' and first vowel can slip in before the
// doubling keys.  Unikey-compatible: second 'd' = "dd"→"đ", second 'o' = "oo"→"ô".

#[test]
fn telex_dodong_yields_dong() {
    assert_eq!(compose(&raw("dodong"), &telex_opts()).text, "đông",
        "Telex dodong (fast-type, no tone) must produce đông");
}

#[test]
fn telex_dodongf_yields_dong_grave() {
    assert_eq!(compose(&raw("dodongf"), &telex_opts()).text, "đồng",
        "Telex dodongf (fast-type onset slip) must produce đồng");
}

// ── Phase 2: attestation gate on non-adjacent transforms ─────────────────────
// Test Scenario Matrix from phase-02-attestation-gate-compose.md.

#[test]
fn critical_data_stays_literal() {
    // The flagship bug: "data" — non-adjacent 'a' would produce unattested
    // "dât" — must demote to the literal keystrokes.
    let r = compose(&raw("data"), &telex_opts());
    assert_eq!(r.text, "data", "unattested 'dât' must demote to literal 'data'");
}

#[test]
fn critical_vietej_fires_attested() {
    assert_eq!(compose(&raw("vietej"), &telex_opts()).text, "việt",
        "flexible non-adjacent typing must still produce attested 'việt'");
}

#[test]
fn critical_nasa_stays_literal() {
    // "nấ" (raw-adjacency bug: tone key 's' sits between the two 'a's) is
    // unattested — must demote to literal, not leak a spurious diacritic
    // through the elongation fallback either (see `try_elongation_fallback`).
    let r = compose(&raw("nasa"), &telex_opts());
    assert_eq!(r.text, "nasa", "unattested 'nấ' must demote to literal 'nasa'");
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
    assert_eq!(compose(&raw("nhat6"), &vni_opts()).text, "nhât",
        "mid-typing 'nhât' must not flicker to literal before the tone key");
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
        assert_eq!(r.text, word, "'{word}' must stay literal (unattested non-adjacent result)");
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
        assert_eq!(r.text, word, "'{word}' must stay literal via the attestation gate (unattested non-adjacent result)");
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
    cfg.validation = Some(ValidationSettings { syllable_structure: "hmong".to_string(), allow_invalid: true });
    let opts = ComposeOpts::from_config(&cfg);
    assert!(!opts.attest_non_adjacent, "Hmong validator must not enable the attestation gate");
    assert_eq!(compose(&raw("nasa"), &opts).text, "nấ",
        "gate-off: the non-adjacent mark fires ungated, unaffected by attestation");
}

// ── Fallback bypass regression (red-team C2) ──────────────────────────────────
// `check_tone_toggle`/`check_transform_toggle`'s prefix reconstruction must be
// gated exactly like the main compose() path — no â/ê leaking through.

#[test]
fn c2_dataeee_no_bypass() {
    let r = compose(&raw("dataeee"), &telex_opts());
    assert_eq!(r.text, "dataee");
    assert!(!r.text.contains(['â', 'ê']), "no diacritic must leak through the transform-toggle bypass");
}

#[test]
fn c2_vietess_no_bypass() {
    // "viet" + demoted literal 'e' + literal 's' suffix = "vietes".
    let r = compose(&raw("vietess"), &telex_opts());
    assert_eq!(r.text, "vietes");
    assert!(!r.text.contains(['â', 'ê']), "no diacritic must leak through the tone-toggle bypass");
}

#[test]
fn c2_databaaa_no_bypass() {
    let r = compose(&raw("databaaa"), &telex_opts());
    assert_eq!(r.text, "databaa");
    assert!(!r.text.contains(['â', 'ê']), "no diacritic must leak through the transform-toggle bypass");
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
    assert!(!r.text.contains(['â', 'ê', 'đ']), "demoted output must carry no leftover diacritics: {}", r.text);
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
//   is no persistent state, so re-running compose on the grown raw buffer
//   "canaan" does not "resume" a prior undo — it recomputes from scratch,
//   and the same 3-occurrence guard that protects "banana"/"dataa" also
//   blocks any transform from firing here, so the result is the full raw
//   string typed so far ("canaan", not the display-trimmed "canan"). What
//   Phase 4 guarantees at this layer — and what the test below asserts — is
//   the "Gatekeeper passthrough" invariant: no diacritic leaks back in and
//   no incorrect re-fire happens. Trimming the display to "canan" is a
//   caller/executor concern (there is no `PipelineExecutor` wiring for this
//   `compose` module yet — out of Phase 4's file-ownership scope).

#[test]
fn critical_cana_a_undoes_to_literal_latched() {
    // Verified: compose("cana") == "cân" (attested collision, see
    // `medium_cana_collision_canal_self_heals` above) — the pre-undo state
    // this escape hatch targets.
    assert_eq!(compose(&raw("cana"), &telex_opts()).text, "cân");
    let r = compose(&raw("canaa"), &telex_opts());
    assert_eq!(r.text, "cana", "retyping 'a' must undo the non-adjacent â mark");
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
    assert_eq!(r.text, "dataa", "no keystroke may be dropped when the prefix's mark was already gate-demoted");
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
    assert!(!r.text.contains(['ệ', 'ê']), "no undo-related diacritic must leak");
}

#[test]
fn high_vni_can6_digit_parity_undoes_to_literal_latched() {
    // Method parity (S8): the VNI digit-triggered equivalent of "cana"+"a".
    // See module-level deviation note for why '6' (not '7') is the correct
    // trigger digit for â in this codebase's VNI table.
    assert_eq!(compose(&raw("can6"), &vni_opts()).text, "cân",
        "can6 must be the VNI attested collision analogous to Telex cana");
    let r = compose(&raw("can66"), &vni_opts());
    assert_eq!(r.text, "can6", "retyping the VNI digit trigger must undo exactly like Telex");
    assert!(r.temp_english);
}

#[test]
fn high_cana_uppercase_trigger_case_insensitive() {
    // Retyping the trigger in the OPPOSITE case must still undo.
    let r = compose(&raw("canaA"), &telex_opts());
    assert_eq!(r.text, "cana", "uppercase retype of the trigger key must still undo");
    assert!(r.temp_english);
}

#[test]
fn high_cana_latch_survives_recompute_no_reentry() {
    // "cana"+"a"+"n": see module-level deviation note. At the pure compose()
    // layer, no diacritic leaks back in and the buffer is never re-entered
    // into the fired-mark path (the count-of-'a'==3 guard blocks it, exactly
    // like "banana"/"dataa") — this is the "Gatekeeper passthrough" invariant
    // Phase 4 owns; display-level trimming to "canan" is a caller concern.
    let r = compose(&raw("canaan"), &telex_opts());
    assert_eq!(r.text, "canaan");
    assert!(!r.text.contains(['â']), "the undone â mark must never re-fire on further typing");
}

#[test]
fn high_dand_d_consonant_undo_equivalence_note() {
    // Substitute for the matrix's "dodongd" row (see module-level deviation
    // note): "dand" fires the backward-referring đ mark on its OWN final raw
    // key (base_ends_with_coda("dan") makes it a "committed syllable"), so
    // retyping 'd' immediately after DOES satisfy immediacy.
    assert_eq!(compose(&raw("dand"), &telex_opts()).text, "đan",
        "dand must be an attested đ collision (đan = to knit/weave)");
    let r = compose(&raw("dandd"), &telex_opts());
    assert_eq!(r.text, "dand", "retyping 'd' right after dand must undo the đ mark");
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
    assert!(!r.text.contains('â'), "repeated retypes must never resurrect the diacritic: {}", r.text);
}

