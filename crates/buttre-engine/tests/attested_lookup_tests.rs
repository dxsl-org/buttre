//! Tests for `pipeline::validation::{is_attested, is_shape_attested}` —
//! the embedded attested-Vietnamese-syllable bitset lookup.

use buttre_engine::pipeline::validation::{is_attested, is_shape_attested};

#[test]
fn attested_true_for_common_words() {
    for w in ["việt", "đông", "cân", "dật", "gì", "gìn"] {
        assert!(is_attested(w), "{w} should be attested");
    }
}

#[test]
fn attested_false_for_non_words() {
    for w in ["dât", "mêm", "phôt", "fâllb"] {
        assert!(!is_attested(w), "{w} should NOT be attested");
    }
}

#[test]
fn shape_attested_ignores_tone() {
    // "nhât" itself carries no tone, but "nhất" (nh + â + t + sắc) is
    // attested — the shape (onset nh, nucleus â, coda t) must match under
    // ANY tone.
    assert!(is_shape_attested("nhât"), "nhât shape should be attested via nhất");
    assert!(is_shape_attested("nhất"), "nhất itself must also match its own shape");
}

#[test]
fn shape_attested_false_for_unattested_shape() {
    assert!(!is_shape_attested("fâllb"));
}

#[test]
fn attested_old_and_new_tone_placement_agree() {
    // "hoà" (old placement) and "hòa" (new placement) are the same syllable;
    // both must decompose to the same (onset, nucleus, coda, tone) tuple.
    assert!(is_attested("hoà"));
    assert!(is_attested("hòa"));
    assert_eq!(is_attested("hoà"), is_attested("hòa"));
}

#[test]
fn attested_handles_nfd_and_uppercase() {
    use unicode_normalization::UnicodeNormalization;

    let nfc = "việt";
    let nfd: String = nfc.nfd().collect();
    assert_ne!(nfc.as_bytes(), nfd.as_bytes(), "test fixture must actually be NFD-decomposed");

    assert!(is_attested(nfc));
    assert!(is_attested(&nfd), "NFD input must decompose the same as NFC");
    assert!(is_attested("VIỆT"), "uppercase input must be handled");
    assert!(is_attested("Việt"), "title-case input must be handled");
}

#[test]
fn attested_empty_and_garbage_input_fail_open() {
    // Fail-open: unparseable input returns false, never panics.
    assert!(!is_attested(""));
    assert!(!is_attested("123"));
    assert!(!is_attested("!!!"));
    assert!(!is_shape_attested(""));
    assert!(!is_shape_attested("123"));
}

#[test]
fn attested_rejects_conflicting_double_tone() {
    // Two different tone diacritics mashed together is not a real syllable;
    // decompose_ids must fail closed (return false) rather than pick one.
    assert!(!is_attested("việtầ"));
}

#[test]
fn attested_true_for_reembedded_k_coda_place_names() {
    // P6: the former "k-coda" skip category (data/attested-syllables.txt
    // header) is resolved — these 9 dict entries now decompose and are
    // embedded in the bitset, same as any other attested syllable.
    for w in ["đắk", "đăk", "lắk", "lăk", "măk", "ắk", "ăk", "búk", "úk"] {
        assert!(is_attested(w), "{w} should now be attested (P6 coda-k re-embed)");
    }
}

#[test]
fn attested_false_for_unattested_k_coda_shapes() {
    // No dict evidence exists for these — the per-nucleus gate in
    // `is_valid_combination` keeps them off the bitset AND off the shape
    // check; a blanket "k" coda allowance would wrongly relax this.
    for w in ["đik", "đok", "đek"] {
        assert!(!is_attested(w), "{w} should NOT be attested");
        assert!(!is_shape_attested(w), "{w} should NOT be shape-attested either");
    }
}
