//! Regression: interior spent-tone-undo fold (compose step 1.5).
//!
//! "reset" was untypeable: "ress" correctly undid the acute tone ("res"),
//! but the next 'e' recomposed raw "resse" from scratch — segment collected
//! both spent 's' keys as tones, last-tone-wins resurrected the removed
//! acute, and the non-adjacent `e..e` mark produced attested "rế", which
//! passed every un-latch condition and overwrote the display. The undo
//! event must stay folded for the rest of the word (Unikey multi-level
//! toggle: undo is final until the word boundary).

use buttre_core::KeyboardBuilder;

fn type_seq(seq: &str) -> String {
    let mut kb = KeyboardBuilder::telex().unwrap();
    for ch in seq.chars() {
        kb.process(ch).unwrap();
    }
    kb.buffer().to_string()
}

#[test]
fn resset_types_reset_via_double_s_undo() {
    assert_eq!(type_seq("ress"), "res", "double-s undo must fire");
    assert_eq!(
        type_seq("resse"),
        "rese",
        "the 'e' after a spent undo must append literally, never resurrect the tone as 'rế'"
    );
    assert_eq!(type_seq("resset"), "reset");
}

#[test]
fn plain_reset_keeps_unikey_flexible_composition() {
    // No undo typed: the interior 's' is a live tone and the delayed 'e'
    // completes ê — same as Unikey. The escape hatch is the double-s above.
    assert_eq!(type_seq("reset"), "rết");
}

#[test]
fn spent_undo_fold_matches_display_for_sibling_words() {
    // Same class as "resset": deliberate undo mid-word, literal tail.
    assert_eq!(type_seq("tesst"), "test");
    assert_eq!(type_seq("dessign"), "design");
    // Tone never displayed ("glá" has an invalid onset): the visibility gate
    // must keep the fold out of English double-letter words.
    assert_eq!(type_seq("glasses"), "glasses");
}
